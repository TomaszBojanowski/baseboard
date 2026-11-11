// SPDX-License-Identifier: GPL-3.0-or-later

//! The one place that knows the sysfs ABI of the `nct6683` driver
//! (docs/PROJEKT.md §3.7, §5.5.1). For now it only detects what the driver
//! allows for each channel (§5.5.2); reading and writing modes, duty
//! cycles and curves come with M2.
//!
//! Detection looks only at the attributes, separately for every channel:
//! whether a file exists and whether its owner write bit is set. It uses
//! the file mode (`stat`), not `access(W_OK)`, because the app does not run
//! as root and must reach the same result as the helper. It never looks at
//! the kernel version.

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::profile::Profile;

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
    /// Mode and duty cycle, no curves: only the series with manual mode.
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
}
