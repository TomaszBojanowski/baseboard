// SPDX-License-Identifier: GPL-3.0-or-later

//! A model of the `nct6683` driver in memory, for tests of everything that
//! sits on [`FanDriver`] (docs/PROJEKT.md §3.7). For now it models reading:
//! which attributes exist, their values, their write bits and failing
//! reads. Writing, with the driver's clamping, `-EIO`, `-ETIMEDOUT`, the
//! gradual output in automatic mode and resume, comes with the helper
//! (M2b).

use std::collections::BTreeMap;

use super::{Capabilities, Capability, CurvePoint, FanDriver, Mode, ReadError, Source};

/// An attribute of a channel, for failing reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    Mode,
    Duty,
    Curve,
    Source,
}

/// One channel of the model. An attribute exists when it is `Some`; the
/// write bits decide the capabilities the same way as the file modes do in
/// sysfs (§5.5.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelChannel {
    pub mode: Option<Mode>,
    pub duty: Option<u8>,
    pub curve: Option<Vec<CurvePoint>>,
    pub source: Option<Source>,
    /// `pwmN_enable` and `pwmN` have the owner write bit.
    pub manual_writable: bool,
    /// Every curve point has the owner write bit.
    pub curve_writable: bool,
    /// `pwmN_auto_channels_temp` has the owner write bit.
    pub source_writable: bool,
    /// Reads of these attributes fail with the errno.
    pub failing: Vec<(Attribute, i32)>,
}

/// A driver in memory: channels by N of `pwmN`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelDriver {
    channels: BTreeMap<u8, ModelChannel>,
}

impl ModelDriver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_channel(mut self, pwm: u8, channel: ModelChannel) -> Self {
        self.channels.insert(pwm, channel);
        self
    }

    pub fn channel_mut(&mut self, pwm: u8) -> Option<&mut ModelChannel> {
        self.channels.get_mut(&pwm)
    }

    /// The channel, unless a read of `attribute` is set to fail.
    fn get(&self, pwm: u8, attribute: Attribute) -> Result<&ModelChannel, ReadError> {
        let channel = self.channels.get(&pwm).ok_or(ReadError::Missing)?;
        match channel.failing.iter().find(|(a, _)| *a == attribute) {
            Some((_, errno)) => Err(ReadError::Os(*errno)),
            None => Ok(channel),
        }
    }
}

impl FanDriver for ModelDriver {
    fn capabilities(&self, pwm: u8) -> Capabilities {
        let mut caps = Capabilities::default();
        let Some(c) = self.channels.get(&pwm) else {
            return caps;
        };
        let manual = c.mode.is_some() && c.manual_writable;
        caps.insert_if(Capability::DutyRead, c.duty.is_some());
        caps.insert_if(Capability::Mode, manual);
        caps.insert_if(Capability::DutyWrite, manual && c.duty.is_some());
        let points = c
            .curve
            .as_ref()
            .and_then(|curve| u8::try_from(curve.len()).ok())
            .unwrap_or(0);
        if points > 0 {
            caps.curve_points = points;
            caps.insert(Capability::CurveRead);
            caps.insert_if(Capability::CurveWrite, c.curve_writable);
        }
        caps.insert_if(Capability::SourceRead, c.source.is_some());
        caps.insert_if(
            Capability::SourceWrite,
            c.source.is_some() && c.source_writable,
        );
        caps
    }

    fn mode(&self, pwm: u8) -> Result<Mode, ReadError> {
        self.get(pwm, Attribute::Mode)?
            .mode
            .ok_or(ReadError::Missing)
    }

    fn duty(&self, pwm: u8) -> Result<u8, ReadError> {
        self.get(pwm, Attribute::Duty)?
            .duty
            .ok_or(ReadError::Missing)
    }

    fn curve(&self, pwm: u8) -> Result<Vec<CurvePoint>, ReadError> {
        match &self.get(pwm, Attribute::Curve)?.curve {
            Some(curve) if !curve.is_empty() => Ok(curve.clone()),
            _ => Err(ReadError::Missing),
        }
    }

    fn source(&self, pwm: u8) -> Result<Source, ReadError> {
        self.get(pwm, Attribute::Source)?
            .source
            .ok_or(ReadError::Missing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fans::driver::{DriverLevel, read};

    fn curve(points: &[(u8, u8)]) -> Vec<CurvePoint> {
        points
            .iter()
            .map(|&(temp, duty)| CurvePoint { temp, duty })
            .collect()
    }

    /// A channel as `hwmon-next` shows it: manual mode, a curve to read.
    fn hwmon_next() -> ModelChannel {
        ModelChannel {
            mode: Some(Mode::Auto),
            duty: Some(98),
            curve: Some(curve(&[
                (0, 94),
                (5, 96),
                (84, 99),
                (85, 255),
                (85, 255),
                (85, 255),
                (85, 255),
            ])),
            source: Some(Source::Temp(1)),
            manual_writable: true,
            ..ModelChannel::default()
        }
    }

    #[test]
    fn capabilities_follow_the_write_bits_as_in_sysfs() {
        let driver = ModelDriver::new()
            // Fedora 7.3.3: a duty cycle, nothing else.
            .with_channel(
                1,
                ModelChannel {
                    duty: Some(124),
                    ..ModelChannel::default()
                },
            )
            .with_channel(16, hwmon_next())
            .with_channel(
                15,
                ModelChannel {
                    curve_writable: true,
                    ..hwmon_next()
                },
            )
            .with_channel(
                14,
                ModelChannel {
                    curve_writable: true,
                    source_writable: true,
                    ..hwmon_next()
                },
            );
        assert_eq!(driver.capabilities(1).level(), DriverLevel::ReadOnly);
        assert_eq!(
            driver.capabilities(16).level(),
            DriverLevel::ManualCurveView
        );
        assert_eq!(driver.capabilities(16).curve_points(), 7);
        assert_eq!(driver.capabilities(15).level(), DriverLevel::Curves);
        assert_eq!(driver.capabilities(14).level(), DriverLevel::CurvesSource);
        // A channel the model does not have.
        assert_eq!(driver.capabilities(2), Capabilities::default());
    }

    #[test]
    fn reads_what_the_channel_has() {
        let driver = ModelDriver::new()
            .with_channel(
                1,
                ModelChannel {
                    duty: Some(124),
                    ..ModelChannel::default()
                },
            )
            .with_channel(16, hwmon_next());
        let fedora = read(&driver, 1);
        assert_eq!(fedora.duty, Some(Ok(124)));
        assert_eq!(
            (fedora.mode, fedora.curve, fedora.source),
            (None, None, None)
        );

        let next = read(&driver, 16);
        assert_eq!(next.mode, Some(Ok(Mode::Auto)));
        assert_eq!(
            next.curve.unwrap().unwrap()[2],
            CurvePoint { temp: 84, duty: 99 }
        );
        assert_eq!(next.source, Some(Ok(Source::Temp(1))));
    }

    #[test]
    fn failing_reads() {
        let mut driver = ModelDriver::new().with_channel(16, hwmon_next());
        driver.channel_mut(16).unwrap().failing =
            vec![(Attribute::Curve, 5), (Attribute::Duty, 110)];
        let reading = read(&driver, 16);
        assert_eq!(reading.curve, Some(Err(ReadError::Os(5))));
        assert_eq!(reading.duty, Some(Err(ReadError::Os(110))));
        // Other attributes still read.
        assert_eq!(reading.mode, Some(Ok(Mode::Auto)));
        // A failing read does not change the capabilities.
        assert_eq!(driver.capabilities(16).curve_points(), 7);
    }
}
