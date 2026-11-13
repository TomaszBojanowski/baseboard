// SPDX-License-Identifier: GPL-3.0-or-later

//! The card of a part of the computer (docs/PROJEKT.md §5.1, D67): its
//! name, its temperature in large type, the last five minutes as a line
//! with their range, a few facts in plain words (one number of the part,
//! the highest temperature since the app started), and a note under them.
//! The processor's card is the same on the Overview and on the Fans page.
//! Close to its limit the card says so with the icon, the number and the
//! line in the color of warnings.
//!
//! In a window of one card a row (`Width::Medium`) the temperature and the
//! facts stand beside the line; otherwise they are above and under it.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use gettextrs::gettext;

use crate::history_chart::{self, HistoryChart, Points};
use crate::layout::Width;
use baseboard_core::history::Trend;

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
    pub facts: Vec<Fact>,
    pub note: Option<String>,
}

/// One row of facts.
struct FactRow {
    row: gtk::Box,
    title: gtk::Label,
    icon: gtk::Image,
    value: gtk::Label,
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
    history: gtk::Box,
    range: gtk::Label,
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

/// The numbers and the line by the width of the page.
fn sizes(width: Width) -> (&'static str, i32) {
    match width {
        Width::Wide => ("large", 170),
        Width::Compact | Width::Medium => ("medium", 96),
        Width::Narrow => ("small", 70),
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
            head.append(
                &gtk::Image::builder()
                    .icon_name("go-next-symbolic")
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .css_classes(["dimmed"])
                    .build(),
            );
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
        // Translators: under the line of a temperature on its card.
        let span = gtk::Label::builder()
            .label(gettext("Last 5 minutes"))
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["caption", "dimmed"])
            .build();
        let range = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["caption", "dimmed", "numeric"])
            .build();
        let caption = gtk::Box::builder().spacing(8).build();
        caption.append(&span);
        caption.append(&range);
        let history = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .hexpand(true)
            .valign(gtk::Align::End)
            .build();
        history.append(&chart);
        history.append(&caption);

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
            history,
            range,
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
            self.history.upcast_ref(),
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
            // of at least 210 px and as wide as the facts on one line.
            self.number.set_size_request(SIDE, -1);
            self.facts.set_size_request(SIDE, -1);
            self.grid.attach(&self.number, 0, 1, 1, 1);
            self.grid.attach(&self.facts, 0, 2, 1, 1);
            self.grid.attach(&self.history, 1, 1, 1, 2);
            self.grid.attach(&self.note, 0, 3, 2, 1);
        } else {
            self.number.set_size_request(-1, -1);
            self.facts.set_size_request(-1, -1);
            self.grid.attach(&self.number, 0, 1, 2, 1);
            self.grid.attach(&self.history, 0, 2, 2, 1);
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
        self.badge.set_icon_name(Some(if hot {
            "dialog-warning-symbolic"
        } else {
            self.icon
        }));
        self.chart.set_points(view.points.clone(), hot);
        let range = history_chart::range_text(&view.points);
        if self.range.label() != range {
            self.range.set_label(&range);
        }
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
        // and the facts in words.
        let history = history_chart::describe(&view.points, view.trend);
        self.chart
            .update_property(&[gtk::accessible::Property::Label(&history)]);
        let mut description = vec![history];
        description.extend(
            view.facts
                .iter()
                .map(|fact| format!("{}: {}", fact.title, fact.value)),
        );
        description.extend(view.note.clone());
        self.widget.update_property(&[
            gtk::accessible::Property::Label(&format!(
                "{}: {}",
                self.name,
                history_chart::degrees(view.value)
            )),
            gtk::accessible::Property::Description(&description.join(" ")),
        ]);
        self.shown.replace(Some(view.clone()));
    }

    fn show_facts(&self, facts: &[Fact]) {
        let mut rows = self.rows.borrow_mut();
        while rows.len() > facts.len() {
            if let Some(old) = rows.pop() {
                self.facts.remove(&old.row);
            }
        }
        while rows.len() < facts.len() {
            let row = gtk::Box::builder()
                .spacing(12)
                .css_classes(["part-fact"])
                .build();
            let title = gtk::Label::builder()
                .xalign(0.0)
                .hexpand(true)
                .wrap(true)
                .natural_wrap_mode(gtk::NaturalWrapMode::Word)
                .css_classes(["dimmed"])
                .build();
            let icon = gtk::Image::builder()
                .icon_name("dialog-warning-symbolic")
                .visible(false)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .css_classes(["warning"])
                .build();
            let value = gtk::Label::builder()
                .xalign(1.0)
                .css_classes(["heading", "numeric"])
                .build();
            row.append(&title);
            row.append(&icon);
            row.append(&value);
            self.facts.append(&row);
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
        self.grid.query_child(&self.history).0 == 1
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
            facts: vec![
                Fact {
                    title: "CPU fan".into(),
                    value: "890\u{a0}rpm".into(),
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

    /// A card: the temperature in whole degrees, the facts, the range of
    /// the line, the warning colors close to the limit, and one row a card
    /// with its parts side by side in a window of about 1024 px. For
    /// screen readers the name with the temperature and the rest in words.
    /// Needs a display.
    #[test]
    fn card_of_a_part() {
        crate::gtk_test::run("card_of_a_part", || {
            let card = PartCard::new("baseboard-processor-symbolic", "Processor", Some("cpu"));
            card.set_width(Width::Wide);
            card.update(&view(false));
            assert_eq!(card.value(), "68");
            assert_eq!(card.range.label(), "63–68\u{a0}°C");
            assert_eq!(card.facts().len(), 2);
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

    /// In a window of about 1024 px the temperature and the facts stand
    /// beside the line, each fact on one line in English and in Polish, in
    /// the narrowest card of that width (a page of 600 sp without its
    /// margins); the line keeps the rest. Needs a display.
    #[test]
    fn facts_on_one_line() {
        crate::gtk_test::run("facts_on_one_line", || {
            crate::gtk_test::in_app_style(|| {
                for titles in [
                    ["CPU fan", "Highest since the app started"],
                    ["Wentylator procesora", "Najwyższa od uruchomienia"],
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
                    assert!(card.history.width() >= 200, "{}", card.history.width());
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
