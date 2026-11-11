// SPDX-License-Identifier: GPL-3.0-or-later

//! Board profiles: the data that describes one board (fan headers, sensors,
//! limits), read from TOML files (docs/PROJEKT.md §6.2).
//!
//! [`parse`] checks the syntax and the closed vocabularies (roles, tested
//! writes) and then [`Profile::validate`] checks everything else, so a
//! profile that loads is safe to use. Unknown fields are errors, so that a
//! misspelt limit is never silently ignored.

use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::sensors::Reading;

/// The only profile format this code understands.
pub const SCHEMA: u32 = 1;

/// Fixed lower limit of the duty cycle of a CPU fan, in percent (B5).
/// A profile may raise it, never lower it.
pub const CPU_MIN_DUTY_FLOOR: u8 = 20;
/// Fixed lower limit of the duty cycle of a pump, in percent (B5).
pub const PUMP_MIN_DUTY_FLOOR: u8 = 40;

/// Highest emergency threshold a profile may set; above it a reading is
/// treated as implausible anyway (B4).
const MAX_EMERGENCY_C: u8 = 125;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: u32,
    /// Short name of the profile, equal to the file name without `.toml`.
    pub id: String,
    /// Board model for display, as DMI reports it.
    pub model: String,
    #[serde(rename = "match")]
    pub board_match: BoardMatch,
    pub hwmon: Hwmon,
    #[serde(default, rename = "fan")]
    pub fans: Vec<Fan>,
    #[serde(default, rename = "temp")]
    pub temps: Vec<Temp>,
    #[serde(default, rename = "voltage")]
    pub voltages: Vec<Voltage>,
    #[serde(default)]
    pub hidden: Hidden,
    pub rgb: Option<Rgb>,
}

/// Which boards the profile is for (§6.1).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoardMatch {
    pub board_vendor: String,
    /// The four hex digits of "MS-xxxx", upper case.
    pub board_code: String,
    /// PCB revisions checked with this profile; others are read-only (B9).
    pub pcb_revisions: Vec<u8>,
    /// Board models the profile is for, as DMI names them without the
    /// "(MS-xxxx)" part, e.g. "MAG B850M MORTAR WIFI". Empty: every model
    /// with this board code. One code can stand for boards with different
    /// fan headers (§6.1, D28).
    #[serde(default)]
    pub board_names: Vec<String>,
}

impl BoardMatch {
    /// Whether the profile lists the board model `model` (from
    /// [`crate::board::Dmi::model_name`]). Case and runs of spaces do not
    /// matter.
    pub fn lists_model(&self, model: &str) -> bool {
        let model = normalized_name(model);
        self.board_names
            .iter()
            .any(|name| normalized_name(name) == model)
    }
}

/// A board name in upper case with single spaces, for comparing.
fn normalized_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hwmon {
    /// The `name` attribute of the hwmon device.
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FanRole {
    Cpu,
    Pump,
    Sys,
    Aux,
}

/// A write to a fan channel that was checked on the board (D16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestedWrite {
    Manual,
    Curve,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fan {
    /// Channel key used over D-Bus, e.g. `sys1`.
    pub key: String,
    pub role: FanRole,
    /// Number in the translated name, e.g. 1 in "System fan 1".
    pub number: u8,
    /// Header name printed on the board, e.g. `SYS_FAN1`; not translated.
    pub header: String,
    /// N of `fanN_input`.
    #[serde(rename = "fan")]
    pub fan_index: u8,
    /// N of `pwmN` (and `pwmN_enable`), if the channel has one.
    #[serde(default, rename = "pwm")]
    pub pwm_index: Option<u8>,
    /// Whether Baseboard may control the channel at all. Off unless set.
    #[serde(default)]
    pub controllable: bool,
    /// Minimum duty cycle in percent; the fixed floor of the role if absent.
    #[serde(default)]
    pub min_duty: Option<u8>,
    /// Whether the header mapping was checked physically.
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub tested_writes: Vec<TestedWrite>,
    /// Replaces the translated name, untranslated.
    #[serde(default)]
    pub label: Option<String>,
}

impl Fan {
    /// The fixed lower limit of the duty cycle for this role, in percent.
    pub fn min_duty_floor(&self) -> u8 {
        match self.role {
            FanRole::Cpu => CPU_MIN_DUTY_FLOOR,
            FanRole::Pump => PUMP_MIN_DUTY_FLOOR,
            FanRole::Sys | FanRole::Aux => 0,
        }
    }

    /// The minimum duty cycle in percent: the profile's, never below the
    /// fixed floor (validation rejects a lower one).
    pub fn min_duty(&self) -> u8 {
        self.min_duty.unwrap_or_default().max(self.min_duty_floor())
    }
}

/// In the order the Overview lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TempRole {
    Cpu,
    System,
    Vrm,
    Chipset,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Temp {
    pub key: String,
    pub role: TempRole,
    /// N of `tempN_input`.
    pub input: u8,
    /// Emergency threshold in °C (B4).
    #[serde(default)]
    pub emergency_c: Option<u8>,
    #[serde(default)]
    pub label: Option<String>,
}

/// In the order the Overview lists them: the rated ATX rails first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoltageRole {
    P12v,
    P5v,
    P3v3,
    Vcore,
    Soc,
    Vddio,
}

impl VoltageRole {
    /// The nominal voltage of an ATX rail, which names it ("+12 V", D26);
    /// `None` for the rails named in words.
    pub fn nominal(self) -> Option<f64> {
        match self {
            Self::P12v => Some(12.0),
            Self::P5v => Some(5.0),
            Self::P3v3 => Some(3.3),
            Self::Vcore | Self::Soc | Self::Vddio => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Voltage {
    pub key: String,
    pub role: VoltageRole,
    /// N of `inN_input`.
    pub input: u8,
    /// Factor from the hwmon value to volts, unless lm-sensors already
    /// applies one (§5.1, no double scaling).
    #[serde(default = "one")]
    pub multiplier: f64,
    #[serde(default)]
    pub nominal: Option<f64>,
    /// Allowed deviation from `nominal`, in percent.
    #[serde(default)]
    pub tolerance_pct: Option<f64>,
    #[serde(default)]
    pub label: Option<String>,
}

fn one() -> f64 {
    1.0
}

impl Voltage {
    /// Volts on this rail from its reading (§5.1, no double scaling): the
    /// profile's multiplier, unless the source already converted the value
    /// with a `compute` statement of lm-sensors.
    pub fn volts(&self, reading: &Reading) -> f64 {
        if reading.scaled {
            reading.value
        } else {
            reading.value * self.multiplier
        }
    }
}

/// Channels of the hwmon device that are not shown.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hidden {
    #[serde(default)]
    pub fan: Vec<u8>,
    #[serde(default)]
    pub pwm: Vec<u8>,
    #[serde(default)]
    pub temp: Vec<u8>,
    #[serde(default, rename = "in")]
    pub voltage: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rgb {
    /// `vendor:product` of the USB controller, lower-case hex.
    pub usb_id: String,
    /// Start of the USB serial number that identifies the board.
    pub serial_prefix: String,
}

#[derive(Debug)]
pub enum ProfileError {
    Io(std::io::Error),
    Syntax(toml::de::Error),
    MissingSchema,
    UnsupportedSchema(i64),
    Invalid(String),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read the profile: {e}"),
            Self::Syntax(e) => write!(f, "invalid profile: {e}"),
            Self::MissingSchema => write!(f, "the profile has no `schema` field"),
            Self::UnsupportedSchema(n) => {
                write!(f, "profile format {n} is not supported (expected {SCHEMA})")
            }
            Self::Invalid(reason) => write!(f, "invalid profile: {reason}"),
        }
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Syntax(e) => Some(e),
            _ => None,
        }
    }
}

fn invalid(reason: impl Into<String>) -> ProfileError {
    ProfileError::Invalid(reason.into())
}

/// Parses and validates a profile.
pub fn parse(text: &str) -> Result<Profile, ProfileError> {
    // The format version first: a profile of another version gets that
    // error, not a confusing one about its fields.
    let table: toml::Table = toml::from_str(text).map_err(ProfileError::Syntax)?;
    match table.get("schema") {
        None => return Err(ProfileError::MissingSchema),
        Some(toml::Value::Integer(n)) if *n == i64::from(SCHEMA) => {}
        Some(toml::Value::Integer(n)) => return Err(ProfileError::UnsupportedSchema(*n)),
        Some(_) => return Err(invalid("`schema` must be a number")),
    }
    let profile: Profile = toml::from_str(text).map_err(ProfileError::Syntax)?;
    profile.validate()?;
    Ok(profile)
}

/// Reads, parses and validates a profile file. Its `id` must be the file
/// name without `.toml`.
pub fn load(path: &Path) -> Result<Profile, ProfileError> {
    let text = std::fs::read_to_string(path).map_err(ProfileError::Io)?;
    let profile = parse(&text)?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if profile.id != stem {
        return Err(invalid(format!(
            "id {:?} does not match the file name {:?}",
            profile.id,
            path.display().to_string()
        )));
    }
    Ok(profile)
}

impl Profile {
    /// Checks everything the types do not: formats, ranges, unique keys and
    /// channels, the minima of B5 and the hidden channels.
    pub fn validate(&self) -> Result<(), ProfileError> {
        if self.id.is_empty()
            || !self
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            || self.id.starts_with('-')
        {
            return Err(invalid(format!(
                "id {:?}: use lower-case letters, digits and '-'",
                self.id
            )));
        }
        non_empty("model", &self.model)?;
        self.board_match.validate()?;
        non_empty("hwmon.name", &self.hwmon.name)?;
        if self.hwmon.name.contains('/') {
            return Err(invalid("hwmon.name must not contain '/'"));
        }
        self.validate_fans()?;
        self.validate_temps()?;
        self.validate_voltages()?;
        self.validate_hidden()?;
        if let Some(rgb) = &self.rgb {
            rgb.validate()?;
        }
        Ok(())
    }

    fn validate_fans(&self) -> Result<(), ProfileError> {
        let mut keys = HashSet::new();
        let mut fan_indices = HashSet::new();
        let mut pwm_indices = HashSet::new();
        let mut names = HashSet::new();
        for fan in &self.fans {
            let at = format!("fan {:?}", fan.key);
            check_key(&fan.key)?;
            unique(&mut keys, fan.key.as_str(), &at, "key")?;
            if fan.number == 0 {
                return Err(invalid(format!("{at}: number must be at least 1")));
            }
            unique(&mut names, (fan.role, fan.number), &at, "role and number")?;
            non_empty(&format!("{at}: header"), &fan.header)?;
            check_label(&at, fan.label.as_deref())?;
            if fan.fan_index == 0 {
                return Err(invalid(format!("{at}: fan must be at least 1")));
            }
            unique(&mut fan_indices, fan.fan_index, &at, "fan")?;
            if let Some(pwm) = fan.pwm_index {
                if pwm == 0 {
                    return Err(invalid(format!("{at}: pwm must be at least 1")));
                }
                unique(&mut pwm_indices, pwm, &at, "pwm")?;
            }
            if let Some(min) = fan.min_duty {
                if min > 100 {
                    return Err(invalid(format!("{at}: min_duty {min} is above 100 %")));
                }
                if min < fan.min_duty_floor() {
                    return Err(invalid(format!(
                        "{at}: min_duty {min} % is below the fixed floor of {} % (B5)",
                        fan.min_duty_floor()
                    )));
                }
            }
            if fan.controllable && fan.pwm_index.is_none() {
                return Err(invalid(format!("{at}: a controllable fan needs pwm")));
            }
            if !fan.controllable && !fan.tested_writes.is_empty() {
                return Err(invalid(format!(
                    "{at}: tested_writes on a fan that is not controllable"
                )));
            }
            let mut writes = HashSet::new();
            for write in &fan.tested_writes {
                unique(&mut writes, *write, &at, "tested_writes entry")?;
            }
        }
        Ok(())
    }

    fn validate_temps(&self) -> Result<(), ProfileError> {
        let mut keys = HashSet::new();
        let mut inputs = HashSet::new();
        let mut roles = HashSet::new();
        for temp in &self.temps {
            let at = format!("temp {:?}", temp.key);
            check_key(&temp.key)?;
            unique(&mut keys, temp.key.as_str(), &at, "key")?;
            unique(&mut roles, temp.role, &at, "role")?;
            check_label(&at, temp.label.as_deref())?;
            if temp.input == 0 {
                return Err(invalid(format!("{at}: input must be at least 1")));
            }
            unique(&mut inputs, temp.input, &at, "input")?;
            if let Some(limit) = temp.emergency_c
                && !(1..=MAX_EMERGENCY_C).contains(&limit)
            {
                return Err(invalid(format!(
                    "{at}: emergency_c {limit} is outside 1–{MAX_EMERGENCY_C} °C"
                )));
            }
        }
        Ok(())
    }

    fn validate_voltages(&self) -> Result<(), ProfileError> {
        let mut keys = HashSet::new();
        let mut inputs = HashSet::new();
        let mut roles = HashSet::new();
        for voltage in &self.voltages {
            let at = format!("voltage {:?}", voltage.key);
            check_key(&voltage.key)?;
            unique(&mut keys, voltage.key.as_str(), &at, "key")?;
            unique(&mut roles, voltage.role, &at, "role")?;
            unique(&mut inputs, voltage.input, &at, "input")?;
            check_label(&at, voltage.label.as_deref())?;
            if !(voltage.multiplier.is_finite() && voltage.multiplier > 0.0) {
                return Err(invalid(format!("{at}: multiplier must be above 0")));
            }
            if let Some(nominal) = voltage.nominal
                && !(nominal.is_finite() && nominal > 0.0)
            {
                return Err(invalid(format!("{at}: nominal must be above 0")));
            }
            if let Some(tolerance) = voltage.tolerance_pct {
                if !(tolerance.is_finite() && tolerance > 0.0 && tolerance <= 50.0) {
                    return Err(invalid(format!(
                        "{at}: tolerance_pct must be above 0 and at most 50"
                    )));
                }
                if voltage.nominal.is_none() {
                    return Err(invalid(format!("{at}: tolerance_pct needs nominal")));
                }
            }
        }
        Ok(())
    }

    fn validate_hidden(&self) -> Result<(), ProfileError> {
        let used_fans: HashSet<u8> = self.fans.iter().map(|f| f.fan_index).collect();
        let used_pwms: HashSet<u8> = self.fans.iter().filter_map(|f| f.pwm_index).collect();
        let used_temps: HashSet<u8> = self.temps.iter().map(|t| t.input).collect();
        let used_voltages: HashSet<u8> = self.voltages.iter().map(|v| v.input).collect();
        for (name, hidden, used, lowest) in [
            ("fan", &self.hidden.fan, used_fans, 1),
            ("pwm", &self.hidden.pwm, used_pwms, 1),
            ("temp", &self.hidden.temp, used_temps, 1),
            ("in", &self.hidden.voltage, used_voltages, 0),
        ] {
            let mut seen = HashSet::new();
            for &n in hidden {
                let at = format!("hidden.{name}");
                if n < lowest {
                    return Err(invalid(format!("{at}: {n} is not a channel")));
                }
                unique(&mut seen, n, &at, "channel")?;
                if used.contains(&n) {
                    return Err(invalid(format!(
                        "{at}: {name}{n} is hidden but used by the profile"
                    )));
                }
            }
        }
        Ok(())
    }
}

impl BoardMatch {
    fn validate(&self) -> Result<(), ProfileError> {
        non_empty("match.board_vendor", &self.board_vendor)?;
        let code = &self.board_code;
        if code.len() != 4
            || !code
                .chars()
                .all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && c.is_ascii_hexdigit()))
        {
            return Err(invalid(format!(
                "match.board_code {code:?}: use four upper-case hex digits, e.g. \"7E61\""
            )));
        }
        if self.pcb_revisions.is_empty() {
            return Err(invalid("match.pcb_revisions is empty"));
        }
        let mut seen = HashSet::new();
        for &revision in &self.pcb_revisions {
            if revision > 9 {
                return Err(invalid(format!(
                    "match.pcb_revisions: {revision} is not a single digit"
                )));
            }
            unique(&mut seen, revision, "match.pcb_revisions", "revision")?;
        }
        let mut names = HashSet::new();
        for name in &self.board_names {
            if name.trim().is_empty() {
                return Err(invalid("match.board_names: a name is empty"));
            }
            if name.contains("(MS-") {
                return Err(invalid(format!(
                    "match.board_names: {name:?}: leave out the \"(MS-xxxx)\" part"
                )));
            }
            unique(
                &mut names,
                normalized_name(name),
                "match.board_names",
                "name",
            )?;
        }
        Ok(())
    }
}

impl Rgb {
    fn validate(&self) -> Result<(), ProfileError> {
        let valid_id = self
            .usb_id
            .split_once(':')
            .is_some_and(|(vendor, product)| {
                [vendor, product].iter().all(|part| {
                    part.len() == 4
                        && part
                            .chars()
                            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
                })
            });
        if !valid_id {
            return Err(invalid(format!(
                "rgb.usb_id {:?}: use lower-case \"vvvv:pppp\"",
                self.usb_id
            )));
        }
        if self.serial_prefix.is_empty()
            || !self
                .serial_prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric())
        {
            return Err(invalid(format!(
                "rgb.serial_prefix {:?}: use letters and digits",
                self.serial_prefix
            )));
        }
        Ok(())
    }
}

fn non_empty(field: &str, value: &str) -> Result<(), ProfileError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{field} is empty")));
    }
    Ok(())
}

/// Keys are part of the D-Bus contract: short, lower-case ASCII.
fn check_key(key: &str) -> Result<(), ProfileError> {
    if key.is_empty()
        || key.len() > 16
        || !key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return Err(invalid(format!(
            "key {key:?}: use 1–16 lower-case letters and digits"
        )));
    }
    Ok(())
}

fn check_label(at: &str, label: Option<&str>) -> Result<(), ProfileError> {
    match label {
        Some(label) if label.trim().is_empty() => Err(invalid(format!("{at}: label is empty"))),
        _ => Ok(()),
    }
}

fn unique<T: Eq + std::hash::Hash + fmt::Debug>(
    seen: &mut HashSet<T>,
    value: T,
    at: &str,
    what: &str,
) -> Result<(), ProfileError> {
    let message = format!("{at}: {what} {value:?} is used twice");
    if seen.insert(value) {
        Ok(())
    } else {
        Err(invalid(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS_7E61: &str = include_str!("../../../data/boards/ms-7e61.toml");

    /// A small valid profile that the tests below break one field at a time.
    const MINIMAL: &str = r#"
schema = 1
id = "test-board"
model = "TEST BOARD"

[match]
board_vendor = "Example Inc."
board_code = "7E61"
pcb_revisions = [1]

[hwmon]
name = "nct6687"

[[fan]]
key = "cpu1"
role = "cpu"
number = 1
header = "CPU_FAN1"
fan = 1
pwm = 1
controllable = true
min_duty = 30

[[temp]]
key = "cpu"
role = "cpu"
input = 1
emergency_c = 95

[[voltage]]
key = "12v"
role = "p12v"
input = 0
multiplier = 12.0
nominal = 12.0
tolerance_pct = 5.0
"#;

    #[test]
    fn multiplier_applies_once() {
        let profile = parse(MINIMAL).unwrap();
        let rail = &profile.voltages[0];
        let reading = |value: f64, scaled: bool| Reading {
            kind: crate::sensors::Kind::Voltage,
            index: 0,
            label: None,
            value,
            scaled,
            sensors_label: None,
        };
        // Raw sysfs value: the profile's ×12.
        assert_eq!(rail.volts(&reading(1.008, false)), 1.008 * 12.0);
        // Already converted by lm-sensors (`compute in0 @*12`): as it is.
        assert_eq!(rail.volts(&reading(1.008 * 12.0, true)), 1.008 * 12.0);
    }

    fn with(old: &str, new: &str) -> String {
        assert!(
            MINIMAL.contains(old),
            "{old:?} is not in the minimal profile"
        );
        MINIMAL.replacen(old, new, 1)
    }

    fn reason(text: &str) -> String {
        match parse(text) {
            Err(ProfileError::Invalid(reason)) => reason,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn shipped_profile_is_valid() {
        let profile = parse(MS_7E61).unwrap();
        assert_eq!(profile.id, "ms-7e61");
        assert_eq!(profile.board_match.board_code, "7E61");
        assert_eq!(profile.board_match.pcb_revisions, [1]);
        assert_eq!(profile.board_match.board_names, ["MAG B850M MORTAR WIFI"]);
        assert_eq!(profile.hwmon.name, "nct6687");

        let keys: Vec<&str> = profile.fans.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(
            keys,
            ["cpu1", "pump1", "sys1", "sys2", "sys3", "sys4", "ezconn"]
        );

        let fan = |key: &str| profile.fans.iter().find(|f| f.key == key).unwrap();
        assert_eq!(fan("cpu1").min_duty(), 30);
        assert_eq!(fan("pump1").min_duty(), 50);
        assert_eq!(fan("sys1").min_duty(), 0);
        // SYS headers are numbered in reverse by the EC.
        assert_eq!(
            (fan("sys1").fan_index, fan("sys1").pwm_index),
            (16, Some(16))
        );
        assert_eq!(
            (fan("sys4").fan_index, fan("sys4").pwm_index),
            (13, Some(13))
        );
        assert_eq!(
            fan("sys1").tested_writes,
            [TestedWrite::Manual, TestedWrite::Curve]
        );
        assert!(fan("cpu1").tested_writes.is_empty());
        assert!(fan("pump1").tested_writes.is_empty());
        assert!(!fan("pump1").verified);
        assert!(!fan("ezconn").controllable);

        let emergency: Vec<_> = profile.temps.iter().map(|t| t.emergency_c).collect();
        assert_eq!(emergency, [Some(95), Some(80), Some(90), Some(80)]);
        assert_eq!(profile.hidden.fan, [3]);
        assert_eq!(profile.hidden.voltage, [5, 6, 7]);
    }

    #[test]
    fn minimal_profile_is_valid() {
        let profile = parse(MINIMAL).unwrap();
        assert_eq!(profile.voltages[0].multiplier, 12.0);
        assert!(profile.rgb.is_none());
        assert_eq!(profile.hidden, Hidden::default());
    }

    #[test]
    fn defaults() {
        let profile = parse(&with("min_duty = 30\n", "")).unwrap();
        // Without min_duty the fixed floor applies.
        assert_eq!(profile.fans[0].min_duty(), CPU_MIN_DUTY_FLOOR);
        assert!(!profile.fans[0].verified);
        let profile = parse(&with("multiplier = 12.0\n", "")).unwrap();
        assert_eq!(profile.voltages[0].multiplier, 1.0);
        let profile = parse(&with("controllable = true\n", "")).unwrap();
        assert!(!profile.fans[0].controllable);
    }

    #[test]
    fn schema_version() {
        assert!(matches!(
            parse(&with("schema = 1", "schema = 2")),
            Err(ProfileError::UnsupportedSchema(2))
        ));
        assert!(matches!(
            parse(&with("schema = 1\n", "")),
            Err(ProfileError::MissingSchema)
        ));
        assert!(matches!(
            parse(&with("schema = 1", "schema = \"1\"")),
            Err(ProfileError::Invalid(_))
        ));
        // Another version is reported as such even with unknown fields.
        let future = with("schema = 1", "schema = 2\nfuture_field = true");
        assert!(matches!(
            parse(&future),
            Err(ProfileError::UnsupportedSchema(2))
        ));
    }

    #[test]
    fn syntax_errors() {
        assert!(matches!(parse("schema = "), Err(ProfileError::Syntax(_))));
        // Unknown role, unknown field, missing field, wrong type.
        for text in [
            with("role = \"cpu\"\nnumber", "role = \"gpu\"\nnumber"),
            with("header = \"CPU_FAN1\"", "heder = \"CPU_FAN1\""),
            with("header = \"CPU_FAN1\"\n", ""),
            with("input = 1\n", "input = \"temp1\"\n"),
            with("role = \"p12v\"", "role = \"12v\""),
            with("min_duty = 30", "min_duty = 30\ntested_writes = [\"full\"]"),
        ] {
            assert!(
                matches!(parse(&text), Err(ProfileError::Syntax(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn identity() {
        assert!(reason(&with("id = \"test-board\"", "id = \"Test Board\"")).contains("id"));
        assert!(reason(&with("model = \"TEST BOARD\"", "model = \" \"")).contains("model"));
        for code in ["7e61", "7E6", "7E61A", "7G61"] {
            let text = with("board_code = \"7E61\"", &format!("board_code = \"{code}\""));
            assert!(reason(&text).contains("board_code"), "{code}");
        }
        assert!(reason(&with("pcb_revisions = [1]", "pcb_revisions = []")).contains("empty"));
        assert!(reason(&with("pcb_revisions = [1]", "pcb_revisions = [1, 1]")).contains("twice"));
        assert!(reason(&with("pcb_revisions = [1]", "pcb_revisions = [10]")).contains("digit"));
        assert!(reason(&with("name = \"nct6687\"", "name = \"a/b\"")).contains("hwmon.name"));
    }

    /// Model names in `[match]` (D28): optional, so older profiles load as
    /// they did.
    #[test]
    fn board_names() {
        let names = |list: &str| {
            with(
                "pcb_revisions = [1]",
                &format!("pcb_revisions = [1]\nboard_names = {list}"),
            )
        };
        // Without the field: every model with the code.
        assert!(parse(MINIMAL).unwrap().board_match.board_names.is_empty());
        assert!(
            parse(&names("[]"))
                .unwrap()
                .board_match
                .board_names
                .is_empty()
        );

        let profile = parse(&names("[\"TEST BOARD\", \"TEST BOARD MAX\"]")).unwrap();
        let matcher = &profile.board_match;
        assert!(matcher.lists_model("TEST BOARD"));
        assert!(matcher.lists_model("Test  Board  Max"));
        assert!(!matcher.lists_model("TEST BOARD PLUS"));
        assert!(!matcher.lists_model(""));

        assert!(reason(&names("[\"\"]")).contains("empty"));
        assert!(reason(&names("[\"  \"]")).contains("empty"));
        assert!(reason(&names("[\"TEST BOARD (MS-7E61)\"]")).contains("(MS-xxxx)"));
        assert!(reason(&names("[\"TEST BOARD\", \"test  board\"]")).contains("twice"));
    }

    #[test]
    fn fan_rules() {
        assert!(reason(&with("key = \"cpu1\"", "key = \"CPU-1\"")).contains("key"));
        assert!(reason(&with("number = 1", "number = 0")).contains("number"));
        assert!(reason(&with("fan = 1\n", "fan = 0\n")).contains("fan must"));
        assert!(reason(&with("pwm = 1\n", "pwm = 0\n")).contains("pwm must"));
        assert!(reason(&with("pwm = 1\n", "")).contains("needs pwm"));
        assert!(reason(&with("header = \"CPU_FAN1\"", "header = \"\"")).contains("header"));
        assert!(reason(&with("min_duty = 30", "min_duty = 30\nlabel = \"\"")).contains("label"));
    }

    #[test]
    fn duplicates() {
        let second_fan = "\n[[fan]]\nkey = \"sys1\"\nrole = \"sys\"\nnumber = 1\nheader = \"SYS_FAN1\"\nfan = 16\npwm = 16\n";
        let text = |fan: &str| with("[[temp]]", &format!("{fan}\n[[temp]]"));
        assert!(parse(&text(second_fan)).is_ok());
        assert!(reason(&text(&second_fan.replace("sys1", "cpu1"))).contains("key"));
        assert!(reason(&text(&second_fan.replace("fan = 16", "fan = 1"))).contains("fan 1"));
        assert!(reason(&text(&second_fan.replace("pwm = 16", "pwm = 1"))).contains("pwm 1"));
        assert!(
            reason(&text(
                &second_fan.replace("role = \"sys\"", "role = \"cpu\"")
            ))
            .contains("role and number")
        );
        let second_temp = "\n[[temp]]\nkey = \"vrm\"\nrole = \"vrm\"\ninput = 1\n";
        assert!(reason(&format!("{MINIMAL}{second_temp}")).contains("input"));
        let second_voltage = "\n[[voltage]]\nkey = \"5v\"\nrole = \"p12v\"\ninput = 1\n";
        assert!(reason(&format!("{MINIMAL}{second_voltage}")).contains("role"));
    }

    #[test]
    fn minimum_duty_cannot_go_below_the_floor() {
        assert!(reason(&with("min_duty = 30", "min_duty = 19")).contains("B5"));
        assert!(parse(&with("min_duty = 30", "min_duty = 20")).is_ok());
        assert!(reason(&with("min_duty = 30", "min_duty = 101")).contains("above 100"));

        let pump = with("role = \"cpu\"\nnumber", "role = \"pump\"\nnumber");
        assert!(reason(&pump.replace("min_duty = 30", "min_duty = 39")).contains("B5"));
        assert!(parse(&pump.replace("min_duty = 30", "min_duty = 40")).is_ok());
        let profile = parse(&pump.replace("min_duty = 30\n", "")).unwrap();
        assert_eq!(profile.fans[0].min_duty(), PUMP_MIN_DUTY_FLOOR);

        let sys = with("role = \"cpu\"\nnumber", "role = \"sys\"\nnumber");
        assert!(parse(&sys.replace("min_duty = 30", "min_duty = 0")).is_ok());
    }

    #[test]
    fn tested_writes() {
        let writes = |list: &str| {
            with(
                "min_duty = 30",
                &format!("min_duty = 30\ntested_writes = {list}"),
            )
        };
        assert!(parse(&writes("[\"manual\", \"curve\"]")).is_ok());
        assert!(reason(&writes("[\"manual\", \"manual\"]")).contains("twice"));
        let not_controllable = writes("[\"manual\"]").replace("controllable = true\n", "");
        assert!(reason(&not_controllable).contains("not controllable"));
    }

    #[test]
    fn temperature_and_voltage_rules() {
        assert!(reason(&with("emergency_c = 95", "emergency_c = 126")).contains("emergency_c"));
        assert!(reason(&with("emergency_c = 95", "emergency_c = 0")).contains("emergency_c"));
        assert!(
            reason(&with("input = 1\nemergency_c", "input = 0\nemergency_c")).contains("input")
        );
        assert!(reason(&with("multiplier = 12.0", "multiplier = 0.0")).contains("multiplier"));
        assert!(reason(&with("nominal = 12.0", "nominal = -12.0")).contains("nominal"));
        assert!(
            reason(&with("tolerance_pct = 5.0", "tolerance_pct = 60.0")).contains("tolerance_pct")
        );
        assert!(reason(&with("nominal = 12.0\n", "")).contains("needs nominal"));
    }

    #[test]
    fn hidden_channels() {
        let hidden = |table: &str| format!("{MINIMAL}\n[hidden]\n{table}\n");
        assert!(parse(&hidden("fan = [3]\npwm = [3]\ntemp = [5]\nin = [5, 6, 7]")).is_ok());
        assert!(reason(&hidden("fan = [1]")).contains("used by the profile"));
        assert!(reason(&hidden("pwm = [1]")).contains("used by the profile"));
        assert!(reason(&hidden("temp = [1]")).contains("used by the profile"));
        assert!(reason(&hidden("in = [0]")).contains("used by the profile"));
        assert!(reason(&hidden("fan = [0]")).contains("not a channel"));
        assert!(reason(&hidden("temp = [5, 5]")).contains("twice"));
        assert!(matches!(
            parse(&hidden("gpu = [1]")),
            Err(ProfileError::Syntax(_))
        ));
    }

    #[test]
    fn rgb_section() {
        let rgb = |id: &str, prefix: &str| {
            format!("{MINIMAL}\n[rgb]\nusb_id = \"{id}\"\nserial_prefix = \"{prefix}\"\n")
        };
        assert!(parse(&rgb("0db0:0076", "7E61")).is_ok());
        assert!(reason(&rgb("0DB0:0076", "7E61")).contains("usb_id"));
        assert!(reason(&rgb("0db0-0076", "7E61")).contains("usb_id"));
        assert!(reason(&rgb("0db0:076", "7E61")).contains("usb_id"));
        assert!(reason(&rgb("0db0:0076", "")).contains("serial_prefix"));
    }

    #[test]
    fn file_name_must_match_the_id() {
        let dir = std::env::temp_dir().join(format!("baseboard-profile-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("test-board.toml");
        std::fs::write(&good, MINIMAL).unwrap();
        assert_eq!(load(&good).unwrap().id, "test-board");
        let bad = dir.join("other.toml");
        std::fs::write(&bad, MINIMAL).unwrap();
        assert!(matches!(load(&bad), Err(ProfileError::Invalid(r)) if r.contains("file name")));
        assert!(matches!(
            load(&dir.join("missing.toml")),
            Err(ProfileError::Io(_))
        ));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
