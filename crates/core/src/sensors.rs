// SPDX-License-Identifier: GPL-3.0-or-later

//! Sensor readings (docs/PROJEKT.md §5.1): the [`SensorSource`] trait, its
//! implementation on raw sysfs hwmon, and [`merge`], which lays what
//! libsensors reports over the sysfs readings.
//!
//! A hwmon device is identified by its `name` and the path of its parent
//! device, never by the number in `hwmonN`, which changes between boots.
//! The sysfs root is replaceable, so tests and the app can use a fake tree.
//! Files are only ever opened for reading.

use std::cell::OnceCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::profile::Profile;

/// Kinds of readings, in the order they are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// °C, from `tempN_input`.
    Temp,
    /// rpm, from `fanN_input`.
    Fan,
    /// V, from `inN_input`.
    Voltage,
    /// W, from `powerN_input`, or else `powerN_average`.
    Power,
    /// Duty cycle 0–255 as the driver reports it, from `pwmN`.
    Pwm,
}

impl Kind {
    /// The prefix of the sysfs attribute names.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Temp => "temp",
            Self::Fan => "fan",
            Self::Voltage => "in",
            Self::Power => "power",
            Self::Pwm => "pwm",
        }
    }

    /// The sysfs value divided by this is in the unit of the kind
    /// (m°C, mV, µW). Dividing keeps e.g. 1008 mV exactly 1.008 V.
    fn divisor(self) -> f64 {
        match self {
            Self::Temp | Self::Voltage => 1e3,
            Self::Power => 1e6,
            Self::Fan | Self::Pwm => 1.0,
        }
    }

    const ALL: [Self; 5] = [Self::Temp, Self::Fan, Self::Voltage, Self::Power, Self::Pwm];
}

/// One reading of a hwmon device.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub kind: Kind,
    /// N of the channel, e.g. 1 for `temp1`.
    pub index: u8,
    /// The driver's label (`tempN_label`), if it has one.
    pub label: Option<String>,
    /// The value in the unit of [`Kind`].
    pub value: f64,
    /// The source already converted the value (a `compute` statement of
    /// lm-sensors), so a profile multiplier must not be applied again.
    /// Only found for the voltage rails of the board profile (see
    /// [`computed`]); `false` elsewhere.
    pub scaled: bool,
    /// The name `sensors` shows: the label from the lm-sensors
    /// configuration, or else the driver's. `None` without libsensors.
    pub sensors_label: Option<String>,
}

/// One hwmon device with its readings.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    /// The `name` attribute.
    pub name: String,
    /// The parent device relative to the sysfs root, e.g.
    /// `devices/platform/nct6683.2592`; `None` if the device has none.
    pub device: Option<PathBuf>,
    /// The hwmon folder with the attributes (canonical path).
    pub path: PathBuf,
    /// Sorted by kind and index.
    pub readings: Vec<Reading>,
}

impl Chip {
    /// The reading of one channel.
    pub fn reading(&self, kind: Kind, index: u8) -> Option<&Reading> {
        self.readings
            .iter()
            .find(|r| r.kind == kind && r.index == index)
    }
}

/// A source of sensor readings: raw sysfs here, libsensors in the app.
pub trait SensorSource {
    /// All hwmon devices with their current readings, sorted by name and
    /// device. Devices without a readable `name` are left out.
    fn chips(&self) -> Vec<Chip>;
}

/// One entry of `<root>/class/hwmon`, to notice when devices change.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Device {
    /// `hwmonN`.
    pub entry: String,
    /// The folder the entry points to.
    pub path: PathBuf,
    /// The inode of that folder. A driver loaded again makes a new folder,
    /// often under the same `hwmonN` and path, but never with the same
    /// inode.
    pub inode: u64,
}

/// The hwmon devices under `<root>/class/hwmon`, sorted. When the list
/// changes, a driver came, went or was loaded again, and a source of
/// readings has to be made again (§5.1).
pub fn devices(root: &Path) -> Vec<Device> {
    let Ok(entries) = std::fs::read_dir(root.join("class/hwmon")) else {
        return Vec::new();
    };
    let mut devices: Vec<Device> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = std::fs::canonicalize(entry.path()).ok()?;
            let inode = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&path).ok()?);
            Some(Device {
                entry: entry.file_name().to_string_lossy().into_owned(),
                path,
                inode,
            })
        })
        .collect();
    devices.sort();
    devices
}

/// The only device with the given name. `None` if there is none or more
/// than one: a board profile names a single device, and a second one with
/// the same name would make the choice a guess.
pub fn find<'a>(chips: &'a [Chip], name: &str) -> Option<&'a Chip> {
    let mut matching = chips.iter().filter(|chip| chip.name == name);
    let chip = matching.next()?;
    matching.next().is_none().then_some(chip)
}

/// Readings straight from `<root>/class/hwmon`. The layout (devices, names,
/// labels and the file of every value) is read on the first call and kept;
/// later calls read only the values. So a source belongs to one set of
/// devices: when [`devices`] change, make a new one.
#[derive(Debug, Clone)]
pub struct SysfsSource {
    root: PathBuf,
    layout: OnceCell<Vec<ChipLayout>>,
}

#[derive(Debug, Clone)]
struct ChipLayout {
    name: String,
    device: Option<PathBuf>,
    path: PathBuf,
    channels: Vec<ChannelLayout>,
}

#[derive(Debug, Clone)]
struct ChannelLayout {
    kind: Kind,
    index: u8,
    label: Option<String>,
    /// The file with the value: `tempN_input`, `pwmN`, …
    file: PathBuf,
}

impl SysfsSource {
    /// `root` is `/sys`, or the root of a fake tree.
    pub fn new(root: &Path) -> Self {
        let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
        Self {
            root,
            layout: OnceCell::new(),
        }
    }

    /// The chips, with the values that `wanted` (chip folder, kind) selects
    /// read; the others are NaN, to be set by libsensors or by
    /// [`fill`](Self::fill). A value that cannot be read is NaN too: the
    /// reading stays and the page shows "–" (§5.1), the rest goes on.
    pub fn read(&self, wanted: impl Fn(&Path, Kind) -> bool) -> Vec<Chip> {
        self.layout()
            .iter()
            .map(|chip| Chip {
                name: chip.name.clone(),
                device: chip.device.clone(),
                path: chip.path.clone(),
                readings: chip
                    .channels
                    .iter()
                    .map(|channel| {
                        let value = if wanted(&chip.path, channel.kind) {
                            read_value(&channel.file, channel.kind).unwrap_or(f64::NAN)
                        } else {
                            f64::NAN
                        };
                        Reading {
                            kind: channel.kind,
                            index: channel.index,
                            label: channel.label.clone(),
                            value,
                            scaled: false,
                            sensors_label: None,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// Reads the values that are still NaN. Afterwards NaN means that the
    /// value could not be read; the reading stays (§5.1: shown as "–").
    pub fn fill(&self, chips: &mut [Chip]) {
        for chip in chips {
            let layout = self.layout().iter().find(|l| l.path == chip.path);
            for reading in chip.readings.iter_mut().filter(|r| r.value.is_nan()) {
                if let Some(value) = layout
                    .and_then(|l| {
                        l.channels
                            .iter()
                            .find(|c| c.kind == reading.kind && c.index == reading.index)
                    })
                    .and_then(|c| read_value(&c.file, c.kind))
                {
                    reading.value = value;
                }
            }
        }
    }

    fn layout(&self) -> &[ChipLayout] {
        self.layout.get_or_init(|| {
            let Ok(entries) = std::fs::read_dir(self.root.join("class/hwmon")) else {
                return Vec::new();
            };
            let mut chips: Vec<ChipLayout> = entries
                .filter_map(|entry| self.chip_layout(&entry.ok()?.path()))
                .collect();
            chips.sort_by(|a, b| (&a.name, &a.device).cmp(&(&b.name, &b.device)));
            chips
        })
    }

    fn chip_layout(&self, link: &Path) -> Option<ChipLayout> {
        let path = std::fs::canonicalize(link).ok()?;
        let name = read_trimmed(&path.join("name"))?;
        let device = std::fs::canonicalize(path.join("device"))
            .ok()
            .and_then(|device| device.strip_prefix(&self.root).ok().map(Path::to_path_buf));
        let names: Vec<String> = std::fs::read_dir(&path)
            .ok()?
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        let mut channels = Vec::new();
        for kind in Kind::ALL {
            let mut indices: Vec<u8> = names
                .iter()
                .filter_map(|attr| channel_index(attr, kind))
                .collect();
            indices.sort_unstable();
            indices.dedup();
            for index in indices {
                let base = format!("{}{index}", kind.prefix());
                let candidates: &[&str] = match kind {
                    Kind::Pwm => &[""],
                    Kind::Power => &["_input", "_average"],
                    _ => &["_input"],
                };
                let Some(file) = candidates
                    .iter()
                    .map(|suffix| format!("{base}{suffix}"))
                    .find(|file| names.contains(file))
                else {
                    continue;
                };
                channels.push(ChannelLayout {
                    kind,
                    index,
                    label: read_trimmed(&path.join(format!("{base}_label"))),
                    file: path.join(file),
                });
            }
        }
        Some(ChipLayout {
            name,
            device,
            path,
            channels,
        })
    }
}

impl SensorSource for SysfsSource {
    fn chips(&self) -> Vec<Chip> {
        self.read(|_, _| true)
    }
}

/// N if `attr` is the value attribute of channel N of `kind`: `pwmN`, or
/// `tempN_input` and the like (also `powerN_average`).
fn channel_index(attr: &str, kind: Kind) -> Option<u8> {
    let rest = attr.strip_prefix(kind.prefix())?;
    let digits = match kind {
        Kind::Pwm => rest,
        Kind::Power => rest
            .strip_suffix("_input")
            .or_else(|| rest.strip_suffix("_average"))?,
        _ => rest.strip_suffix("_input")?,
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// The value in a sysfs file, in the unit of `kind`.
fn read_value(file: &Path, kind: Kind) -> Option<f64> {
    let raw = read_trimmed(file)?;
    Some(raw.parse::<i64>().ok()? as f64 / kind.divisor())
}

/// What libsensors reports for one chip; the app's FFI module fills it in.
#[derive(Debug, Clone, PartialEq)]
pub struct LmChip {
    /// The chip name as `sensors` prints it, e.g. `nct6687-isa-0a20`.
    pub name: String,
    /// The hwmon folder libsensors reads, e.g. `/sys/class/hwmon/hwmon0`.
    pub path: PathBuf,
    /// The features left after the `ignore` statements.
    pub features: Vec<LmFeature>,
}

/// One feature of an [`LmChip`].
#[derive(Debug, Clone, PartialEq)]
pub struct LmFeature {
    /// The sysfs channel name: `in0`, `fan1`, `temp1`, `power1`, …
    pub name: String,
    /// The name `sensors` shows.
    pub label: Option<String>,
    /// The input value after the `compute` statement; `None` if libsensors
    /// could not read it.
    pub value: Option<f64>,
}

/// A feature of a chip: the canonical hwmon folder and the channel name.
pub type FeatureKey = (PathBuf, String);

/// The voltage rails of `profile` on its hwmon device in `chips`: the
/// candidates of [`computed`]. Empty without that device.
pub fn profile_rails(chips: &[Chip], profile: &Profile) -> HashSet<FeatureKey> {
    let Some(chip) = find(chips, &profile.hwmon.name) else {
        return HashSet::new();
    };
    profile
        .voltages
        .iter()
        .map(|rail| (chip.path.clone(), format!("in{}", rail.input)))
        .collect()
}

/// One round of the check for `compute` statements: sysfs, libsensors and
/// sysfs again, read one right after another.
#[derive(Debug, Clone)]
pub struct Round {
    pub before: Vec<Chip>,
    pub lm: Vec<LmChip>,
    pub after: Vec<Chip>,
}

/// Which of the `candidates` a `compute` statement converts (§5.1, no
/// double scaling): in every round, the libsensors value differs from the
/// raw sysfs value both before and after it.
///
/// The candidates are the voltage rails of the board profile, the only
/// readings a multiplier applies to. Their driver keeps a reading for a
/// while (nct6683: one second), so a round sees at most one change, and
/// the before-and-after pair absorbs it. Other readings change between
/// any two reads (a CPU chiplet by degrees, a graphics card's power by
/// watts within milliseconds), so this comparison cannot tell a `compute`
/// statement from a change there.
pub fn computed(rounds: &[Round], candidates: &HashSet<FeatureKey>) -> HashSet<FeatureKey> {
    let mut result: Option<HashSet<FeatureKey>> = None;
    for round in rounds {
        let mut differing = HashSet::new();
        for lm_chip in &round.lm {
            let path = canonical(&lm_chip.path);
            for feature in &lm_chip.features {
                if !candidates.contains(&(path.clone(), feature.name.clone())) {
                    continue;
                }
                let raw = |chips: &[Chip]| {
                    let (kind, index) = channel(&feature.name)?;
                    let chip = chips.iter().find(|c| c.path == path)?;
                    Some(chip.reading(kind, index)?.value)
                };
                let (Some(value), Some(before), Some(after)) =
                    (feature.value, raw(&round.before), raw(&round.after))
                else {
                    continue;
                };
                if !same(value, before) && !same(value, after) {
                    differing.insert((path.clone(), feature.name.clone()));
                }
            }
        }
        result = Some(match result {
            None => differing,
            Some(earlier) => earlier.intersection(&differing).cloned().collect(),
        });
    }
    result.unwrap_or_default()
}

/// The sysfs readings with what libsensors adds (§5.1):
/// - a reading whose feature libsensors does not list is ignored by the
///   configuration and left out;
/// - the value comes from libsensors, marked `scaled` for the features in
///   `computed` (see [`computed`]);
/// - the label `sensors` shows goes to `sensors_label`.
///
/// A chip of libsensors matches a sysfs chip by folder and by name.
/// Duty cycles (`pwmN`) are not features of libsensors and stay as they are.
/// So do chips libsensors does not report, and readings it could not read:
/// they keep the sysfs value, or NaN when it was not read
/// ([`SysfsSource::read`]), for [`SysfsSource::fill`].
pub fn merge(mut chips: Vec<Chip>, lm: &[LmChip], computed: &HashSet<FeatureKey>) -> Vec<Chip> {
    for lm_chip in lm {
        let path = canonical(&lm_chip.path);
        let Some(chip) = chips.iter_mut().find(|chip| {
            chip.path == path
                && lm_chip
                    .name
                    .strip_prefix(chip.name.as_str())
                    .is_some_and(|rest| rest.starts_with('-'))
        }) else {
            continue;
        };
        chip.readings.retain_mut(|reading| {
            if reading.kind == Kind::Pwm {
                return true;
            }
            let name = format!("{}{}", reading.kind.prefix(), reading.index);
            let Some(feature) = lm_chip.features.iter().find(|f| f.name == name) else {
                return false;
            };
            reading.sensors_label = feature.label.clone();
            if let Some(value) = feature.value {
                reading.value = value;
                reading.scaled = computed.contains(&(path.clone(), name));
            }
            true
        });
    }
    chips
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// The kind and index of a channel name such as `in0` or `temp1`.
fn channel(name: &str) -> Option<(Kind, u8)> {
    Kind::ALL.into_iter().find_map(|kind| {
        let digits = name.strip_prefix(kind.prefix())?;
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some((kind, digits.parse().ok()?))
    })
}

/// Equal up to rounding: libsensors and this module divide the same
/// integer, but a different order of operations may differ in the last bit.
fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// The contents of a sysfs file without surrounding white space; `None` if
/// it is missing, unreadable or blank.
fn read_trimmed(path: &Path) -> Option<String> {
    let value = std::fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names() {
        assert_eq!(channel_index("temp1_input", Kind::Temp), Some(1));
        assert_eq!(channel_index("in13_input", Kind::Voltage), Some(13));
        assert_eq!(channel_index("pwm16", Kind::Pwm), Some(16));
        assert_eq!(channel_index("power1_average", Kind::Power), Some(1));
        assert_eq!(channel_index("power1_input", Kind::Power), Some(1));
        for (attr, kind) in [
            ("temp1_label", Kind::Temp),
            ("temp1_crit", Kind::Temp),
            ("temp_input", Kind::Temp),
            ("pwm1_enable", Kind::Pwm),
            ("pwm1_auto_point1_pwm", Kind::Pwm),
            ("temp1_average", Kind::Temp),
            ("intrusion0_alarm", Kind::Voltage),
            ("fan1_input", Kind::Temp),
            ("temp+1_input", Kind::Temp),
            ("pwm999", Kind::Pwm),
        ] {
            assert_eq!(channel_index(attr, kind), None, "{attr}");
        }
    }

    #[test]
    fn only_one_chip_of_a_name_is_found() {
        let chip = |name: &str, device: &str| Chip {
            name: name.into(),
            device: Some(device.into()),
            path: PathBuf::new(),
            readings: Vec::new(),
        };
        let chips = [
            chip("nct6687", "devices/platform/nct6683.2592"),
            chip("nvme", "a"),
            chip("nvme", "b"),
        ];
        assert_eq!(
            find(&chips, "nct6687").map(|c| c.name.as_str()),
            Some("nct6687")
        );
        assert_eq!(find(&chips, "nvme"), None);
        assert_eq!(find(&chips, "k10temp"), None);
    }

    #[test]
    fn channel_of_a_feature_name() {
        assert_eq!(channel("in0"), Some((Kind::Voltage, 0)));
        assert_eq!(channel("temp13"), Some((Kind::Temp, 13)));
        assert_eq!(channel("fan16"), Some((Kind::Fan, 16)));
        assert_eq!(channel("power1"), Some((Kind::Power, 1)));
        // "intrusion0" starts with "in", but is not a voltage.
        for name in ["intrusion0", "beep_enable", "in", "temp1_input", "curr1"] {
            assert_eq!(channel(name), None, "{name}");
        }
    }

    #[test]
    fn same_value_up_to_rounding() {
        assert!(same(1.008, 1008.0 / 1000.0));
        assert!(same(1.008, 1.008 + 1e-12));
        assert!(!same(1.008, 1.009));
        assert!(!same(12.096, 1.008));
        assert!(same(0.0, 0.0));
        assert!(!same(0.0, 0.001));
    }

    #[test]
    fn missing_hwmon_class_gives_no_chips() {
        let root = std::env::temp_dir().join(format!("baseboard-sensors-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        assert!(SysfsSource::new(&root).chips().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
