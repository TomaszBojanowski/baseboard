// SPDX-License-Identifier: GPL-3.0-or-later

//! The System page (docs/PROJEKT.md §5.4, D64, D67), read only. On top one
//! status row in the words of the status sentence of the Overview, then a
//! short list of the computer in plain words. Two pages the window pushes
//! over it hold the rest: "Diagnostics" (the drivers and programs around
//! the sensors and fans, the board profile, skipped files, warnings of the
//! readings) and "Technical Details" (the board, BIOS, processor and
//! system, memory, graphics cards, disks and network cards with their
//! codes and versions). In a wide window the groups of the page and of
//! "Technical Details" are in two columns (layout.rs); "Diagnostics" is one
//! list at any width (D76). The data comes from a worker thread
//! (`system_reader`), the groups as data from `sections`; the text of
//! "Copy system information" from `core::report`, always in English.

pub mod sections;

use std::cell::{OnceCell, RefCell};
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::board::{Detection, Dmi};
use baseboard_core::report::Report;
use baseboard_core::system::SystemInfo;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config;
use crate::layout::{self, Columns, Place, Width};
use crate::sensors::Choice;
use crate::system_reader::Worker;
use crate::widgets::chevron;
use sections::{Extras, Health, Line, Section, Status, Style, health, status_texts, views};
pub use sections::{no_profile, read_only_fans};

/// What the page needs besides the data it reads.
pub struct Setup {
    /// The sysfs root: /sys, or a fake tree in a debug build.
    pub root: PathBuf,
    pub dmi: Dmi,
    pub detection: Detection,
    /// Files left out at start because they are wrong.
    pub skipped: Vec<SkippedFile>,
}

/// A file left out at start, which the app works without (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub kind: FileKind,
    /// Why, as the parser says it; not translated.
    pub reason: String,
}

impl SkippedFile {
    /// `reason` in one line: a TOML error shows the line of the file under
    /// its message, which the page leaves out ("…at line 1, column 1:
    /// missing field `model`").
    pub fn new(path: PathBuf, kind: FileKind, reason: &str) -> Self {
        let parts: Vec<&str> = reason
            .lines()
            .map(str::trim)
            .filter(|line| {
                let quoted = line.starts_with('|')
                    || line
                        .split_once(" |")
                        .is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit()));
                !line.is_empty() && !quoted
            })
            .collect();
        Self {
            path,
            kind,
            reason: parts.join(": "),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A board profile (§6.2).
    Profile,
    /// The user's custom names for readings (D22).
    Labels,
}

/// The row on top of the page, which opens "Diagnostics", in its own list
/// with the background of a warning when it warns, as on the Overview.
fn status_group(health: &Health) -> adw::PreferencesGroup {
    let (icon, class, title, subtitle) = status_texts(health);
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(&title)
        .subtitle(&subtitle)
        .activatable(true)
        .action_name("win.show-system-view")
        .action_target(&"diagnostics".to_variant())
        .css_classes(["status-row"])
        .build();
    let image = gtk::Image::builder()
        .icon_name(icon)
        .css_classes(["status-icon", class])
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    row.add_prefix(&image);
    row.add_suffix(&chevron());
    crate::widgets::describe_row(&row, None);
    let group = adw::PreferencesGroup::builder()
        .css_classes(["status-list"])
        .build();
    if matches!(health, Health::Attention(_)) {
        group.add_css_class("warning-status");
    }
    group.add(&row);
    group
}

/// The row that opens "Technical Details".
fn details_row() -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(gettext("Technical details"))
        .subtitle(gettext(
            "Codes, versions, drivers and identifiers for bug reports",
        ))
        .activatable(true)
        .action_name("win.show-system-view")
        .action_target(&"details".to_variant())
        .build();
    row.add_suffix(&chevron());
    crate::widgets::describe_row(&row, None);
    row
}

/// The widgets of one group.
fn group_widget(section: &Section) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(glib::markup_escape_text(&section.title))
        .build();
    if let Some(description) = &section.description {
        group.set_description(Some(&glib::markup_escape_text(description)));
    }
    for line in &section.lines {
        group.add(&row(line));
    }
    group
}

/// A row of the page; a line without a value, e.g. a network card in the
/// short list, has its name alone.
fn row(line: &Line) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(&line.title)
        .build();
    let mut value_label = None;
    match line.style {
        Style::Value => {
            // The value next to the name; it wraps in a narrow window and
            // can be selected and copied.
            if !line.value.is_empty() {
                // In one line while there is room: it only wraps when the
                // row is narrower than the name and the value.
                let value = gtk::Label::builder()
                    .label(&line.value)
                    .wrap(true)
                    .natural_wrap_mode(gtk::NaturalWrapMode::None)
                    .xalign(1.0)
                    .justify(gtk::Justification::Right)
                    .selectable(true)
                    .css_classes(["dimmed"])
                    .build();
                row.add_suffix(&value);
                value_label = Some(value);
            }
        }
        Style::Property => row.add_css_class("property"),
        Style::Item => {}
        Style::NoData => row.add_css_class("dimmed"),
    }
    if !line.value.is_empty() && line.style != Style::Value {
        row.set_subtitle(&line.value);
    }
    if line.monospace {
        row.add_css_class("monospace-value");
        // Paths and commands can be selected and copied.
        row.set_subtitle_selectable(true);
    }
    if let Some(status) = line.status {
        let (icon, class) = match status {
            Status::Ok => ("baseboard-ok-symbolic", "success"),
            Status::Warning => ("dialog-warning-symbolic", "warning"),
            Status::Info => ("help-about-symbolic", "dimmed"),
        };
        let image = gtk::Image::builder()
            .icon_name(icon)
            .css_classes([class])
            .valign(gtk::Align::Center)
            .build();
        // The text says the same.
        image.update_state(&[gtk::accessible::State::Hidden(true)]);
        row.add_prefix(&image);
    }
    if let Some(icon) = line.icon {
        // In a light circle, as on the cards of the Overview; the title
        // says what it is.
        let image = gtk::Image::builder()
            .icon_name(icon)
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["part-badge", "row-badge"])
            .build();
        row.add_prefix(&image);
    }
    // The title is the row's name; its value is read after it.
    crate::widgets::describe_row(&row, value_label.as_ref());
    row
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/me/bojanowski/Baseboard/system.ui")]
    pub struct SystemPage {
        #[template_child]
        pub bin: TemplateChild<adw::BreakpointBin>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub clamp: TemplateChild<adw::Clamp>,
        #[template_child]
        pub page: TemplateChild<gtk::Box>,
        #[template_child]
        pub diagnostics_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub diagnostics_bin: TemplateChild<adw::BreakpointBin>,
        #[template_child]
        pub diagnostics_copy: TemplateChild<adw::ButtonContent>,
        #[template_child]
        pub diagnostics: TemplateChild<gtk::Box>,
        #[template_child]
        pub details_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub details_bin: TemplateChild<adw::BreakpointBin>,
        #[template_child]
        pub details_clamp: TemplateChild<adw::Clamp>,
        #[template_child]
        pub details_copy: TemplateChild<adw::ButtonContent>,
        #[template_child]
        pub details_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub details: TemplateChild<gtk::Box>,
        /// The status row on top of the page, to take it off again.
        pub(super) status: RefCell<Option<adw::PreferencesGroup>>,
        /// The groups of the page and of "Technical Details".
        pub(super) columns: Columns,
        pub(super) details_columns: Columns,
        /// Warnings of the readings from the Overview, in words.
        pub(super) alerts: RefCell<Vec<String>>,
        pub(super) board: OnceCell<(Dmi, Detection)>,
        pub(super) info: RefCell<Option<SystemInfo>>,
        pub(super) worker: RefCell<Option<Worker>>,
        pub(super) copy: OnceCell<gio::SimpleAction>,
        pub(super) source: RefCell<Option<Choice>>,
        pub(super) skipped: OnceCell<Vec<SkippedFile>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SystemPage {
        const NAME: &'static str = "BaseboardSystemPage";
        type Type = super::SystemPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SystemPage {
        fn constructed(&self) {
            self.parent_constructed();
            self.page.append(&self.columns.widget);
            self.details.append(&self.details_columns.widget);
            // In a narrow window the title of "Technical Details" does not
            // fit beside the buttons: a shorter one there, never a cut one.
            let short = gettext(
                // Translators: the title of the page "Technical Details" in
                // a narrow window, where the whole one does not fit.
                "Details",
            );
            // "Diagnostics" keeps the width of a list (system.ui); the page
            // and "Technical Details" grow with the window.
            let views = [
                (&self.bin, Some(&self.clamp), None, None),
                (
                    &self.diagnostics_bin,
                    None,
                    Some(&self.diagnostics_copy),
                    None,
                ),
                (
                    &self.details_bin,
                    Some(&self.details_clamp),
                    Some(&self.details_copy),
                    Some((&self.details_title, short)),
                ),
            ];
            for (index, (bin, clamp, copy, title)) in views.into_iter().enumerate() {
                if let Some(clamp) = clamp {
                    layout::limit(clamp);
                }
                let page = self.obj().downgrade();
                // The copy button of a narrow view shows only its icon, as
                // the one of the window does.
                let copy = copy.map(|copy| ((**copy).clone(), copy.label()));
                let title =
                    title.map(|(widget, short)| ((**widget).clone(), widget.title(), short));
                layout::watch(bin, move |width| {
                    let narrow = width == Width::Narrow;
                    if let Some((copy, label)) = &copy {
                        copy.set_label(if narrow { "" } else { label });
                    }
                    if let Some((widget, full, short)) = &title {
                        widget.set_title(if narrow { short } else { full });
                    }
                    if let Some(page) = page.upgrade() {
                        let imp = page.imp();
                        let columns = [Some(&imp.columns), None, Some(&imp.details_columns)];
                        if let Some(columns) = columns[index] {
                            columns.set_two(width.columns());
                        }
                    }
                });
            }
        }
    }

    impl WidgetImpl for SystemPage {}
    impl BinImpl for SystemPage {}
}

glib::wrapper! {
    pub struct SystemPage(ObjectSubclass<imp::SystemPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SystemPage {
    /// Starts the worker thread, idle until the page is shown. `copy` is
    /// the action of "Copy system information", enabled once there is data.
    pub fn start(&self, setup: Setup, copy: &gio::SimpleAction) {
        let imp = self.imp();
        let page = glib::SendWeakRef::from(self.downgrade());
        let worker = Worker::start(
            setup.root,
            setup.detection.profile().cloned(),
            move |info| {
                let page = page.clone();
                glib::idle_add_once(move || {
                    if let Some(page) = page.upgrade() {
                        page.show(info);
                    }
                });
            },
        );
        let _ = imp.board.set((setup.dmi, setup.detection));
        let _ = imp.skipped.set(setup.skipped);
        let _ = imp.copy.set(copy.clone());
        imp.worker.replace(Some(worker));
    }

    /// Reads while `active`: the page is shown and the window visible.
    /// `reread`: the page was entered, so read again at once.
    pub fn set_active(&self, active: bool, reread: bool) {
        if let Some(worker) = self.imp().worker.borrow().as_ref() {
            worker.set_active(active, reread);
        }
    }

    /// The text of "Copy system information", once there is data.
    pub fn report(&self) -> Option<String> {
        let imp = self.imp();
        let (dmi, detection) = imp.board.get()?;
        let info = imp.info.borrow();
        Some(
            Report {
                version: config::VERSION,
                installation: &config::installation(),
                dmi,
                detection,
                info: info.as_ref()?,
            }
            .text(),
        )
    }

    /// Debug builds: scrolls to the end, for a screenshot of the lower
    /// groups.
    #[cfg(debug_assertions)]
    pub fn scroll_to_end(&self) {
        let adjustment = self.imp().scrolled.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }

    /// How the Overview reads the sensors (libsensors or raw sysfs), for
    /// the line about libsensors.
    pub fn set_source(&self, source: Choice) {
        let imp = self.imp();
        if imp.source.borrow().as_ref() == Some(&source) {
            return;
        }
        imp.source.replace(Some(source));
        self.render();
    }

    /// Warnings of the readings from the Overview, in words (D64, D65); the
    /// status row and "Diagnostics" change only when they do.
    pub fn set_alerts(&self, alerts: Vec<String>) {
        let imp = self.imp();
        if *imp.alerts.borrow() == alerts {
            return;
        }
        imp.alerts.replace(alerts);
        self.render();
    }

    /// The view "Diagnostics" for the window to push.
    pub fn diagnostics_page(&self) -> adw::NavigationPage {
        self.imp().diagnostics_page.clone()
    }

    /// The view "Technical Details" for the window to push.
    pub fn details_page(&self) -> adw::NavigationPage {
        self.imp().details_page.clone()
    }

    fn show(&self, info: SystemInfo) {
        let imp = self.imp();
        // The same data: the widgets, and where the page is scrolled, stay.
        if imp.info.borrow().as_ref() == Some(&info) {
            return;
        }
        imp.info.replace(Some(info));
        self.render();
    }

    /// Makes the groups of the page and of its two views again from the
    /// data and the extras, once there is data.
    fn render(&self) {
        let imp = self.imp();
        let Some((dmi, detection)) = imp.board.get() else {
            return;
        };
        let info = imp.info.borrow();
        let Some(info) = info.as_ref() else {
            return;
        };
        let source = imp.source.borrow();
        let extras = Extras {
            source: source.as_ref(),
            skipped: imp.skipped.get().map_or(&[], Vec::as_slice),
        };
        let alerts = imp.alerts.borrow();
        let views = views(dmi, detection, info, extras, &alerts);
        let status = status_group(&health(detection, info, extras, &alerts));
        imp.page.prepend(&status);
        if let Some(old) = imp.status.replace(Some(status)) {
            imp.page.remove(&old);
        }
        let all = [
            (&imp.columns, &views.summary),
            (&imp.details_columns, &views.details),
        ];
        for (columns, sections) in all {
            columns.clear();
            for section in sections {
                columns.push(&group_widget(section), section.place);
            }
        }
        while let Some(old) = imp.diagnostics.first_child() {
            imp.diagnostics.remove(&old);
        }
        for section in &views.diagnostics {
            imp.diagnostics.append(&group_widget(section));
        }
        let details = adw::PreferencesGroup::new();
        details.add(&details_row());
        imp.columns.push(&details, Place::Right);
        imp.stack.set_visible_child_name("info");
        if let Some(copy) = imp.copy.get() {
            copy.set_enabled(true);
        }
    }
}

#[cfg(test)]
mod tests {
    //! gettext without a translation gives the English text.

    use super::*;

    /// The widgets under `widget` of the type `T`, in order.
    fn descendants<T: IsA<gtk::Widget>>(widget: &gtk::Widget) -> Vec<T> {
        let mut found = Vec::new();
        let mut child = widget.first_child();
        while let Some(current) = child {
            if let Some(wanted) = current.downcast_ref::<T>() {
                found.push(wanted.clone());
            }
            found.extend(descendants::<T>(&current));
            child = current.next_sibling();
        }
        found
    }

    /// "Diagnostics" is one list at any width (D76): in a wide window its
    /// groups stand one under another, as wide as a list of GNOME Settings.
    /// The sign of the status row is in the middle of its two lines. Needs
    /// a display.
    #[test]
    fn diagnostics_in_one_list() {
        crate::gtk_test::run("diagnostics_in_one_list", || {
            crate::gtk_test::resources();
            let page = glib::Object::new::<SystemPage>();
            let imp = page.imp();
            let _ = imp.board.set((Dmi::default(), Detection::Generic));
            imp.alerts
                .replace(vec!["CPU fan stopped".into(), "+12 V below normal".into()]);
            imp.info.replace(Some(SystemInfo::default()));
            page.render();
            let width = 1600;
            for bin in [&*imp.diagnostics_bin, &*imp.bin] {
                let (_, height, _, _) = bin.measure(gtk::Orientation::Vertical, width);
                bin.allocate(width, height.max(800), -1, None);
            }

            let groups = descendants::<adw::PreferencesGroup>(imp.diagnostics.upcast_ref());
            assert!(groups.len() >= 3, "{}", groups.len());
            let bounds: Vec<_> = groups
                .iter()
                .map(|group| group.compute_bounds(&*imp.diagnostics_bin).unwrap())
                .collect();
            for group in &bounds {
                assert_eq!(group.x(), bounds[0].x(), "{bounds:?}");
                assert!(group.width() <= 600.0, "{bounds:?}");
            }

            let status = imp.status.borrow().clone().unwrap();
            let row = &descendants::<adw::ActionRow>(status.upcast_ref())[0];
            assert_eq!(row.subtitle().as_deref(), Some("And 1 more problem"));
            let icon = descendants::<gtk::Image>(row.upcast_ref())
                .into_iter()
                .find(|image| image.has_css_class("status-icon"))
                .unwrap();
            let icon = icon.compute_bounds(row).unwrap();
            let middle = icon.y() + icon.height() / 2.0;
            assert!(
                (middle - row.height() as f32 / 2.0).abs() <= 1.0,
                "{middle} in {}",
                row.height()
            );
        });
    }

    /// The title of "Technical Details" is whole: shorter in a window of
    /// 360 px, where it does not fit beside the buttons, the whole one
    /// wider; the page keeps its name for screen readers. Needs a display.
    #[test]
    fn title_of_the_details_fits() {
        crate::gtk_test::run("title_of_the_details_fits", || {
            crate::gtk_test::resources();
            let page = glib::Object::new::<SystemPage>();
            let imp = page.imp();
            let title_at = |width: i32| {
                let (_, height, _, _) = imp.details_bin.measure(gtk::Orientation::Vertical, width);
                imp.details_bin.allocate(width, height.max(400), -1, None);
                imp.details_title.title().to_string()
            };
            assert_eq!(title_at(360), "Details");
            assert_eq!(title_at(1200), "Technical Details");
            assert_eq!(imp.details_page.title(), "Technical Details");
        });
    }
}
