// SPDX-License-Identifier: GPL-3.0-or-later

//! The user's custom names for the readings of a board profile (D22,
//! docs/PROJEKT.md §6.2): one TOML file per profile in
//! `~/.local/share/baseboard/boards/`, named after the profile's `id`. Only
//! the app reads them; they never choose a profile and the helper never
//! sees them.
//!
//! ```toml
//! schema = 1
//!
//! [fan]
//! sys1 = "Front, bottom"
//!
//! [temp]
//! system = "Case"
//!
//! [voltage]
//! 12v = "Power supply 12 V"
//! ```
//!
//! The keys are the keys of the profile's `[[fan]]`, `[[temp]]` and
//! `[[voltage]]` entries.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::profile::Profile;

/// The version of the format.
pub const SCHEMA: u32 = 1;

/// The longest name, in characters.
pub const MAX_CHARS: usize = 64;

/// The names, by the profile's keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserLabels {
    pub fans: BTreeMap<String, String>,
    pub temps: BTreeMap<String, String>,
    pub voltages: BTreeMap<String, String>,
}

impl UserLabels {
    pub fn fan(&self, key: &str) -> Option<&str> {
        self.fans.get(key).map(String::as_str)
    }

    pub fn temp(&self, key: &str) -> Option<&str> {
        self.temps.get(key).map(String::as_str)
    }

    pub fn voltage(&self, key: &str) -> Option<&str> {
        self.voltages.get(key).map(String::as_str)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    /// Checked before, in [`parse`].
    #[serde(rename = "schema")]
    _schema: u32,
    #[serde(default)]
    fan: BTreeMap<String, String>,
    #[serde(default)]
    temp: BTreeMap<String, String>,
    #[serde(default)]
    voltage: BTreeMap<String, String>,
}

#[derive(Debug)]
pub enum LabelsError {
    Io(std::io::Error),
    Syntax(toml::de::Error),
    MissingSchema,
    UnsupportedSchema(i64),
    Invalid(String),
}

impl fmt::Display for LabelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read the labels: {e}"),
            Self::Syntax(e) => write!(f, "invalid labels: {e}"),
            Self::MissingSchema => write!(f, "the labels have no `schema` field"),
            Self::UnsupportedSchema(n) => {
                write!(f, "labels format {n} is not supported (expected {SCHEMA})")
            }
            Self::Invalid(reason) => write!(f, "invalid labels: {reason}"),
        }
    }
}

impl std::error::Error for LabelsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Syntax(e) => Some(e),
            _ => None,
        }
    }
}

/// The file with the names for `profile` in `dir`.
pub fn path(dir: &Path, profile: &Profile) -> PathBuf {
    dir.join(format!("{}.toml", profile.id))
}

/// Reads the names for `profile` from `dir`; `Ok(None)` without a file.
pub fn load(dir: &Path, profile: &Profile) -> Result<Option<UserLabels>, LabelsError> {
    match std::fs::read_to_string(path(dir, profile)) {
        Ok(text) => parse(&text, profile).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(LabelsError::Io(e)),
    }
}

/// Parses and checks the names against `profile`: every key must be one of
/// its keys of that kind, every name 1–[`MAX_CHARS`] characters without
/// control characters. Names are trimmed.
pub fn parse(text: &str, profile: &Profile) -> Result<UserLabels, LabelsError> {
    let table: toml::Table = toml::from_str(text).map_err(LabelsError::Syntax)?;
    match table.get("schema") {
        None => return Err(LabelsError::MissingSchema),
        Some(toml::Value::Integer(n)) if *n == i64::from(SCHEMA) => {}
        Some(toml::Value::Integer(n)) => return Err(LabelsError::UnsupportedSchema(*n)),
        Some(_) => return Err(LabelsError::Invalid("`schema` must be a number".into())),
    }
    let file: File = toml::from_str(text).map_err(LabelsError::Syntax)?;
    Ok(UserLabels {
        fans: names(file.fan, "fan", profile.fans.iter().map(|f| f.key.as_str()))?,
        temps: names(
            file.temp,
            "temp",
            profile.temps.iter().map(|t| t.key.as_str()),
        )?,
        voltages: names(
            file.voltage,
            "voltage",
            profile.voltages.iter().map(|v| v.key.as_str()),
        )?,
    })
}

fn names<'a>(
    names: BTreeMap<String, String>,
    section: &str,
    keys: impl Iterator<Item = &'a str>,
) -> Result<BTreeMap<String, String>, LabelsError> {
    let keys: Vec<&str> = keys.collect();
    names
        .into_iter()
        .map(|(key, name)| {
            if !keys.contains(&key.as_str()) {
                return Err(LabelsError::Invalid(format!(
                    "[{section}] {key:?} is not a key of the profile"
                )));
            }
            let name = name.trim();
            let chars = name.chars().count();
            if chars == 0 || chars > MAX_CHARS || name.chars().any(char::is_control) {
                return Err(LabelsError::Invalid(format!(
                    "[{section}] {key}: use 1–{MAX_CHARS} characters without control characters"
                )));
            }
            Ok((key, name.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Profile {
        crate::profile::load(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/boards/ms-7e61.toml"
        )))
        .unwrap()
    }

    fn reason(text: &str) -> String {
        parse(text, &profile()).unwrap_err().to_string()
    }

    #[test]
    fn names_by_profile_keys() {
        let labels = parse(
            "schema = 1\n[fan]\nsys1 = \"  Front, bottom \"\n[temp]\nsystem = \"Case\"\n\
             [voltage]\n12v = \"Power supply 12 V\"\n",
            &profile(),
        )
        .unwrap();
        assert_eq!(labels.fan("sys1"), Some("Front, bottom"));
        assert_eq!(labels.fan("sys2"), None);
        assert_eq!(labels.temp("system"), Some("Case"));
        assert_eq!(labels.voltage("12v"), Some("Power supply 12 V"));
        assert_eq!(
            parse("schema = 1\n", &profile()).unwrap(),
            UserLabels::default()
        );
    }

    #[test]
    fn errors() {
        assert!(reason("[fan]\nsys1 = \"x\"\n").contains("no `schema`"));
        assert!(reason("schema = 2\n").contains("format 2"));
        assert!(reason("schema = 1\n[fan]\nsys9 = \"x\"\n").contains("\"sys9\""));
        // A temperature key is not a fan key.
        assert!(reason("schema = 1\n[fan]\nsystem = \"x\"\n").contains("not a key"));
        assert!(reason("schema = 1\n[fan]\nsys1 = \"  \"\n").contains("1–64"));
        assert!(
            reason(&format!(
                "schema = 1\n[fan]\nsys1 = \"{}\"\n",
                "x".repeat(65)
            ))
            .contains("1–64")
        );
        assert!(reason("schema = 1\n[fan]\nsys1 = \"a\\tb\"\n").contains("control"));
        assert!(reason("schema = 1\n[rgb]\nx = \"y\"\n").contains("rgb"));
    }

    #[test]
    fn file_per_profile() {
        let dir = std::env::temp_dir().join(format!("baseboard-labels-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(path(&dir, &profile()), dir.join("ms-7e61.toml"));
        assert!(load(&dir, &profile()).unwrap().is_none());
        std::fs::write(
            dir.join("ms-7e61.toml"),
            "schema = 1\n[fan]\ncpu1 = \"Tower\"\n",
        )
        .unwrap();
        let labels = load(&dir, &profile()).unwrap().unwrap();
        assert_eq!(labels.fan("cpu1"), Some("Tower"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
