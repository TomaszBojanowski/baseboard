// SPDX-License-Identifier: GPL-3.0-or-later

//! The main window: a sidebar with the pages and the page shown next to it.

use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::board::{Detection, Dmi, code_label};
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use crate::config;
use crate::overview::OverviewPage;
use crate::preferences::{self, DialogSetup};
use crate::sampler::Setup;
use crate::system::{self, SystemPage};

/// The pages, in the order of the sidebar items in window.ui.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Overview,
    System,
}

impl Page {
    const ALL: [Self; 2] = [Self::Overview, Self::System];

    fn index(self) -> u32 {
        match self {
            Self::Overview => 0,
            Self::System => 1,
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
            Self::System => "system",
        }
    }

    fn title(self) -> String {
        match self {
            Self::Overview => gettext("Overview"),
            Self::System => gettext("System"),
        }
    }
}

mod imp {
    use std::cell::{OnceCell, RefCell};

    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/me/bojanowski/Baseboard/window.ui")]
    pub struct BaseboardWindow {
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub sidebar: TemplateChild<adw::Sidebar>,
        #[template_child]
        pub board_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub board_model: TemplateChild<gtk::Label>,
        #[template_child]
        pub board_detail: TemplateChild<gtk::Label>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
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
        pub system: TemplateChild<SystemPage>,
        pub settings: OnceCell<gio::Settings>,
        pub copy_action: OnceCell<gio::SimpleAction>,
        /// The file with the user's own names for the board (D22).
        pub labels_file: RefCell<Option<PathBuf>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BaseboardWindow {
        const NAME: &'static str = "BaseboardWindow";
        type Type = super::BaseboardWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            OverviewPage::ensure_type();
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
                            obj.imp()
                                .overview
                                .set_options(preferences::options(settings));
                            obj.show_selected_page_title();
                        }
                        preferences::HIDDEN | preferences::SHOWN => {
                            obj.imp()
                                .overview
                                .set_visibility(preferences::visibility(settings));
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

    /// Starts reading the sensors for the Overview page.
    pub fn start_overview(&self, mut setup: Setup) {
        let settings = self.settings();
        setup.options = preferences::options(settings);
        let overview = &self.imp().overview;
        overview.set_visibility(preferences::visibility(settings));
        overview.start(setup);
        self.update_reading();
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
        imp.pages.set_visible_child_name(page.name());
        self.show_selected_page_title();
        imp.copy_button.set_visible(page == Page::System);
        imp.system
            .set_active(page == Page::System && self.running(), true);
    }

    /// The title of the page shown, and on the Overview the refresh
    /// interval from the settings.
    fn show_selected_page_title(&self) {
        let imp = self.imp();
        let page = self.page();
        let title = page.title();
        imp.content_page.set_title(&title);
        imp.content_title.set_title(&title);
        let subtitle = match page {
            Page::Overview => refresh_subtitle(preferences::interval_seconds(self.settings())),
            Page::System => String::new(),
        };
        imp.content_title.set_subtitle(&subtitle);
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

    /// Reads while the window is visible and not suspended: the sensors
    /// always, the system information on its page only.
    fn update_reading(&self) {
        let running = self.running();
        glib::g_debug!(
            "baseboard",
            "readings {}",
            if running { "running" } else { "paused" }
        );
        let imp = self.imp();
        imp.overview.set_running(running);
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

    /// Shows the detected board in the sidebar footer: the model, and the
    /// board code, a warning about the PCB revision, a note about another
    /// model with the same code, or the lack of a profile.
    pub fn show_board(&self, dmi: &Dmi, detection: &Detection) {
        let imp = self.imp();
        let dmi_model = || {
            dmi.board_name
                .clone()
                .or_else(|| dmi.product_name.clone())
                .unwrap_or_else(|| gettext("Unknown board"))
        };
        let (model, detail, warning) = match detection {
            Detection::Supported(profile) => (
                profile.model.clone(),
                code_label(&profile.board_match.board_code),
                false,
            ),
            Detection::UnverifiedRevision { profile, revision } => (
                profile.model.clone(),
                system::unverified_revision(*revision),
                true,
            ),
            Detection::OtherModel { code, .. } => (dmi_model(), system::other_model(code), false),
            Detection::Generic => (dmi_model(), system::no_profile(), false),
        };
        imp.board_model.set_label(&model);
        imp.board_model.set_tooltip_text(Some(&model));
        imp.board_detail.set_label(&detail);
        if warning {
            imp.board_icon
                .set_icon_name(Some("dialog-warning-symbolic"));
            imp.board_icon.remove_css_class("dimmed");
            imp.board_icon.add_css_class("warning");
        }
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

/// The subtitle of the Overview page.
fn refresh_subtitle(seconds: u32) -> String {
    // Translators: the subtitle of the Overview page; {} is a whole
    // number of seconds.
    ngettext(
        "Refreshing every {} second",
        "Refreshing every {} seconds",
        seconds,
    )
    .replace("{}", &seconds.to_string())
}
