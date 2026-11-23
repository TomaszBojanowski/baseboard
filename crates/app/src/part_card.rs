// SPDX-License-Identifier: GPL-3.0-or-later

//! The card of a part of the computer (docs/PROJEKT.md §5.1, D67, D80): its
//! name, its temperature in large type, the last five minutes as a line
//! (`history_chart`), a few facts in plain words (one number of the part,
//! the range of the line, the highest temperature since the app started),
//! and a note under them. The processor's card is the same on the Overview
//! and on the Fans page. Close to its limit the card says so with the icon,
//! the number and the line in the color of warnings.
//!
//! In a window of one card a row (`Width::Medium`) the temperature and the
//! facts stand beside the line, which is as tall as they are; otherwise
//! they are above and under it, and the line has a fixed height. The title
//! of a fact never breaks: where the widest fact of the card does not fit
//! on one line, every value of the card goes under its title (`FactLine`).

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{glib, graphene, gsk};

use crate::history_chart::{self, HistoryChart, Limit, Points, Scale};
use crate::layout::Width;
use baseboard_core::history::Trend;
use baseboard_core::units;

/// A fact under the line: what, how much, and whether it is a warning.
#[derive(Debug, Clone, PartialEq)]
pub struct Fact {
    pub title: String,
    pub value: String,
    pub warn: bool,
}

/// What a card shows.
#[derive(Debug, Clone, PartialEq)]
pub struct CardView {
    /// In °C.
    pub value: f64,
    pub points: Points,
    pub trend: Trend,
    /// Close to its limit.
    pub hot: bool,
    /// The limit on the line (D80), when the part has one.
    pub limit: Option<Limit>,
    /// The scale of every card's line (D80).
    pub scale: Scale,
    pub facts: Vec<Fact>,
    pub note: Option<String>,
}

/// One row of facts.
struct FactRow {
    row: FactLine,
    title: gtk::Label,
    icon: gtk::Image,
    value: gtk::Label,
}

/// Between the title and the value of a fact, and before the value's
/// warning icon.
const FACT_SPACING: i32 = 12;
/// Between a title and the value under it.
const FACT_STACKED_GAP: i32 = 2;

mod fact_imp {
    use super::*;

    #[derive(Default)]
    pub struct FactLine {
        pub(super) title: OnceCell<gtk::Label>,
        pub(super) icon: OnceCell<gtk::Image>,
        pub(super) value: OnceCell<gtk::Label>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FactLine {
        const NAME: &'static str = "BaseboardFactLine";
        type Type = super::FactLine;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for FactLine {
        fn dispose(&self) {
            let obj = self.obj();
            while let Some(child) = obj.first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for FactLine {
        // Its height depends on the width it gets.
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let obj = self.obj();
            let (title, value) = (obj.title_size(), obj.value_size());
            if orientation == gtk::Orientation::Horizontal {
                // At least the title or the value, one under the other; at
                // best both on one line.
                let least = title.0.max(value.0);
                return (least, obj.line_width().max(least), -1, -1);
            }
            let height = if obj.stacked(for_size) {
                title.1 + FACT_STACKED_GAP + value.1
            } else {
                title.1.max(value.1)
            };
            (height, height, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            let obj = self.obj();
            let (Some(title), Some(icon), Some(value)) =
                (self.title.get(), self.icon.get(), self.value.get())
            else {
                return;
            };
            let (title_size, value_size) = (obj.title_size(), obj.value_size());
            let at = |x: i32, y: i32| {
                Some(gsk::Transform::new().translate(&graphene::Point::new(x as f32, y as f32)))
            };
            let (x, y, line) = if obj.stacked(width) {
                title.allocate(width, title_size.1, -1, None);
                (0, title_size.1 + FACT_STACKED_GAP, value_size.1)
            } else {
                title.allocate(width - value_size.0 - FACT_SPACING, height, -1, None);
                (width - value_size.0, 0, height)
            };
            let mut x = x;
            if icon.get_visible() {
                let (_, icon_width, _, _) = icon.measure(gtk::Orientation::Horizontal, -1);
                icon.allocate(icon_width, line, -1, at(x, y));
                x += icon_width + FACT_SPACING;
            }
            let (_, value_width, _, _) = value.measure(gtk::Orientation::Horizontal, -1);
            value.allocate(value_width, line, -1, at(x, y));
        }
    }
}

glib::wrapper! {
    /// A fact: its title and its value, with a warning icon before the
    /// value when it is one. On one line, the value on the right, while the
    /// widest fact of the card fits on one line at this width; otherwise
    /// the value under its title. The title never breaks.
    pub struct FactLine(ObjectSubclass<fact_imp::FactLine>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FactLine {
    fn new() -> Self {
        let line: Self = glib::Object::new();
        line.add_css_class("part-fact");
        let title = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["dimmed"])
            .build();
        let icon = gtk::Image::builder()
            .icon_name("dialog-warning-symbolic")
            .visible(false)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["warning"])
            .build();
        let value = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["heading", "numeric"])
            .build();
        for child in [
            title.upcast_ref::<gtk::Widget>(),
            icon.upcast_ref(),
            value.upcast_ref(),
        ] {
            child.set_parent(&line);
        }
        let imp = line.imp();
        let _ = imp.title.set(title);
        let _ = imp.icon.set(icon);
        let _ = imp.value.set(value);
        line
    }

    /// The natural width and height of the title on one line.
    fn title_size(&self) -> (i32, i32) {
        self.imp().title.get().map_or((0, 0), natural)
    }

    /// The natural width and height of the value with its icon.
    fn value_size(&self) -> (i32, i32) {
        let imp = self.imp();
        let (Some(icon), Some(value)) = (imp.icon.get(), imp.value.get()) else {
            return (0, 0);
        };
        let (width, height) = natural(value);
        if !icon.get_visible() {
            return (width, height);
        }
        let (icon_width, icon_height) = natural(icon);
        (icon_width + FACT_SPACING + width, height.max(icon_height))
    }

    /// The width of the fact on one line.
    fn line_width(&self) -> i32 {
        self.title_size().0 + FACT_SPACING + self.value_size().0
    }

    /// Whether at `width` the value goes under the title: when the widest
    /// fact of the card does not fit on one line, so that all facts of a
    /// card are laid out alike.
    fn stacked(&self, width: i32) -> bool {
        if width < 0 {
            return false;
        }
        let mut widest = self.line_width();
        let mut sibling = self.parent().and_then(|parent| parent.first_child());
        while let Some(widget) = sibling {
            if let Some(line) = widget.downcast_ref::<Self>() {
                widest = widest.max(line.line_width());
            }
            sibling = widget.next_sibling();
        }
        width < widest
    }
}

/// The natural width and height of `widget`.
fn natural(widget: &impl IsA<gtk::Widget>) -> (i32, i32) {
    let (_, width, _, _) = widget.measure(gtk::Orientation::Horizontal, -1);
    let (_, height, _, _) = widget.measure(gtk::Orientation::Vertical, width);
    (width, height)
}

pub struct PartCard {
    /// A button that opens the group of "All Sensors", or a plain box.
    pub widget: gtk::Widget,
    name: String,
    icon: &'static str,
    grid: gtk::Grid,
    head: gtk::Box,
    badge: gtk::Image,
    number: gtk::Box,
    value: gtk::Label,
    chart: HistoryChart,
    facts: gtk::Box,
    rows: RefCell<Vec<FactRow>>,
    note: gtk::Label,
    /// The note keeps its place while it is not shown, so that what is
    /// under the card does not move.
    note_reserved: Cell<bool>,
    width: Cell<Option<Width>>,
    shown: RefCell<Option<CardView>>,
}

/// The narrowest column of the temperature and the facts beside the line.
const SIDE: i32 = 210;

/// The numbers and the height of the line by the width of the page (D80);
/// `None` makes the line as tall as the temperature and the facts beside
/// it.
fn sizes(width: Width) -> (&'static str, Option<i32>) {
    match width {
        Width::Wide => ("large", Some(160)),
        Width::Compact => ("medium", Some(160)),
        Width::Medium => ("medium", None),
        Width::Narrow => ("small", Some(120)),
    }
}

impl PartCard {
    /// `opens` is the key of the group of "All Sensors" a click opens;
    /// `None` for a card that opens nothing.
    pub fn new(icon: &'static str, name: &str, opens: Option<&str>) -> Self {
        let grid = gtk::Grid::builder()
            .column_spacing(24)
            .row_spacing(12)
            .build();
        let badge = gtk::Image::builder()
            .icon_name(icon)
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["part-badge"])
            .build();
        let title = gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .hexpand(true)
            .wrap(true)
            .natural_wrap_mode(gtk::NaturalWrapMode::Word)
            .css_classes(["part-name"])
            .build();
        let head = gtk::Box::builder().spacing(12).build();
        head.append(&badge);
        head.append(&title);
        if opens.is_some() {
            head.append(&crate::widgets::chevron());
        }
        let value = gtk::Label::builder()
            .css_classes(["part-value", "numeric"])
            .build();
        let unit = gtk::Label::builder()
            .label("°C")
            .valign(gtk::Align::Start)
            .css_classes(["part-unit"])
            .build();
        let number = gtk::Box::builder().halign(gtk::Align::Start).build();
        number.append(&value);
        number.append(&unit);

        let chart = HistoryChart::new();

        // Beside the line the facts take the width of their titles on one
        // line and the line has the rest: the box does not take the
        // expanding of the titles from them.
        let facts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(false)
            .valign(gtk::Align::End)
            .build();
        let note = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["dimmed"])
            .build();

        let widget: gtk::Widget = match opens {
            Some(key) => {
                let button = gtk::Button::builder()
                    .child(&grid)
                    .css_classes(["card", "part-card"])
                    .build();
                // The target before the name: GTK checks the action as soon
                // as it has a name, and without its parameter turns it off
                // with a warning.
                button.set_action_target_value(Some(&key.to_variant()));
                button.set_action_name(Some("win.show-sensors"));
                button.upcast()
            }
            // A bin gives the grid the whole width it is measured for, so
            // that its height for that width is right.
            None => adw::Bin::builder()
                .child(&grid)
                .accessible_role(gtk::AccessibleRole::Group)
                .css_classes(["card", "part-card"])
                .build()
                .upcast(),
        };
        Self {
            widget,
            name: name.to_owned(),
            icon,
            grid,
            head,
            badge,
            number,
            value,
            chart,
            facts,
            rows: RefCell::new(Vec::new()),
            note,
            note_reserved: Cell::new(false),
            width: Cell::new(None),
            shown: RefCell::new(None),
        }
    }

    /// Keeps the room of a note `text` under the facts, also while there
    /// is none to show.
    pub fn reserve_note(&self, text: &str) {
        self.note_reserved.set(true);
        self.note.set_label(text);
        self.note.set_visible(true);
        self.note.set_opacity(0.0);
        self.note
            .update_state(&[gtk::accessible::State::Hidden(true)]);
    }

    /// Lays the card out for the width of the page.
    pub fn set_width(&self, width: Width) {
        if self.width.replace(Some(width)) == Some(width) {
            return;
        }
        for part in [
            self.head.upcast_ref::<gtk::Widget>(),
            self.number.upcast_ref(),
            self.chart.upcast_ref(),
            self.facts.upcast_ref(),
            self.note.upcast_ref(),
        ] {
            if part.parent().is_some() {
                self.grid.remove(part);
            }
        }
        self.grid.attach(&self.head, 0, 0, 2, 1);
        if width == Width::Medium {
            // The temperature and the facts beside the line, in a column
            // of at least 210 px and as wide as the facts on one line; the
            // line from the top of the temperature to the bottom of the
            // last fact (D80).
            self.number.set_size_request(SIDE, -1);
            self.facts.set_size_request(SIDE, -1);
            self.grid.attach(&self.number, 0, 1, 1, 1);
            self.grid.attach(&self.facts, 0, 2, 1, 1);
            self.grid.attach(&self.chart, 1, 1, 1, 2);
            self.grid.attach(&self.note, 0, 3, 2, 1);
            self.chart.set_valign(gtk::Align::Fill);
        } else {
            self.chart.set_valign(gtk::Align::Start);
            self.number.set_size_request(-1, -1);
            self.facts.set_size_request(-1, -1);
            self.grid.attach(&self.number, 0, 1, 2, 1);
            self.grid.attach(&self.chart, 0, 2, 2, 1);
            self.grid.attach(&self.facts, 0, 3, 2, 1);
            self.grid.attach(&self.note, 0, 4, 2, 1);
        }
        let (size, height) = sizes(width);
        for class in ["large", "medium", "small"] {
            self.number.remove_css_class(class);
        }
        self.number.add_css_class(size);
        self.chart.set_height(height);
    }

    pub fn update(&self, view: &CardView) {
        if self.shown.borrow().as_ref() == Some(view) {
            return;
        }
        let value = if view.value.is_finite() {
            format!("{}", view.value.round() as i64)
        } else {
            "–".to_owned()
        };
        if self.value.label() != value {
            self.value.set_label(&value);
        }
        let hot = view.hot;
        for widget in [self.widget.clone(), self.badge.clone().upcast()] {
            if hot {
                widget.add_css_class("hot");
            } else {
                widget.remove_css_class("hot");
            }
        }
        crate::widgets::set_icon(
            &self.badge,
            if hot {
                "dialog-warning-symbolic"
            } else {
                self.icon
            },
        );
        self.chart.set_view(
            view.points.clone(),
            hot,
            view.limit.as_ref().map(|limit| limit.value),
            view.scale,
        );
        self.show_facts(&view.facts);
        match &view.note {
            Some(note) => {
                self.note.set_label(note);
                self.note.set_visible(true);
                self.note.set_opacity(1.0);
            }
            None if self.note_reserved.get() => self.note.set_opacity(0.0),
            None => self.note.set_visible(false),
        }
        self.note
            .update_state(&[gtk::accessible::State::Hidden(view.note.is_none())]);

        // For screen readers: the name with the temperature, then the line
        // and the facts in words; the line already says its range, so the
        // fact with it is not said twice.
        let history = history_chart::describe(
            &view.points,
            view.trend,
            view.limit.as_ref().map(|limit| limit.words.as_str()),
        );
        self.chart
            .update_property(&[gtk::accessible::Property::Label(&history)]);
        let period = history_chart::period(&view.points);
        let mut description = vec![history];
        description.extend(
            view.facts
                .iter()
                .filter(|fact| fact.title != period)
                .map(|fact| format!("{}: {}", fact.title, fact.value)),
        );
        description.extend(view.note.clone());
        self.widget.update_property(&[
            gtk::accessible::Property::Label(&format!(
                "{}: {}",
                self.name,
                units::degrees(view.value)
            )),
            gtk::accessible::Property::Description(&description.join(" ")),
        ]);
        self.shown.replace(Some(view.clone()));
    }

    fn show_facts(&self, facts: &[Fact]) {
        // The view shown so far: the facts change far less often than the
        // temperature.
        if self
            .shown
            .borrow()
            .as_ref()
            .is_some_and(|old| old.facts == facts)
        {
            return;
        }
        let mut rows = self.rows.borrow_mut();
        while rows.len() > facts.len() {
            if let Some(old) = rows.pop() {
                self.facts.remove(&old.row);
            }
        }
        while rows.len() < facts.len() {
            let row = FactLine::new();
            self.facts.append(&row);
            let imp = row.imp();
            let (Some(title), Some(icon), Some(value)) =
                (imp.title.get(), imp.icon.get(), imp.value.get())
            else {
                continue;
            };
            let (title, icon, value) = (title.clone(), icon.clone(), value.clone());
            rows.push(FactRow {
                row,
                title,
                icon,
                value,
            });
        }
        for (row, fact) in rows.iter().zip(facts) {
            if row.title.label() != fact.title {
                row.title.set_label(&fact.title);
            }
            if row.value.label() != fact.value {
                row.value.set_label(&fact.value);
            }
            row.icon.set_visible(fact.warn);
            if fact.warn {
                row.value.add_css_class("warning");
            } else {
                row.value.remove_css_class("warning");
            }
        }
        // Whether a fact goes under its title depends on the widest fact of
        // the card, so a change of one is a change of all.
        for row in rows.iter() {
            row.row.queue_resize();
        }
    }

    /// The facts shown, as title and value (tests).
    #[cfg(test)]
    pub fn facts(&self) -> Vec<(String, String)> {
        self.rows
            .borrow()
            .iter()
            .map(|row| (row.title.label().into(), row.value.label().into()))
            .collect()
    }

    /// The temperature shown (tests).
    #[cfg(test)]
    pub fn value(&self) -> String {
        self.value.label().into()
    }

    /// Whether the line, the temperature and the facts stand beside each
    /// other (tests).
    #[cfg(test)]
    pub fn side_by_side(&self) -> bool {
        self.grid.query_child(&self.chart).0 == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(hot: bool) -> CardView {
        CardView {
            value: 68.2,
            points: vec![(120.0, 63.0), (60.0, 66.0), (0.0, 68.2)],
            trend: Trend::Rising,
            hot,
            limit: Some(Limit {
                value: 95.0,
                words: "At 95\u{a0}°C it slows down to avoid overheating.".into(),
            }),
            scale: (0.0, 100.0),
            facts: vec![
                Fact {
                    title: "CPU fan".into(),
                    value: "890\u{a0}rpm".into(),
                    warn: false,
                },
                Fact {
                    title: "Last 3 minutes".into(),
                    value: "From 63 to 68\u{a0}°C".into(),
                    warn: false,
                },
                Fact {
                    title: "Highest since the app started".into(),
                    value: "78\u{a0}°C".into(),
                    warn: false,
                },
            ],
            note: None,
        }
    }

    /// A card: the temperature in whole degrees, the facts with the range
    /// of the line, the warning colors close to the limit, and one row a
    /// card with its parts side by side in a window of about 1024 px. For
    /// screen readers the name with the temperature and the rest in words.
    /// Needs a display.
    #[test]
    fn card_of_a_part() {
        crate::gtk_test::run("card_of_a_part", || {
            let card = PartCard::new("baseboard-processor-symbolic", "Processor", Some("cpu"));
            card.set_width(Width::Wide);
            card.update(&view(false));
            assert_eq!(card.value(), "68");
            assert_eq!(card.facts().len(), 3);
            assert!(!card.widget.has_css_class("hot"));
            assert!(!card.side_by_side());
            card.set_width(Width::Medium);
            assert!(card.side_by_side());

            card.update(&view(true));
            assert!(card.widget.has_css_class("hot"));
            assert_eq!(
                card.badge.icon_name().as_deref(),
                Some("dialog-warning-symbolic")
            );
            let button = card.widget.downcast_ref::<gtk::Button>().unwrap();
            assert_eq!(button.action_name().as_deref(), Some("win.show-sensors"));
        });
    }

    /// A new temperature, as every sample brings, leaves the badge as it is
    /// (`widgets::set_icon`); close to the limit it shows the sign of
    /// warnings, and back. Needs a display.
    #[test]
    fn a_new_temperature_keeps_the_badge() {
        crate::gtk_test::run("a_new_temperature_keeps_the_badge", || {
            let card = PartCard::new("baseboard-processor-symbolic", "Processor", Some("cpu"));
            let mut shown = view(false);
            card.update(&shown);
            let mut changes = |hot: bool| {
                shown.value += 0.5;
                shown.hot = hot;
                crate::gtk_test::notifications(&card.badge, "icon-name", || card.update(&shown))
            };
            assert_eq!(changes(false), 0);
            assert_eq!(changes(true), 1);
            assert_eq!(
                card.badge.icon_name().as_deref(),
                Some("dialog-warning-symbolic")
            );
            assert_eq!(changes(true), 0);
            assert_eq!(changes(false), 1);
        });
    }

    /// The line has a height of its own, whatever the window (D80): the
    /// same wide and denser, 40 px less narrow, and given more room than
    /// the card needs it stays so. In a card of one row it reaches from the
    /// top of the temperature to the bottom of the last fact. Needs a
    /// display.
    #[test]
    fn heights_of_the_line() {
        crate::gtk_test::run("heights_of_the_line", || {
            crate::gtk_test::in_app_style(|| {
                let card = PartCard::new("baseboard-processor-symbolic", "Processor", Some("cpu"));
                card.update(&view(false));
                let lay_out = |width: i32, more: i32| {
                    let (minimum, natural, _, _) =
                        card.widget.measure(gtk::Orientation::Vertical, width);
                    assert_eq!(minimum, natural);
                    card.widget.allocate(width, natural + more, -1, None);
                    card.chart.height()
                };
                card.set_width(Width::Wide);
                let wide = lay_out(620, 0);
                assert_eq!(lay_out(900, 400), wide);
                card.set_width(Width::Compact);
                assert_eq!(lay_out(400, 0), wide);
                card.set_width(Width::Narrow);
                assert_eq!(lay_out(340, 0), wide - 40);

                card.set_width(Width::Medium);
                lay_out(700, 0);
                let bounds = |widget: &gtk::Widget| widget.compute_bounds(&card.widget).unwrap();
                let chart = bounds(card.chart.upcast_ref());
                let number = bounds(card.number.upcast_ref());
                let facts = bounds(card.facts.upcast_ref());
                assert_eq!(chart.y(), number.y());
                assert_eq!(chart.y() + chart.height(), facts.y() + facts.height());
                assert!(chart.height() > 150.0, "{}", chart.height());
            });
        });
    }

    /// In a window of about 1024 px the temperature and the facts stand
    /// beside the line, each fact on one line in English and in Polish, in
    /// the narrowest card of that width (a page of 600 sp without its
    /// margins); the line keeps the rest. Needs a display.
    #[test]
    fn facts_on_one_line() {
        crate::gtk_test::run("facts_on_one_line", || {
            crate::gtk_test::in_app_style(|| {
                for titles in [
                    ["CPU fan", "Last 5 minutes", "Highest since the app started"],
                    [
                        "Wentylator procesora",
                        "Ostatnie 5 minut",
                        "Najwyższa od uruchomienia",
                    ],
                ] {
                    let card =
                        PartCard::new("baseboard-processor-symbolic", "Processor", Some("cpu"));
                    card.set_width(Width::Medium);
                    let mut view = view(false);
                    for (fact, title) in view.facts.iter_mut().zip(titles) {
                        fact.title = title.into();
                    }
                    card.update(&view);
                    let width = 576;
                    let (_, height, _, _) = card.widget.measure(gtk::Orientation::Vertical, width);
                    card.widget.allocate(width, height, -1, None);
                    for row in card.rows.borrow().iter() {
                        let lines = row.title.layout().line_count();
                        assert_eq!(lines, 1, "{:?} on {lines} lines", row.title.label());
                    }
                    assert!(card.facts.width() >= SIDE);
                    assert!(card.chart.width() >= 200, "{}", card.chart.width());
                }
            });
        });
    }

    /// The titles of the facts never break, at any width a card can get in
    /// any layout, in English and in Polish: where the widest fact does not
    /// fit on one line, the values of the card go under their titles. The
    /// facts of the board's card (its widest value) and the processor's.
    /// Needs a display.
    #[test]
    fn fact_titles_never_break() {
        crate::gtk_test::run("fact_titles_never_break", || {
            crate::gtk_test::in_app_style(|| {
                for facts in [
                    [
                        ("Power", "Normal"),
                        ("Last 5 minutes", "From 36 to 38\u{a0}°C"),
                        ("Highest since the app started", "38\u{a0}°C"),
                    ],
                    [
                        ("Zasilanie", "W normie"),
                        ("Ostatnie 5 minut", "Od 36 do 38\u{a0}°C"),
                        ("Najwyższa od uruchomienia", "38\u{a0}°C"),
                    ],
                    [
                        ("CPU fan", "1880\u{a0}rpm"),
                        ("Last 2 minutes", "From 72 to 93\u{a0}°C"),
                        ("Highest since the app started", "93\u{a0}°C"),
                    ],
                    [
                        ("Wentylator procesora", "1880\u{a0}rpm"),
                        ("Ostatnie 2 minuty", "Od 72 do 93\u{a0}°C"),
                        ("Najwyższa od uruchomienia", "93\u{a0}°C"),
                    ],
                ] {
                    let card = PartCard::new("baseboard-board-symbolic", "Board", Some("board"));
                    let mut view = view(false);
                    for (fact, (title, value)) in view.facts.iter_mut().zip(facts) {
                        fact.title = title.into();
                        fact.value = value.into();
                    }
                    card.update(&view);
                    // Both layouts of the facts are seen: under each other
                    // in the narrowest cards, on one line in wide ones.
                    let (mut under, mut beside) = (false, false);
                    for layout in [Width::Wide, Width::Compact, Width::Medium, Width::Narrow] {
                        card.set_width(layout);
                        let (least, _, _, _) =
                            card.widget.measure(gtk::Orientation::Horizontal, -1);
                        for width in (least..least + 400).step_by(3) {
                            let (_, height, _, _) =
                                card.widget.measure(gtk::Orientation::Vertical, width);
                            card.widget.allocate(width, height, -1, None);
                            for row in card.rows.borrow().iter() {
                                let (_, whole, _, _) =
                                    row.title.measure(gtk::Orientation::Horizontal, -1);
                                assert!(
                                    row.title.layout().line_count() == 1
                                        && row.title.width() >= whole,
                                    "{layout:?}, {width} px: {:?} broken or cut",
                                    row.title.label()
                                );
                                let below = row
                                    .value
                                    .compute_point(&row.row, &graphene::Point::zero())
                                    .is_some_and(|point| point.y() > 0.0);
                                under |= below;
                                beside |= !below;
                            }
                        }
                    }
                    assert!(under && beside, "{facts:?}: under {under}, beside {beside}");
                }
            });
        });
    }

    /// A card that opens nothing, as on the Fans page, with room for its
    /// note: GTK measures its height for a width without a warning, at the
    /// widths of the column of the Fans page. Needs a display.
    #[test]
    fn card_that_opens_nothing() {
        crate::gtk_test::run("card_that_opens_nothing", || {
            let card = PartCard::new("baseboard-processor-symbolic", "Processor", None);
            card.reserve_note("Fans are speeding up");
            card.update(&view(false));
            for (width, size) in [
                (Width::Wide, 636),
                (Width::Compact, 431),
                (Width::Narrow, 336),
            ] {
                card.set_width(width);
                let (minimum, natural, _, _) =
                    card.widget.measure(gtk::Orientation::Vertical, size);
                assert!(minimum <= natural, "{width:?}: {minimum} > {natural}");
            }
            assert!(card.widget.downcast_ref::<gtk::Button>().is_none());
        });
    }
}
