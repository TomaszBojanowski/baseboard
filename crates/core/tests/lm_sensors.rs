// SPDX-License-Identifier: GPL-3.0-or-later

//! Readings of libsensors laid over sysfs (docs/PROJEKT.md §5.1), on the
//! fake Fedora 7.3.3 tree. The libsensors side is what the lm-sensors
//! configuration of this board (lm-sensors PR #582) gives: `compute` for
//! in0 (×12), in1 (×5) and in3 (×2), `ignore` for in5–in7, fan3 and temp5,
//! and labels. Voltages must be scaled exactly once, with libsensors and
//! without it, also when live values change between reads.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use baseboard_core::profile::{self, Profile};
use baseboard_core::sensors::{
    self, Chip, FeatureKey, Kind, LmChip, LmFeature, Round, SensorSource, SysfsSource,
};

const TREE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/fixtures/sysfs-ms7e61-fedora-7.3.3"
);
const PROFILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/boards/ms-7e61.toml"
);

const IGNORED: [&str; 5] = ["in5", "in6", "in7", "fan3", "temp5"];

fn profile() -> Profile {
    profile::load(Path::new(PROFILE)).unwrap()
}

fn sysfs() -> Vec<Chip> {
    SysfsSource::new(Path::new(TREE)).chips()
}

fn board(chips: &[Chip]) -> &Chip {
    sensors::find(chips, "nct6687").unwrap()
}

fn board_path() -> PathBuf {
    board(&sysfs()).path.clone()
}

fn key(name: &str) -> FeatureKey {
    (board_path(), name.to_owned())
}

/// What libsensors reports for nct6687 with the board's configuration.
fn configured(chips: &[Chip]) -> LmChip {
    let factor = |name: &str| match name {
        "in0" => 12.0,
        "in1" => 5.0,
        "in3" => 2.0,
        _ => 1.0,
    };
    let label = |name: &str| match name {
        "in0" => Some("+12V"),
        "in1" => Some("+5V"),
        "in3" => Some("CPU VDDIO"),
        "temp1" => Some("CPU"),
        "fan16" => Some("SYS Fan 1"),
        _ => None,
    };
    let features = board(chips)
        .readings
        .iter()
        .filter(|r| r.kind != Kind::Pwm)
        .map(|r| (format!("{}{}", r.kind.prefix(), r.index), r))
        .filter(|(name, _)| !IGNORED.contains(&name.as_str()))
        .map(|(name, r)| LmFeature {
            // Without a label in the configuration, `sensors` shows the
            // driver's label or the channel name.
            label: Some(
                label(&name)
                    .map(String::from)
                    .or_else(|| r.label.clone())
                    .unwrap_or_else(|| name.clone()),
            ),
            value: Some(r.value * factor(&name)),
            name,
        })
        .collect();
    LmChip {
        name: "nct6687-isa-0a20".into(),
        // libsensors names the folder through the class link.
        path: Path::new(TREE).join("class/hwmon/hwmon0"),
        features,
    }
}

fn set_value(lm: &mut LmChip, name: &str, value: Option<f64>) {
    lm.features
        .iter_mut()
        .find(|f| f.name == name)
        .unwrap()
        .value = value;
}

fn round(lm: LmChip) -> Round {
    Round {
        before: sysfs(),
        lm: vec![lm],
        after: sysfs(),
    }
}

/// The voltage rails of the profile: in0–in4 and in8.
fn rails() -> HashSet<FeatureKey> {
    sensors::profile_rails(&sysfs(), &profile())
}

/// Three rounds without noise: the rails with `compute`.
fn computed() -> HashSet<FeatureKey> {
    sensors::computed(&[0, 1, 2].map(|_| round(configured(&sysfs()))), &rails())
}

fn volts(chip: &Chip, key: &str) -> (f64, bool) {
    let profile = profile();
    let rail = profile.voltages.iter().find(|v| v.key == key).unwrap();
    let reading = chip.reading(Kind::Voltage, rail.input).unwrap();
    (rail.volts(reading), reading.scaled)
}

#[test]
fn compute_statements_are_found() {
    assert_eq!(
        computed(),
        HashSet::from([key("in0"), key("in1"), key("in3")])
    );
    assert_eq!(
        rails(),
        ["in0", "in1", "in2", "in3", "in4", "in8"]
            .map(key)
            .into_iter()
            .collect()
    );
    assert!(sensors::computed(&[], &rails()).is_empty());
    // Only candidates are checked: a temperature that differs in every
    // round (a fast-changing chiplet, or a `compute` there) is not one.
    let mut hot = configured(&sysfs());
    set_value(&mut hot, "temp1", Some(99.0));
    let rounds = [round(hot.clone()), round(hot.clone()), round(hot)];
    assert!(!sensors::computed(&rounds, &rails()).contains(&key("temp1")));
    // Without a profile there are no candidates.
    let mut other = profile();
    other.hwmon.name = "it8688".into();
    assert!(sensors::profile_rails(&sysfs(), &other).is_empty());
}

#[test]
fn voltages_are_scaled_exactly_once() {
    let plain = sysfs();
    let merged = sensors::merge(sysfs(), &[configured(&plain)], &computed());
    // in0 1008 mV, in1 992 mV, in3 672 mV, in4 1344 mV, in8 3312 mV.
    for (key, expected, compute) in [
        ("12v", 1.008 * 12.0, true),
        ("5v", 0.992 * 5.0, true),
        ("vddio", 0.672 * 2.0, true),
        ("vcore", 1.344, false),
        ("3v3", 3.312, false),
    ] {
        // With libsensors: its `compute`, not the profile's multiplier.
        assert_eq!(volts(board(&merged), key), (expected, compute), "{key}");
        // Without libsensors: the profile's multiplier.
        assert_eq!(volts(board(&plain), key), (expected, false), "{key}");
    }
}

/// A live value that changed between the reads is not a `compute`
/// statement: libsensors equal to sysfs after it, or different in one
/// round only (a flip there and back).
#[test]
fn changing_values_are_not_compute() {
    let plain = sysfs();
    // in4 read 1.360 V by libsensors, 1.344 V by sysfs before and after.
    let mut flipped = configured(&plain);
    set_value(&mut flipped, "in4", Some(1.360));
    // in8 changed right before libsensors read it: sysfs after agrees.
    let mut changed = configured(&plain);
    set_value(&mut changed, "in8", Some(3.328));
    let mut after = sysfs();
    after
        .iter_mut()
        .find(|c| c.name == "nct6687")
        .unwrap()
        .readings
        .iter_mut()
        .find(|r| r.kind == Kind::Voltage && r.index == 8)
        .unwrap()
        .value = 3.328;
    let rounds = [
        round(flipped),
        Round {
            before: sysfs(),
            lm: vec![changed.clone()],
            after: after.clone(),
        },
        Round {
            before: sysfs(),
            lm: vec![changed],
            after,
        },
    ];
    let found = sensors::computed(&rounds, &rails());
    assert!(!found.contains(&key("in4")));
    assert!(!found.contains(&key("in8")));
    assert_eq!(found, HashSet::from([key("in0"), key("in1"), key("in3")]));

    // Merged with that noise, in4 keeps the profile's multiplier (×1).
    let mut noisy = configured(&plain);
    set_value(&mut noisy, "in4", Some(1.360));
    let merged = sensors::merge(sysfs(), &[noisy], &found);
    assert_eq!(volts(board(&merged), "vcore"), (1.360, false));
}

#[test]
fn ignored_features_are_left_out() {
    let plain = sysfs();
    let merged = sensors::merge(sysfs(), &[configured(&plain)], &computed());
    let names = |chip: &Chip| -> Vec<String> {
        chip.readings
            .iter()
            .map(|r| format!("{}{}", r.kind.prefix(), r.index))
            .collect()
    };
    let before = names(board(&plain));
    let after = names(board(&merged));
    for name in IGNORED {
        assert!(before.contains(&name.to_owned()), "{name}");
        assert!(!after.contains(&name.to_owned()), "{name}");
    }
    assert_eq!(before.len() - IGNORED.len(), after.len());
    // Duty cycles are not features of libsensors and stay.
    for n in 1..=4 {
        assert!(board(&merged).reading(Kind::Pwm, n).is_some(), "pwm{n}");
    }
}

#[test]
fn labels_from_the_configuration() {
    let plain = sysfs();
    let merged = sensors::merge(sysfs(), &[configured(&plain)], &computed());
    let chip = board(&merged);
    let labels = |kind: Kind, index: u8| {
        let r = chip.reading(kind, index).unwrap();
        (r.label.as_deref(), r.sensors_label.as_deref())
    };
    assert_eq!(labels(Kind::Voltage, 0), (Some("VIN0"), Some("+12V")));
    assert_eq!(labels(Kind::Voltage, 2), (Some("VIN2"), Some("VIN2")));
    assert_eq!(
        labels(Kind::Temp, 1),
        (Some("AMD TSI Addr 98h"), Some("CPU"))
    );
    assert_eq!(labels(Kind::Fan, 16), (None, Some("SYS Fan 1")));
    assert_eq!(labels(Kind::Fan, 1), (None, Some("fan1")));
    assert_eq!(labels(Kind::Pwm, 1), (None, None));
    // Without libsensors there is no `sensors` label.
    assert!(
        board(&plain)
            .readings
            .iter()
            .all(|r| r.sensors_label.is_none() && !r.scaled)
    );
}

#[test]
fn other_chips_stay_as_in_sysfs() {
    let plain = sysfs();
    let merged = sensors::merge(sysfs(), &[configured(&plain)], &computed());
    assert_eq!(plain.len(), merged.len());
    for (a, b) in plain.iter().zip(&merged) {
        if a.name != "nct6687" {
            assert_eq!(a, b, "{}", a.name);
        }
    }
}

/// Reading every sensor once (libsensors for its chips, sysfs for duty
/// cycles, for the other chips and for what libsensors could not read)
/// gives what reading everything from both gives.
#[test]
fn every_sensor_read_once() {
    let plain = sysfs();
    let mut lm = configured(&plain);
    set_value(&mut lm, "in4", None);
    let both = sensors::merge(sysfs(), std::slice::from_ref(&lm), &computed());

    let source = SysfsSource::new(Path::new(TREE));
    let nct = board_path();
    let raw = source.read(|path, kind| kind == Kind::Pwm || path != nct);
    for chip in &raw {
        for reading in &chip.readings {
            let skipped = chip.path == nct && reading.kind != Kind::Pwm;
            assert_eq!(reading.value.is_nan(), skipped, "{} {reading:?}", chip.name);
        }
    }
    let mut once = sensors::merge(raw, &[lm], &computed());
    source.fill(&mut once);
    assert_eq!(once, both);
    // The value libsensors could not read came from sysfs.
    assert_eq!(volts(board(&once), "vcore"), (1.344, false));
}

#[test]
fn unreadable_value_keeps_the_sysfs_value() {
    let plain = sysfs();
    let mut lm = configured(&plain);
    set_value(&mut lm, "in0", None);
    let merged = sensors::merge(sysfs(), &[lm], &computed());
    // Not converted by libsensors, so the profile's multiplier applies.
    assert_eq!(volts(board(&merged), "12v"), (1.008 * 12.0, false));
}

#[test]
fn chips_are_matched_by_folder_and_name() {
    let plain = sysfs();
    // Another folder changes nothing.
    let mut elsewhere = configured(&plain);
    elsewhere.path = PathBuf::from("/nonexistent/hwmon0");
    assert_eq!(sensors::merge(sysfs(), &[elsewhere], &computed()), plain);
    // Nor does another chip in the same folder (e.g. a stale libsensors
    // list after the driver was reloaded).
    let mut renamed = configured(&plain);
    renamed.name = "nct6687x-isa-0a20".into();
    assert_eq!(sensors::merge(sysfs(), &[renamed], &computed()), plain);
    assert_eq!(sensors::merge(sysfs(), &[], &HashSet::new()), plain);
}
