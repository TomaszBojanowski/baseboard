// SPDX-License-Identifier: GPL-3.0-or-later

//! Debug builds only: `BASEBOARD_DEBUG_TOUR=1` walks through the window as
//! a user would. On the Overview, Fans and System pages it clicks every
//! large number and every row that can be clicked, and checks that each
//! one opens what its text names; then it opens Preferences with "Visible
//! Readings", About and the keyboard shortcuts, copies the system
//! information and quits. It prints a line for each check and "baseboard:
//! tour done" when all of them passed. `tests/tour.sh` runs it on an
//! invisible X server and also fails on any warning of GTK or GLib.

use std::cell::Cell;
use std::time::Duration;

use adw::prelude::*;
use baseboard_core::overview::GroupKind;
use gettextrs::{gettext, pgettext};
use gtk::glib;

use crate::fans::FansPage;
use crate::overview::{OverviewPage, SensorsView};
use crate::window::{BaseboardWindow, Page};

/// What a large number or a row of the Overview opens.
#[derive(Debug, Clone, Copy)]
enum Opens {
    /// "All Sensors" on a group, or at its top.
    Sensors(Option<GroupKind>),
    FansPage,
}

/// What the texts of the Overview open, from what they say.
fn overview_expected() -> [(String, Opens); 8] {
    let sensors = |kind| Opens::Sensors(Some(kind));
    [
        (gettext("Processor"), sensors(GroupKind::Cpu)),
        (gettext("Graphics card"), sensors(GroupKind::Gpu)),
        (gettext("Motherboard"), sensors(GroupKind::Board)),
        (gettext("Power"), sensors(GroupKind::Voltages)),
        (gettext("Fans"), Opens::FansPage),
        (
            pgettext("system memory", "Memory"),
            sensors(GroupKind::Memory),
        ),
        (gettext("Disks"), sensors(GroupKind::Disks)),
        (gettext("All sensors"), Opens::Sensors(None)),
    ]
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
        tour.overview().await;
        tour.fans().await;
        tour.system().await;
        tour.dialogs().await;
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

    async fn overview(&self) {
        let clickables = self.open(Page::Overview).await;
        self.check(
            !clickables.is_empty(),
            "the Overview has something to click",
        );
        let expected = overview_expected();
        for widget in clickables {
            let texts = texts(&widget);
            let what = format!("Overview {:?}", texts.first().cloned().unwrap_or_default());
            let Some(&(_, opens)) = expected.iter().find(|(text, _)| texts.contains(text)) else {
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
            self.window.close_subpages();
            self.window.select_page(Page::Overview);
            pause(200).await;
        }
    }

    /// Every clickable row of the Fans page opens the curve page named as
    /// the row. Without curves (the driver of the kernel) there is none.
    async fn fans(&self) {
        let clickables = self.open(Page::Fans).await;
        if clickables.is_empty() {
            eprintln!("baseboard: tour: the Fans page has nothing to click");
        }
        for widget in clickables {
            let texts = texts(&widget);
            let what = format!("Fans {:?}", texts.first().cloned().unwrap_or_default());
            let opened = || self.subpage().is_some_and(|title| texts.contains(&title));
            let Some(ok) = self.click(&widget, &what, opened).await else {
                continue;
            };
            self.check(
                ok,
                &format!("{what} opens its curve page (seen: {})", self.seen()),
            );
            self.window.close_subpages();
            pause(200).await;
        }
    }

    /// The status row opens "Diagnostics", "Technical details" its page;
    /// both are there.
    async fn system(&self) {
        let clickables = self.open(Page::System).await;
        let diagnostics = gettext("Diagnostics");
        let details = gettext("Technical Details");
        let expected = [
            (gettext("Everything works"), &diagnostics),
            (gettext("Sensors work"), &diagnostics),
            (gettext("Needs attention"), &diagnostics),
            (gettext("Technical details"), &details),
        ];
        let mut opened = Vec::new();
        for widget in clickables {
            let texts = texts(&widget);
            let what = format!("System {:?}", texts.first().cloned().unwrap_or_default());
            let Some(&(_, page)) = expected.iter().find(|(text, _)| texts.contains(text)) else {
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
            opened.extend(self.subpage());
            self.window.close_subpages();
            pause(200).await;
        }
        self.check(
            opened.contains(&diagnostics) && opened.contains(&details),
            "the System page opens both of its views",
        );
    }

    async fn dialogs(&self) {
        // As Ctrl+, opens it: nothing but the dialog is kept.
        self.app.activate_action("preferences", None);
        pause(500).await;
        let preferences = self
            .window
            .visible_dialog()
            .and_downcast::<adw::PreferencesDialog>();
        self.check(preferences.is_some(), "app.preferences opens Preferences");
        if let Some(preferences) = preferences {
            let row = crate::preferences::row_titled(&preferences, &gettext("Visible readings"));
            let clicked = row.is_some_and(|row| WidgetExt::activate(&row));
            pause(500).await;
            self.check(
                clicked && preferences.pop_subpage(),
                "Preferences \"Visible readings\" opens its subpage",
            );
            preferences.close();
            pause(300).await;
        }
        for action in ["about", "shortcuts"] {
            self.app.activate_action(action, None);
            pause(500).await;
            let dialog = self.window.visible_dialog();
            self.check(dialog.is_some(), &format!("app.{action} opens a dialog"));
            if let Some(dialog) = dialog {
                dialog.close();
            }
            pause(300).await;
        }
        self.window.select_page(Page::System);
        pause(300).await;
        let copied = WidgetExt::activate_action(&self.window, "win.copy-system-info", None);
        self.check(copied.is_ok(), "the system information can be copied");
        pause(500).await;
    }
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
