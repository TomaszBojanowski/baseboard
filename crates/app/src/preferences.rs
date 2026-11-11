// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings (docs/PROJEKT.md §3.2) and the Preferences dialog: the
//! refresh interval, names from lm-sensors, and the readings and headers
//! shown on the Overview page. Every change goes to GSettings at once; the
//! window listens to GSettings and applies it without a restart.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use adw::prelude::*;
use baseboard_core::overview::Group;
use baseboard_core::units::{Numbers, UNIT_SPACE};
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::overview;
use crate::sampler::Options;

/// Seconds between samples, 1–10.
pub const INTERVAL: &str = "refresh-interval";
/// Names from lm-sensors first.
pub const SENSORS_LABELS: &str = "sensors-labels";
/// The keys of hidden rows (`RowId::key`), sorted.
pub const HIDDEN: &str = "hidden-readings";

/// The longest refresh interval the schema allows.
const MAX_INTERVAL: u32 = 10;

/// The options of the sensor reader from the settings.
pub fn options(settings: &gio::Settings) -> Options {
    Options {
        interval: Duration::from_secs(settings.uint(INTERVAL).clamp(1, MAX_INTERVAL).into()),
        sensors_labels: settings.boolean(SENSORS_LABELS),
    }
}

/// The refresh interval in whole seconds.
pub fn interval_seconds(settings: &gio::Settings) -> u32 {
    settings.uint(INTERVAL).clamp(1, MAX_INTERVAL)
}

pub fn set_interval(settings: &gio::Settings, seconds: u32) -> Result<(), glib::BoolError> {
    settings.set_uint(INTERVAL, seconds.clamp(1, MAX_INTERVAL))
}

/// The keys of the hidden rows.
pub fn hidden(settings: &gio::Settings) -> HashSet<String> {
    settings
        .strv(HIDDEN)
        .iter()
        .map(|key| key.to_string())
        .collect()
}

/// Hides the row with `key`, or shows it again. The list stays sorted and
/// has each key once.
pub fn set_hidden(settings: &gio::Settings, key: &str, hide: bool) -> Result<(), glib::BoolError> {
    let mut keys: Vec<String> = hidden(settings).into_iter().filter(|k| k != key).collect();
    if hide {
        keys.push(key.to_owned());
    }
    keys.sort();
    settings.set_strv(HIDDEN, keys)
}

/// What the dialog shows besides the settings.
pub struct DialogSetup<'a> {
    pub settings: &'a gio::Settings,
    /// The latest sample of the Overview, with the hidden rows.
    pub groups: &'a [Group],
    pub numbers: &'a Numbers,
    /// The file with the user's own names for the board (D22), when a
    /// profile matches.
    pub labels_file: Option<PathBuf>,
}

/// The Preferences dialog: a page with the general settings and a page
/// with a switch for every reading.
pub fn dialog(setup: &DialogSetup) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::new();
    dialog.add(&general_page(setup));
    dialog.add(&readings_page(setup));
    dialog
}

fn general_page(setup: &DialogSetup) -> adw::PreferencesPage {
    let settings = setup.settings;
    let page = adw::PreferencesPage::builder()
        .name("general")
        .title(gettext("General"))
        .icon_name("preferences-system-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Overview"))
        .build();

    let interval = adw::SpinRow::with_range(1.0, f64::from(MAX_INTERVAL), 1.0);
    interval.set_title(&gettext("Refresh interval"));
    interval.set_subtitle(&gettext("Seconds between two readings of the sensors"));
    // The value with its unit, "1 s"; typing "5" or "5 s" both work. A
    // numeric spin row would refuse the unit.
    interval.set_numeric(false);
    interval.connect_output(|row| {
        row.set_text(&seconds_text(row.value().round() as u32));
        true
    });
    interval.connect_input(|row| Some(parse_seconds(&row.text()).map(f64::from).ok_or(())));
    interval.set_value(f64::from(interval_seconds(settings)));
    interval.connect_value_notify(glib::clone!(
        #[strong]
        settings,
        move |row| {
            // Whole seconds within the range of the spin row.
            let seconds = row.value().round().clamp(1.0, f64::from(MAX_INTERVAL)) as u32;
            if let Err(e) = set_interval(&settings, seconds) {
                eprintln!("baseboard: cannot save the refresh interval: {e}");
            }
        }
    ));
    group.add(&interval);

    let names = adw::SwitchRow::builder()
        .title(gettext("Names from lm-sensors"))
        .subtitle(gettext(
            "As the sensors command shows them, where the lm-sensors configuration has a label",
        ))
        .build();
    settings.bind(SENSORS_LABELS, &names, "active").build();
    group.add(&names);

    if let Some(file) = &setup.labels_file {
        group.add(&own_names_row(file));
    }
    page.add(&group);
    page
}

/// A number of seconds with its unit, "5 s".
fn seconds_text(seconds: u32) -> String {
    format!("{seconds}{UNIT_SPACE}s")
}

/// The seconds typed into the refresh interval, with or without the unit.
fn parse_seconds(text: &str) -> Option<u32> {
    let number = text.trim().trim_end_matches('s').trim_end();
    number.parse().ok()
}

/// The user's own names (D22): the file in one line, with a button that
/// opens it in the default app, or its folder when there is no file yet.
fn own_names_row(file: &Path) -> adw::ActionRow {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(gettext("Own names"))
        .subtitle(own_names(&name))
        .tooltip_text(home_relative(file))
        .build();
    let (tooltip, target) = if file.exists() {
        (gettext("Open the file"), file.to_owned())
    } else {
        (
            gettext("Open the folder"),
            file.parent().map(Path::to_owned).unwrap_or_default(),
        )
    };
    let button = gtk::Button::builder()
        .icon_name("document-edit-symbolic")
        .tooltip_text(&tooltip)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    button.update_property(&[gtk::accessible::Property::Label(&tooltip)]);
    button.connect_clicked(move |button| open(button, &target));
    row.add_suffix(&button);
    row.set_activatable_widget(Some(&button));
    row
}

/// Opens a file or a folder in the default app; a missing folder is made
/// first.
fn open(button: &gtk::Button, path: &Path) {
    if !path.exists()
        && let Err(e) = std::fs::create_dir_all(path)
    {
        eprintln!("baseboard: cannot make {}: {e}", path.display());
        return;
    }
    let window = button.root().and_downcast::<gtk::Window>();
    gtk::FileLauncher::new(Some(&gio::File::for_path(path))).launch(
        window.as_ref(),
        gio::Cancellable::NONE,
        |result| {
            if let Err(e) = result {
                eprintln!("baseboard: cannot open the own names: {e}");
            }
        },
    );
}

/// A path with the home folder written as "~".
pub fn home_relative(path: &std::path::Path) -> String {
    match path.strip_prefix(glib::home_dir()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

// No doc comment: xgettext would take it for the translators.
fn own_names(file: &str) -> String {
    // Translators: {} is a file name, e.g. ms-7e61.toml, with the user's
    // own names for the board's readings. Keep it to one short line.
    gettext("File {}; changes apply after a restart").replace("{}", file)
}

fn readings_page(setup: &DialogSetup) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .name("readings")
        .title(gettext("Readings"))
        .icon_name("view-reveal-symbolic")
        .description(gettext(
            "Readings that are turned off are not shown on the Overview page, \
             for example headers without a fan.",
        ))
        .build();
    if setup.groups.is_empty() {
        let group = adw::PreferencesGroup::new();
        group.add(
            &adw::ActionRow::builder()
                .title(gettext("No sensor readings"))
                .css_classes(["dimmed"])
                .build(),
        );
        page.add(&group);
        return page;
    }
    let hidden = hidden(setup.settings);
    for data in setup.groups {
        let group = adw::PreferencesGroup::builder()
            .title(glib::markup_escape_text(&overview::group_title(data.kind)))
            .build();
        if let Some(description) = overview::device_description(data) {
            group.set_description(Some(&glib::markup_escape_text(&description)));
        }
        for row in data.rows.iter().chain(&data.auxiliary) {
            let key = row.id.key();
            let title = overview::name(&row.name, setup.numbers);
            let line = adw::ActionRow::builder()
                .use_markup(false)
                .title(&title)
                .subtitle(overview::subtitle(row, setup.numbers))
                .build();
            // The value when the dialog opened, e.g. 0 rpm for a header
            // without a fan; before the switch, as on the Overview page.
            let value = gtk::Label::builder()
                .label(
                    setup
                        .numbers
                        .value_of(row.kind, row.value, overview::nominal(row)),
                )
                .css_classes(["dimmed", "numeric"])
                .build();
            line.add_suffix(&value);
            let switch = gtk::Switch::builder()
                .active(!hidden.contains(&key))
                .valign(gtk::Align::Center)
                .build();
            switch.update_property(&[gtk::accessible::Property::Label(&title)]);
            line.add_suffix(&switch);
            line.set_activatable_widget(Some(&switch));
            switch.connect_active_notify(glib::clone!(
                #[strong(rename_to = settings)]
                setup.settings,
                move |switch| {
                    if let Err(e) = set_hidden(&settings, &key, !switch.is_active()) {
                        eprintln!("baseboard: cannot save the hidden readings: {e}");
                    }
                }
            ));
            group.add(&line);
        }
        page.add(&group);
    }
    page
}

#[cfg(test)]
mod tests {
    //! The real schema from data/, compiled into a temporary folder, with
    //! settings kept in memory. Without glib-compile-schemas the tests are
    //! skipped.

    use std::path::Path;
    use std::process::Command;

    use super::*;

    const SCHEMA_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data");

    /// Settings of the app's schema in memory, or `None` without the tool.
    fn settings(test: &str) -> Option<gio::Settings> {
        let dir =
            std::env::temp_dir().join(format!("baseboard-schema-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let schema = format!("{}.gschema.xml", crate::config::APP_ID);
        std::fs::copy(Path::new(SCHEMA_DIR).join(&schema), dir.join(&schema)).unwrap();
        let compiled = Command::new("glib-compile-schemas")
            .arg("--strict")
            .arg(&dir)
            .status();
        if !compiled.is_ok_and(|status| status.success()) {
            eprintln!("glib-compile-schemas not available, test skipped");
            return None;
        }
        let source =
            gio::SettingsSchemaSource::from_directory(&dir, None, false).expect("compiled schema");
        let schema = source.lookup(crate::config::APP_ID, false).unwrap();
        let settings =
            gio::Settings::new_full(&schema, Some(&gio::memory_settings_backend_new()), None);
        std::fs::remove_dir_all(dir).unwrap();
        Some(settings)
    }

    #[test]
    fn defaults() {
        let Some(settings) = settings("defaults") else {
            return;
        };
        assert_eq!(options(&settings), Options::default());
        assert!(hidden(&settings).is_empty());
    }

    #[test]
    fn interval_and_names() {
        let Some(settings) = settings("options") else {
            return;
        };
        set_interval(&settings, 5).unwrap();
        settings.set_boolean(SENSORS_LABELS, true).unwrap();
        assert_eq!(
            options(&settings),
            Options {
                interval: Duration::from_secs(5),
                sensors_labels: true,
            }
        );
        // Out of range: kept within it.
        set_interval(&settings, 0).unwrap();
        assert_eq!(interval_seconds(&settings), 1);
        set_interval(&settings, 60).unwrap();
        assert_eq!(interval_seconds(&settings), 10);
    }

    #[test]
    fn seconds_with_their_unit() {
        assert_eq!(seconds_text(1), "1\u{a0}s");
        assert_eq!(seconds_text(10), "10\u{a0}s");
        for text in ["5", "5 s", "5s", " 5\u{a0}s ", "5\u{a0}s"] {
            assert_eq!(parse_seconds(text), Some(5), "{text:?}");
        }
        for text in ["", "s", "five", "-1", "1.5"] {
            assert_eq!(parse_seconds(text), None, "{text:?}");
        }
    }

    #[test]
    fn hidden_readings() {
        let Some(settings) = settings("hidden") else {
            return;
        };
        let pump = "nct6687/fan2@devices/platform/nct6683.2592";
        let aux = "nct6687/fan4@devices/platform/nct6683.2592";
        set_hidden(&settings, pump, true).unwrap();
        set_hidden(&settings, aux, true).unwrap();
        set_hidden(&settings, pump, true).unwrap();
        let stored: Vec<String> = settings
            .strv(HIDDEN)
            .iter()
            .map(|k| k.to_string())
            .collect();
        assert_eq!(stored, [pump, aux], "sorted, each once");
        set_hidden(&settings, pump, false).unwrap();
        assert_eq!(hidden(&settings), HashSet::from([aux.to_owned()]));
    }
}
