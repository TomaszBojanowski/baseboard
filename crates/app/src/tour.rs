// SPDX-License-Identifier: GPL-3.0-or-later

//! Debug builds only: `BASEBOARD_DEBUG_TOUR=1` walks through the window as
//! a user would (docs/PROJEKT.md §8, Z10). It clicks every row of the
//! sidebar; on the Overview, Fans and System pages it clicks every card,
//! tile and row that can be clicked, and checks that each one opens what
//! its text names, that the status sentence says what the tree shows,
//! that nothing expected is missing, and that no text of a page or subpage
//! is cut off; from the main menu it opens
//! Preferences (a switch, "Visible Readings" and a switch there), the
//! keyboard shortcuts and About; it copies the system information, checks
//! the clipboard and quits. It prints a line for each check and
//! "baseboard: tour done" when all of them passed. `tests/tour.sh` runs it
//! on an invisible X server, tells it how many fans of the tree open their
//! curve (`BASEBOARD_TOUR_CURVES`) and whether its processor is close to
//! its limit (`BASEBOARD_TOUR_HOT`), and also fails on any warning of GTK
//! or GLib.

use std::cell::Cell;
use std::time::Duration;

use adw::prelude::*;
use baseboard_core::overview::GroupKind;
use gettextrs::{gettext, pgettext};
use gtk::{gio, glib, pango};

use crate::config;
use crate::fans::FansPage;
use crate::overview::{OverviewPage, SensorsView};
use crate::system::SystemPage;
use crate::window::{BaseboardWindow, Page};

/// What a large number or a row of the Overview opens.
#[derive(Debug, Clone, Copy)]
enum Opens {
    /// "All Sensors" on a group, or at its top.
    Sensors(Option<GroupKind>),
    FansPage,
}

/// What the cards, tiles and rows of the Overview open, from what they
/// say (D67).
fn overview_expected() -> [(String, Opens); 7] {
    let sensors = |kind| Opens::Sensors(Some(kind));
    [
        (gettext("Processor"), sensors(GroupKind::Cpu)),
        (gettext("Graphics card"), sensors(GroupKind::Gpu)),
        (gettext("Motherboard"), sensors(GroupKind::Board)),
        (gettext("Fans"), Opens::FansPage),
        (
            pgettext("system memory", "Memory"),
            sensors(GroupKind::Memory),
        ),
        (gettext("Disks"), sensors(GroupKind::Disks)),
        (gettext("All sensors"), Opens::Sensors(None)),
    ]
}

/// The tree has a processor close to its limit (`tests/tour.sh`).
fn hot() -> bool {
    std::env::var_os("BASEBOARD_TOUR_HOT").is_some_and(|hot| !hot.is_empty())
}

/// The status sentence of the tree: the processor close to its limit, or
/// all fine; it is a row to click only when it warns.
fn status_says(texts: &[String]) -> bool {
    match texts.first() {
        Some(title) if hot() => {
            let words = gettext("Processor is very hot: {value}");
            let start = words.split("{value}").next().unwrap_or_default();
            title.starts_with(start)
        }
        Some(title) => *title == crate::alert::everything_fine(),
        None => false,
    }
}

/// The row of the status sentence on the Overview and the System page.
fn is_status(widget: &gtk::Widget) -> bool {
    widget.has_css_class("status-row")
}

/// Starts the tour when `BASEBOARD_DEBUG_TOUR` is set.
pub fn start(app: &adw::Application, window: &BaseboardWindow) {
    if std::env::var_os("BASEBOARD_DEBUG_TOUR").is_none() {
        return;
    }
    // Pages open at once, without animations to wait for.
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_enable_animations(false);
    }
    let tour = Tour {
        app: app.clone(),
        window: window.clone(),
        failed: Cell::new(0),
    };
    glib::MainContext::default().spawn_local(async move {
        tour.sidebar().await;
        tour.overview().await;
        tour.fans().await;
        tour.system().await;
        tour.menu().await;
        tour.copy().await;
        match tour.failed.get() {
            0 => eprintln!("baseboard: tour done"),
            failed => eprintln!("baseboard: tour failed: {failed} checks"),
        }
        tour.app.quit();
    });
}

struct Tour {
    app: adw::Application,
    window: BaseboardWindow,
    failed: Cell<u32>,
}

impl Tour {
    fn check(&self, ok: bool, what: &str) {
        if ok {
            eprintln!("baseboard: tour: ok: {what}");
        } else {
            self.failed.set(self.failed.get() + 1);
            eprintln!("baseboard: tour: FAILED: {what}");
        }
    }

    /// Shows `page` and waits up to ten seconds for something to click
    /// on it, which comes with the first readings.
    async fn open(&self, page: Page) -> Vec<gtk::Widget> {
        self.window.close_subpages();
        self.window.select_page(page);
        for _ in 0..100 {
            if !self.clickables().is_empty() {
                // The rest of the page comes with the same readings.
                pause(300).await;
                return self.clickables();
            }
            pause(100).await;
        }
        Vec::new()
    }

    fn clickables(&self) -> Vec<gtk::Widget> {
        self.window
            .shown()
            .0
            .map(|page| clickables(&page))
            .unwrap_or_default()
    }

    /// Clicks `widget` and waits up to two seconds for `opened`, asked
    /// every 50 ms: the readings go on and can move a group soon after.
    /// It must hold twice in a row, so that a page left from before does
    /// not count. `None` when the widget cannot be clicked.
    async fn click(
        &self,
        widget: &gtk::Widget,
        what: &str,
        opened: impl Fn() -> bool,
    ) -> Option<bool> {
        if !(widget.is_sensitive() && widget.activate()) {
            self.check(false, &format!("{what} can be clicked"));
            return None;
        }
        let mut before = false;
        for _ in 0..40 {
            pause(50).await;
            let now = opened();
            if before && now {
                return Some(true);
            }
            before = now;
        }
        Some(false)
    }

    /// The title of the subpage shown.
    fn subpage(&self) -> Option<String> {
        self.window.shown().1.map(|page| page.title().to_string())
    }

    /// The page shown and its subpage, in words; for "All Sensors" also
    /// where it is scrolled to.
    fn seen(&self) -> String {
        let (page, subpage) = self.window.shown();
        let name = page.as_ref().map_or("no page", |p| p.type_().name());
        let Some(subpage) = subpage else {
            return name.to_owned();
        };
        let mut seen = format!("{name} > {:?}", subpage.title());
        if let Some(view) = page
            .and_downcast::<OverviewPage>()
            .map(|overview| overview.sensors_view())
        {
            seen += &format!(
                ", scrolled to {:.0} of {:.0}, groups at {:?}",
                view.value, view.max, view.groups
            );
        }
        seen
    }

    fn opened(&self, opens: Opens) -> bool {
        let (page, subpage) = self.window.shown();
        match opens {
            Opens::FansPage => page.is_some_and(|p| p.is::<FansPage>()) && subpage.is_none(),
            Opens::Sensors(group) => {
                subpage.is_some_and(|p| p.title() == gettext("All Sensors"))
                    && page
                        .and_downcast::<OverviewPage>()
                        .is_some_and(|overview| shows(&overview.sensors_view(), group))
            }
        }
    }

    /// Each row of the sidebar shows its page, also in a narrow window,
    /// where the sidebar and the page take turns.
    async fn sidebar(&self) {
        let pages = [Page::Overview, Page::Fans, Page::System];
        for (index, page) in pages.into_iter().enumerate() {
            self.window.close_subpages();
            self.window.show_sidebar();
            pause(200).await;
            let rows: Vec<gtk::Widget> = find::<adw::Sidebar>(self.window.upcast_ref())
                .map(|sidebar| clickables(sidebar.upcast_ref()))
                .unwrap_or_default();
            self.check(rows.len() == pages.len(), "the sidebar has a row per page");
            let Some(row) = rows.get(index) else {
                continue;
            };
            let what = format!(
                "sidebar {:?}",
                texts(row).first().cloned().unwrap_or_default()
            );
            let shown = || self.window.shown().0.is_some_and(|w| is_page(&w, page));
            if let Some(ok) = self.click(row, &what, shown).await {
                self.check(ok, &format!("{what} shows {page:?}"));
            }
        }
    }

    /// Every text of the window shows whole, in `place`.
    fn whole_texts(&self, place: &str) {
        let cut = cut_texts(self.window.upcast_ref());
        self.check(
            cut.is_empty(),
            &format!("no text is cut on {place}: {cut:?}"),
        );
    }

    /// The row of the status sentence of the page shown.
    fn status_row(&self) -> Option<gtk::Widget> {
        let page = self.window.shown().0?;
        find_all::<adw::ActionRow>(&page)
            .into_iter()
            .map(|row| row.upcast::<gtk::Widget>())
            .find(|row| row.is_mapped() && is_status(row))
    }

    async fn overview(&self) {
        let mut clickables = self.open(Page::Overview).await;
        // The processor warns after three readings close to its limit.
        for _ in 0..100 {
            if !hot() || clickables.iter().any(is_status) {
                break;
            }
            pause(100).await;
            clickables = self.clickables();
        }
        let status = self.status_row().map(|row| texts(&row));
        self.check(
            status.as_deref().is_some_and(status_says),
            &format!("the Overview says what the tree shows: {status:?}"),
        );
        self.whole_texts("the Overview");
        let expected = overview_expected();
        let mut found = Vec::new();
        let mut warned = false;
        for widget in clickables {
            let texts = texts(&widget);
            let what = format!("Overview {:?}", texts.first().cloned().unwrap_or_default());
            let opens = if is_status(&widget) {
                // Only a warning is a row to click: it opens the group of
                // the processor.
                warned = true;
                Opens::Sensors(Some(GroupKind::Cpu))
            } else if let Some((text, opens)) =
                expected.iter().find(|(text, _)| texts.contains(text))
            {
                found.push(text.clone());
                *opens
            } else {
                self.check(false, &format!("{what}: nothing expected of {texts:?}"));
                continue;
            };
            let Some(ok) = self.click(&widget, &what, || self.opened(opens)).await else {
                continue;
            };
            self.check(
                ok,
                &format!("{what} opens {opens:?} (seen: {})", self.seen()),
            );
            self.whole_texts(&self.seen());
            self.window.close_subpages();
            self.window.select_page(Page::Overview);
            pause(200).await;
        }
        self.check(
            warned == hot(),
            "the status sentence is a row to click when it warns, and only then",
        );
        // Nothing expected is missing.
        for (text, _) in &expected {
            self.check(
                found.contains(text),
                &format!("the Overview shows {text:?}"),
            );
        }
    }

    /// Every clickable row of the Fans page opens the curve page named as
    /// the row; as many rows as the tree has fans with curves
    /// (`BASEBOARD_TOUR_CURVES`; none with the driver of the kernel).
    async fn fans(&self) {
        let clickables = self.open(Page::Fans).await;
        self.whole_texts("the Fans page");
        let mut opened = 0;
        for widget in clickables {
            let texts = texts(&widget);
            let what = format!("Fans {:?}", texts.first().cloned().unwrap_or_default());
            let shown = || self.subpage().is_some_and(|title| texts.contains(&title));
            let Some(ok) = self.click(&widget, &what, shown).await else {
                continue;
            };
            self.check(
                ok,
                &format!("{what} opens its curve page (seen: {})", self.seen()),
            );
            self.whole_texts(&self.seen());
            opened += usize::from(ok);
            self.window.close_subpages();
            pause(200).await;
        }
        let expected = std::env::var("BASEBOARD_TOUR_CURVES")
            .ok()
            .and_then(|count| count.parse::<usize>().ok());
        self.check(
            expected == Some(opened),
            &format!("{opened} fans open their curve, {expected:?} expected"),
        );
    }

    /// The status row says what the Overview says (a system that only
    /// reads the fans: that the readings work) and opens "Diagnostics",
    /// "Technical details" its page; both are there.
    async fn system(&self) {
        let clickables = self.open(Page::System).await;
        let diagnostics = gettext("Diagnostics");
        let details = gettext("Technical Details");
        let status = self.status_row().map(|row| texts(&row));
        let says = status.as_deref().is_some_and(|texts| {
            status_says(texts) || (!hot() && texts.first() == Some(&crate::alert::readings_work()))
        });
        self.check(
            says,
            &format!("the System page says what the tree shows: {status:?}"),
        );
        self.whole_texts("the System page");
        let mut opened = Vec::new();
        for widget in clickables {
            let texts = texts(&widget);
            let what = format!("System {:?}", texts.first().cloned().unwrap_or_default());
            let page = if is_status(&widget) {
                &diagnostics
            } else if texts.contains(&gettext("Technical details")) {
                &details
            } else {
                self.check(false, &format!("{what}: nothing expected of {texts:?}"));
                continue;
            };
            let shown = || self.subpage().as_ref() == Some(page);
            let Some(ok) = self.click(&widget, &what, shown).await else {
                continue;
            };
            self.check(
                ok,
                &format!("{what} opens {page:?} (seen: {})", self.seen()),
            );
            self.whole_texts(&self.seen());
            opened.extend(self.subpage());
            self.window.close_subpages();
            pause(200).await;
        }
        self.check(
            opened.contains(&diagnostics) && opened.contains(&details),
            "the System page opens both of its views",
        );
    }

    /// The items of the main menu: each one's action opens its dialog, as
    /// a click on the item does. (A popover of the menu on the invisible X
    /// server warns of its frames, so the tour reads the menu's model.)
    async fn menu(&self) {
        let actions = ["app.preferences", "app.shortcuts", "app.about"];
        let model = find::<gtk::MenuButton>(self.window.upcast_ref())
            .and_then(|button| button.menu_model());
        let items = model.map(|model| menu_actions(&model)).unwrap_or_default();
        self.check(
            items == actions,
            &format!("the main menu has {actions:?}: {items:?}"),
        );
        for action in items {
            let Some(name) = action.strip_prefix("app.") else {
                continue;
            };
            self.app.activate_action(name, None);
            pause(500).await;
            let dialog = self.window.visible_dialog();
            self.check(
                dialog.is_some(),
                &format!("main menu {action} opens a dialog"),
            );
            if let Some(preferences) = dialog.clone().and_downcast::<adw::PreferencesDialog>() {
                self.preferences(&preferences).await;
            }
            if let Some(dialog) = dialog {
                dialog.close();
            }
            pause(300).await;
        }
    }

    /// The switch "Names from lm-sensors" changes its setting; "Visible
    /// readings" opens its subpage, where a switch hides a reading. Both
    /// switches are put back. "Custom names" opens an outside app, so it is
    /// only looked for.
    async fn preferences(&self, preferences: &adw::PreferencesDialog) {
        use crate::preferences::{HIDDEN, SENSORS_LABELS, row_titled};

        let settings = gio::Settings::new(config::APP_ID);
        let row = |title: &str| row_titled(preferences, &gettext(title));
        let click = |title: &str| row(title).is_some_and(|row| WidgetExt::activate(&row));
        self.whole_texts("Preferences");

        let names = settings.boolean(SENSORS_LABELS);
        let clicked = click("Names from lm-sensors");
        pause(300).await;
        let changed = settings.boolean(SENSORS_LABELS) != names;
        let clicked = clicked && click("Names from lm-sensors");
        pause(300).await;
        self.check(
            clicked && changed && settings.boolean(SENSORS_LABELS) == names,
            "Preferences \"Names from lm-sensors\" changes its setting and back",
        );
        self.check(
            row("Custom names").is_some(),
            "Preferences has \"Custom names\"",
        );

        let clicked = click("Visible readings");
        pause(500).await;
        // The rows of the readings, each with its switch.
        let rows: Vec<gtk::Widget> = find_all::<gtk::Switch>(preferences.upcast_ref())
            .into_iter()
            .filter(|switch| switch.is_mapped())
            .filter_map(|switch| switch.ancestor(adw::ActionRow::static_type()))
            .collect();
        self.check(
            clicked && !rows.is_empty(),
            "Preferences \"Visible readings\" opens its subpage with switches",
        );
        self.whole_texts("Visible readings");
        if let Some(row) = rows.first() {
            let hidden = || settings.strv(HIDDEN).len();
            let before = hidden();
            row.activate();
            pause(300).await;
            let changed = hidden() != before;
            row.activate();
            pause(300).await;
            self.check(
                changed && hidden() == before,
                "a switch of \"Visible readings\" hides its reading and shows it again",
            );
        }
        preferences.pop_subpage();
        pause(300).await;
    }

    /// "Copy system information" puts the text of a report on the
    /// clipboard.
    async fn copy(&self) {
        self.window.select_page(Page::System);
        pause(300).await;
        let copied = WidgetExt::activate_action(&self.window, "win.copy-system-info", None);
        pause(500).await;
        let text = self.window.clipboard().read_text_future().await;
        let text = text.ok().flatten().unwrap_or_default();
        self.check(
            copied.is_ok() && text.starts_with("Baseboard ") && text.contains("\nBoard\n"),
            "the system information is copied to the clipboard",
        );
    }
}

/// The page of the sidebar `widget` is.
fn is_page(widget: &gtk::Widget, page: Page) -> bool {
    match page {
        Page::Overview => widget.is::<OverviewPage>(),
        Page::Fans => widget.is::<FansPage>(),
        Page::System => widget.is::<SystemPage>(),
    }
}

/// The actions of the items of `menu` and of its sections, in order.
fn menu_actions(menu: &gio::MenuModel) -> Vec<String> {
    let mut actions = Vec::new();
    for item in 0..menu.n_items() {
        let action = menu.item_attribute_value(item, "action", Some(glib::VariantTy::STRING));
        actions.extend(action.and_then(|value| value.get::<String>()));
        for link in [gio::MENU_LINK_SECTION, gio::MENU_LINK_SUBMENU] {
            if let Some(inner) = menu.item_link(item, link) {
                actions.extend(menu_actions(&inner));
            }
        }
    }
    actions
}

/// The first widget of type `W` under `root`, depth first.
fn find<W: IsA<gtk::Widget>>(root: &gtk::Widget) -> Option<W> {
    find_all(root).into_iter().next()
}

/// Every widget of type `W` under `root`, depth first.
fn find_all<W: IsA<gtk::Widget>>(root: &gtk::Widget) -> Vec<W> {
    let mut found = Vec::new();
    let mut child = root.first_child();
    while let Some(widget) = child {
        if let Some(wanted) = widget.downcast_ref::<W>() {
            found.push(wanted.clone());
        }
        found.extend(find_all::<W>(&widget));
        child = widget.next_sibling();
    }
    found
}

async fn pause(milliseconds: u64) {
    glib::timeout_future(Duration::from_millis(milliseconds)).await;
}

/// The widgets on screen under `root` that can be clicked: those with an
/// action and the rows that can be activated. A banner that is not shown
/// has no height.
fn clickables(root: &gtk::Widget) -> Vec<gtk::Widget> {
    let mut found = Vec::new();
    let mut child = root.first_child();
    while let Some(widget) = child {
        if widget.is_mapped() && widget.width() > 0 && widget.height() > 0 {
            let action = widget
                .dynamic_cast_ref::<gtk::Actionable>()
                .and_then(|actionable| actionable.action_name());
            let row = widget
                .downcast_ref::<gtk::ListBoxRow>()
                .is_some_and(|row| row.is_activatable());
            if action.is_some() || row {
                found.push(widget.clone());
            }
            found.extend(clickables(&widget));
        }
        child = widget.next_sibling();
    }
    found
}

/// The texts on screen under `root` that do not show whole: a label cut
/// with an ellipsis, or an inscription whose text needs more lines than it
/// has room for (D67: no text ends in an ellipsis). A few pixels of the
/// metrics of the font are no missing line.
fn cut_texts(root: &gtk::Widget) -> Vec<String> {
    let mut cut = Vec::new();
    let mut child = root.first_child();
    while let Some(widget) = child {
        if widget.is_mapped() && widget.width() > 0 {
            if let Some(label) = widget.downcast_ref::<gtk::Label>()
                && label.layout().is_ellipsized()
            {
                cut.push(label.label().to_string());
            }
            if let Some(line) = widget.downcast_ref::<gtk::Inscription>()
                && let Some(text) = line.text()
            {
                let layout = line.create_pango_layout(Some(&text));
                layout.set_width(line.width() * pango::SCALE);
                layout.set_wrap(pango::WrapMode::Word);
                if layout.pixel_size().1 > line.height() + 4 {
                    cut.push(text.to_string());
                }
            }
            cut.extend(cut_texts(&widget));
        }
        child = widget.next_sibling();
    }
    cut
}

/// The texts of the labels in `widget`, in order.
fn texts(widget: &gtk::Widget) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(label) = widget.downcast_ref::<gtk::Label>()
        && !label.label().is_empty()
    {
        found.push(label.label().to_string());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(texts(&current));
        child = current.next_sibling();
    }
    found
}

/// "All Sensors" shows `group` on top, or as far down as it scrolls with
/// the group in view; `None`: the top of the page.
fn shows(view: &SensorsView, group: Option<GroupKind>) -> bool {
    let Some(kind) = group else {
        return view.value < 1.0;
    };
    view.groups.iter().any(|&(other, top)| {
        other == kind
            && ((top - view.value).abs() < 1.0
                || (view.value >= view.max - 1.0
                    && top >= view.value - 1.0
                    && top < view.value + view.height))
    })
}
