// SPDX-License-Identifier: GPL-3.0-or-later

//! Build-time configuration. Meson passes the install paths through
//! environment variables; plain `cargo build` falls back to defaults.

use std::path::{Path, PathBuf};

pub const APP_ID: &str = "me.bojanowski.Baseboard";
/// Where the interface files and the icons are in the resource bundle.
pub const RESOURCE_BASE_PATH: &str = "/me/bojanowski/Baseboard";
pub const GETTEXT_PACKAGE: &str = "baseboard";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The project's website and where to report bugs, for the About dialog;
/// the same addresses as the metainfo `url` entries and the README.
pub const WEBSITE: &str = "https://baseboard.bojanowski.me";
pub const ISSUES: &str = "https://github.com/TomaszBojanowski/baseboard/issues";
const INSTALLED_LOCALEDIR: &str = match option_env!("BASEBOARD_LOCALEDIR") {
    Some(dir) => dir,
    None => "/usr/local/share/locale",
};

/// The folder with the translations. Debug builds run under `meson devenv`
/// read the ones in the build directory, which `BASEBOARD_LOCALEDIR` names
/// there, so that they can be checked without installing.
pub fn localedir() -> String {
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("BASEBOARD_LOCALEDIR") {
        return dir;
    }
    INSTALLED_LOCALEDIR.to_owned()
}

const INSTALLED_PKGDATADIR: &str = match option_env!("BASEBOARD_PKGDATADIR") {
    Some(dir) => dir,
    None => "/usr/local/share/baseboard",
};

/// Folders with board profiles, in the order of docs/PROJEKT.md §6.2: the
/// shipped ones, then the administrator's, which replace them. Debug builds
/// run under `meson devenv` use only the folder that `BASEBOARD_BOARDS_DIR`
/// names there: the profiles in the source tree.
pub fn board_profile_dirs() -> Vec<PathBuf> {
    #[cfg(debug_assertions)]
    if let Ok(dir) = std::env::var("BASEBOARD_BOARDS_DIR") {
        return vec![dir.into()];
    }
    vec![
        Path::new(INSTALLED_PKGDATADIR).join("boards"),
        PathBuf::from("/etc/baseboard/boards"),
    ]
}

/// The folder with the user's own names for readings (D22, §6.2):
/// `$XDG_DATA_HOME/baseboard/boards`, usually
/// `~/.local/share/baseboard/boards`. Only the app reads it.
pub fn user_labels_dir() -> PathBuf {
    gtk::glib::user_data_dir().join("baseboard").join("boards")
}

/// The root of sysfs. Debug builds can point it at a fake tree with
/// `BASEBOARD_SYSFS_ROOT`; release builds always read /sys. Only the app
/// can do this, never the helper (§7, M1).
pub fn sysfs_root() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(root) = std::env::var("BASEBOARD_SYSFS_ROOT") {
        return root.into();
    }
    PathBuf::from("/sys")
}
