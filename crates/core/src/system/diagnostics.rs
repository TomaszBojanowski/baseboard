// SPDX-License-Identifier: GPL-3.0-or-later

//! What "Diagnostics" of the System page checks (docs/PROJEKT.md §5.4): the
//! fan driver and what it allows, the hwmon devices, the lm-sensors files
//! for the board's chip and other programs that may control the fans.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::{
    FAN_DRIVER_MODULE, FAN_PROGRAMS, FanDriver, ModuleOrigin, canonical, entries, read_text,
};
use crate::fans::driver;
use crate::profile::Profile;

/// Whether the EC driver is loaded and what it allows on the profile's
/// channels. Reads only `name` files, the modes of the `pwm` files and
/// `<proc>/modules`.
pub fn fan_driver(sys: &Path, proc: &Path, profile: Option<&Profile>) -> FanDriver {
    let sys = canonical(sys);
    let hwmon = profile.and_then(|p| hwmon_by_name(&sys, &p.hwmon.name));
    let level = profile
        .zip(hwmon.as_deref())
        .map(|(profile, hwmon)| driver::driver_level(&driver::channels(profile, hwmon)));
    let listing = module_listing(proc, FAN_DRIVER_MODULE);
    let loaded = matches!(listing, Listing::Listed { .. }) || hwmon.is_some();
    let origin = match listing {
        Listing::Listed { out_of_tree: true } => Some(ModuleOrigin::OutOfTree),
        Listing::Listed { out_of_tree: false } => Some(ModuleOrigin::InTree),
        // Not a loadable module, yet its device is there: built in.
        Listing::NotListed if loaded => Some(ModuleOrigin::InTree),
        Listing::NotListed | Listing::Unknown => None,
    };
    FanDriver {
        loaded,
        origin,
        device: hwmon.is_some(),
        level,
    }
}

/// What `/proc/modules` says of one module.
enum Listing {
    /// Loaded as a module; flag `O` among its taint flags means out of the
    /// kernel's tree.
    Listed { out_of_tree: bool },
    /// Not a loaded module. A kernel without loadable modules has no list
    /// at all, which means the same.
    NotListed,
    /// The list cannot be read: the origin stays unknown.
    Unknown,
}

/// The module's line in `<proc>/modules`, e.g.
/// `nct6683 53248 0 - Live 0x0000000000000000 (O)`; its taint flags, if
/// any, close the line in brackets. Read each time, so a module loaded or
/// replaced while the app runs shows as it is now (D40).
fn module_listing(proc: &Path, name: &str) -> Listing {
    let text = match std::fs::read_to_string(proc.join("modules")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Listing::NotListed,
        Err(_) => return Listing::Unknown,
    };
    let Some(line) = text
        .lines()
        .find(|line| line.split_whitespace().next() == Some(name))
    else {
        return Listing::NotListed;
    };
    let flags = line
        .trim_end()
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once('('))
        .map_or("", |(_, flags)| flags);
    Listing::Listed {
        out_of_tree: flags.contains('O'),
    }
}

/// The names of the hwmon devices, sorted.
pub fn hwmon_names(sys: &Path) -> Vec<String> {
    let mut names: Vec<String> = entries(&sys.join("class/hwmon"))
        .into_iter()
        .filter_map(|(_, path)| read_text(&path.join("name")))
        .collect();
    names.sort();
    names
}

/// The only hwmon folder with the given `name`, as in
/// [`crate::sensors::find`].
fn hwmon_by_name(sys: &Path, name: &str) -> Option<PathBuf> {
    let mut found = entries(&sys.join("class/hwmon"))
        .into_iter()
        .filter(|(_, path)| read_text(&path.join("name")).as_deref() == Some(name))
        .filter_map(|(_, path)| std::fs::canonicalize(path).ok());
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// The lm-sensors files with a `chip` statement for `chip` (the hwmon name,
/// e.g. `nct6687`): a pattern whose part before the first "-" is that name.
/// The files are the ones libsensors reads: `sensors3.conf` (else
/// `sensors.conf`) and `sensors.d/*` without hidden files, in its order.
pub fn sensors_config(etc: &Path, chip: &str) -> Vec<PathBuf> {
    let main = ["sensors3.conf", "sensors.conf"]
        .iter()
        .map(|name| etc.join(name))
        .find(|path| path.is_file());
    let mut extra: Vec<PathBuf> = entries(&etc.join("sensors.d"))
        .into_iter()
        .filter(|(name, path)| !name.starts_with('.') && path.is_file())
        .map(|(_, path)| path)
        .collect();
    extra.sort();
    main.into_iter()
        .chain(extra)
        .filter(|path| std::fs::read_to_string(path).is_ok_and(|text| names_chip(&text, chip)))
        .collect()
}

/// Whether a configuration has a `chip` statement for `chip`. A line ending
/// in "\" goes on in the next one; "#" starts a comment.
fn names_chip(config: &str, chip: &str) -> bool {
    let mut statement = String::new();
    for line in config.lines() {
        let line = line.split('#').next().unwrap_or_default().trim_end();
        if let Some(start) = line.strip_suffix('\\') {
            statement.push_str(start);
            statement.push(' ');
            continue;
        }
        statement.push_str(line);
        let mut words = statement.split_whitespace();
        if words.next() == Some("chip")
            && words.any(|pattern| pattern.trim_matches('"').split('-').next() == Some(chip))
        {
            return true;
        }
        statement.clear();
    }
    false
}

/// The programs of [`FAN_PROGRAMS`] that run, by `<proc>/<pid>/comm`.
pub fn fan_programs(proc: &Path) -> Vec<String> {
    let running: BTreeSet<String> = entries(proc)
        .into_iter()
        .filter(|(name, _)| name.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(_, path)| read_text(&path.join("comm")))
        .filter(|comm| FAN_PROGRAMS.contains(&comm.as_str()))
        .collect();
    running.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_statements() {
        let config = "# chip \"nct6687-*\"\nchip \"k10temp-*\"\n  label temp1 \"x\"\n";
        assert!(!names_chip(config, "nct6687"));
        assert!(names_chip("chip \"nct6687-isa-*\"\n", "nct6687"));
        assert!(names_chip(
            "chip \"it87-*\" \\\n  \"nct6687-*\"\n",
            "nct6687"
        ));
        assert!(names_chip("chip nct6687-isa-0a20\n", "nct6687"));
        assert!(!names_chip("chip \"nct6683-*\"\n", "nct6687"));
        assert!(!names_chip("label nct6687 \"x\"\n", "nct6687"));
    }
}
