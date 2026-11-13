// SPDX-License-Identifier: GPL-3.0-or-later

//! Board detection (docs/PROJEKT.md §6.1): DMI from sysfs, the board code
//! and PCB revision, and the choice of a board profile.
//!
//! DMI is read from a replaceable sysfs root, so tests and the app can use a
//! fake tree. Only the fields in [`DMI_FIELDS`] are ever opened: the files
//! with serial numbers and the UUID are never touched (§1.3).

use std::path::{Path, PathBuf};

use crate::profile::{self, Profile, ProfileError};

/// The only DMI files this module opens, relative to `class/dmi/id`.
pub const DMI_FIELDS: [&str; 4] = [
    "board_vendor",
    "board_name",
    "board_version",
    "product_name",
];

/// The DMI fields used for detection. A missing, unreadable or blank file
/// is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dmi {
    pub board_vendor: Option<String>,
    pub board_name: Option<String>,
    pub board_version: Option<String>,
    pub product_name: Option<String>,
}

impl Dmi {
    /// Reads the fields of [`DMI_FIELDS`] under `<sysfs_root>/class/dmi/id`.
    pub fn read(sysfs_root: &Path) -> Self {
        let dir = sysfs_root.join("class/dmi/id");
        let field = |name: &str| {
            debug_assert!(DMI_FIELDS.contains(&name));
            std::fs::read_to_string(dir.join(name))
                .ok()
                .and_then(|value| clean(&value))
        };
        Self {
            board_vendor: field("board_vendor"),
            board_name: field("board_name"),
            board_version: field("board_version"),
            product_name: field("product_name"),
        }
    }

    /// The four hex digits of the board code, upper case: from "(MS-xxxx)"
    /// in `board_name`, or else from a `product_name` of exactly "MS-xxxx".
    pub fn board_code(&self) -> Option<String> {
        let from_name = self.board_name.as_deref().and_then(|name| {
            let start = name.find("(MS-")? + "(MS-".len();
            let rest = name.get(start..)?;
            let code = rest.get(..4)?;
            (rest.get(4..5) == Some(")")).then_some(code)
        });
        let from_product = || {
            self.product_name
                .as_deref()
                .and_then(|name| name.strip_prefix("MS-"))
        };
        let code = from_name.or_else(from_product)?;
        (code.len() == 4 && code.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| code.to_ascii_uppercase())
    }

    /// The board model: `board_name` without the "(MS-xxxx)" part, with
    /// single spaces ("MAG B850M MORTAR WIFI (MS-7E61)" → "MAG B850M MORTAR
    /// WIFI").
    pub fn model_name(&self) -> Option<String> {
        let name = self.board_name.as_deref()?;
        let without_code = match name.find("(MS-") {
            Some(start) => {
                let end = name[start..]
                    .find(')')
                    .map_or(name.len(), |close| start + close + 1);
                format!("{} {}", &name[..start], &name[end..])
            }
            None => name.to_owned(),
        };
        let model = without_code
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        (!model.is_empty()).then_some(model)
    }

    /// The PCB revision: the first character of `board_version` if it is a
    /// digit ("1.0" → 1).
    pub fn pcb_revision(&self) -> Option<u8> {
        let first = self.board_version.as_deref()?.chars().next()?;
        first
            .to_digit(10)
            .and_then(|digit| u8::try_from(digit).ok())
    }
}

/// The board code as people and the manual write it: "MS-7E61" (§5.4).
pub fn code_label(code: &str) -> String {
    format!("MS-{code}")
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The result of detection.
#[derive(Debug, Clone, PartialEq)]
pub enum Detection {
    /// A profile matches the board and its PCB revision.
    Supported(Profile),
    /// A profile matches the board, but not its PCB revision (or the
    /// revision is unknown): read-only, with a warning (B9).
    UnverifiedRevision {
        profile: Profile,
        revision: Option<u8>,
    },
    /// There are profiles for the board code, but all of them list other
    /// models (or DMI does not name the model). Their fan headers may not
    /// fit this board: generic mode with a note, never control (D28).
    OtherModel { code: String, model: Option<String> },
    /// No profile matches: generic mode (§6.1 point 5).
    Generic,
}

impl Detection {
    /// The matching profile, if any.
    pub fn profile(&self) -> Option<&Profile> {
        match self {
            Self::Supported(profile) | Self::UnverifiedRevision { profile, .. } => Some(profile),
            Self::OtherModel { .. } | Self::Generic => None,
        }
    }

    /// Whether Baseboard may control anything on this board. Only a profile
    /// with a verified PCB revision allows it (B9).
    pub fn allows_control(&self) -> bool {
        matches!(self, Self::Supported(_))
    }
}

/// Chooses the profile for the board described by `dmi` (§6.1), in three
/// steps:
/// 1. the vendor must match exactly (after trimming), and the board code;
/// 2. the model: profiles that list it win; without one, profiles that list
///    no models; a profile that lists models never fits another model or a
///    board whose DMI names no model ([`Detection::OtherModel`]);
/// 3. the PCB revision: a profile that lists it wins; otherwise the first
///    one is used read-only.
pub fn detect(dmi: &Dmi, profiles: &[Profile]) -> Detection {
    let (Some(vendor), Some(code)) = (dmi.board_vendor.as_deref(), dmi.board_code()) else {
        return Detection::Generic;
    };
    let same_code: Vec<&Profile> = profiles
        .iter()
        .filter(|p| p.board_match.board_vendor == vendor && p.board_match.board_code == code)
        .collect();
    if same_code.is_empty() {
        return Detection::Generic;
    }
    let model = dmi.model_name();
    let naming_the_model: Vec<&Profile> = same_code
        .iter()
        .copied()
        .filter(|p| {
            model
                .as_deref()
                .is_some_and(|m| p.board_match.lists_model(m))
        })
        .collect();
    let candidates = if naming_the_model.is_empty() {
        same_code
            .into_iter()
            .filter(|p| p.board_match.board_names.is_empty())
            .collect()
    } else {
        naming_the_model
    };
    let Some(&first) = candidates.first() else {
        return Detection::OtherModel { code, model };
    };
    let revision = dmi.pcb_revision();
    match revision.and_then(|r| {
        candidates
            .iter()
            .find(|p| p.board_match.pcb_revisions.contains(&r))
    }) {
        Some(profile) => Detection::Supported((*profile).clone()),
        None => Detection::UnverifiedRevision {
            profile: first.clone(),
            revision,
        },
    }
}

/// Profiles read from the given folders, in their order (§6.2): a profile
/// in a later folder replaces one with the same `id` from an earlier one.
/// Missing folders are skipped; invalid files are reported and skipped.
#[derive(Debug, Default)]
pub struct Profiles {
    pub profiles: Vec<Profile>,
    pub errors: Vec<(PathBuf, ProfileError)>,
}

impl Profiles {
    pub fn load(dirs: &[PathBuf]) -> Self {
        let mut result = Self::default();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut files: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .filter(|path| path.extension().is_some_and(|ext| ext == "toml") && path.is_file())
                .collect();
            files.sort();
            for path in files {
                match profile::load(&path) {
                    Ok(profile) => {
                        result.profiles.retain(|p| p.id != profile.id);
                        result.profiles.push(profile);
                    }
                    Err(e) => result.errors.push((path, e)),
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS_7E61: &str = include_str!("../../../data/boards/ms-7e61.toml");

    /// The DMI of the MS-7E61 this was developed on.
    fn this_board() -> Dmi {
        Dmi {
            board_vendor: Some("Micro-Star International Co., Ltd.".into()),
            board_name: Some("MAG B850M MORTAR WIFI (MS-7E61)".into()),
            board_version: Some("1.0".into()),
            product_name: Some("MS-7E61".into()),
        }
    }

    fn profiles() -> Vec<Profile> {
        vec![profile::parse(MS_7E61).unwrap()]
    }

    /// The board code is always shown as "MS-xxxx" (§5.4).
    #[test]
    fn code_with_its_prefix() {
        let code = this_board().board_code().unwrap();
        assert_eq!(code, "7E61");
        assert_eq!(code_label(&code), "MS-7E61");
    }

    /// A fresh folder in the system temporary directory for one test.
    fn scratch(test: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("baseboard-board-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_dmi_from_a_sysfs_root() {
        let root = scratch("dmi");
        let dir = root.join("class/dmi/id");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("board_vendor"),
            "Micro-Star International Co., Ltd.\n",
        )
        .unwrap();
        std::fs::write(dir.join("board_name"), "MAG B850M MORTAR WIFI (MS-7E61)\n").unwrap();
        std::fs::write(dir.join("board_version"), "1.0\n").unwrap();
        std::fs::write(dir.join("product_name"), "  \n").unwrap();
        std::fs::write(dir.join("board_serial"), "SECRET\n").unwrap();
        let dmi = Dmi::read(&root);
        assert_eq!(
            dmi.board_vendor.as_deref(),
            Some("Micro-Star International Co., Ltd.")
        );
        assert_eq!(
            dmi.board_name.as_deref(),
            Some("MAG B850M MORTAR WIFI (MS-7E61)")
        );
        assert_eq!(dmi.board_version.as_deref(), Some("1.0"));
        // Blank is the same as missing.
        assert_eq!(dmi.product_name, None);
        assert!(!format!("{dmi:?}").contains("SECRET"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_dmi_folder() {
        let root = scratch("empty");
        assert_eq!(Dmi::read(&root), Dmi::default());
        assert_eq!(detect(&Dmi::read(&root), &profiles()), Detection::Generic);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn this_board_is_supported() {
        let dmi = this_board();
        assert_eq!(dmi.board_code().as_deref(), Some("7E61"));
        assert_eq!(dmi.model_name().as_deref(), Some("MAG B850M MORTAR WIFI"));
        assert_eq!(dmi.pcb_revision(), Some(1));
        let detection = detect(&dmi, &profiles());
        assert!(matches!(&detection, Detection::Supported(p) if p.id == "ms-7e61"));
        assert!(detection.allows_control());
        // Case and spaces of the model name do not matter.
        let dmi = Dmi {
            board_name: Some("Mag B850M  Mortar WiFi (MS-7E61)".into()),
            ..this_board()
        };
        assert!(matches!(detect(&dmi, &profiles()), Detection::Supported(_)));
    }

    #[test]
    fn model_name_forms() {
        let model = |name: Option<&str>| {
            Dmi {
                board_name: name.map(Into::into),
                ..Dmi::default()
            }
            .model_name()
        };
        assert_eq!(
            model(Some("MAG B850M MORTAR WIFI (MS-7E61)")).as_deref(),
            Some("MAG B850M MORTAR WIFI")
        );
        assert_eq!(
            model(Some("  TEST   BOARD  (MS-7E56) ")).as_deref(),
            Some("TEST BOARD")
        );
        assert_eq!(model(Some("TEST BOARD")).as_deref(), Some("TEST BOARD"));
        assert_eq!(model(Some("TEST (MS-7E56")).as_deref(), Some("TEST"));
        assert_eq!(model(Some("(MS-7E61)")), None);
        assert_eq!(model(None), None);
    }

    /// A made-up profile for boards with code 7E56: invented model names
    /// and fan headers (all with the role `sys`; roles do not matter here).
    fn made_up(id: &str, names: &[&str], headers: &[&str]) -> Profile {
        let names: Vec<String> = names.iter().map(|name| format!("{name:?}")).collect();
        let mut text = format!(
            "schema = 1\nid = \"{id}\"\nmodel = \"{id}\"\n\n[match]\n\
             board_vendor = \"Micro-Star International Co., Ltd.\"\n\
             board_code = \"7E56\"\npcb_revisions = [1]\nboard_names = [{}]\n\n\
             [hwmon]\nname = \"nct6687\"\n",
            names.join(", ")
        );
        for (i, header) in headers.iter().enumerate() {
            text += &format!(
                "\n[[fan]]\nkey = \"sys{n}\"\nrole = \"sys\"\nnumber = {n}\n\
                 header = \"{header}\"\nfan = {n}\n",
                n = i + 1
            );
        }
        profile::parse(&text).unwrap()
    }

    /// A board with code 7E56 and the given model, PCB revision 1.
    fn board_7e56(model: Option<&str>) -> Dmi {
        Dmi {
            board_name: model.map(|model| format!("{model} (MS-7E56)")),
            product_name: Some("MS-7E56".into()),
            ..this_board()
        }
    }

    fn headers(detection: &Detection) -> Vec<&str> {
        detection
            .profile()
            .unwrap()
            .fans
            .iter()
            .map(|fan| fan.header.as_str())
            .collect()
    }

    /// One board code, three models, two layouts of fan headers: the case
    /// of MS-7E56, with made-up data (D28).
    #[test]
    fn one_code_three_models() {
        let with_jaf = made_up(
            "test-a",
            &["TEST BOARD A", "TEST BOARD A MAX"],
            &["SYS_FAN1", "SYS_FAN2", "JAF_1"],
        );
        let without_jaf = made_up("test-b", &["TEST BOARD B"], &["SYS_FAN1", "SYS_FAN2"]);
        // The order of the profiles does not matter.
        for profiles in [
            [with_jaf.clone(), without_jaf.clone()],
            [without_jaf.clone(), with_jaf.clone()],
        ] {
            for (model, id) in [
                ("TEST BOARD A", "test-a"),
                ("TEST BOARD A MAX", "test-a"),
                ("TEST BOARD B", "test-b"),
            ] {
                let detection = detect(&board_7e56(Some(model)), &profiles);
                assert!(
                    matches!(&detection, Detection::Supported(p) if p.id == id),
                    "{model}"
                );
                assert!(detection.allows_control(), "{model}");
            }
            assert_eq!(
                headers(&detect(&board_7e56(Some("TEST BOARD A MAX")), &profiles)),
                ["SYS_FAN1", "SYS_FAN2", "JAF_1"]
            );
            assert_eq!(
                headers(&detect(&board_7e56(Some("TEST BOARD B")), &profiles)),
                ["SYS_FAN1", "SYS_FAN2"]
            );
        }
    }

    /// The code fits, no model does: generic mode with a note, never a
    /// profile and never control (D28).
    #[test]
    fn unknown_model_is_generic_with_a_note() {
        let for_7e56 = [
            made_up("test-a", &["TEST BOARD A"], &["SYS_FAN1", "JAF_1"]),
            made_up("test-b", &["TEST BOARD B"], &["SYS_FAN1"]),
        ];
        let detection = detect(&board_7e56(Some("TEST BOARD C")), &for_7e56);
        assert_eq!(
            detection,
            Detection::OtherModel {
                code: "7E56".into(),
                model: Some("TEST BOARD C".into())
            }
        );
        assert!(detection.profile().is_none());
        assert!(!detection.allows_control());
        // DMI without a model name: no profile that lists models fits.
        assert_eq!(
            detect(&board_7e56(None), &for_7e56),
            Detection::OtherModel {
                code: "7E56".into(),
                model: None
            }
        );
        // The shipped profile lists its model too.
        let other = Dmi {
            board_name: Some("OTHER BOARD (MS-7E61)".into()),
            ..this_board()
        };
        assert_eq!(
            detect(&other, &profiles()),
            Detection::OtherModel {
                code: "7E61".into(),
                model: Some("OTHER BOARD".into())
            }
        );
        // Another code is plain generic mode.
        assert_eq!(detect(&this_board(), &for_7e56), Detection::Generic);
    }

    /// A profile without model names fits every model with its code, unless
    /// a profile names the model.
    #[test]
    fn profile_without_names_fits_every_model() {
        let any = made_up("test-any", &[], &["SYS_FAN1"]);
        let named = made_up("test-a", &["TEST BOARD A"], &["SYS_FAN1", "JAF_1"]);
        for model in [Some("TEST BOARD A"), Some("TEST BOARD C"), None] {
            assert!(
                matches!(
                    detect(&board_7e56(model), std::slice::from_ref(&any)),
                    Detection::Supported(p) if p.id == "test-any"
                ),
                "{model:?}"
            );
        }
        let both = [any.clone(), named.clone()];
        assert!(matches!(
            detect(&board_7e56(Some("TEST BOARD A")), &both),
            Detection::Supported(p) if p.id == "test-a"
        ));
        assert!(matches!(
            detect(&board_7e56(Some("TEST BOARD C")), &both),
            Detection::Supported(p) if p.id == "test-any"
        ));
        // The model comes before the PCB revision: the profile naming the
        // model, read only, and not one for every model with the revision.
        let mut any_rev2 = any;
        any_rev2.id = "test-any-rev2".into();
        any_rev2.board_match.pcb_revisions = vec![2];
        let dmi = Dmi {
            board_version: Some("2.0".into()),
            ..board_7e56(Some("TEST BOARD A"))
        };
        assert!(matches!(
            detect(&dmi, &[any_rev2, named]),
            Detection::UnverifiedRevision { profile, revision: Some(2) } if profile.id == "test-a"
        ));
    }

    /// Profiles without model names (before D28) load and match as they
    /// did: every model with the code, also when DMI names no model.
    #[test]
    fn older_profiles_match_as_before() {
        let old_text = MS_7E61.replace("board_names = [\"MAG B850M MORTAR WIFI\"]\n", "");
        assert_ne!(old_text, MS_7E61);
        let old = [profile::parse(&old_text).unwrap()];
        assert!(old[0].board_match.board_names.is_empty());
        assert!(matches!(
            detect(&this_board(), &old),
            Detection::Supported(_)
        ));
        for name in [None, Some("OTHER BOARD (MS-7E61)")] {
            let dmi = Dmi {
                board_name: name.map(Into::into),
                ..this_board()
            };
            assert!(
                matches!(detect(&dmi, &old), Detection::Supported(_)),
                "{name:?}"
            );
        }
        let dmi = Dmi {
            board_version: Some("2.0".into()),
            ..this_board()
        };
        assert!(matches!(
            detect(&dmi, &old),
            Detection::UnverifiedRevision {
                revision: Some(2),
                ..
            }
        ));
    }

    #[test]
    fn other_msi_boards_are_generic() {
        for name in [
            "MAG B650 TOMAHAWK WIFI (MS-7D75)",
            "PRO B850-P WIFI (MS-7E56)",
            "MAG X870E TOMAHAWK WIFI (MS-7E59)",
        ] {
            let dmi = Dmi {
                board_name: Some(name.into()),
                product_name: None,
                ..this_board()
            };
            assert_eq!(detect(&dmi, &profiles()), Detection::Generic, "{name}");
        }
    }

    #[test]
    fn other_pcb_revision_is_read_only() {
        let dmi = Dmi {
            board_version: Some("2.0".into()),
            ..this_board()
        };
        let detection = detect(&dmi, &profiles());
        assert!(matches!(
            &detection,
            Detection::UnverifiedRevision { profile, revision: Some(2) } if profile.id == "ms-7e61"
        ));
        assert!(!detection.allows_control());
        assert!(detection.profile().is_some());
    }

    #[test]
    fn unknown_pcb_revision_is_read_only() {
        for version in [None, Some("Default string"), Some("v1.0")] {
            let dmi = Dmi {
                board_version: version.map(Into::into),
                ..this_board()
            };
            assert_eq!(dmi.pcb_revision(), None, "{version:?}");
            assert!(
                matches!(
                    detect(&dmi, &profiles()),
                    Detection::UnverifiedRevision { revision: None, .. }
                ),
                "{version:?}"
            );
        }
    }

    #[test]
    fn other_vendors_are_generic() {
        // Even with a board name that looks like this board's.
        for vendor in [
            "ASUSTeK COMPUTER INC.",
            "Micro-Star International",
            "Gigabyte Technology Co., Ltd.",
        ] {
            let dmi = Dmi {
                board_vendor: Some(vendor.into()),
                ..this_board()
            };
            assert_eq!(detect(&dmi, &profiles()), Detection::Generic, "{vendor}");
        }
    }

    #[test]
    fn empty_and_missing_fields() {
        let without = |change: fn(&mut Dmi)| {
            let mut dmi = this_board();
            change(&mut dmi);
            detect(&dmi, &profiles())
        };
        assert_eq!(without(|d| d.board_vendor = None), Detection::Generic);
        assert_eq!(
            without(|d| {
                d.board_name = None;
                d.product_name = None;
            }),
            Detection::Generic
        );
        // Without the name, the product name gives the code, but the model
        // is unknown and the profile lists its model (D28).
        assert_eq!(
            without(|d| d.board_name = None),
            Detection::OtherModel {
                code: "7E61".into(),
                model: None
            }
        );
        assert!(matches!(
            without(|d| d.board_name = Some("MAG B850M MORTAR WIFI".into())),
            Detection::Supported(_)
        ));
        assert_eq!(Dmi::default().board_code(), None);
        assert_eq!(detect(&Dmi::default(), &profiles()), Detection::Generic);
        assert_eq!(detect(&this_board(), &[]), Detection::Generic);
    }

    #[test]
    fn board_code_forms() {
        let code = |name: Option<&str>, product: Option<&str>| {
            Dmi {
                board_name: name.map(Into::into),
                product_name: product.map(Into::into),
                ..Dmi::default()
            }
            .board_code()
        };
        assert_eq!(code(Some("X (MS-7E61)"), None).as_deref(), Some("7E61"));
        assert_eq!(code(Some("X (MS-7e61)"), None).as_deref(), Some("7E61"));
        assert_eq!(code(Some("X (MS-7E6)"), None), None);
        assert_eq!(code(Some("X (MS-7E61A)"), None), None);
        assert_eq!(code(Some("X (MS-7G61)"), None), None);
        assert_eq!(code(Some("X MS-7E61"), None), None);
        assert_eq!(code(None, Some("MS-7E61")).as_deref(), Some("7E61"));
        assert_eq!(code(None, Some("MS-7E61X")), None);
        assert_eq!(code(None, Some("To be filled by O.E.M.")), None);
        // The name wins over the product name.
        assert_eq!(
            code(Some("X (MS-7D75)"), Some("MS-7E61")).as_deref(),
            Some("7D75")
        );
    }

    #[test]
    fn profile_folders_in_order() {
        let base = scratch("dirs");
        let shipped = base.join("usr");
        let admin = base.join("etc");
        std::fs::create_dir_all(&shipped).unwrap();
        std::fs::create_dir_all(&admin).unwrap();
        std::fs::write(shipped.join("ms-7e61.toml"), MS_7E61).unwrap();
        std::fs::write(shipped.join("README"), "not a profile").unwrap();
        std::fs::write(shipped.join("broken.toml"), "schema = 1\n").unwrap();
        // The administrator's copy replaces the shipped one.
        let override_ = MS_7E61.replace(
            "model = \"MAG B850M MORTAR WIFI\"",
            "model = \"Local copy\"",
        );
        std::fs::write(admin.join("ms-7e61.toml"), override_).unwrap();

        let loaded = Profiles::load(&[shipped.clone(), admin, base.join("missing")]);
        assert_eq!(loaded.profiles.len(), 1);
        assert_eq!(loaded.profiles[0].model, "Local copy");
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.errors[0].0, shipped.join("broken.toml"));

        let only_shipped = Profiles::load(&[shipped]);
        assert_eq!(only_shipped.profiles[0].model, "MAG B850M MORTAR WIFI");
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_profile_listing_the_revision_wins() {
        let rev1 = profile::parse(MS_7E61).unwrap();
        let rev2 = profile::parse(
            &MS_7E61
                .replace("id = \"ms-7e61\"", "id = \"ms-7e61-rev2\"")
                .replace("pcb_revisions = [1]", "pcb_revisions = [2]"),
        )
        .unwrap();
        let both = [rev1, rev2];
        let dmi = Dmi {
            board_version: Some("2.0".into()),
            ..this_board()
        };
        assert!(matches!(detect(&dmi, &both), Detection::Supported(p) if p.id == "ms-7e61-rev2"));
        assert!(
            matches!(detect(&this_board(), &both), Detection::Supported(p) if p.id == "ms-7e61")
        );
    }
}
