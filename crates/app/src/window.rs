// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: a sidebar with the pages and the page shown next to it.

use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::board::{Detection, Dmi};
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use crate::config;
use crate::fans::FansPage;
use crate::overview::OverviewPage;
use crate::preferences::{self, DialogSetup};
use crate::sampler::{FanReads, Sample, Setup, Worker};
use crate::system::{self, SystemPage};

/// The pages, in the order of the sidebar items in window.ui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    Fans,
    System,
}

impl Page {
    const ALL: [Self; 3] = [Self::Overview, Self::Fans, Self::System];

    fn index(self) -> u32 {
        match self {
            Self::Overview => 0,
            Self::Fans => 1,
            Self::System => 2,
        }
    }

    fn from_index(index: u32) -> Self {
        Self::ALL
            .into_iter()
            .find(|page| page.index() == index)
            .unwrap_or(Self::Overview)
    }

    /// The name of the page in the stack of window.ui.
    fn name(self) -> &'static str {
        match self {
            Self::Overview => "overview",
            Self::Fans => "fans",
            Self::System => "system",
        }
    }

    fn title(self) -> String {
        match self {
            Self::Overview => gettext("Overview"),
            Self::Fans => gettext("Fans"),
            Self::System => gettext("System"),
        }
    }
}

mod imp {
    use std::cell::{Cell, OnceCell, RefCell};

    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/me/bojanowski/Baseboard/window.ui")]
    pub struct BaseboardWindow {
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub sidebar: TemplateChild<adw::Sidebar>,
        #[template_child]
        pub board_model: TemplateChild<gtk::Label>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_nav: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub root_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_title: TemplateChild<adw::WindowTitle>,
        #[template_child]
        pub copy_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub toasts: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub pages: TemplateChild<gtk::Stack>,
        #[template_child]
        pub overview: TemplateChild<OverviewPage>,
        #[template_child]
        pub fans: TemplateChild<FansPage>,
        #[template_child]
        pub system: TemplateChild<SystemPage>,
        pub settings: OnceCell<gio::Settings>,
        pub copy_action: OnceCell<gio::SimpleAction>,
        /// The file with the user's own names for the board (D22).
        pub labels_file: RefCell<Option<PathBuf>>,
        /// The sensor reader of the Overview and Fans pages.
        pub worker: RefCell<Option<Worker>>,
        /// What the reader was last told to read of the fan channels.
        pub fan_reads: Cell<Option<FanReads>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BaseboardWindow {
        const NAME: &'static str = "BaseboardWindow";
        type Type = super::BaseboardWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            OverviewPage::ensure_type();
            FansPage::ensure_type();
            SystemPage::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for BaseboardWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.load_window_state();

            // With the window narrow the sidebar and the page are shown one
            // at a time; choosing a page in the sidebar shows it.
            self.sidebar.connect_activated(glib::clone!(
                #[weak(rename_to = split_view)]
                self.split_view,
                move |_, _| split_view.set_show_content(true)
            ));
            self.sidebar.connect_selected_notify(glib::clone!(
                #[weak]
                obj,
                move |_| obj.show_selected_page()
            ));

            // "Copy system information", enabled once the System page has
            // its data.
            let copy = gio::SimpleAction::new("copy-system-info", None);
            copy.set_enabled(false);
            copy.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |_, _| obj.copy_system_info()
            ));
            obj.add_action(&copy);
            let _ = self.copy_action.set(copy);

            // From the Overview's banner and the toast about skipped files.
            let show_system = gio::SimpleAction::new("show-system", None);
            show_system.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |_, _| {
                    let imp = obj.imp();
                    imp.sidebar.set_selected(Page::System.index());
                    imp.split_view.set_show_content(true);
                }
            ));
            obj.add_action(&show_system);

            // A large number or a row of the Overview: "All Sensors" on a
            // group, or at its top for an empty key.
            let show_sensors =
                gio::SimpleAction::new("show-sensors", Some(glib::VariantTy::STRING));
            show_sensors.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |_, key| {
                    if let Some(key) = key.and_then(|k| k.get::<String>()) {
                        obj.show_sensors(&key);
                    }
                }
            ));
            obj.add_action(&show_sensors);

            // The status row and "Technical details" of the System page:
            // its views "diagnostics" and "details".
            let show_system_view =
                gio::SimpleAction::new("show-system-view", Some(glib::VariantTy::STRING));
            show_system_view.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |_, view| {
                    if let Some(view) = view.and_then(|v| v.get::<String>()) {
                        obj.show_system_view(&view);
                    }
                }
            ));
            obj.add_action(&show_system_view);

            // The fans row of the Overview: the Fans page.
            let show_fans = gio::SimpleAction::new("show-fans", None);
            show_fans.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |_, _| {
                    let imp = obj.imp();
                    imp.sidebar.set_selected(Page::Fans.index());
                    imp.split_view.set_show_content(true);
                }
            ));
            obj.add_action(&show_fans);

            // A row of the list of fans: the curve page of that header.
            let show_curve = gio::SimpleAction::new("show-curve", Some(glib::VariantTy::STRING));
            show_curve.connect_activate(glib::clone!(
                #[weak]
                obj,
                move |_, key| {
                    if let Some(key) = key.and_then(|k| k.get::<String>()) {
                        obj.show_curve(&key);
                    }
                }
            ));
            obj.add_action(&show_curve);

            // How the Overview reads the sensors, for the System page.
            self.overview.connect_status(glib::clone!(
                #[weak(rename_to = system)]
                self.system,
                move |status| system.set_source(status.source.clone())
            ));

            // Settings from the Preferences dialog apply at once.
            obj.settings().connect_changed(
                None,
                glib::clone!(
                    #[weak]
                    obj,
                    move |settings, key| match key {
                        preferences::INTERVAL | preferences::SENSORS_LABELS => {
                            if let Some(worker) = obj.imp().worker.borrow().as_ref() {
                                worker.set_options(preferences::options(settings));
                            }
                        }
                        preferences::HIDDEN | preferences::SHOWN => {
                            let visibility = preferences::visibility(settings);
                            obj.imp().fans.set_visibility(visibility.clone());
                            obj.imp().overview.set_visibility(visibility);
                        }
                        _ => {}
                    }
                ),
            );

            obj.show_selected_page();

            // Reading stops while the window cannot be seen (§5.1).
            obj.connect_visible_notify(|window| window.update_reading());
            obj.connect_suspended_notify(|window| window.update_reading());
        }
    }

    impl WidgetImpl for BaseboardWindow {}

    impl WindowImpl for BaseboardWindow {
        fn close_request(&self) -> glib::Propagation {
            self.obj().save_window_state();
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for BaseboardWindow {}
    impl AdwApplicationWindowImpl for BaseboardWindow {}
}

glib::wrapper! {
    pub struct BaseboardWindow(ObjectSubclass<imp::BaseboardWindow>)
        @extends adw::ApplicationWindow, gtk::ApplicationWindow, gtk::Window, gtk::Widget,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
            gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl BaseboardWindow {
    pub fn new(app: &adw::Application) -> Self {
        glib::Object::builder().property("application", app).build()
    }

    /// Starts reading the sensors in a worker thread for the Overview and
    /// Fans pages; samples come to both through the main loop.
    pub fn start_readings(&self, mut setup: Setup) {
        let settings = self.settings();
        setup.options = preferences::options(settings);
        let reads = self.fan_reads();
        setup.reads = reads;
        let imp = self.imp();
        let visibility = preferences::visibility(settings);
        imp.fans.set_visibility(visibility.clone());
        imp.overview.set_visibility(visibility);
        let window = glib::SendWeakRef::from(self.downgrade());
        let worker = Worker::start(setup, move |sample| {
            let window = window.clone();
            glib::idle_add_once(move || {
                if let Some(window) = window.upgrade() {
                    window.show_sample(sample);
                }
            });
        });
        imp.worker.replace(Some(worker));
        imp.fan_reads.set(Some(reads));
        self.update_reading();
    }

    fn show_sample(&self, sample: Sample) {
        let imp = self.imp();
        imp.fans.show(&sample);
        imp.overview.show(sample);
        // The warnings of the readings for the status row of the System
        // page (D64, D65).
        imp.system.set_alerts(imp.overview.alerts());
    }

    /// Opens a view of the System page over it: "diagnostics" or
    /// "details".
    fn show_system_view(&self, view: &str) {
        let imp = self.imp();
        let page = match view {
            "diagnostics" => imp.system.diagnostics_page(),
            "details" => imp.system.details_page(),
            other => {
                eprintln!("baseboard: no view {other:?} of the System page");
                return;
            }
        };
        if imp.content_nav.visible_page().as_ref() != Some(&page) {
            imp.content_nav.push(&page);
        }
    }

    /// Debug builds: opens a view of the System page, as its rows would.
    #[cfg(debug_assertions)]
    pub fn open_system_view(&self, view: &str) {
        self.show_system_view(view);
    }

    /// Opens the curve page of header `key` over the Fans page.
    fn show_curve(&self, key: &str) {
        let imp = self.imp();
        if let Some(page) = imp.fans.curve_page(key) {
            imp.content_nav.push(&page);
        }
    }

    /// Debug builds: opens the curve page of header `key`, as its row does.
    #[cfg(debug_assertions)]
    pub fn open_curve(&self, key: &str) {
        self.show_curve(key);
    }

    /// Opens "All Sensors" over the Overview on the group of `key`, or
    /// scrolls it there when it is open.
    fn show_sensors(&self, key: &str) {
        let imp = self.imp();
        let page = imp.overview.sensors_page(key);
        if imp.content_nav.visible_page().as_ref() != Some(&page) {
            imp.content_nav.push(&page);
        }
    }

    /// Debug builds: opens "All Sensors" on the group of `key`, as a large
    /// number of the Overview would.
    #[cfg(debug_assertions)]
    pub fn open_sensors(&self, key: &str) {
        self.show_sensors(key);
    }

    /// Starts the reader of the System page, which reads when the page is
    /// shown.
    pub fn start_system(&self, setup: system::Setup) {
        let imp = self.imp();
        if let Some(copy) = imp.copy_action.get() {
            imp.system.start(setup, copy);
        }
        self.update_reading();
    }

    /// Debug builds: the page shown and the subpage over it, if any.
    #[cfg(debug_assertions)]
    pub fn shown(&self) -> (Option<gtk::Widget>, Option<adw::NavigationPage>) {
        let imp = self.imp();
        let subpage = imp
            .content_nav
            .visible_page()
            .filter(|page| page != &*imp.root_page);
        (imp.pages.visible_child(), subpage)
    }

    /// Debug builds: back to the page from its subpages.
    #[cfg(debug_assertions)]
    pub fn close_subpages(&self) {
        self.imp().content_nav.pop_to_tag("root");
    }

    /// Shows the page instead of the sidebar when the window is narrow.
    #[cfg(debug_assertions)]
    pub fn show_content(&self) {
        self.imp().split_view.set_show_content(true);
    }

    /// Chooses a page in the sidebar.
    #[cfg(debug_assertions)]
    pub fn select_page(&self, page: Page) {
        self.imp().sidebar.set_selected(page.index());
    }

    /// Scrolls the page shown to its end, or the page of an open dialog.
    #[cfg(debug_assertions)]
    pub fn scroll_page_to_end(&self) {
        if let Some(dialog) = self.visible_dialog() {
            let page = dialog
                .downcast_ref::<adw::PreferencesDialog>()
                .and_then(|d| d.visible_page());
            if let Some(scrolled) = page.as_ref().and_then(find_scrolled) {
                let adjustment = scrolled.vadjustment();
                adjustment.set_value(adjustment.upper() - adjustment.page_size());
            }
            return;
        }
        match self.page() {
            Page::Overview => self.imp().overview.scroll_to_end(),
            Page::Fans => self.imp().fans.scroll_to_end(),
            Page::System => self.imp().system.scroll_to_end(),
        }
    }

    fn page(&self) -> Page {
        Page::from_index(self.imp().sidebar.selected())
    }

    /// The window is visible and not suspended (minimized, or on another
    /// workspace where the compositor says so).
    fn running(&self) -> bool {
        self.is_visible() && !self.is_suspended()
    }

    /// Shows the page chosen in the sidebar, with its title and buttons.
    /// Entering the System page reads its data again.
    fn show_selected_page(&self) {
        let imp = self.imp();
        let page = self.page();
        // Another page in the sidebar closes a subpage.
        if imp.content_nav.visible_page().as_ref() != Some(&*imp.root_page) {
            imp.content_nav.pop_to_tag("root");
        }
        imp.pages.set_visible_child_name(page.name());
        self.show_selected_page_title();
        imp.copy_button.set_visible(page == Page::System);
        imp.system
            .set_active(page == Page::System && self.running(), true);
        self.update_reading();
    }

    /// The title of the page shown, without a subtitle (D64).
    fn show_selected_page_title(&self) {
        let imp = self.imp();
        let title = self.page().title();
        imp.content_page.set_title(&title);
        imp.root_page.set_title(&title);
        imp.content_title.set_title(&title);
    }

    /// A toast when files were skipped at start, with a button to the
    /// System page, which names them.
    pub fn tell_skipped(&self, count: usize) {
        if count == 0 {
            return;
        }
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        let text = ngettext(
            "{} file skipped because of errors",
            "{} files skipped because of errors",
            count,
        )
        .replace("{}", &count.to_string());
        let toast = adw::Toast::builder()
            .title(text)
            .button_label(gettext("Details"))
            .action_name("win.show-system")
            .timeout(10)
            .build();
        self.imp().toasts.add_toast(toast);
    }

    /// The file with the user's own names for the board, shown in the
    /// Preferences dialog.
    pub fn set_labels_file(&self, file: Option<PathBuf>) {
        self.imp().labels_file.replace(file);
    }

    /// Opens the Preferences dialog with the latest readings.
    pub fn show_preferences(&self) -> adw::PreferencesDialog {
        let imp = self.imp();
        let groups = imp.overview.groups();
        let dialog = preferences::dialog(&DialogSetup {
            settings: self.settings(),
            groups: &groups,
            numbers: imp.overview.numbers(),
            labels_file: imp.labels_file.borrow().clone(),
        });
        dialog.present(Some(self));
        dialog
    }

    /// What the page shown needs of the fan channels: the modes on the
    /// Overview, the channels on the Fans page and its subpages (§5.5.3).
    fn fan_reads(&self) -> FanReads {
        match self.page() {
            Page::Overview => FanReads::Modes,
            Page::Fans => FanReads::Channels,
            Page::System => FanReads::Nothing,
        }
    }

    /// Reads while the window is visible and not suspended: the sensors
    /// always, of the fan channels what the page shows, the system
    /// information on its page only.
    fn update_reading(&self) {
        let running = self.running();
        let reads = self.fan_reads();
        glib::g_debug!(
            "baseboard",
            "readings {}, fan channels: {reads:?}",
            if running { "running" } else { "paused" },
        );
        let imp = self.imp();
        if let Some(worker) = imp.worker.borrow().as_ref() {
            // Each change gives a sample at once, so only changes go; and
            // before `Run`, so that the first sample reads what the page
            // shows.
            if imp.fan_reads.replace(Some(reads)) != Some(reads) {
                worker.set_reads(reads);
            }
            worker.set_running(running);
        }
        imp.system
            .set_active(running && self.page() == Page::System, false);
    }

    /// Puts the text of "Copy system information" on the clipboard and says
    /// so.
    fn copy_system_info(&self) {
        let imp = self.imp();
        let Some(text) = imp.system.report() else {
            return;
        };
        self.clipboard().set_text(&text);
        glib::g_debug!("baseboard", "system information copied:\n{text}");
        imp.toasts
            .add_toast(adw::Toast::new(&gettext("System information copied")));
    }

    /// Shows the detected board in the sidebar footer: its name alone
    /// (D64). The board code, the PCB revision and the profile are on the
    /// System page, which also says when the revision is not verified.
    pub fn show_board(&self, dmi: &Dmi, detection: &Detection) {
        let imp = self.imp();
        let model = match detection.profile() {
            Some(profile) => profile.model.clone(),
            None => dmi
                .board_name
                .clone()
                .or_else(|| dmi.product_name.clone())
                .unwrap_or_else(|| gettext("Unknown board")),
        };
        // The Fans page needs a profile; without one it says why, as the
        // System page does.
        let no_profile = match detection {
            Detection::OtherModel { code, .. } => system::other_model(code),
            _ => system::no_profile(),
        };
        imp.fans.set_board(detection.profile().cloned(), no_profile);
        imp.board_model.set_label(&model);
        imp.board_model.set_tooltip_text(Some(&model));
    }

    fn settings(&self) -> &gio::Settings {
        self.imp()
            .settings
            .get_or_init(|| gio::Settings::new(config::APP_ID))
    }

    fn load_window_state(&self) {
        let settings = self.settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("window-maximized") {
            self.maximize();
        }
    }

    fn save_window_state(&self) {
        let settings = self.settings();
        // The size before maximizing, so that unmaximizing restores it.
        let (width, height) = self.default_size();
        let results = [
            settings.set_int("window-width", width),
            settings.set_int("window-height", height),
            settings.set_boolean("window-maximized", self.is_maximized()),
        ];
        if let Some(Err(e)) = results.into_iter().find(Result::is_err) {
            eprintln!("baseboard: cannot save the window size: {e}");
        }
    }
}

/// Debug builds: the first scrolled window in `widget`, depth first.
#[cfg(debug_assertions)]
fn find_scrolled(widget: &impl IsA<gtk::Widget>) -> Option<gtk::ScrolledWindow> {
    let mut child = widget.as_ref().first_child();
    while let Some(current) = child {
        if let Some(scrolled) = current.downcast_ref::<gtk::ScrolledWindow>() {
            return Some(scrolled.clone());
        }
        if let Some(found) = find_scrolled(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}
