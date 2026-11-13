// SPDX-License-Identifier: GPL-3.0-or-later

//! The Fans page as a model (docs/PROJEKT.md §5.5.3, §5.5.4, §7 M2,
//! M2.4, D62), without text: the temperature on top and the source of the
//! curves, the fans (rows of the list in the app) and the headers without
//! control, for what the driver allows and reports. Version 0.2.0 shows
//! the settings only: nothing changes anything.
//!
//! The speeds, duty cycles and temperatures come from the Overview's model
//! of the same sample, so both pages show the same values; the modes,
//! curves and sources come from [`ChannelReading`]s, which the app reads
//! only while the page is shown.

use crate::fans::curve;
use crate::fans::driver::{
    self, Capabilities, Channel, CurvePoint, DriverLevel, FanDriver, Mode, ReadError, Source,
};
use crate::overview::{Group, Row, Subtitle, Visibility};
use crate::profile::{self, FanRole, Profile, Temp, TempRole};

/// What the app reads for one fan channel while the page is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelReading {
    pub key: String,
    /// N of `pwmN`.
    pub pwm: u8,
    pub capabilities: Capabilities,
    /// The profile marks it controllable and it has `Mode` (§5.5.2).
    pub controllable: bool,
    /// Read every sample. `None`: the driver has no such attribute.
    pub mode: Option<Result<Mode, ReadError>>,
    /// Read on entering the page and every 10 s: each read of a point goes
    /// to the board's controller.
    pub curve: Option<Result<Vec<CurvePoint>, ReadError>>,
    pub source: Option<Result<Source, ReadError>>,
}

impl ChannelReading {
    /// Reads the mode of `channel`, and its curve and source when `curve`
    /// is `None`; otherwise those are taken as given (read earlier).
    pub fn read(
        driver: &dyn FanDriver,
        channel: &Channel,
        curve: Option<(CurveRead, SourceRead)>,
    ) -> Self {
        let (curve, source) = curve.unwrap_or_else(|| {
            (
                present(driver.curve(channel.pwm)),
                present(driver.source(channel.pwm)),
            )
        });
        Self {
            key: channel.key.clone(),
            pwm: channel.pwm,
            capabilities: channel.capabilities,
            controllable: channel.controllable,
            mode: present(driver.mode(channel.pwm)),
            curve,
            source,
        }
    }
}

/// `None` for an attribute the driver does not have.
fn present<T>(result: Result<T, ReadError>) -> Option<Result<T, ReadError>> {
    match result {
        Err(ReadError::Missing) => None,
        other => Some(other),
    }
}

/// A curve as read: `None` without the attribute.
pub type CurveRead = Option<Result<Vec<CurvePoint>, ReadError>>;
/// A source as read: `None` without the attribute.
pub type SourceRead = Option<Result<Source, ReadError>>;

/// The page.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// With `ReadOnly` the page says that the fans cannot be controlled
    /// yet (§5.5.3).
    pub level: DriverLevel,
    /// The temperature the curves follow, once for all fans; `None`
    /// without a curve source.
    pub source: Option<SourceRow>,
    /// The temperature on top of the page (D62): the source the curves
    /// follow, or without one the processor's temperature of the profile.
    pub temperature: Option<SourceRow>,
    /// In the order of the headers on the board.
    pub fans: Vec<Fan>,
    /// Headers the driver does not control (the auxiliary fan on MS-7E61),
    /// as the Overview shows them.
    pub read_only: Vec<Row>,
}

/// The temperature source of the curves.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceRow {
    pub source: Source,
    /// The temperature of the profile, with its emergency threshold.
    pub temp: Option<Temp>,
    /// Its row of the Overview, with the value and the range since the
    /// start.
    pub row: Option<Row>,
    /// The name of the board's hwmon device, e.g. nct6687.
    pub chip: String,
}

impl SourceRow {
    /// The temperature now, in millidegrees, when it could be read.
    pub fn millidegrees(&self) -> Option<i64> {
        let value = self.row.as_ref()?.value;
        value.is_finite().then(|| (value * 1000.0).round() as i64)
    }
}

/// One header of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Fan {
    pub key: String,
    pub role: FanRole,
    /// Its row of the Overview: the name, the speed and its range since
    /// the start, the duty cycle and the header.
    pub row: Row,
    pub header: String,
    /// The header mapping was checked on this board (§6.2).
    pub verified: bool,
    /// The minimum duty cycle in percent (B5), shown where something could
    /// be controlled; 0 without one.
    pub min_percent: u8,
    /// The duty cycle the driver reports, 0–255.
    pub duty: Option<u8>,
    pub kind: FanKind,
}

impl Fan {
    /// Whether the fan spun at any sample since the start.
    pub fn spun(&self) -> bool {
        let max = self.row.range.map_or(self.row.value, |(_, max)| max);
        max > 0.0 || self.row.value > 0.0
    }

    /// No speed since the start although the duty cycle is not 0: the
    /// header may be empty (§7 M2, M2.4). A fan at 0 % does not spin on
    /// purpose.
    pub fn maybe_empty(&self) -> bool {
        !self.spun() && self.duty != Some(0)
    }

    /// The chart, when the curve sets the fan's duty cycle.
    pub fn chart(&self) -> Option<&Chart> {
        match &self.kind {
            FanKind::Full {
                manual: false,
                body: Body::Chart(chart),
            } => Some(chart.as_ref()),
            _ => None,
        }
    }

    /// The curve in the board's controller, also in manual mode, when the
    /// controller keeps it but holds a fixed duty cycle: what the page
    /// "Curve Details" shows.
    pub fn curve(&self) -> Option<&Chart> {
        match &self.kind {
            FanKind::Full {
                body: Body::Chart(chart),
                ..
            } => Some(chart.as_ref()),
            _ => None,
        }
    }

    /// The driver has the curve, but reading it failed.
    pub fn curve_unreadable(&self) -> bool {
        matches!(
            self.kind,
            FanKind::Full {
                body: Body::CurveUnreadable,
                ..
            }
        )
    }

    /// Whether the page "Curve Details" has something to show: the curve,
    /// or that it could not be read.
    pub fn has_details(&self) -> bool {
        self.curve().is_some() || self.curve_unreadable()
    }

    /// The duty cycle the curve gives now, when the fan is still moving to
    /// it (in automatic mode the output changes gradually, §4) and the two
    /// differ in whole percent.
    pub fn moving_to(&self) -> Option<u8> {
        let target = self.chart()?.target?;
        let duty = self.duty?;
        (crate::units::duty_to_percent(target) != crate::units::duty_to_percent(duty))
            .then_some(target)
    }
}

/// What the page knows of a header beyond its row.
#[derive(Debug, Clone, PartialEq)]
pub enum FanKind {
    /// The driver controls nothing: the speed, and the duty cycle where
    /// the driver reports it.
    Low,
    /// The driver controls the header: its curve. In manual mode the fan
    /// holds a fixed duty cycle and the curve stays in the board's
    /// controller.
    Full { manual: bool, body: Body },
}

/// The curve of a header the driver controls.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Chart(Box<Chart>),
    /// The driver reports no curve (level `manual`).
    NoCurve,
    /// The driver has the curve, but reading it failed.
    CurveUnreadable,
}

/// A curve with what the chart needs.
#[derive(Debug, Clone, PartialEq)]
pub struct Chart {
    pub points: Vec<CurvePoint>,
    /// The temperature the curve follows, when it is not the page's.
    pub own_source: Option<SourceRow>,
    /// The temperature now, in millidegrees.
    pub millidegrees: Option<i64>,
    /// The emergency threshold of the source in °C (B4).
    pub threshold: Option<u8>,
    /// The duty cycle the curve gives now, in automatic mode.
    pub target: Option<u8>,
    /// 100 % by the threshold (B14: CPU and pump).
    pub needs_full: bool,
}

/// What [`build`] needs.
pub struct Input<'a> {
    pub profile: &'a Profile,
    /// The Overview's model of the sample, with ranges since the start.
    pub groups: &'a [Group],
    /// The channels of the profile as read for the page.
    pub channels: &'a [ChannelReading],
    /// The readings the user turned off in the preferences.
    pub visibility: &'a Visibility,
}

/// The page for one sample.
pub fn build(input: &Input) -> Page {
    let profile = input.profile;
    let chip = &profile.hwmon.name;
    let row_of = |channel: String| {
        input
            .groups
            .iter()
            .flat_map(|g| g.rows.iter().chain(&g.auxiliary))
            .find(|r| r.id.chip == *chip && r.id.channel == channel)
    };
    let channels: Vec<Channel> = input
        .channels
        .iter()
        .map(|c| Channel {
            key: c.key.clone(),
            pwm: c.pwm,
            capabilities: c.capabilities,
            controllable: c.controllable,
        })
        .collect();
    let level = driver::driver_level(&channels);
    let source_row = |source: Source| SourceRow {
        source,
        temp: driver::source_temp(profile, source).cloned(),
        row: driver::source_temp(profile, source)
            .and_then(|t| row_of(format!("temp{}", t.input)))
            .cloned(),
        chip: chip.clone(),
    };
    // The page's source: the one most curves follow.
    let sources: Vec<Source> = input
        .channels
        .iter()
        .filter(|c| c.controllable)
        .filter_map(|c| c.source.clone()?.ok())
        .collect();
    let common = sources
        .iter()
        .max_by_key(|s| {
            (
                sources.iter().filter(|o| o == s).count(),
                // Ties: the first one.
                std::cmp::Reverse(sources.iter().position(|o| o == *s)),
            )
        })
        .copied();

    let mut fans = Vec::new();
    let mut read_only = Vec::new();
    for fan in &profile.fans {
        let Some(row) = row_of(format!("fan{}", fan.fan_index)) else {
            continue;
        };
        if !input.visibility.shows(&row.id) {
            continue;
        }
        let reading = fan
            .pwm_index
            .and_then(|pwm| input.channels.iter().find(|c| c.pwm == pwm));
        let kind = match reading {
            _ if level == DriverLevel::ReadOnly => FanKind::Low,
            Some(reading) if reading.controllable => full(fan, reading, common, &source_row),
            _ => {
                read_only.push(row.clone());
                continue;
            }
        };
        fans.push(Fan {
            key: fan.key.clone(),
            role: fan.role,
            row: row.clone(),
            header: fan.header.clone(),
            verified: fan.verified,
            min_percent: fan.min_duty(),
            duty: duty_of(row),
            kind,
        });
    }
    let source = common
        .filter(|_| level != DriverLevel::ReadOnly)
        .map(source_row);
    let cpu = profile
        .temps
        .iter()
        .find(|t| t.role == TempRole::Cpu)
        .map(|t| source_row(Source::Temp(t.input)));
    Page {
        level,
        temperature: source.clone().or(cpu),
        source,
        fans,
        read_only,
    }
}

/// A header the driver controls.
fn full(
    fan: &profile::Fan,
    reading: &ChannelReading,
    common: Option<Source>,
    source_row: &dyn Fn(Source) -> SourceRow,
) -> FanKind {
    let mode = reading.mode.clone().and_then(Result::ok);
    let chart = |points: &Vec<CurvePoint>| {
        let source = reading.source.clone().and_then(Result::ok);
        let row = source.map(source_row);
        let millidegrees = row.as_ref().and_then(SourceRow::millidegrees);
        Box::new(Chart {
            points: points.clone(),
            target: millidegrees
                .filter(|_| mode == Some(Mode::Auto))
                .and_then(|t| curve::expected_duty(points, t)),
            millidegrees,
            threshold: row.as_ref().and_then(|r| r.temp.as_ref()?.emergency_c),
            needs_full: matches!(fan.role, FanRole::Cpu | FanRole::Pump),
            own_source: row.filter(|r| Some(r.source) != common),
        })
    };
    let body = match &reading.curve {
        None => Body::NoCurve,
        Some(Err(_)) => Body::CurveUnreadable,
        Some(Ok(points)) => Body::Chart(chart(points)),
    };
    FanKind::Full {
        manual: mode == Some(Mode::Manual),
        body,
    }
}

/// The duty cycle of a fan's row, 0–255.
fn duty_of(row: &Row) -> Option<u8> {
    match row.subtitle {
        Subtitle::Header { duty: Some(d), .. } if d.is_finite() => {
            Some(d.round().clamp(0.0, 255.0) as u8)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fans::driver::Capability;
    use crate::fans::driver::model::{Attribute, ModelChannel, ModelDriver};
    use crate::overview::{GroupKind, Name, RowId};
    use crate::sensors::Kind;

    fn profile() -> Profile {
        profile::parse(include_str!("../../../../data/boards/ms-7e61.toml")).unwrap()
    }

    fn points(points: &[(u8, u8)]) -> Vec<CurvePoint> {
        let mut curve: Vec<CurvePoint> = points
            .iter()
            .map(|&(temp, duty)| CurvePoint { temp, duty })
            .collect();
        while curve.len() < 7 {
            curve.push(*curve.last().unwrap());
        }
        curve
    }

    /// The curves of MS-7E61 with the owner's BIOS settings (§6.3).
    fn curves() -> Vec<(u8, Vec<CurvePoint>)> {
        vec![
            (1, points(&[(0, 122), (75, 124), (80, 191), (85, 255)])),
            (2, points(&[(35, 255), (40, 255), (70, 255), (90, 255)])),
            (16, points(&[(0, 94), (5, 96), (84, 99), (85, 255)])),
            (15, points(&[(0, 104), (5, 107), (84, 109), (85, 255)])),
            (14, points(&[(40, 76), (55, 76), (70, 76), (85, 76)])),
            (13, points(&[(40, 102), (55, 102), (70, 102), (85, 102)])),
        ]
    }

    /// The driver of each level: Fedora's (duty cycles of pwm1, pwm2 and
    /// pwm4 only), `hwmon-next` (mode, curves to read) and v1 (curves to
    /// write); `manual` has the mode without curves.
    fn driver(level: DriverLevel) -> ModelDriver {
        let mut driver = ModelDriver::new();
        for (pwm, curve) in curves() {
            let read_only = level == DriverLevel::ReadOnly;
            driver = driver.with_channel(
                pwm,
                ModelChannel {
                    mode: (!read_only).then_some(Mode::Auto),
                    duty: (!read_only || pwm <= 2).then_some(100),
                    curve: (level >= DriverLevel::ManualCurveView).then_some(curve),
                    source: (level >= DriverLevel::ManualCurveView).then_some(Source::Temp(1)),
                    manual_writable: !read_only,
                    curve_writable: level >= DriverLevel::Curves,
                    ..ModelChannel::default()
                },
            );
        }
        driver.with_channel(
            4,
            ModelChannel {
                duty: Some(153),
                ..ModelChannel::default()
            },
        )
    }

    fn readings(driver: &ModelDriver) -> Vec<ChannelReading> {
        profile()
            .fans
            .iter()
            .filter_map(|fan| {
                let pwm = fan.pwm_index?;
                let capabilities = driver.capabilities(pwm);
                let channel = Channel {
                    key: fan.key.clone(),
                    pwm,
                    capabilities,
                    controllable: fan.controllable && capabilities.contains(Capability::Mode),
                };
                Some(ChannelReading::read(driver, &channel, None))
            })
            .collect()
    }

    fn row(channel: &str, name: Name, subtitle: Subtitle, kind: Kind, value: f64) -> Row {
        Row {
            id: RowId {
                chip: "nct6687".into(),
                device: None,
                channel: channel.into(),
            },
            role: name.clone(),
            name,
            subtitle,
            kind,
            value,
            rating: None,
            range: Some((value, value)),
        }
    }

    /// The Overview's rows of the sample: speeds, duty cycles where the
    /// driver reports them, temp1 at `temp` °C.
    fn groups(driver: &ModelDriver, temp: f64) -> Vec<Group> {
        let profile = profile();
        let fans = profile
            .fans
            .iter()
            .map(|fan| {
                let duty = fan
                    .pwm_index
                    .and_then(|pwm| driver.duty(pwm).ok())
                    .map(f64::from);
                let rpm = if fan.key == "pump1" || fan.key == "ezconn" {
                    0.0
                } else {
                    900.0
                };
                row(
                    &format!("fan{}", fan.fan_index),
                    Name::Fan {
                        role: fan.role,
                        number: fan.number,
                    },
                    Subtitle::Header {
                        header: fan.header.clone(),
                        duty,
                        verified: fan.verified,
                        mode: None,
                    },
                    Kind::Fan,
                    rpm,
                )
            })
            .collect();
        let temps = vec![row(
            "temp1",
            Name::Temp(TempRole::Cpu),
            Subtitle::Source {
                driver: "nct6687".into(),
                channel: "temp1".into(),
            },
            Kind::Temp,
            temp,
        )];
        vec![
            Group {
                kind: GroupKind::Board,
                description: None,
                rows: temps,
                auxiliary: Vec::new(),
            },
            Group {
                kind: GroupKind::Fans,
                description: None,
                rows: fans,
                auxiliary: Vec::new(),
            },
        ]
    }

    fn page_of(driver: &ModelDriver, visibility: &Visibility) -> Page {
        let profile = profile();
        let groups = groups(driver, 46.0);
        let channels = readings(driver);
        build(&Input {
            profile: &profile,
            groups: &groups,
            channels: &channels,
            visibility,
        })
    }

    fn page(level: DriverLevel) -> Page {
        page_of(&driver(level), &Visibility::default())
    }

    fn keys(fans: &[Fan]) -> Vec<&str> {
        fans.iter().map(|c| c.key.as_str()).collect()
    }

    fn fan<'a>(page: &'a Page, key: &str) -> &'a Fan {
        page.fans.iter().find(|c| c.key == key).unwrap()
    }

    #[test]
    fn driver_without_control() {
        let page = page(DriverLevel::ReadOnly);
        assert_eq!(page.level, DriverLevel::ReadOnly);
        // Every header without control, the auxiliary fan too; no source.
        assert_eq!(
            keys(&page.fans),
            ["cpu1", "pump1", "sys1", "sys2", "sys3", "sys4", "ezconn"]
        );
        assert!(page.fans.iter().all(|c| c.kind == FanKind::Low));
        assert!(page.read_only.is_empty());
        assert_eq!(page.source, None);
        // On top the processor's temperature of the profile (D62).
        let temperature = page.temperature.as_ref().unwrap();
        assert_eq!(temperature.source, Source::Temp(1));
        assert_eq!(temperature.temp.as_ref().unwrap().role, TempRole::Cpu);
        assert_eq!(temperature.millidegrees(), Some(46_000));
        assert_eq!(fan(&page, "cpu1").duty, Some(100));
        assert_eq!(fan(&page, "sys1").duty, None);
    }

    #[test]
    fn curves_to_read() {
        let page = page(DriverLevel::ManualCurveView);
        // The auxiliary fan has no control: a row of its own.
        assert_eq!(
            keys(&page.fans),
            ["cpu1", "pump1", "sys1", "sys2", "sys3", "sys4"]
        );
        assert_eq!(page.read_only.len(), 1);
        assert_eq!(page.read_only[0].id.channel, "fan4");
        let source = page.source.as_ref().unwrap();
        assert_eq!(source.source, Source::Temp(1));
        assert_eq!(source.temp.as_ref().unwrap().emergency_c, Some(95));
        assert_eq!(source.millidegrees(), Some(46_000));
        // On top the source of the curves.
        assert_eq!(page.temperature.as_ref(), Some(source));
        let cpu = fan(&page, "cpu1");
        let FanKind::Full {
            manual: false,
            body,
        } = &cpu.kind
        else {
            panic!("{cpu:?}");
        };
        let Body::Chart(chart) = body else {
            panic!("{body:?}");
        };
        assert_eq!(chart.points.len(), 7);
        assert_eq!(chart.threshold, Some(95));
        assert!(chart.needs_full);
        assert_eq!(chart.own_source, None);
        // At 46 °C the CPU curve gives 124 (123.2 rounded up).
        assert_eq!(chart.target, Some(124));
        assert_eq!(cpu.min_percent, 30);
        // The pump, not checked on this board, shows its curve too.
        let pump = fan(&page, "pump1");
        assert!(!pump.verified);
        assert!(matches!(
            pump.kind,
            FanKind::Full {
                body: Body::Chart(_),
                ..
            }
        ));
        assert!(!fan(&page, "sys1").chart().unwrap().needs_full);
        assert!(page.fans.iter().all(Fan::has_details));
    }

    #[test]
    fn manual_level_and_manual_mode() {
        let page = page(DriverLevel::Manual);
        assert_eq!(page.level, DriverLevel::Manual);
        assert_eq!(page.source, None);
        // Without curves, the processor's temperature on top.
        assert_eq!(
            page.temperature.as_ref().map(|t| t.source),
            Some(Source::Temp(1))
        );
        let sys3 = fan(&page, "sys3");
        assert!(matches!(
            sys3.kind,
            FanKind::Full {
                manual: false,
                body: Body::NoCurve,
            }
        ));
        assert!(!sys3.has_details());
        // A channel in manual mode: the curve sets nothing, but it stays in
        // the controller, so its details can be shown.
        let mut driver = driver(DriverLevel::Curves);
        driver.channel_mut(14).unwrap().mode = Some(Mode::Manual);
        let page = page_of(&driver, &Visibility::default());
        let sys3 = fan(&page, "sys3");
        assert!(matches!(
            sys3.kind,
            FanKind::Full {
                manual: true,
                body: Body::Chart(_),
            }
        ));
        assert_eq!(sys3.chart(), None);
        let kept = sys3.curve().unwrap();
        assert_eq!(
            kept.points,
            points(&[(40, 76), (55, 76), (70, 76), (85, 76)])
        );
        assert_eq!(kept.target, None);
        assert_eq!(sys3.moving_to(), None);
        // Manual mode on a driver without curves: nothing to show.
        let mut driver = self::driver(DriverLevel::Manual);
        driver.channel_mut(14).unwrap().mode = Some(Mode::Manual);
        let page = page_of(&driver, &Visibility::default());
        let sys3 = fan(&page, "sys3");
        assert!(matches!(
            sys3.kind,
            FanKind::Full {
                manual: true,
                body: Body::NoCurve,
            }
        ));
        assert!(!sys3.has_details());
    }

    #[test]
    fn moving_to_the_target() {
        // SYS_FAN1 at 46 °C: the curve gives 98, the driver reports 76.
        let mut driver = driver(DriverLevel::ManualCurveView);
        driver.channel_mut(16).unwrap().duty = Some(76);
        let page = page_of(&driver, &Visibility::default());
        assert_eq!(fan(&page, "sys1").moving_to(), Some(98));
        // Settled: the same in whole percent.
        driver.channel_mut(16).unwrap().duty = Some(98);
        let page = page_of(&driver, &Visibility::default());
        assert_eq!(fan(&page, "sys1").moving_to(), None);
        // In manual mode the curve sets nothing.
        driver.channel_mut(16).unwrap().mode = Some(Mode::Manual);
        driver.channel_mut(16).unwrap().duty = Some(76);
        let page = page_of(&driver, &Visibility::default());
        assert_eq!(fan(&page, "sys1").moving_to(), None);
    }

    #[test]
    fn empty_header_and_fan_that_does_not_spin() {
        let mut driver = driver(DriverLevel::ManualCurveView);
        // The pump: 0 rpm since the start at 100/255.
        let page = page_of(&driver, &Visibility::default());
        assert!(fan(&page, "pump1").maybe_empty());
        assert!(!fan(&page, "sys1").maybe_empty());
        // At 0 % a fan does not spin on purpose.
        driver.channel_mut(2).unwrap().duty = Some(0);
        let page = page_of(&driver, &Visibility::default());
        assert!(!fan(&page, "pump1").maybe_empty());
        // A fan that spun earlier is not an empty header.
        let mut fan = fan(&page, "sys1").clone();
        fan.row.value = 0.0;
        fan.row.range = Some((0.0, 880.0));
        fan.duty = Some(100);
        assert!(fan.spun() && !fan.maybe_empty());
    }

    #[test]
    fn a_curve_with_its_own_source() {
        let mut driver = driver(DriverLevel::ManualCurveView);
        driver.channel_mut(13).unwrap().source = Some(Source::Temp(2));
        let page = page_of(&driver, &Visibility::default());
        // Most curves follow temp1, so the page's source stays temp1.
        assert_eq!(page.source.as_ref().unwrap().source, Source::Temp(1));
        let own = fan(&page, "sys4").chart().unwrap().own_source.as_ref();
        let own = own.unwrap();
        assert_eq!(own.source, Source::Temp(2));
        // The system temperature has its own threshold (80 °C).
        assert_eq!(fan(&page, "sys4").chart().unwrap().threshold, Some(80));
        assert_eq!(fan(&page, "sys1").chart().unwrap().own_source, None);
    }

    #[test]
    fn errors_and_hidden_headers() {
        let mut driver = driver(DriverLevel::ManualCurveView);
        driver
            .channel_mut(15)
            .unwrap()
            .failing
            .push((Attribute::Curve, 5));
        driver
            .channel_mut(14)
            .unwrap()
            .failing
            .push((Attribute::Mode, 5));
        let mut visibility = Visibility::default();
        visibility.hidden.insert("nct6687/fan13".into());
        let page = page_of(&driver, &visibility);
        // The read error is kept apart from a driver without curves, and
        // the details say it.
        let sys2 = fan(&page, "sys2");
        assert!(matches!(
            sys2.kind,
            FanKind::Full {
                body: Body::CurveUnreadable,
                ..
            }
        ));
        assert!(sys2.curve_unreadable() && sys2.has_details());
        assert_eq!(sys2.curve(), None);
        // A mode that cannot be read: the curve shows, but without a
        // target.
        let sys3 = fan(&page, "sys3");
        assert_eq!(sys3.chart().unwrap().target, None);
        // Turned off in the preferences: no row.
        assert!(!keys(&page.fans).contains(&"sys4"));
        // In manual mode too, the read error stays.
        driver.channel_mut(15).unwrap().mode = Some(Mode::Manual);
        let page = page_of(&driver, &visibility);
        assert!(fan(&page, "sys2").curve_unreadable());
    }

    #[test]
    fn reading_keeps_the_curve_read_earlier() {
        let driver = driver(DriverLevel::ManualCurveView);
        let profile = profile();
        let fan = profile.fans.iter().find(|f| f.key == "sys1").unwrap();
        let channel = Channel {
            key: fan.key.clone(),
            pwm: 16,
            capabilities: driver.capabilities(16),
            controllable: true,
        };
        let first = ChannelReading::read(&driver, &channel, None);
        assert_eq!(first.mode, Some(Ok(Mode::Auto)));
        let kept = Some((Some(Ok(points(&[(10, 10)]))), None));
        let again = ChannelReading::read(&driver, &channel, kept);
        assert_eq!(again.curve, Some(Ok(points(&[(10, 10)]))));
        assert_eq!(again.source, None);
    }
}
