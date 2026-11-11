// SPDX-License-Identifier: GPL-3.0-or-later

//! The one place that knows the sysfs ABI of the `nct6683` driver
//! (docs/PROJEKT.md §3.7, §5.5.1): what the driver allows for each channel
//! (§5.5.2), and reading its mode, duty cycle, curve and curve source
//! through [`FanDriver`], from sysfs ([`SysfsDriver`]) or from a model in
//! memory for tests ([`model::ModelDriver`]). Writing comes with the helper
//! (M2b), and only the helper will have it (B1).
//!
//! Detection looks only at the attributes, separately for every channel:
//! whether a file exists and whether its owner write bit is set. It uses
//! the file mode (`stat`), not `access(W_OK)`, because the app does not run
//! as root and must reach the same result as the helper. It never looks at
//! the kernel version.
//!
//! The ABI as the driver documents it: `pwmN_enable` is 1 (manual) or 2
//! (automatic, the current curve); `pwmN` and `pwmN_auto_pointK_pwm` are
//! 0–255; `pwmN_auto_pointK_temp` is in millidegrees Celsius;
//! `pwmN_auto_channels_temp` is a bitmap of temperature channels (1 is
//! temp1, 2 is temp2, 4 is temp3) and fails with ENODATA when the curve
//! follows a temperature the driver does not report. Anything else is
//! [`ReadError::Invalid`], never a guess.

pub mod model;

use std::collections::BTreeSet;
use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::profile::{Profile, Temp};

/// What the driver allows for one channel (§5.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// `pwmN` exists.
    DutyRead,
    /// `pwmN_enable` exists and is writable.
    Mode,
    /// `Mode`, and `pwmN` is writable.
    DutyWrite,
    /// `pwmN_auto_pointK_temp` and `_pwm` exist for K = 1…n.
    CurveRead,
    /// `CurveRead`, and all those files are writable.
    CurveWrite,
    /// `pwmN_auto_channels_temp` exists.
    SourceRead,
    /// `SourceRead`, and the file is writable (used only as D17 says).
    SourceWrite,
}

impl Capability {
    pub const ALL: [Self; 7] = [
        Self::DutyRead,
        Self::Mode,
        Self::DutyWrite,
        Self::CurveRead,
        Self::CurveWrite,
        Self::SourceRead,
        Self::SourceWrite,
    ];

    /// The name used over D-Bus (§3.5).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DutyRead => "duty-read",
            Self::Mode => "mode",
            Self::DutyWrite => "duty-write",
            Self::CurveRead => "curve-read",
            Self::CurveWrite => "curve-write",
            Self::SourceRead => "source-read",
            Self::SourceWrite => "source-write",
        }
    }

    fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// The capabilities of one channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    set: u8,
    curve_points: u8,
}

impl Capabilities {
    /// Detects the capabilities of channel `pwm` (N of `pwmN`) from the
    /// attributes in the hwmon folder `hwmon`. Only reads file metadata.
    pub fn detect(hwmon: &Path, pwm: u8) -> Self {
        let attr = |suffix: &str| Attr::of(&hwmon.join(format!("pwm{pwm}{suffix}")));
        let duty = attr("");
        let enable = attr("_enable");
        let source = attr("_auto_channels_temp");
        let mut caps = Self::default();
        caps.insert_if(Capability::DutyRead, duty != Attr::Missing);
        caps.insert_if(Capability::Mode, enable == Attr::Writable);
        caps.insert_if(
            Capability::DutyWrite,
            enable == Attr::Writable && duty == Attr::Writable,
        );
        if let Some(curve) = Curve::find(hwmon, pwm) {
            caps.curve_points = curve.points;
            caps.insert(Capability::CurveRead);
            caps.insert_if(Capability::CurveWrite, curve.writable);
        }
        caps.insert_if(Capability::SourceRead, source != Attr::Missing);
        caps.insert_if(Capability::SourceWrite, source == Attr::Writable);
        caps
    }

    pub fn contains(self, capability: Capability) -> bool {
        self.set & capability.bit() != 0
    }

    /// The capabilities present, in the order of [`Capability::ALL`].
    pub fn iter(self) -> impl Iterator<Item = Capability> {
        Capability::ALL
            .into_iter()
            .filter(move |c| self.contains(*c))
    }

    /// The number of curve points (n of §5.5.2); 0 without `CurveRead`.
    pub fn curve_points(self) -> u8 {
        self.curve_points
    }

    /// The level of this channel alone (§5.5.2).
    pub fn level(self) -> DriverLevel {
        let has = |c| self.contains(c);
        if !(has(Capability::Mode) && has(Capability::DutyWrite)) {
            DriverLevel::ReadOnly
        } else if !has(Capability::CurveRead) {
            DriverLevel::Manual
        } else if !has(Capability::CurveWrite) {
            DriverLevel::ManualCurveView
        } else if !has(Capability::SourceWrite) {
            DriverLevel::Curves
        } else {
            DriverLevel::CurvesSource
        }
    }

    fn insert(&mut self, capability: Capability) {
        self.set |= capability.bit();
    }

    fn insert_if(&mut self, capability: Capability, condition: bool) {
        if condition {
            self.insert(capability);
        }
    }
}

/// What the driver allows, from lowest to highest (§5.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DriverLevel {
    /// No mode switch: Fedora's kernel 7.3.3.
    ReadOnly,
    /// Mode and duty cycle, no curves: a kernel with only part of the
    /// `hwmon-next` changes, e.g. taken over by a distribution.
    Manual,
    /// Also reads the curves: `hwmon-next`.
    ManualCurveView,
    /// Also writes the curves: `hwmon-next` with v1.
    Curves,
    /// Also writes the temperature source of the curves: after patch 5.
    CurvesSource,
}

impl DriverLevel {
    /// The name used over D-Bus (§3.5).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Manual => "manual",
            Self::ManualCurveView => "manual+curve-view",
            Self::Curves => "curves",
            Self::CurvesSource => "curves+source",
        }
    }
}

/// A fan channel of the profile with the capabilities found for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    /// The key of the channel in the profile, e.g. `sys1`.
    pub key: String,
    /// N of `pwmN`.
    pub pwm: u8,
    pub capabilities: Capabilities,
    /// The profile marks the channel controllable and it has `Mode`.
    /// Otherwise the channel is read-only, even with writable files.
    pub controllable: bool,
}

/// The fan channels of `profile` that have a `pwm`, in profile order, with
/// their capabilities in the hwmon folder `hwmon`.
pub fn channels(profile: &Profile, hwmon: &Path) -> Vec<Channel> {
    profile
        .fans
        .iter()
        .filter_map(|fan| {
            let pwm = fan.pwm_index?;
            let capabilities = Capabilities::detect(hwmon, pwm);
            Some(Channel {
                key: fan.key.clone(),
                pwm,
                capabilities,
                controllable: fan.controllable && capabilities.contains(Capability::Mode),
            })
        })
        .collect()
}

/// The summary for the interface: the lowest level common to the
/// controllable channels, or `ReadOnly` if no channel is controllable.
pub fn driver_level(channels: &[Channel]) -> DriverLevel {
    channels
        .iter()
        .filter(|c| c.controllable)
        .map(|c| c.capabilities.level())
        .min()
        .unwrap_or(DriverLevel::ReadOnly)
}

/// The mode of a channel, from `pwmN_enable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 1: a fixed duty cycle, set with `pwmN`.
    Manual,
    /// 2: the controller runs the current curve.
    Auto,
}

/// One point of a curve in the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurvePoint {
    /// In whole °C; the driver reports millidegrees.
    pub temp: u8,
    /// Duty cycle, 0–255.
    pub duty: u8,
}

/// The temperature a curve follows, from `pwmN_auto_channels_temp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// One temperature the driver reports: N of `tempN_input`.
    Temp(u8),
    /// No temperature, or several at once: the bitmap as read (bit 0 is
    /// temp1). The interface shows it, but cannot name one source.
    Mask(u32),
    /// A temperature the driver does not report (the read fails with
    /// ENODATA).
    Unreported,
}

/// Why an attribute could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The attribute does not exist.
    Missing,
    /// The read failed with this errno, e.g. EIO when the controller did
    /// not answer.
    Os(i32),
    /// The attribute holds a value its ABI does not allow.
    Invalid { attribute: String, text: String },
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => write!(f, "attribute missing"),
            Self::Os(errno) => write!(f, "{}", std::io::Error::from_raw_os_error(*errno)),
            Self::Invalid { attribute, text } => {
                write!(f, "unexpected value in {attribute}: {text:?}")
            }
        }
    }
}

impl std::error::Error for ReadError {}

/// What one channel reports now. `None`: the driver does not have the
/// attribute; `Some(Err(…))`: it has it, but the read failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub mode: Option<Result<Mode, ReadError>>,
    pub duty: Option<Result<u8, ReadError>>,
    pub curve: Option<Result<Vec<CurvePoint>, ReadError>>,
    pub source: Option<Result<Source, ReadError>>,
}

/// The fan attributes of the driver, for one hwmon device (§3.7). The app,
/// the helper, the rules of §4 and the tests use only this trait, so a
/// change of the driver ABI touches only its implementations.
pub trait FanDriver {
    /// What the driver allows for channel `pwm` (N of `pwmN`).
    fn capabilities(&self, pwm: u8) -> Capabilities;
    fn mode(&self, pwm: u8) -> Result<Mode, ReadError>;
    fn duty(&self, pwm: u8) -> Result<u8, ReadError>;
    /// The n points of the curve, n from [`Capabilities::curve_points`];
    /// [`ReadError::Missing`] without `CurveRead`.
    fn curve(&self, pwm: u8) -> Result<Vec<CurvePoint>, ReadError>;
    fn source(&self, pwm: u8) -> Result<Source, ReadError>;
}

/// Reads everything channel `pwm` has. Every attribute is read once, and
/// the curve only when the driver reports one; each read of a curve point
/// goes to the controller, outside the driver's cache, so callers read
/// curves seldom (§7 M2, M2.4).
pub fn read(driver: &dyn FanDriver, pwm: u8) -> Reading {
    fn present<T>(result: Result<T, ReadError>) -> Option<Result<T, ReadError>> {
        match result {
            Err(ReadError::Missing) => None,
            other => Some(other),
        }
    }
    Reading {
        mode: present(driver.mode(pwm)),
        duty: present(driver.duty(pwm)),
        curve: present(driver.curve(pwm)),
        source: present(driver.source(pwm)),
    }
}

/// The temperature of `profile` that a curve follows, when the source is
/// one temperature the profile names (e.g. temp1 → `cpu`, with its
/// emergency threshold).
pub fn source_temp(profile: &Profile, source: Source) -> Option<&Temp> {
    match source {
        Source::Temp(input) => profile.temps.iter().find(|t| t.input == input),
        Source::Mask(_) | Source::Unreported => None,
    }
}

/// The fan attributes in sysfs, read only. The app reads sysfs itself;
/// only the helper will write (B1), and writing is not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysfsDriver {
    hwmon: PathBuf,
}

impl SysfsDriver {
    /// The driver of the hwmon device in folder `hwmon`.
    pub fn new(hwmon: impl Into<PathBuf>) -> Self {
        Self {
            hwmon: hwmon.into(),
        }
    }

    pub fn hwmon(&self) -> &Path {
        &self.hwmon
    }

    fn text(&self, attribute: &str) -> Result<String, ReadError> {
        read_text(&self.hwmon.join(attribute), attribute)
    }
}

impl FanDriver for SysfsDriver {
    fn capabilities(&self, pwm: u8) -> Capabilities {
        Capabilities::detect(&self.hwmon, pwm)
    }

    fn mode(&self, pwm: u8) -> Result<Mode, ReadError> {
        let attribute = format!("pwm{pwm}_enable");
        parse_mode(&attribute, &self.text(&attribute)?)
    }

    fn duty(&self, pwm: u8) -> Result<u8, ReadError> {
        let attribute = format!("pwm{pwm}");
        parse_duty(&attribute, &self.text(&attribute)?)
    }

    fn curve(&self, pwm: u8) -> Result<Vec<CurvePoint>, ReadError> {
        let points = self.capabilities(pwm).curve_points();
        if points == 0 {
            return Err(ReadError::Missing);
        }
        (1..=points)
            .map(|k| {
                let temp = format!("pwm{pwm}_auto_point{k}_temp");
                let duty = format!("pwm{pwm}_auto_point{k}_pwm");
                Ok(CurvePoint {
                    temp: parse_temp(&temp, &self.text(&temp)?)?,
                    duty: parse_duty(&duty, &self.text(&duty)?)?,
                })
            })
            .collect()
    }

    fn source(&self, pwm: u8) -> Result<Source, ReadError> {
        let attribute = format!("pwm{pwm}_auto_channels_temp");
        source_from(&attribute, self.text(&attribute))
    }
}

/// ENODATA in the generic Linux errno table, which x86 and Arm use.
const ENODATA: i32 = 61;

fn read_text(path: &Path, attribute: &str) -> Result<String, ReadError> {
    std::fs::read_to_string(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => ReadError::Missing,
        std::io::ErrorKind::InvalidData => invalid(attribute, "(not UTF-8)"),
        _ => ReadError::Os(e.raw_os_error().unwrap_or(-1)),
    })
}

fn invalid(attribute: &str, text: &str) -> ReadError {
    ReadError::Invalid {
        attribute: attribute.to_owned(),
        text: text.trim().to_owned(),
    }
}

fn number(attribute: &str, text: &str) -> Result<i64, ReadError> {
    text.trim().parse().map_err(|_| invalid(attribute, text))
}

fn parse_mode(attribute: &str, text: &str) -> Result<Mode, ReadError> {
    match number(attribute, text)? {
        1 => Ok(Mode::Manual),
        2 => Ok(Mode::Auto),
        _ => Err(invalid(attribute, text)),
    }
}

fn parse_duty(attribute: &str, text: &str) -> Result<u8, ReadError> {
    u8::try_from(number(attribute, text)?).map_err(|_| invalid(attribute, text))
}

/// Millidegrees to whole °C. The driver reports whole degrees times 1000;
/// anything else, or a value outside 0–255 °C, is not its ABI.
fn parse_temp(attribute: &str, text: &str) -> Result<u8, ReadError> {
    let millidegrees = number(attribute, text)?;
    if millidegrees % 1000 != 0 {
        return Err(invalid(attribute, text));
    }
    u8::try_from(millidegrees / 1000).map_err(|_| invalid(attribute, text))
}

fn source_from(attribute: &str, text: Result<String, ReadError>) -> Result<Source, ReadError> {
    let text = match text {
        Err(ReadError::Os(ENODATA)) => return Ok(Source::Unreported),
        other => other?,
    };
    let bits = u32::try_from(number(attribute, &text)?).map_err(|_| invalid(attribute, &text))?;
    Ok(if bits.count_ones() == 1 {
        // At most 32, so it fits.
        Source::Temp(bits.trailing_zeros() as u8 + 1)
    } else {
        Source::Mask(bits)
    })
}

/// The state of one attribute file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attr {
    Missing,
    ReadOnly,
    /// The owner write bit is set.
    Writable,
}

impl Attr {
    fn of(path: &Path) -> Self {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_file() => {
                if meta.permissions().mode() & 0o200 != 0 {
                    Self::Writable
                } else {
                    Self::ReadOnly
                }
            }
            _ => Self::Missing,
        }
    }
}

/// The curve attributes of one channel.
struct Curve {
    points: u8,
    writable: bool,
}

impl Curve {
    /// The curve of channel `pwm`, if `pwmN_auto_pointK_temp` and `_pwm`
    /// exist for exactly K = 1…n. Any other point number (0, a leading
    /// zero, a gap) means an ABI this code does not know: no curve.
    fn find(hwmon: &Path, pwm: u8) -> Option<Self> {
        let prefix = format!("pwm{pwm}_auto_point");
        let mut temps = BTreeSet::new();
        let mut duties = BTreeSet::new();
        let mut writable = true;
        for entry in std::fs::read_dir(hwmon).ok()?.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let Some((digits, what)) = name
                .strip_prefix(&prefix)
                .and_then(|rest| rest.split_once('_'))
            else {
                continue;
            };
            let set = match what {
                "temp" => &mut temps,
                "pwm" => &mut duties,
                // e.g. `_temp_hyst`, which this driver does not have.
                _ => continue,
            };
            let canonical = !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit());
            let point = digits.parse::<u8>().ok().filter(|_| canonical)?;
            match Attr::of(&entry.path()) {
                Attr::Missing => continue,
                Attr::ReadOnly => writable = false,
                Attr::Writable => {}
            }
            set.insert(point);
        }
        let points = u8::try_from(temps.len()).ok()?;
        let complete = points > 0 && temps == duties && temps.last() == Some(&points);
        complete.then_some(Self { points, writable })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A fresh hwmon folder for one test, removed at the end.
    struct Hwmon(PathBuf);

    impl Hwmon {
        fn new(test: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("baseboard-driver-{}-{test}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// Creates or replaces a file with the given mode.
        fn file(&self, name: &str, mode: u32) -> &Self {
            let path = self.0.join(name);
            let _ = std::fs::remove_file(&path);
            std::fs::write(&path, "0\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            self
        }

        fn curve(&self, pwm: u8, points: u8, mode: u32) -> &Self {
            for k in 1..=points {
                self.file(&format!("pwm{pwm}_auto_point{k}_temp"), mode);
                self.file(&format!("pwm{pwm}_auto_point{k}_pwm"), mode);
            }
            self
        }

        fn caps(&self, pwm: u8) -> Capabilities {
            Capabilities::detect(&self.0, pwm)
        }

        /// Creates or replaces a read-only file with the given content.
        fn write(&self, name: &str, content: &str) -> &Self {
            let path = self.0.join(name);
            let _ = std::fs::remove_file(&path);
            std::fs::write(&path, content).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
            self
        }

        /// A curve of channel `pwm` with these points, as the driver writes
        /// them: millidegrees and 0–255.
        fn curve_values(&self, pwm: u8, points: &[(u32, u8)]) -> &Self {
            for (k, (temp, duty)) in points.iter().enumerate() {
                let k = k + 1;
                self.write(
                    &format!("pwm{pwm}_auto_point{k}_temp"),
                    &format!("{}\n", temp * 1000),
                );
                self.write(&format!("pwm{pwm}_auto_point{k}_pwm"), &format!("{duty}\n"));
            }
            self
        }

        fn driver(&self) -> SysfsDriver {
            SysfsDriver::new(&self.0)
        }
    }

    /// SYS_FAN1 on MS-7E61 with the owner's BIOS settings (§6.3).
    const SYS1: [(u32, u8); 7] = [
        (0, 94),
        (5, 96),
        (84, 99),
        (85, 255),
        (85, 255),
        (85, 255),
        (85, 255),
    ];

    fn points(points: &[(u32, u8)]) -> Vec<CurvePoint> {
        points
            .iter()
            .map(|&(temp, duty)| CurvePoint {
                temp: u8::try_from(temp).unwrap(),
                duty,
            })
            .collect()
    }

    fn is_invalid<T: std::fmt::Debug>(result: Result<T, ReadError>) -> bool {
        matches!(result, Err(ReadError::Invalid { .. }))
    }

    impl Drop for Hwmon {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(caps: Capabilities) -> Vec<&'static str> {
        caps.iter().map(Capability::as_str).collect()
    }

    #[test]
    fn names_follow_the_dbus_contract() {
        let all: Vec<_> = Capability::ALL.iter().map(|c| c.as_str()).collect();
        assert_eq!(
            all,
            [
                "duty-read",
                "mode",
                "duty-write",
                "curve-read",
                "curve-write",
                "source-read",
                "source-write"
            ]
        );
        let levels = [
            DriverLevel::ReadOnly,
            DriverLevel::Manual,
            DriverLevel::ManualCurveView,
            DriverLevel::Curves,
            DriverLevel::CurvesSource,
        ];
        let names: Vec<_> = levels.iter().map(|l| l.as_str()).collect();
        assert_eq!(
            names,
            [
                "read-only",
                "manual",
                "manual+curve-view",
                "curves",
                "curves+source"
            ]
        );
        assert!(levels.is_sorted());
    }

    #[test]
    fn nothing_without_attributes() {
        let hwmon = Hwmon::new("empty");
        assert_eq!(hwmon.caps(1), Capabilities::default());
        assert_eq!(hwmon.caps(1).level(), DriverLevel::ReadOnly);
        // A missing folder is the same.
        assert_eq!(
            Capabilities::detect(&hwmon.0.join("missing"), 1),
            Capabilities::default()
        );
    }

    #[test]
    fn mode_and_duty_need_the_owner_write_bit() {
        let hwmon = Hwmon::new("duty");
        hwmon.file("pwm1", 0o444);
        assert_eq!(names(hwmon.caps(1)), ["duty-read"]);
        // Group and other write bits do not count.
        hwmon.file("pwm1_enable", 0o466);
        assert_eq!(names(hwmon.caps(1)), ["duty-read"]);
        // Mode without a writable duty cycle is still read-only.
        hwmon.file("pwm1_enable", 0o644);
        assert_eq!(names(hwmon.caps(1)), ["duty-read", "mode"]);
        assert_eq!(hwmon.caps(1).level(), DriverLevel::ReadOnly);
        hwmon.file("pwm1", 0o644);
        assert_eq!(names(hwmon.caps(1)), ["duty-read", "mode", "duty-write"]);
        assert_eq!(hwmon.caps(1).level(), DriverLevel::Manual);

        // A writable duty cycle without mode gives no duty-write.
        hwmon.file("pwm2", 0o644);
        assert_eq!(names(hwmon.caps(2)), ["duty-read"]);
    }

    #[test]
    fn curve_needs_every_point() {
        let hwmon = Hwmon::new("points");
        hwmon.curve(1, 7, 0o444);
        assert_eq!(hwmon.caps(1).curve_points(), 7);
        assert!(hwmon.caps(1).contains(Capability::CurveRead));

        // A missing `_pwm` of one point.
        hwmon.curve(2, 7, 0o444);
        std::fs::remove_file(hwmon.0.join("pwm2_auto_point4_pwm")).unwrap();
        // A gap: points 1, 2 and 4.
        hwmon.curve(3, 2, 0o444);
        hwmon.file("pwm3_auto_point4_temp", 0o444);
        hwmon.file("pwm3_auto_point4_pwm", 0o444);
        // Only temperatures.
        for k in 1..=7 {
            hwmon.file(&format!("pwm4_auto_point{k}_temp"), 0o444);
        }
        // A point 0, and a point with a leading zero.
        hwmon.curve(5, 2, 0o444);
        hwmon.file("pwm5_auto_point0_temp", 0o444);
        hwmon.file("pwm5_auto_point0_pwm", 0o444);
        hwmon.curve(6, 2, 0o444);
        hwmon.file("pwm6_auto_point03_temp", 0o444);
        hwmon.file("pwm6_auto_point03_pwm", 0o444);
        for pwm in 2..=6 {
            assert!(!hwmon.caps(pwm).contains(Capability::CurveRead), "pwm{pwm}");
            assert_eq!(hwmon.caps(pwm).curve_points(), 0, "pwm{pwm}");
        }
        // Other attributes of a point are ignored.
        hwmon.file("pwm1_auto_point1_temp_hyst", 0o444);
        assert_eq!(hwmon.caps(1).curve_points(), 7);
    }

    #[test]
    fn curves_of_other_channels_do_not_count() {
        let hwmon = Hwmon::new("prefix");
        hwmon.file("pwm1", 0o444);
        hwmon.curve(13, 7, 0o644);
        assert_eq!(names(hwmon.caps(1)), ["duty-read"]);
        assert_eq!(hwmon.caps(13).curve_points(), 7);
    }

    #[test]
    fn curve_write_needs_every_file_writable() {
        let hwmon = Hwmon::new("curve-write");
        hwmon.curve(1, 7, 0o644);
        assert!(hwmon.caps(1).contains(Capability::CurveWrite));
        hwmon.file("pwm1_auto_point7_pwm", 0o444);
        let caps = hwmon.caps(1);
        assert!(caps.contains(Capability::CurveRead));
        assert!(!caps.contains(Capability::CurveWrite));
    }

    #[test]
    fn source() {
        let hwmon = Hwmon::new("source");
        hwmon.file("pwm1_auto_channels_temp", 0o444);
        assert_eq!(names(hwmon.caps(1)), ["source-read"]);
        hwmon.file("pwm1_auto_channels_temp", 0o644);
        assert_eq!(names(hwmon.caps(1)), ["source-read", "source-write"]);
    }

    #[test]
    fn levels_of_one_channel() {
        let hwmon = Hwmon::new("levels");
        hwmon
            .file("pwm1", 0o644)
            .file("pwm1_enable", 0o644)
            .file("pwm1_auto_channels_temp", 0o444);
        assert_eq!(hwmon.caps(1).level(), DriverLevel::Manual);
        hwmon.curve(1, 7, 0o444);
        assert_eq!(hwmon.caps(1).level(), DriverLevel::ManualCurveView);
        hwmon.curve(1, 7, 0o644);
        assert_eq!(hwmon.caps(1).level(), DriverLevel::Curves);
        hwmon.file("pwm1_auto_channels_temp", 0o644);
        assert_eq!(hwmon.caps(1).level(), DriverLevel::CurvesSource);
        // Without mode, nothing else counts.
        hwmon.file("pwm1_enable", 0o444);
        assert_eq!(hwmon.caps(1).level(), DriverLevel::ReadOnly);
    }

    #[test]
    fn profile_decides_what_is_controllable() {
        let profile =
            crate::profile::parse(include_str!("../../../../data/boards/ms-7e61.toml")).unwrap();
        let hwmon = Hwmon::new("profile");
        // cpu1 (pwm1) can be controlled; the auxiliary fan (pwm4) has the
        // same files, but the profile does not allow it.
        for pwm in [1, 4] {
            hwmon
                .file(&format!("pwm{pwm}"), 0o644)
                .file(&format!("pwm{pwm}_enable"), 0o644);
        }
        hwmon.file("pwm2", 0o444);
        let channels = channels(&profile, &hwmon.0);
        let keys: Vec<_> = channels.iter().map(|c| (c.key.as_str(), c.pwm)).collect();
        assert_eq!(
            keys,
            [
                ("cpu1", 1),
                ("pump1", 2),
                ("sys1", 16),
                ("sys2", 15),
                ("sys3", 14),
                ("sys4", 13),
                ("ezconn", 4)
            ]
        );
        let controllable: Vec<_> = channels
            .iter()
            .filter(|c| c.controllable)
            .map(|c| c.key.as_str())
            .collect();
        assert_eq!(controllable, ["cpu1"]);
        assert_eq!(channels[6].capabilities.level(), DriverLevel::Manual);
        assert_eq!(driver_level(&channels), DriverLevel::Manual);
    }

    #[test]
    fn summary_is_the_lowest_controllable_level() {
        let hwmon = Hwmon::new("summary");
        hwmon.file("pwm1", 0o644).file("pwm1_enable", 0o644);
        hwmon.curve(1, 7, 0o644);
        hwmon.file("pwm2", 0o644).file("pwm2_enable", 0o644);
        hwmon.curve(2, 7, 0o444);
        let channel = |key: &str, pwm: u8, controllable: bool| Channel {
            key: key.into(),
            pwm,
            capabilities: hwmon.caps(pwm),
            controllable,
        };
        assert_eq!(driver_level(&[]), DriverLevel::ReadOnly);
        assert_eq!(driver_level(&[channel("a", 1, true)]), DriverLevel::Curves);
        assert_eq!(
            driver_level(&[channel("a", 1, true), channel("b", 2, true)]),
            DriverLevel::ManualCurveView
        );
        // Channels that are not controllable do not count.
        assert_eq!(
            driver_level(&[channel("a", 1, true), channel("b", 2, false)]),
            DriverLevel::Curves
        );
        assert_eq!(
            driver_level(&[channel("a", 1, false)]),
            DriverLevel::ReadOnly
        );
    }

    #[test]
    fn reads_mode_duty_curve_and_source() {
        let hwmon = Hwmon::new("read");
        hwmon
            .write("pwm16", "98\n")
            .write("pwm16_enable", "2\n")
            .write("pwm16_auto_channels_temp", "1\n")
            .curve_values(16, &SYS1);
        let reading = read(&hwmon.driver(), 16);
        assert_eq!(
            reading,
            Reading {
                mode: Some(Ok(Mode::Auto)),
                duty: Some(Ok(98)),
                curve: Some(Ok(points(&SYS1))),
                source: Some(Ok(Source::Temp(1))),
            }
        );
        hwmon.write("pwm16_enable", "1\n");
        assert_eq!(hwmon.driver().mode(16), Ok(Mode::Manual));
        // Whitespace around the number does not matter.
        hwmon.write("pwm16", " 0 \n");
        assert_eq!(hwmon.driver().duty(16), Ok(0));
    }

    #[test]
    fn values_outside_the_abi_are_invalid() {
        let hwmon = Hwmon::new("invalid");
        let driver = hwmon.driver();
        for text in ["0", "3", "auto", ""] {
            hwmon.write("pwm1_enable", text);
            assert!(is_invalid(driver.mode(1)), "pwm1_enable {text:?}");
        }
        for text in ["256", "-1", "1.5"] {
            hwmon.write("pwm1", text);
            assert!(is_invalid(driver.duty(1)), "pwm1 {text:?}");
        }
        // Temperatures: not whole degrees, negative, above 255 °C.
        for text in ["45500", "-1000", "256000"] {
            hwmon.curve_values(2, &SYS1);
            hwmon.write("pwm2_auto_point3_temp", text);
            assert!(is_invalid(driver.curve(2)), "temp {text:?}");
        }
        hwmon.curve_values(2, &SYS1);
        hwmon.write("pwm2_auto_point7_pwm", "300");
        assert!(is_invalid(driver.curve(2)));
        for text in ["-1", "x"] {
            hwmon.write("pwm1_auto_channels_temp", text);
            assert!(is_invalid(driver.source(1)), "source {text:?}");
        }
        // The error names the attribute and the text.
        hwmon.write("pwm1", "abc\n");
        assert_eq!(
            driver.duty(1),
            Err(ReadError::Invalid {
                attribute: "pwm1".into(),
                text: "abc".into()
            })
        );
    }

    #[test]
    fn source_is_a_bitmap_of_temperatures() {
        let hwmon = Hwmon::new("source-bits");
        let cases = [
            ("1", Source::Temp(1)),
            ("4", Source::Temp(3)),
            ("2147483648", Source::Temp(32)),
            ("0", Source::Mask(0)),
            ("5", Source::Mask(5)),
        ];
        for (text, source) in cases {
            hwmon.write("pwm1_auto_channels_temp", text);
            assert_eq!(hwmon.driver().source(1), Ok(source), "{text}");
        }
        // ENODATA: a temperature the driver does not report. Other errors
        // stay errors.
        let attribute = "pwm1_auto_channels_temp";
        assert_eq!(
            source_from(attribute, Err(ReadError::Os(ENODATA))),
            Ok(Source::Unreported)
        );
        assert_eq!(
            source_from(attribute, Err(ReadError::Os(5))),
            Err(ReadError::Os(5))
        );
    }

    #[test]
    fn missing_and_unreadable_attributes() {
        let hwmon = Hwmon::new("missing");
        let none = Reading {
            mode: None,
            duty: None,
            curve: None,
            source: None,
        };
        assert_eq!(read(&hwmon.driver(), 1), none);
        // A folder in place of the file: the read fails, the rest is read.
        std::fs::create_dir(hwmon.0.join("pwm1")).unwrap();
        hwmon.write("pwm1_enable", "2\n");
        let reading = read(&hwmon.driver(), 1);
        assert!(matches!(reading.duty, Some(Err(ReadError::Os(_)))));
        assert_eq!(reading.mode, Some(Ok(Mode::Auto)));
        // A curve without every point is no curve.
        hwmon.curve_values(1, &SYS1);
        std::fs::remove_file(hwmon.0.join("pwm1_auto_point5_pwm")).unwrap();
        assert_eq!(hwmon.driver().curve(1), Err(ReadError::Missing));
    }

    #[test]
    fn source_names_the_profile_temperature() {
        let profile =
            crate::profile::parse(include_str!("../../../../data/boards/ms-7e61.toml")).unwrap();
        let cpu = source_temp(&profile, Source::Temp(1)).unwrap();
        assert_eq!((cpu.key.as_str(), cpu.emergency_c), ("cpu", Some(95)));
        assert_eq!(source_temp(&profile, Source::Temp(3)).unwrap().key, "vrm");
        // temp5 is hidden in the profile; a bitmap names no one temperature.
        assert!(source_temp(&profile, Source::Temp(5)).is_none());
        assert!(source_temp(&profile, Source::Mask(3)).is_none());
        assert!(source_temp(&profile, Source::Unreported).is_none());
    }
}
