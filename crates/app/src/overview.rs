// SPDX-License-Identifier: GPL-3.0-or-later

//! The Overview page (docs/PROJEKT.md §5.1, D67): the status sentence, a
//! card for each part of the computer, tiles and the row "All sensors"
//! (`summary`); every reading on the page "All Sensors", which the window
//! pushes over it, in one or two columns by `layout`: the groups and rows of
//! the model from `core::readings`, with their names, subtitles and ratings
//! from `texts`. A worker thread (`sampler`, started by the window) takes
//! the samples; the page only shows them. The widgets are made again only
//! when the rows change, otherwise their texts are set.

mod summary;
mod texts;

use std::cell::{Cell, OnceCell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::heat::{Hot, Limits};
use baseboard_core::history::Histories;
use baseboard_core::readings::{self, Group, GroupKind, Rating, Row, RowId, Visibility};
use baseboard_core::sensors::Kind;
use baseboard_core::summary::{self as top, Summary};
use baseboard_core::units::Numbers;
use gettextrs::gettext;
use gtk::{glib, graphene};

use crate::layout;
use crate::sampler::{Sample, Status};
use summary::SummaryWidgets;
pub use summary::{card_view, hero_name};
use texts::{device_description, short_name, subtitle};
pub use texts::{group_title, name, nominal};

/// What decides whether the widgets have to be made again.
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    kind: GroupKind,
    description: Option<String>,
    rows: Vec<RowId>,
    auxiliary: Vec<RowId>,
}

impl Shape {
    fn of(group: &Group) -> Self {
        Self {
            kind: group.kind,
            description: group.description.clone(),
            rows: group.rows.iter().map(|r| r.id.clone()).collect(),
            auxiliary: group.auxiliary.iter().map(|r| r.id.clone()).collect(),
        }
    }
}

/// The widgets of one row.
/// Values and ranges are inscriptions, not labels: their size does not
/// depend on their text, so a new value every second redraws them without
/// laying out the whole window again.
pub(crate) struct RowWidgets {
    pub(crate) row: adw::ActionRow,
    value: gtk::Inscription,
    range: gtk::Inscription,
    icon: Option<gtk::Image>,
    /// What the icon shows, to set it only when it changes: within the
    /// tolerance or not, or `None` without a rating.
    shown_rating: Cell<Option<bool>>,
}

impl RowWidgets {
    pub(crate) fn new(rated: bool) -> Self {
        let row = adw::ActionRow::builder().use_markup(false).build();
        // Wide enough for the widest range, e.g. "−12,10–−11,90 V" or
        // "999,9–1000,0 W", so that a narrow window wraps the name rather
        // than cutting the range.
        let range = gtk::Inscription::builder()
            .min_chars(15)
            .nat_chars(15)
            .xalign(1.0)
            .valign(gtk::Align::Center)
            .text_overflow(gtk::InscriptionOverflow::Clip)
            .css_classes(["dimmed", "numeric"])
            .build();
        // Translators: the tooltip of the gray range next to a reading on
        // the page "All Sensors".
        let tooltip = gettext("Lowest and highest value since the app started");
        range.set_tooltip_text(Some(&tooltip));
        let value = gtk::Inscription::builder()
            .min_chars(8)
            .nat_chars(9)
            .xalign(1.0)
            .valign(gtk::Align::Center)
            .css_classes(["numeric"])
            .build();
        row.add_suffix(&range);
        let icon = rated.then(|| {
            let icon = gtk::Image::new();
            set_rating_icon(&icon, None);
            row.add_suffix(&icon);
            icon
        });
        row.add_suffix(&value);
        Self {
            row,
            value,
            range,
            icon,
            shown_rating: Cell::new(None),
        }
    }

    pub(crate) fn update(&self, data: &Row, numbers: &Numbers) {
        let nominal = nominal(data);
        let title = name(&data.name, numbers);
        let subtitle = subtitle(data, numbers);
        let value = numbers.value_of(data.kind, data.value, nominal);
        let range = data
            .range
            .map(|(lo, hi)| numbers.range_of(data.kind, lo, hi, nominal))
            .unwrap_or_default();
        if self.row.title() != title {
            self.row.set_title(&title);
        }
        let same_subtitle = self.row.subtitle().as_deref() == Some(subtitle.as_str());
        let same_value = self.value.text().as_deref() == Some(value.as_str());
        let same_range = self.range.text().as_deref() == Some(range.as_str());
        if same_subtitle && same_value && same_range {
            return;
        }
        if !same_subtitle {
            self.row.set_subtitle(&subtitle);
        }
        if !same_value {
            self.value.set_text(Some(&value));
        }
        if !same_range {
            self.range.set_text(Some(&range));
            // No range while the values have not parted: the subtitle
            // gets the room.
            self.range.set_visible(!range.is_empty());
        }
        // A fan at standstill, or an empty header: the value is dimmed.
        let still = data.kind == Kind::Fan && data.value == 0.0;
        if still != self.value.has_css_class("dimmed") {
            if still {
                self.value.add_css_class("dimmed");
            } else {
                self.value.remove_css_class("dimmed");
            }
        }
        if let Some(icon) = &self.icon {
            let within = match data.rating {
                Some(Rating::Rated { within, .. }) => Some(within),
                _ => None,
            };
            if self.shown_rating.replace(within) != within {
                set_rating_icon(icon, data.rating);
            }
        }
        let description = if range.is_empty() {
            // Translators: read by screen readers after the name of a
            // reading, e.g. "nct6687 · temp2. Now 37.5 °C".
            gettext("{subtitle}. Now {value}")
        } else {
            // Translators: read by screen readers after the name of a
            // reading, e.g. "nct6687 · temp2. Now 37.5 °C, 36.0–38.0 °C
            // since the app started".
            gettext("{subtitle}. Now {value}, {range} since the app started")
        }
        .replace("{subtitle}", &subtitle)
        .replace("{value}", &value)
        .replace("{range}", &range);
        self.row
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Description(&description)]);
    }
}

/// The problems of the status sentence (D67) in words, the most important
/// first, for the status row of the System page.
fn alerts(state: &top::State, summary: &Summary, numbers: &Numbers) -> Vec<String> {
    let top::State::Problems(problems) = state else {
        return Vec::new();
    };
    problems
        .iter()
        .map(|problem| crate::alert::problem(problem, summary.fans_now(), numbers).0)
        .collect()
}

/// The rating as an icon next to the value, never by color alone: the
/// text is in the subtitle. Unrated rails keep the space empty, so that
/// the values stay aligned.
fn set_rating_icon(icon: &gtk::Image, rating: Option<Rating>) {
    match rating {
        Some(Rating::Rated { within: true, .. }) => {
            icon.set_icon_name(Some("baseboard-ok-symbolic"));
            icon.set_css_classes(&["success"]);
            icon.set_opacity(1.0);
            icon.set_tooltip_text(Some(&gettext("Within tolerance")));
            icon.update_state(&[gtk::accessible::State::Hidden(false)]);
        }
        Some(Rating::Rated { within: false, .. }) => {
            icon.set_icon_name(Some("dialog-warning-symbolic"));
            icon.set_css_classes(&["warning"]);
            icon.set_opacity(1.0);
            icon.set_tooltip_text(Some(&gettext("Outside tolerance")));
            icon.update_state(&[gtk::accessible::State::Hidden(false)]);
        }
        _ => {
            icon.set_icon_name(Some("baseboard-ok-symbolic"));
            icon.set_opacity(0.0);
            icon.set_tooltip_text(None);
            icon.update_state(&[gtk::accessible::State::Hidden(true)]);
        }
    }
}

/// The folded row of the board's auxiliary voltages.
struct Auxiliary {
    expander: adw::ExpanderRow,
    rows: Vec<RowWidgets>,
}

impl Auxiliary {
    fn new(count: usize) -> Self {
        let expander = adw::ExpanderRow::builder()
            .use_markup(false)
            .title(gettext("Auxiliary voltages"))
            .build();
        let rows = (0..count)
            .map(|_| {
                let row = RowWidgets::new(false);
                expander.add_row(&row.row);
                row
            })
            .collect();
        Self { expander, rows }
    }

    fn update(&self, data: &[Row], numbers: &Numbers) {
        let summary = data
            .iter()
            .map(|r| {
                format!(
                    "{} {}",
                    short_name(&r.name, numbers),
                    numbers.value_of(r.kind, r.value, nominal(r))
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        if self.expander.subtitle() != summary {
            self.expander.set_subtitle(&summary);
        }
        for (widgets, row) in self.rows.iter().zip(data) {
            widgets.update(row, numbers);
        }
    }
}

/// The widgets of one group.
struct GroupWidgets {
    kind: GroupKind,
    widget: adw::PreferencesGroup,
    rows: Vec<RowWidgets>,
    auxiliary: Option<Auxiliary>,
}

impl GroupWidgets {
    fn new(group: &Group) -> Self {
        let description = match group.kind {
            GroupKind::Fans => Some(gettext("In the order of the headers on the board")),
            GroupKind::Voltages => {
                Some(gettext("Rated against the tolerances in the board profile"))
            }
            _ => device_description(group),
        };
        let widget = adw::PreferencesGroup::builder()
            .title(glib::markup_escape_text(&group_title(group.kind)))
            .build();
        if let Some(description) = description {
            widget.set_description(Some(&glib::markup_escape_text(&description)));
        }
        let rows: Vec<RowWidgets> = group
            .rows
            .iter()
            .map(|row| {
                let widgets = RowWidgets::new(row.rating.is_some());
                widget.add(&widgets.row);
                widgets
            })
            .collect();
        let auxiliary = (!group.auxiliary.is_empty()).then(|| {
            let auxiliary = Auxiliary::new(group.auxiliary.len());
            widget.add(&auxiliary.expander);
            auxiliary
        });
        Self {
            kind: group.kind,
            widget,
            rows,
            auxiliary,
        }
    }

    fn update(&self, group: &Group, numbers: &Numbers) {
        for (widgets, row) in self.rows.iter().zip(&group.rows) {
            widgets.update(row, numbers);
        }
        if let Some(auxiliary) = &self.auxiliary {
            auxiliary.update(&group.auxiliary, numbers);
        }
    }

    /// In two columns: the right one has these, the left one the rest
    /// (memory goes left, to even out the columns; makiety/OPIS.md).
    fn goes_right(&self) -> bool {
        matches!(
            self.kind,
            GroupKind::Voltages | GroupKind::Gpu | GroupKind::Disks | GroupKind::Other
        )
    }
}

type StatusCallback = Box<dyn Fn(&Status)>;

#[derive(Default)]
struct State {
    shape: Vec<Shape>,
    groups: Vec<GroupWidgets>,
    wide: bool,
}

/// Debug builds: the view of "All Sensors", in pixels from the top of its
/// content.
#[cfg(debug_assertions)]
pub struct SensorsView {
    /// Where the view starts.
    pub value: f64,
    pub height: f64,
    /// The farthest it scrolls.
    pub max: f64,
    /// Where each group starts.
    pub groups: Vec<(GroupKind, f64)>,
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/me/bojanowski/Baseboard/overview.ui")]
    pub struct OverviewPage {
        #[template_child]
        pub bin: TemplateChild<adw::BreakpointBin>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub clamp: TemplateChild<adw::Clamp>,
        #[template_child]
        pub column: TemplateChild<gtk::Box>,
        #[template_child]
        pub sensors_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub sensors_bin: TemplateChild<adw::BreakpointBin>,
        #[template_child]
        pub sensors_clamp: TemplateChild<adw::Clamp>,
        #[template_child]
        pub sensors_scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub left: TemplateChild<gtk::Box>,
        #[template_child]
        pub right: TemplateChild<gtk::Box>,
        pub(super) summary: OnceCell<SummaryWidgets>,
        pub(super) state: RefCell<State>,
        pub(super) numbers: OnceCell<Numbers>,
        /// The latest sample with every row; `None` before the first one.
        pub(super) last: RefCell<Option<Vec<Group>>>,
        /// The CPU fans that stopped spinning in the latest sample (D65).
        pub(super) stopped: RefCell<Vec<RowId>>,
        /// The fans speed up in the latest sample.
        pub(super) speeding: Cell<bool>,
        /// The temperatures close to their limit in the latest sample.
        pub(super) hot: RefCell<Vec<Hot>>,
        /// The limits of the temperatures, for the histories of the cards
        /// (D80).
        pub(super) limits: RefCell<Limits>,
        /// A board profile matches: without one the status sentence says
        /// that the readings work rather than that all is fine.
        pub(super) profile: Cell<bool>,
        /// The last five minutes of the temperatures, which the window
        /// keeps for this page and the Fans page.
        pub(super) histories: OnceCell<Rc<RefCell<Histories>>>,
        /// The warnings of the latest sample in words (`alerts`).
        pub(super) alerts: RefCell<Vec<String>>,
        /// The rows turned off and on in the settings.
        pub(super) visibility: RefCell<Visibility>,
        /// How the latest sample was read; `None` before the first one.
        pub(super) reading: RefCell<Option<Status>>,
        /// Told when that changes (the window passes it on).
        pub(super) on_status: RefCell<Option<StatusCallback>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OverviewPage {
        const NAME: &'static str = "BaseboardOverviewPage";
        type Type = super::OverviewPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for OverviewPage {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            let _ = self.summary.set(SummaryWidgets::new(&self.column));
            layout::limit(&self.clamp);
            layout::limit(&self.sensors_clamp);
            let page = obj.downgrade();
            layout::watch(&self.bin, move |width| {
                if let Some(page) = page.upgrade()
                    && let Some(summary) = page.imp().summary.get()
                {
                    summary.set_width(width);
                }
            });
            let page = obj.downgrade();
            layout::watch(&self.sensors_bin, move |width| {
                if let Some(page) = page.upgrade() {
                    page.set_wide(width.columns());
                }
            });
        }
    }

    impl WidgetImpl for OverviewPage {}
    impl BinImpl for OverviewPage {}
}

glib::wrapper! {
    pub struct OverviewPage(ObjectSubclass<imp::OverviewPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl OverviewPage {
    /// The rows turned off and on in the settings; the page changes at once.
    pub fn set_visibility(&self, visibility: Visibility) {
        self.imp().visibility.replace(visibility);
        self.display();
    }

    /// The histories of the temperatures, which the window keeps.
    pub fn set_histories(&self, histories: Rc<RefCell<Histories>>) {
        let _ = self.imp().histories.set(histories);
    }

    /// Whether a board profile matches.
    pub fn set_profile(&self, profile: bool) {
        self.imp().profile.set(profile);
        self.display();
    }

    /// The latest sample with every row, also the hidden ones.
    pub fn groups(&self) -> Vec<Group> {
        self.imp().last.borrow().clone().unwrap_or_default()
    }

    pub fn numbers(&self) -> &Numbers {
        self.imp()
            .numbers
            .get_or_init(|| Numbers::new(&crate::i18n::decimal_separator()))
    }

    /// Calls `f` with how the readings are read, whenever that changes.
    pub fn connect_status(&self, f: impl Fn(&Status) + 'static) {
        self.imp().on_status.replace(Some(Box::new(f)));
    }

    /// A new sample from the window's sensor reader.
    pub fn show(&self, sample: Sample) {
        let imp = self.imp();
        imp.last.replace(Some(sample.groups));
        imp.stopped.replace(sample.stopped);
        imp.speeding.set(sample.speeding);
        imp.hot.replace(sample.hot);
        imp.limits.replace(sample.limits);
        if imp.reading.borrow().as_ref() != Some(&sample.status) {
            self.show_status(&sample.status);
            if let Some(f) = imp.on_status.borrow().as_ref() {
                f(&sample.status);
            }
            imp.reading.replace(Some(sample.status));
        }
        self.display();
    }

    /// The banner when the board's chip is missing: the other chips are
    /// shown, and the System page says how to check the driver.
    fn show_status(&self, status: &Status) {
        if let Some(chip) = &status.missing_chip {
            self.imp().banner.set_title(
                // Translators: {} is the name of the board's chip, e.g. nct6687.
                &gettext("No readings from the board: its {} chip was not found")
                    .replace("{}", chip),
            );
        }
    }

    /// Shows the latest sample without the hidden rows.
    fn display(&self) {
        let imp = self.imp();
        let last = imp.last.borrow();
        let (all, groups) = match last.as_deref() {
            // Still reading the first sample.
            None => return,
            Some(all) => (all, readings::visible(all, &imp.visibility.borrow())),
        };
        let any = !all.is_empty();
        // Without any readings the status page says more than the banner.
        let missing = imp
            .reading
            .borrow()
            .as_ref()
            .is_some_and(|status| status.missing_chip.is_some());
        imp.banner.set_revealed(missing && any);
        let numbers = self.numbers();
        let summary = top::summary(&groups, &imp.stopped.borrow(), imp.speeding.get());
        let state = top::state(&summary, all, &imp.hot.borrow(), imp.profile.get());
        imp.alerts.replace(alerts(&state, &summary, numbers));
        if groups.is_empty() {
            let (title, description) = if any {
                (
                    gettext("All readings are hidden"),
                    Some(gettext("Turn them on again in Preferences.")),
                )
            } else {
                (
                    gettext("No sensor readings"),
                    Some(gettext(
                        "This computer reports no hardware sensors (hwmon), \
                         for example in a virtual machine.",
                    )),
                )
            };
            imp.status.set_title(&title);
            imp.status.set_description(description.as_deref());
            imp.stack.set_visible_child_name("status");
            return;
        }
        let shape: Vec<Shape> = groups.iter().map(Shape::of).collect();
        if imp.state.borrow().shape != shape {
            self.rebuild(&groups, shape);
        }
        for (widgets, group) in imp.state.borrow().groups.iter().zip(&groups) {
            widgets.update(group, numbers);
        }
        if let (Some(widgets), Some(histories)) = (imp.summary.get(), imp.histories.get()) {
            let limits = imp.limits.borrow();
            let limit_of = |hero: &top::Hero| top::card_limit(hero, all, |id| limits.get(id));
            widgets.update(&summary, &state, &histories.borrow(), numbers, &limit_of);
        }
        imp.stack.set_visible_child_name("summary");
    }

    /// The problems of the latest sample in words, for the System page: a
    /// stopped CPU fan (D65), temperatures close to their limit and rails
    /// outside their tolerance (D67).
    pub fn alerts(&self) -> Vec<String> {
        self.imp().alerts.borrow().clone()
    }

    /// The page "All Sensors" for the window to push, opened on the group
    /// of `key` (`summary::group_key`), or at its top for an empty key.
    pub fn sensors_page(&self, key: &str) -> adw::NavigationPage {
        self.scroll_to_group(summary::group_of_key(key));
        self.imp().sensors_page.clone()
    }

    /// Scrolls "All Sensors" so that the group of `kind` stands on top, once
    /// it is laid out, and moves the keyboard focus into it.
    fn scroll_to_group(&self, kind: Option<GroupKind>) {
        let imp = self.imp();
        let target = kind.and_then(|kind| {
            imp.state
                .borrow()
                .groups
                .iter()
                .find(|g| g.kind == kind)
                .map(|g| g.widget.clone())
        });
        // Where the group was in the frame before, and the frames so far.
        let last = Cell::new(None);
        let frames = Cell::new(0);
        imp.sensors_scrolled.add_tick_callback(move |scrolled, _| {
            let adjustment = scrolled.vadjustment();
            let Some(target) = &target else {
                adjustment.set_value(0.0);
                return glib::ControlFlow::Break;
            };
            // The content under the viewport, which does not scroll.
            let content = scrolled.child().and_then(|viewport| viewport.first_child());
            let y = content
                .and_then(|content| target.compute_point(&content, &graphene::Point::zero()))
                .map(|point| f64::from(point.y()));
            // A frame runs this before its layout. The first one after the
            // page is pushed still has the layout from when it was last
            // shown, before the readings since then changed its rows: the
            // group must stay in place for a frame (at most a second).
            frames.set(frames.get() + 1);
            let settled = target.height() > 0 && y.is_some() && last.replace(y) == y;
            if !settled && frames.get() < 60 {
                return glib::ControlFlow::Continue;
            }
            // The focus first: the viewport scrolls to it on its own, by
            // less than the group needs.
            target.child_focus(gtk::DirectionType::TabForward);
            if let Some(y) = y {
                adjustment.set_value(y);
            }
            glib::ControlFlow::Break
        });
    }

    fn rebuild(&self, groups: &[Group], shape: Vec<Shape>) {
        let imp = self.imp();
        {
            let mut state = imp.state.borrow_mut();
            for old in state.groups.drain(..) {
                if let Some(column) = old.widget.parent().and_downcast::<gtk::Box>() {
                    column.remove(&old.widget);
                }
            }
            state.groups = groups.iter().map(GroupWidgets::new).collect();
            state.shape = shape;
        }
        self.layout();
    }

    fn set_wide(&self, wide: bool) {
        self.imp().state.borrow_mut().wide = wide;
        self.layout();
    }

    /// Debug builds: where "All Sensors" is scrolled to, for the tour.
    #[cfg(debug_assertions)]
    pub fn sensors_view(&self) -> SensorsView {
        let imp = self.imp();
        let adjustment = imp.sensors_scrolled.vadjustment();
        let content = imp
            .sensors_scrolled
            .child()
            .and_then(|viewport| viewport.first_child());
        let groups = imp
            .state
            .borrow()
            .groups
            .iter()
            .filter_map(|group| {
                let point = group
                    .widget
                    .compute_point(content.as_ref()?, &graphene::Point::zero())?;
                Some((group.kind, f64::from(point.y())))
            })
            .collect();
        SensorsView {
            value: adjustment.value(),
            height: adjustment.page_size(),
            max: adjustment.upper() - adjustment.page_size(),
            groups,
        }
    }

    /// Debug builds: scrolls to the end, for a screenshot of the lower
    /// groups.
    #[cfg(debug_assertions)]
    pub fn scroll_to_end(&self) {
        let adjustment = self.imp().scrolled.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }

    /// Puts the groups in one column in the order of §5.1, or in two.
    fn layout(&self) {
        let imp = self.imp();
        let state = imp.state.borrow();
        for group in &state.groups {
            if let Some(column) = group.widget.parent().and_downcast::<gtk::Box>() {
                column.remove(&group.widget);
            }
        }
        let mut right = false;
        for group in &state.groups {
            if state.wide && group.goes_right() {
                imp.right.append(&group.widget);
                right = true;
            } else {
                imp.left.append(&group.widget);
            }
        }
        imp.right.set_visible(right);
    }
}
