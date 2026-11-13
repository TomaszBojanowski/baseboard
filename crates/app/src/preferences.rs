// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings (docs/PROJEKT.md §3.2) and the Preferences dialog: the
//! refresh interval, names from lm-sensors, and the readings and headers
//! shown on the Overview page. Every change goes to GSettings at once; the
//! window listens to GSettings and applies it without a restart.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use adw::prelude::*;
use baseboard_core::overview::{self as model, Group, RowId, Visibility};
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
/// The keys of rows hidden by default that were turned on (D42), sorted.
pub const SHOWN: &str = "shown-readings";

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

/// The rows turned off and on.
pub fn visibility(settings: &gio::Settings) -> Visibility {
    Visibility {
        hidden: listed(settings, HIDDEN),
        shown: listed(settings, SHOWN),
    }
}

/// Turns the row with `id` on or off. A row hidden by default (D42) is on
/// while its key is in the shown readings; any other row is off while its
/// key is in the hidden ones.
pub fn set_visible(settings: &gio::Settings, id: &RowId, on: bool) -> Result<(), glib::BoolError> {
    let key = id.key();
    if model::hidden_by_default(id) {
        set_listed(settings, SHOWN, &key, on)?;
        if on {
            set_listed(settings, HIDDEN, &key, false)?;
        }
        Ok(())
    } else {
        set_listed(settings, HIDDEN, &key, !on)
    }
}

/// The keys in the list `name`.
fn listed(settings: &gio::Settings, name: &str) -> HashSet<String> {
    settings
        .strv(name)
        .iter()
        .map(|key| key.to_string())
        .collect()
}

/// Puts `key` into the list `name`, or takes it out. The list stays sorted
/// and has each key once.
fn set_listed(
    settings: &gio::Settings,
    name: &str,
    key: &str,
    present: bool,
) -> Result<(), glib::BoolError> {
    let mut keys: Vec<String> = listed(settings, name)
        .into_iter()
        .filter(|k| k != key)
        .collect();
    if present {
        keys.push(key.to_owned());
    }
    keys.sort();
    settings.set_strv(name, keys)
}

/// What the dialog shows besides the settings.
pub struct DialogSetup<'a> {
    pub settings: &'a gio::Settings,
    /// The latest sample of the Overview, with the hidden rows.
    pub groups: &'a [Group],
    pub numbers: &'a Numbers,
    /// The file with the user's custom names for the board (D22), when a
    /// profile matches.
    pub labels_file: Option<PathBuf>,
}

/// The Preferences dialog (D64): one page without tabs; the switch for
/// every reading is one click further, on the subpage "Visible Readings".
pub fn dialog(setup: &DialogSetup) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::new();
    let readings = readings_page(setup);
    dialog.add(&main_page(setup, &dialog, &readings));
    dialog
}

/// Debug builds and tests: the row titled `title` in the dialog, depth
/// first, to click it as a user does. A dialog takes its content into the
/// window only when presented, so the search starts at its child.
#[cfg(any(test, debug_assertions))]
pub fn row_titled(dialog: &adw::PreferencesDialog, title: &str) -> Option<adw::ActionRow> {
    fn find(widget: &gtk::Widget, title: &str) -> Option<adw::ActionRow> {
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(row) = current.downcast_ref::<adw::ActionRow>()
                && row.title() == title
            {
                return Some(row.clone());
            }
            if let Some(found) = find(&current, title) {
                return Some(found);
            }
            child = current.next_sibling();
        }
        None
    }
    find(&dialog.child()?, title)
}

fn main_page(
    setup: &DialogSetup,
    dialog: &adw::PreferencesDialog,
    readings: &adw::NavigationPage,
) -> adw::PreferencesPage {
    let settings = setup.settings;
    let page = adw::PreferencesPage::builder().name("general").build();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Readings"))
        .build();

    let interval = adw::SpinRow::with_range(1.0, f64::from(MAX_INTERVAL), 1.0);
    interval.set_title(&gettext("Refresh interval"));
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

    // The readings to show, one click further (D64).
    let visible = adw::ActionRow::builder()
        .title(gettext("Visible readings"))
        .subtitle(gettext("Choose which readings Baseboard shows"))
        .activatable(true)
        .build();
    visible.add_suffix(
        &gtk::Image::builder()
            .icon_name("go-next-symbolic")
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["dimmed"])
            .build(),
    );
    // The row keeps the subpage: nothing else holds it until it is shown.
    visible.connect_activated(glib::clone!(
        #[weak]
        dialog,
        #[strong]
        readings,
        move |_| dialog.push_subpage(&readings)
    ));
    group.add(&visible);
    page.add(&group);

    let group = adw::PreferencesGroup::builder()
        .title(gettext("Names"))
        .build();
    let names = adw::SwitchRow::builder()
        .title(gettext("Names from lm-sensors"))
        .subtitle(gettext("As the sensors command shows them"))
        .build();
    settings.bind(SENSORS_LABELS, &names, "active").build();
    // A Flatpak sandbox without the host's /etc has no configuration to
    // take names from (docs/PROJEKT.md §10.3).
    if crate::config::sandbox().is_some_and(|sandbox| !sandbox.host_etc) {
        names.set_sensitive(false);
        names.set_subtitle(&gettext(
            "The system's lm-sensors configuration is not available in Flatpak",
        ));
    }
    group.add(&names);

    if let Some(file) = &setup.labels_file {
        group.add(&custom_names_row(file));
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

/// The user's custom names (D22), with a button that opens their file in
/// the default app, or its folder when there is no file yet. The path is in
/// the tooltip (D64).
fn custom_names_row(file: &Path) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(gettext("Custom names"))
        .subtitle(gettext("Changes apply after a restart"))
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
                eprintln!("baseboard: cannot open the custom names: {e}");
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

/// The subpage with a switch for every reading: its name and its value
/// when the dialog opened, without drivers and sensors (D64).
fn readings_page(setup: &DialogSetup) -> adw::NavigationPage {
    let page = readings_list(setup);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&page));
    adw::NavigationPage::builder()
        .title(gettext("Visible Readings"))
        .tag("readings")
        .child(&view)
        .build()
}

fn readings_list(setup: &DialogSetup) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .name("readings")
        .description(gettext(
            "Readings that are turned off are not shown on any page, for \
             example headers without a fan. The numbered temperature sensors \
             of disks are off until turned on.",
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
    let visibility = visibility(setup.settings);
    for data in setup.groups {
        let group = adw::PreferencesGroup::builder()
            .title(glib::markup_escape_text(&overview::group_title(data.kind)))
            .build();
        for row in data.rows.iter().chain(&data.auxiliary) {
            let id = row.id.clone();
            let title = overview::name(&row.name, setup.numbers);
            let line = adw::ActionRow::builder()
                .use_markup(false)
                .title(&title)
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
                .active(visibility.shows(&id))
                .valign(gtk::Align::Center)
                .build();
            switch.update_property(&[gtk::accessible::Property::Label(&title)]);
            line.add_suffix(&switch);
            line.set_activatable_widget(Some(&switch));
            switch.connect_active_notify(glib::clone!(
                #[strong(rename_to = settings)]
                setup.settings,
                move |switch| {
                    if let Err(e) = set_visible(&settings, &id, switch.is_active()) {
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
        assert_eq!(visibility(&settings), Visibility::default());
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

    fn id(chip: &str, channel: &str, device: &str) -> RowId {
        RowId {
            chip: chip.into(),
            device: Some(PathBuf::from(device)),
            channel: channel.into(),
        }
    }

    fn stored(settings: &gio::Settings, name: &str) -> Vec<String> {
        settings.strv(name).iter().map(|k| k.to_string()).collect()
    }

    #[test]
    fn hidden_readings() {
        let Some(settings) = settings("hidden") else {
            return;
        };
        let board = "devices/platform/nct6683.2592";
        let pump = id("nct6687", "fan2", board);
        let aux = id("nct6687", "fan4", board);
        set_visible(&settings, &pump, false).unwrap();
        set_visible(&settings, &aux, false).unwrap();
        set_visible(&settings, &pump, false).unwrap();
        assert_eq!(
            stored(&settings, HIDDEN),
            [pump.key(), aux.key()],
            "sorted, each once"
        );
        set_visible(&settings, &pump, true).unwrap();
        assert_eq!(stored(&settings, HIDDEN), [aux.key()]);
        assert!(stored(&settings, SHOWN).is_empty());
        let visibility = visibility(&settings);
        assert!(visibility.shows(&pump));
        assert!(!visibility.shows(&aux));
    }

    /// D42: a drive's numbered sensor is off by default and on while its
    /// key is in the shown readings; the composite temperature is like any
    /// other row.
    #[test]
    fn disk_sensors_turned_on() {
        let Some(settings) = settings("shown") else {
            return;
        };
        let disk = "devices/pci0000:00/0000:00:01.2/0000:04:00.0/nvme/nvme0";
        let sensor = id("nvme", "temp2", disk);
        let composite = id("nvme", "temp1", disk);
        assert!(!visibility(&settings).shows(&sensor));
        assert!(visibility(&settings).shows(&composite));

        set_visible(&settings, &sensor, true).unwrap();
        assert_eq!(stored(&settings, SHOWN), [sensor.key()]);
        assert!(stored(&settings, HIDDEN).is_empty());
        assert!(visibility(&settings).shows(&sensor));

        set_visible(&settings, &sensor, false).unwrap();
        assert!(stored(&settings, SHOWN).is_empty());
        assert!(!visibility(&settings).shows(&sensor));

        set_visible(&settings, &composite, false).unwrap();
        assert_eq!(stored(&settings, HIDDEN), [composite.key()]);
        assert!(!visibility(&settings).shows(&composite));
    }

    /// Clicking "Visible readings" opens its subpage, also when nothing but
    /// the dialog is kept, as the app keeps it (B2). Needs a display.
    #[test]
    fn visible_readings_row_opens_the_subpage() {
        crate::gtk_test::run("visible_readings_row_opens_the_subpage", || {
            let Some(settings) = settings("readings-row") else {
                return;
            };
            let numbers = Numbers::new(".");
            let shown = dialog(&DialogSetup {
                settings: &settings,
                groups: &[],
                numbers: &numbers,
                labels_file: None,
            });
            // Over a window, as the app presents it.
            let window = adw::Window::new();
            shown.present(Some(&window));
            let row = row_titled(&shown, "Visible readings").expect("the row");
            assert!(WidgetExt::activate(&row));
            assert!(shown.pop_subpage(), "no subpage was opened");
            window.destroy();
        });
    }
}
