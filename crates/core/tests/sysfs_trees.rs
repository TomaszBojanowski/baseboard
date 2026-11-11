// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests on the fake sysfs trees in `tests/fixtures/`
//! (docs/PROJEKT.md §7, M1 step 4): readings from sysfs, and the fan
//! channel capabilities and driver level for three variants of the
//! `nct6683` driver.
//!
//! Git keeps no write bits, so each tree has a `tryby` file with the file
//! modes. A test copies the tree to a temporary folder, makes every file
//! 0444 and then applies `tryby` (and, for the mixed case, one more file).

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use baseboard_core::fans::driver::{self, Capability, DriverLevel};
use baseboard_core::overview;
use baseboard_core::profile::{self, Profile};
use baseboard_core::sensors::{self, Chip, Kind, SensorSource, SysfsSource};
use baseboard_core::udev::NoUdev;
use baseboard_core::units::Numbers;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
const PROFILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/boards/ms-7e61.toml"
);

const FEDORA: &str = "sysfs-ms7e61-fedora-7.3.3";
const HWMON_NEXT: &str = "sysfs-ms7e61-hwmon-next";
const V1: &str = "sysfs-ms7e61-v1";

/// A copy of one fixture tree with its file modes applied, removed at the
/// end of the test.
struct Tree {
    root: PathBuf,
}

impl Tree {
    /// Copies fixture `name`, then applies its `tryby` and the mode files
    /// in `extra`, in this order (later lines win).
    fn new(name: &str, extra: &[&str]) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "baseboard-tree-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        let source = Path::new(FIXTURES).join(name);
        for part in ["class", "devices"] {
            copy(&source.join(part), &root.join(part));
        }
        let tree = Self { root };
        for modes in std::iter::once("tryby").chain(extra.iter().copied()) {
            let text = std::fs::read_to_string(source.join(modes)).unwrap();
            tree.apply(&text);
        }
        tree
    }

    /// Applies lines of the form `0644 path/to/file`; the file name may
    /// have one `*`. Every line must match at least one file.
    fn apply(&self, modes: &str) {
        for line in modes.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (mode, pattern) = line.split_once(' ').unwrap();
            let mode = u32::from_str_radix(mode, 8).unwrap();
            let pattern = Path::new(pattern);
            let dir = self.root.join(pattern.parent().unwrap());
            let file = pattern.file_name().unwrap().to_str().unwrap();
            let matched: Vec<PathBuf> = match file.split_once('*') {
                None => vec![dir.join(file)],
                Some((prefix, suffix)) => std::fs::read_dir(&dir)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| {
                        let name = path.file_name().unwrap().to_str().unwrap();
                        name.len() >= prefix.len() + suffix.len()
                            && name.starts_with(prefix)
                            && name.ends_with(suffix)
                    })
                    .collect(),
            };
            assert!(!matched.is_empty(), "no file for: {line}");
            for path in matched {
                assert!(path.symlink_metadata().unwrap().is_file(), "{line}");
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            }
        }
    }

    fn chips(&self) -> Vec<Chip> {
        SysfsSource::new(&self.root).chips()
    }

    /// The hwmon device of the board's profile.
    fn board_chip(&self) -> Chip {
        sensors::find(&self.chips(), &profile().hwmon.name)
            .cloned()
            .unwrap()
    }

    fn channels(&self) -> Vec<driver::Channel> {
        driver::channels(&profile(), &self.board_chip().path)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Copies a folder, keeping symbolic links as they are; files become 0444.
fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_symlink() {
            let link = std::fs::read_link(entry.path()).unwrap();
            std::os::unix::fs::symlink(link, &target).unwrap();
        } else if kind.is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
            std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o444)).unwrap();
        }
    }
}

fn profile() -> Profile {
    profile::load(Path::new(PROFILE)).unwrap()
}

/// One attribute that cannot be read (a folder in place of a file) and one
/// with garbage: their readings stay with NaN and show "–" with no range
/// and no rating; every other reading goes on (§5.1).
#[test]
fn unreadable_attributes() {
    let tree = Tree::new(FEDORA, NONE);
    let board = sensors::find(&tree.chips(), "nct6687")
        .unwrap()
        .path
        .clone();
    std::fs::remove_file(board.join("temp2_input")).unwrap();
    std::fs::create_dir(board.join("temp2_input")).unwrap();
    let in0 = board.join("in0_input");
    std::fs::set_permissions(&in0, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::write(&in0, "garbage\n").unwrap();

    let chips = tree.chips();
    let chip = sensors::find(&chips, "nct6687").unwrap();
    assert!(chip.reading(Kind::Temp, 2).unwrap().value.is_nan());
    assert!(chip.reading(Kind::Voltage, 0).unwrap().value.is_nan());
    assert_eq!(chip.reading(Kind::Temp, 1).unwrap().value, 68.0);
    let readable = chips
        .iter()
        .flat_map(|c| &c.readings)
        .filter(|r| r.value.is_finite())
        .count();
    let all: usize = chips.iter().map(|c| c.readings.len()).sum();
    assert_eq!(readable, all - 2);

    let profile = profile();
    let channels = driver::channels(&profile, &board);
    let context = overview::Context {
        sysfs_root: &tree.root,
        udev: &NoUdev,
        board: Some(overview::Board {
            profile: &profile,
            channels: &channels,
            name: None,
        }),
        cpu: None,
        sensors_labels: false,
        labels: None,
    };
    let mut groups = overview::build(&chips, &context);
    overview::Extremes::default().update(&mut groups);
    let row = |kind: overview::GroupKind, channel: &str| {
        groups
            .iter()
            .find(|g| g.kind == kind)
            .and_then(|g| g.rows.iter().find(|r| r.id.channel == channel))
            .unwrap()
            .clone()
    };
    let system = row(overview::GroupKind::Board, "temp2");
    assert!(system.value.is_nan());
    assert_eq!(system.range, None);
    let rail = row(overview::GroupKind::Voltages, "in0");
    assert_eq!(rail.rating, Some(overview::Rating::Unrated));
    let numbers = Numbers::new(",");
    assert_eq!(numbers.value(Kind::Temp, system.value), "–");
    assert_eq!(numbers.range(Kind::Temp, f64::NAN, 40.0), "");
    // The other rows are read as before.
    assert_eq!(row(overview::GroupKind::Board, "temp3").value, 36.0);
}

const NONE: &[&str] = &[];
const READ: &[&str] = &["duty-read"];
const MANUAL_CURVE_VIEW: &[&str] = &[
    "duty-read",
    "mode",
    "duty-write",
    "curve-read",
    "source-read",
];
const CURVES: &[&str] = &[
    "duty-read",
    "mode",
    "duty-write",
    "curve-read",
    "curve-write",
    "source-read",
];

/// Checks the capabilities of every profile channel and the summary.
fn check(tree: &Tree, expected: &[(&str, &[&str])], level: DriverLevel) {
    let channels = tree.channels();
    for channel in &channels {
        let caps = channel.capabilities;
        println!(
            "{:6} pwm{:<2} {:18} [{}] points {}",
            channel.key,
            channel.pwm,
            caps.level().as_str(),
            caps.iter()
                .map(Capability::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            caps.curve_points(),
        );
    }
    let found: Vec<(&str, Vec<&str>)> = channels
        .iter()
        .map(|c| {
            let names = c.capabilities.iter().map(Capability::as_str).collect();
            (c.key.as_str(), names)
        })
        .collect();
    let expected: Vec<(&str, Vec<&str>)> = expected
        .iter()
        .map(|(key, caps)| (*key, caps.to_vec()))
        .collect();
    assert_eq!(found, expected);
    for channel in &channels {
        let points = if channel.capabilities.contains(Capability::CurveRead) {
            7
        } else {
            0
        };
        assert_eq!(
            channel.capabilities.curve_points(),
            points,
            "{}",
            channel.key
        );
    }
    assert_eq!(driver::driver_level(&channels), level);
}

#[test]
fn fedora_7_3_3_is_read_only() {
    let tree = Tree::new(FEDORA, &[]);
    check(
        &tree,
        &[
            ("cpu1", READ),
            ("pump1", READ),
            ("sys1", NONE),
            ("sys2", NONE),
            ("sys3", NONE),
            ("sys4", NONE),
            ("ezconn", READ),
        ],
        DriverLevel::ReadOnly,
    );
    assert!(tree.channels().iter().all(|c| !c.controllable));
}

#[test]
fn hwmon_next_has_manual_mode_and_reads_curves() {
    let tree = Tree::new(HWMON_NEXT, &[]);
    check(
        &tree,
        &[
            ("cpu1", MANUAL_CURVE_VIEW),
            ("pump1", MANUAL_CURVE_VIEW),
            ("sys1", MANUAL_CURVE_VIEW),
            ("sys2", MANUAL_CURVE_VIEW),
            ("sys3", MANUAL_CURVE_VIEW),
            ("sys4", MANUAL_CURVE_VIEW),
            ("ezconn", READ),
        ],
        DriverLevel::ManualCurveView,
    );
}

#[test]
fn v1_writes_curves() {
    let tree = Tree::new(V1, &[]);
    check(
        &tree,
        &[
            ("cpu1", CURVES),
            ("pump1", CURVES),
            ("sys1", CURVES),
            ("sys2", CURVES),
            ("sys3", CURVES),
            ("sys4", CURVES),
            ("ezconn", READ),
        ],
        DriverLevel::Curves,
    );
    let controllable: Vec<_> = tree
        .channels()
        .into_iter()
        .filter(|c| c.controllable)
        .map(|c| c.key)
        .collect();
    assert_eq!(
        controllable,
        ["cpu1", "pump1", "sys1", "sys2", "sys3", "sys4"]
    );
}

/// One channel whose curve did not pass the driver's check at load keeps a
/// read-only curve; the others can still write theirs.
#[test]
fn v1_with_one_read_only_curve() {
    let tree = Tree::new(V1, &["tryby-mieszane"]);
    check(
        &tree,
        &[
            ("cpu1", CURVES),
            ("pump1", CURVES),
            ("sys1", CURVES),
            ("sys2", CURVES),
            ("sys3", CURVES),
            ("sys4", MANUAL_CURVE_VIEW),
            ("ezconn", READ),
        ],
        DriverLevel::ManualCurveView,
    );
    let levels: Vec<_> = tree
        .channels()
        .iter()
        .map(|c| (c.pwm, c.capabilities.level()))
        .collect();
    assert_eq!(
        levels,
        [
            (1, DriverLevel::Curves),
            (2, DriverLevel::Curves),
            (16, DriverLevel::Curves),
            (15, DriverLevel::Curves),
            (14, DriverLevel::Curves),
            (13, DriverLevel::ManualCurveView),
            (4, DriverLevel::ReadOnly),
        ]
    );
}

#[test]
fn devices_are_found_by_name_and_path_not_number() {
    let mut folders = Vec::new();
    for name in [FEDORA, HWMON_NEXT, V1] {
        let tree = Tree::new(name, &[]);
        let chip = tree.board_chip();
        assert_eq!(
            chip.device.as_deref(),
            Some(Path::new("devices/platform/nct6683.2592")),
            "{name}"
        );
        folders.push(chip.path.file_name().unwrap().to_owned());

        let chips = tree.chips();
        let names: Vec<_> = chips.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "amdgpu", "k10temp", "nct6687", "nvme", "nvme", "spd5118", "spd5118"
            ],
            "{name}"
        );
        // Two disks: the same name, told apart by the device.
        let disks: Vec<_> = chips
            .iter()
            .filter(|c| c.name == "nvme")
            .map(|c| c.device.clone().unwrap())
            .collect();
        assert_eq!(
            disks,
            [
                PathBuf::from("devices/pci0000:00/0000:00:01.2/0000:04:00.0/nvme/nvme0"),
                PathBuf::from("devices/pci0000:00/0000:00:02.2/0000:11:00.0/nvme/nvme1"),
            ],
            "{name}"
        );
        assert!(sensors::find(&chips, "nvme").is_none());
    }
    // The trees number the board's hwmon device differently.
    assert_eq!(folders, ["hwmon0", "hwmon3", "hwmon7"]);
}

/// The duty cycle of SYS 1–4 is read only where `pwm13`–`pwm16` exist.
#[test]
fn sys_duty_only_with_pwm13_to_16() {
    let duties = |name: &str| -> Vec<(String, Option<f64>)> {
        let tree = Tree::new(name, &[]);
        let chip = tree.board_chip();
        profile()
            .fans
            .iter()
            .map(|fan| {
                let duty = fan
                    .pwm_index
                    .and_then(|n| chip.reading(Kind::Pwm, n))
                    .map(|r| r.value);
                (fan.key.clone(), duty)
            })
            .collect()
    };
    let expect = |values: [Option<f64>; 7]| -> Vec<(String, Option<f64>)> {
        ["cpu1", "pump1", "sys1", "sys2", "sys3", "sys4", "ezconn"]
            .into_iter()
            .map(String::from)
            .zip(values)
            .collect()
    };
    assert_eq!(
        duties(FEDORA),
        expect([
            Some(124.0),
            Some(255.0),
            None,
            None,
            None,
            None,
            Some(153.0)
        ])
    );
    let with_sys = expect([
        Some(124.0),
        Some(255.0),
        Some(98.0),
        Some(108.0),
        Some(76.0),
        Some(102.0),
        Some(153.0),
    ]);
    assert_eq!(duties(HWMON_NEXT), with_sys);
    assert_eq!(duties(V1), with_sys);
}

#[test]
fn readings_in_display_units() {
    let tree = Tree::new(FEDORA, &[]);
    let chips = tree.chips();
    let chip = |name: &str| sensors::find(&chips, name).unwrap();
    let value = |name: &str, kind: Kind, index: u8| {
        let reading = chip(name).reading(kind, index).unwrap();
        (reading.label.as_deref(), reading.value)
    };
    assert_eq!(
        value("nct6687", Kind::Temp, 1),
        (Some("AMD TSI Addr 98h"), 68.0)
    );
    assert_eq!(value("nct6687", Kind::Fan, 1), (None, 890.0));
    assert_eq!(value("nct6687", Kind::Voltage, 0), (Some("VIN0"), 1.008));
    assert_eq!(value("nct6687", Kind::Voltage, 13), (Some("VREF"), 1.824));
    assert_eq!(value("k10temp", Kind::Temp, 1), (Some("Tctl"), 68.25));
    assert_eq!(value("amdgpu", Kind::Power, 1), (Some("PPT"), 12.0));
    assert_eq!(value("amdgpu", Kind::Voltage, 0), (Some("vddgfx"), 0.173));

    // Every channel of the board's device, in order; no limits or labels
    // as readings of their own.
    let board = chip("nct6687");
    let count = |kind: Kind| board.readings.iter().filter(|r| r.kind == kind).count();
    assert_eq!(count(Kind::Temp), 5);
    assert_eq!(count(Kind::Fan), 8);
    assert_eq!(count(Kind::Voltage), 14);
    assert_eq!(count(Kind::Pwm), 4);
    assert!(board.readings.is_sorted_by_key(|r| (r.kind, r.index)));
}
