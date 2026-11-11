// SPDX-License-Identifier: GPL-3.0-or-later

//! The source of sensor readings (docs/PROJEKT.md §5.1): libsensors laid
//! over raw sysfs, or raw sysfs alone when libsensors is missing or fails,
//! or when a debug build reads a fake sysfs tree (libsensors always reads
//! /sys). Choosing never fails and never shows an error.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use baseboard_core::profile::Profile;
use baseboard_core::sensors::{self, Chip, FeatureKey, Kind, Round, SensorSource, SysfsSource};
use gtk::glib;

use crate::libsensors::Libsensors;

/// Rounds of the check for `compute` statements at start (§5.1).
const ROUNDS: usize = 3;

/// libsensors with the system configuration, over raw sysfs. It stays on
/// the thread that made it, like [`Libsensors`].
///
/// libsensors reads its list of chips once, so the rails with `compute`
/// are found once too. When hwmon devices come and go (a driver reloaded),
/// the source is made again (`sampler`).
struct LibsensorsSource {
    lib: Libsensors,
    sysfs: SysfsSource,
    computed: HashSet<FeatureKey>,
    /// The folders of the chips libsensors reports: their values come from
    /// libsensors only, so that every sensor is read once a sample.
    covered: HashSet<PathBuf>,
}

impl LibsensorsSource {
    fn new(lib: Libsensors, profile: Option<&Profile>) -> Self {
        let sysfs = SysfsSource::new(Path::new("/sys"));
        let rails = profile
            .map(|profile| sensors::profile_rails(&sysfs.chips(), profile))
            .unwrap_or_default();
        let rounds: Vec<Round> = if rails.is_empty() {
            Vec::new()
        } else {
            (0..ROUNDS)
                .map(|_| Round {
                    before: sysfs.chips(),
                    lm: lib.chips(),
                    after: sysfs.chips(),
                })
                .collect()
        };
        let computed = sensors::computed(&rounds, &rails);
        let mut names: Vec<String> = computed
            .iter()
            .map(|(path, name)| format!("{} {name}", path.display()))
            .collect();
        names.sort();
        glib::g_debug!(
            "baseboard",
            "lm-sensors compute statements found for: {}",
            if names.is_empty() {
                "none".to_owned()
            } else {
                names.join(", ")
            }
        );
        let covered = lib
            .chips()
            .iter()
            .filter_map(|chip| std::fs::canonicalize(&chip.path).ok())
            .collect();
        Self {
            lib,
            sysfs,
            computed,
            covered,
        }
    }
}

impl SensorSource for LibsensorsSource {
    /// Each sensor is read once: by libsensors, or from sysfs for duty
    /// cycles, for chips libsensors does not report and for values it could
    /// not read.
    fn chips(&self) -> Vec<Chip> {
        let lm = self.lib.chips();
        let raw = self
            .sysfs
            .read(|path, kind| kind == Kind::Pwm || !self.covered.contains(path));
        let mut chips = sensors::merge(raw, &lm, &self.computed);
        self.sysfs.fill(&mut chips);
        chips
    }
}

/// Which source was chosen: libsensors and its version, or raw sysfs and
/// why not libsensors. The System page shows it (§5.4); the Overview
/// never shows an error for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Libsensors { version: String },
    Sysfs { reason: String },
}

/// The source to read from, for the sysfs at `root`, and which it is. The
/// board profile names the voltage rails that may have a `compute`
/// statement (§5.1).
pub fn source(root: &Path, profile: Option<&Profile>) -> (Box<dyn SensorSource>, Choice) {
    let (source, choice) = choose(root, || {
        let lib = Libsensors::load(None).map_err(|e| e.to_string())?;
        let version = lib.version().to_owned();
        glib::g_debug!("baseboard", "libsensors {version} loaded");
        Ok((Box::new(LibsensorsSource::new(lib, profile)), version))
    });
    match &choice {
        Choice::Libsensors { .. } => {
            glib::g_debug!("baseboard", "readings: libsensors over sysfs")
        }
        Choice::Sysfs { reason } => glib::g_debug!("baseboard", "readings: raw sysfs ({reason})"),
    }
    (source, choice)
}

type Loaded = (Box<dyn SensorSource>, String);

fn choose(
    root: &Path,
    libsensors: impl FnOnce() -> Result<Loaded, String>,
) -> (Box<dyn SensorSource>, Choice) {
    if root != Path::new("/sys") {
        let reason = format!("{} is not /sys, which libsensors reads", root.display());
        return (Box::new(SysfsSource::new(root)), Choice::Sysfs { reason });
    }
    match libsensors() {
        Ok((source, version)) => (source, Choice::Libsensors { version }),
        Err(reason) => (Box::new(SysfsSource::new(root)), Choice::Sysfs { reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;

    impl SensorSource for Fake {
        fn chips(&self) -> Vec<Chip> {
            Vec::new()
        }
    }

    #[test]
    fn libsensors_when_it_loads() {
        let (_, choice) = choose(Path::new("/sys"), || Ok((Box::new(Fake), "3.6.0".into())));
        assert_eq!(
            choice,
            Choice::Libsensors {
                version: "3.6.0".into()
            }
        );
    }

    /// No libsensors: raw sysfs, with the reason for the System page.
    #[test]
    fn raw_sysfs_when_libsensors_fails() {
        let (_, choice) = choose(Path::new("/sys"), || {
            Err("libsensors not loaded: libsensors.so.4: cannot open".into())
        });
        assert_eq!(
            choice,
            Choice::Sysfs {
                reason: "libsensors not loaded: libsensors.so.4: cannot open".into()
            }
        );
    }

    #[test]
    fn raw_sysfs_for_a_fake_tree() {
        let (_, choice) = choose(Path::new("/tmp/fake-sysfs"), || {
            panic!("libsensors must not be loaded for a fake tree")
        });
        assert!(matches!(choice, Choice::Sysfs { .. }));
    }
}
