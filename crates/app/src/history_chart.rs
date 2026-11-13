// SPDX-License-Identifier: GPL-3.0-or-later

//! The last five minutes of a temperature (docs/PROJEKT.md §5.1, D67): a
//! line with a light fill under it and the newest reading as a dot at the
//! right end, in the accent color, or in the color of warnings while the
//! part is close to its limit. Time runs from the left (five minutes ago)
//! to the right (now); a gap in the samples (a hidden window) is a gap in
//! the line. The scale spans at least 12 °C, so that a quiet sensor stays
//! a quiet line.
//!
//! It draws only when it gets a new history, once a sample. For screen
//! readers it is an image described in words (`describe`).

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::history::{History, SPAN, Trend};
use baseboard_core::sensors::Kind;
use baseboard_core::units::{self, RANGE_DASH, UNIT_SPACE};
use gettextrs::gettext;
use gtk::{gdk, glib, graphene, gsk};

/// The narrowest span of the scale, in °C.
const MIN_SPAN: f64 = 12.0;
/// Room for the dot at the right end and for the line at the top and the
/// bottom.
const DOT: f64 = 4.5;
const RING: f64 = 2.5;
const PAD_Y: f64 = DOT + RING;

/// A history as the chart draws it: seconds before the newest point and
/// the value in °C, oldest first.
pub type Points = Vec<(f64, f64)>;

/// The points of `history` for the chart.
pub fn points(history: &History) -> Points {
    let Some((newest, _)) = history.last() else {
        return Vec::new();
    };
    history
        .points()
        .map(|(at, value)| (newest.duration_since(at).as_secs_f64(), value))
        .collect()
}

/// A temperature in whole degrees with its unit, "68 °C", as the cards
/// show it; "–" when it could not be read.
pub fn degrees(value: f64) -> String {
    units::whole(Kind::Temp, value)
}

/// The lowest and highest value, as the caption shows them: "63–70 °C",
/// or one value when they round to the same degree.
pub fn range_text(points: &Points) -> String {
    let Some((lo, hi)) = range(points) else {
        return String::new();
    };
    let (lo, hi) = (lo.round() as i64, hi.round() as i64);
    if lo == hi {
        format!("{hi}{UNIT_SPACE}°C")
    } else {
        format!("{lo}{RANGE_DASH}{hi}{UNIT_SPACE}°C")
    }
}

fn range(points: &Points) -> Option<(f64, f64)> {
    let mut values = points.iter().map(|&(_, v)| v);
    let first = values.next()?;
    Some(values.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
}

/// What a screen reader says for the chart: the range of the last five
/// minutes, the value now and where it is going.
pub fn describe(points: &Points, trend: Trend) -> String {
    let Some(&(_, now)) = points.last() else {
        // Translators: read by screen readers for the history of a
        // temperature before its first reading.
        return gettext("No readings yet");
    };
    let text = match trend {
        // Translators: read by screen readers for the history of a
        // temperature; {range} is e.g. "63–70 °C", {value} e.g. "68 °C".
        Trend::Rising => gettext("Last 5 minutes: {range}. Now {value}, rising."),
        Trend::Falling => gettext("Last 5 minutes: {range}. Now {value}, falling."),
        Trend::Steady => gettext("Last 5 minutes: {range}. Now {value}, steady."),
    };
    text.replace("{range}", &range_text(points))
        .replace("{value}", &degrees(now))
}

/// The pieces of the line between gaps in the samples, in pixels.
fn segments(points: &Points, width: f64, height: f64) -> Vec<Vec<(f64, f64)>> {
    let Some((lo, hi)) = range(points) else {
        return Vec::new();
    };
    let middle = (lo + hi) / 2.0;
    let span = (hi - lo + 2.0).max(MIN_SPAN);
    let top = middle + span / 2.0;
    let x = |age: f64| (width - DOT - RING) * (1.0 - age / SPAN.as_secs_f64());
    let y = |value: f64| PAD_Y + (top - value) / span * (height - 2.0 * PAD_Y);
    // A gap is more than three of the usual steps between samples.
    let mut steps: Vec<f64> = points.windows(2).map(|w| w[0].0 - w[1].0).collect();
    steps.sort_by(f64::total_cmp);
    let usual = steps.get(steps.len() / 2).copied().unwrap_or(1.0);
    let mut pieces: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut last_age: Option<f64> = None;
    for &(age, value) in points {
        let new_piece = last_age.is_none_or(|last| last - age > 3.0 * usual + 0.5);
        if new_piece {
            pieces.push(Vec::new());
        }
        if let Some(piece) = pieces.last_mut() {
            piece.push((x(age), y(value)));
        }
        last_age = Some(age);
    }
    pieces
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct HistoryChart {
        pub(super) points: RefCell<Points>,
        pub(super) hot: Cell<bool>,
        pub(super) height: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HistoryChart {
        const NAME: &'static str = "BaseboardHistoryChart";
        type Type = super::HistoryChart;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for HistoryChart {}

    impl WidgetImpl for HistoryChart {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (0, 0, -1, -1),
                _ => {
                    let height = self.height.get();
                    (height, height, -1, -1)
                }
            }
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct HistoryChart(ObjectSubclass<imp::HistoryChart>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl HistoryChart {
    pub fn new() -> Self {
        let chart: Self = glib::Object::builder().property("hexpand", true).build();
        chart.set_height(96);
        chart
    }

    pub fn set_height(&self, height: i32) {
        if self.imp().height.replace(height) != height {
            self.queue_resize();
        }
    }

    /// The history to draw, and whether the part is close to its limit.
    pub fn set_points(&self, points: Points, hot: bool) {
        let imp = self.imp();
        let changed = *imp.points.borrow() != points || imp.hot.get() != hot;
        if changed {
            imp.points.replace(points);
            imp.hot.set(hot);
            self.queue_draw();
        }
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let points = imp.points.borrow();
        let (width, height) = (f64::from(self.width()), f64::from(self.height()));
        let pieces = segments(&points, width, height);
        if pieces.is_empty() {
            return;
        }
        let style = adw::StyleManager::default();
        let dark = style.is_dark();
        let color = if imp.hot.get() {
            gdk::RGBA::parse(if dark { "#cd9309" } else { "#e5a50a" }).unwrap_or(gdk::RGBA::BLACK)
        } else {
            style.accent_color_rgba()
        };
        let surface =
            gdk::RGBA::parse(if dark { "#38383c" } else { "#ffffff" }).unwrap_or(gdk::RGBA::WHITE);
        let fade = |factor: f32| {
            gdk::RGBA::new(
                color.red(),
                color.green(),
                color.blue(),
                color.alpha() * factor,
            )
        };
        let bottom = height as f32;
        let stroke = gsk::Stroke::new(2.25);
        stroke.set_line_join(gsk::LineJoin::Round);
        stroke.set_line_cap(gsk::LineCap::Round);
        for piece in &pieces {
            let (Some(&(x0, y0)), Some(&(x1, _))) = (piece.first(), piece.last()) else {
                continue;
            };
            let line = gsk::PathBuilder::new();
            let area = gsk::PathBuilder::new();
            line.move_to(x0 as f32, y0 as f32);
            area.move_to(x0 as f32, bottom);
            for &(x, y) in piece {
                line.line_to(x as f32, y as f32);
                area.line_to(x as f32, y as f32);
            }
            area.line_to(x1 as f32, bottom);
            area.close();
            snapshot.push_fill(&area.to_path(), gsk::FillRule::Winding);
            snapshot.append_linear_gradient(
                &graphene::Rect::new(0.0, 0.0, width as f32, bottom),
                &graphene::Point::new(0.0, PAD_Y as f32),
                &graphene::Point::new(0.0, bottom),
                &[
                    gsk::ColorStop::new(0.0, fade(0.28)),
                    gsk::ColorStop::new(1.0, fade(0.0)),
                ],
            );
            snapshot.pop();
            snapshot.append_stroke(&line.to_path(), &stroke, &color);
        }
        if let Some(&(x, y)) = pieces.last().and_then(|piece| piece.last()) {
            let dot = |radius: f64| {
                let builder = gsk::PathBuilder::new();
                builder.add_circle(&graphene::Point::new(x as f32, y as f32), radius as f32);
                builder.to_path()
            };
            let fill = gsk::FillRule::Winding;
            snapshot.append_fill(&dot(DOT + RING), fill, &surface);
            snapshot.append_fill(&dot(DOT), fill, &color);
        }
    }
}

impl Default for HistoryChart {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn history(values: &[(u64, f64)]) -> History {
        let start = Instant::now();
        let mut history = History::default();
        for &(s, v) in values {
            history.push(start + Duration::from_secs(s), v);
        }
        history
    }

    /// The caption and the description: a range, or one value when the
    /// readings round to one degree; the value now and the trend.
    #[test]
    fn words_for_a_history() {
        let rising = history(&[(0, 63.2), (60, 66.0), (120, 69.8)]);
        let points = points(&rising);
        assert_eq!(points.first(), Some(&(120.0, 63.2)));
        assert_eq!(range_text(&points), "63–70\u{a0}°C");
        assert_eq!(
            describe(&points, rising.trend()),
            "Last 5 minutes: 63–70\u{a0}°C. Now 70\u{a0}°C, rising."
        );
        let quiet = super::points(&history(&[(0, 30.2), (1, 30.4)]));
        assert_eq!(range_text(&quiet), "30\u{a0}°C");
        assert_eq!(describe(&Vec::new(), Trend::Steady), "No readings yet");
        assert_eq!(degrees(f64::NAN), units::UNREADABLE);
    }

    /// Time runs from the left to the right end; a gap in the samples
    /// starts a new piece of the line; the scale spans at least 12 °C.
    #[test]
    fn pieces_of_the_line() {
        let mut values: Vec<(u64, f64)> = (0..60).map(|s| (s, 40.0)).collect();
        values.extend((200..260).map(|s| (s, 41.0)));
        let points = points(&history(&values));
        let pieces = segments(&points, 300.0 + DOT + RING, 100.0);
        assert_eq!(pieces.len(), 2);
        let (x, y) = *pieces[1].last().unwrap();
        assert!((x - 300.0).abs() < 1e-9);
        // 41 °C in a scale of 12 °C around 40.5 °C: a little over the
        // middle.
        let middle = 50.0;
        assert!(y < middle && y > middle - 10.0, "{y}");
        // The oldest point, 259 s back, near the left.
        assert!(pieces[0][0].0 < 300.0 * (1.0 - 259.0 / 300.0) + 1e-9);
    }
}
