// SPDX-License-Identifier: GPL-3.0-or-later

//! Board detection at startup (docs/PROJEKT.md §6.1) and the fan driver
//! level (§5.5.2).

use std::path::Path;

use baseboard_core::board::{self, Detection, Dmi, Profiles};
use baseboard_core::fans::driver::{self, Capability, Channel, DriverLevel};
use baseboard_core::profile::Profile;
use baseboard_core::sensors::{self, SensorSource, SysfsSource};
use gtk::glib;

use crate::config;
use crate::system::{FileKind, SkippedFile};

/// Reads DMI and the board profiles and chooses the profile. Invalid
/// profile files are skipped: reported on stderr and returned for the
/// System page.
pub fn detect() -> (Dmi, Detection, Vec<SkippedFile>) {
    let dmi = Dmi::read(&config::sysfs_root());
    let loaded = Profiles::load(&config::board_profile_dirs());
    let skipped = skipped_profiles(&loaded);
    let detection = board::detect(&dmi, &loaded.profiles);
    glib::g_debug!(
        "baseboard",
        "board code {:?}, PCB revision {:?}, {} profile(s) loaded: {}",
        dmi.board_code(),
        dmi.pcb_revision(),
        loaded.profiles.len(),
        match &detection {
            Detection::Supported(profile) => format!("profile {}", profile.id),
            Detection::UnverifiedRevision { profile, .. } => format!(
                "profile {}, PCB revision not verified, read only",
                profile.id
            ),
            Detection::OtherModel { code, model } =>
                format!("the profiles for MS-{code} list other models than {model:?}, generic mode"),
            Detection::Generic => "no profile, generic mode".to_owned(),
        }
    );
    (dmi, detection, skipped)
}

/// The profile files that could not be used, each reported on stderr.
fn skipped_profiles(loaded: &Profiles) -> Vec<SkippedFile> {
    loaded
        .errors
        .iter()
        .map(|(path, error)| {
            eprintln!(
                "baseboard: skipping board profile {}: {error}",
                path.display()
            );
            SkippedFile::new(path.clone(), FileKind::Profile, &error.to_string())
        })
        .collect()
}

/// The fan channels of `profile` with what the driver allows on each, from
/// the attributes of the profile's hwmon device under the sysfs `root`;
/// none without that device. Only reads sysfs. The channels and the driver
/// level (§5.5.2) go to the debug log.
pub fn fan_channels(root: &Path, profile: &Profile) -> Vec<Channel> {
    let chips = SysfsSource::new(root).chips();
    let Some(chip) = sensors::find(&chips, &profile.hwmon.name) else {
        glib::g_debug!(
            "baseboard",
            "no single hwmon device named {}",
            profile.hwmon.name
        );
        return Vec::new();
    };
    let channels = driver::channels(profile, &chip.path);
    for channel in &channels {
        let names: Vec<_> = channel
            .capabilities
            .iter()
            .map(Capability::as_str)
            .collect();
        glib::g_debug!(
            "baseboard",
            "{} pwm{}: {} [{}], {} curve point(s){}",
            channel.key,
            channel.pwm,
            channel.capabilities.level().as_str(),
            names.join(", "),
            channel.capabilities.curve_points(),
            if channel.controllable {
                ", controllable"
            } else {
                ""
            }
        );
    }
    let level: DriverLevel = driver::driver_level(&channels);
    glib::g_debug!(
        "baseboard",
        "{} {}: driver level {}",
        chip.name,
        chip.path.display(),
        level.as_str()
    );
    channels
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A wrong profile next to a good one, as in /etc/baseboard/boards: it
    /// is skipped with its path and reason, and the good one still loads.
    #[test]
    fn wrong_profile_is_skipped() {
        let dir = std::env::temp_dir().join(format!("baseboard-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/boards/ms-7e61.toml");
        std::fs::copy(shipped, dir.join("ms-7e61.toml")).unwrap();
        std::fs::write(dir.join("broken.toml"), "schema = 1\nid = \"broken\"\n").unwrap();

        let loaded = Profiles::load(&[PathBuf::from(&dir)]);
        assert_eq!(loaded.profiles.len(), 1);
        let skipped = skipped_profiles(&loaded);
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].path, dir.join("broken.toml"));
        assert_eq!(skipped[0].kind, FileKind::Profile);
        // One line, without the quoted line of the file.
        assert_eq!(
            skipped[0].reason,
            "invalid profile: TOML parse error at line 1, column 1: missing field `model`"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
