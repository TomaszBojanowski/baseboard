// SPDX-License-Identifier: GPL-3.0-or-later

//! The last five minutes of a temperature on a card (docs/PROJEKT.md §5.1,
//! D67, D80): a line with a light fill under it and the newest reading as a
//! dot at the right end, in the accent color, or in the color of warnings
//! while the part is close to its limit. Every card has the same scale,
//! 30–100 °C, lower or higher in whole 10 °C when a reading needs it, with
//! lines at 40, 60 and 80 °C labeled in a margin on the left and the part's
//! limit as a dashed line in the color of warnings. Time always spans five
//! minutes, from "5 min ago" under the left end to "now" under the dot: a
//! short history grows from the right, and a gap in the samples is a gap in
//! the line.
//!
//! The chart draws its own shapes, so it cannot read colors from CSS: the
//! colors of warnings are those of libadwaita 1.10 for the light and the
//! dark style, as in `fan_chart`. It draws only when it gets a new history,
//! once a sample. For screen readers it is an image described in words
//! (`describe`).

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::history::{History, SPAN, Trend};
use baseboard_core::units::{self, RANGE_DASH, UNIT_SPACE};
use gettextrs::{gettext, ngettext};
use gtk::{gdk, glib, gsk, pango};

use crate::paint::{TEXT_SCALE, alpha, circle, layout, line, point, polyline, size_of, stroke};

/// A history as the chart draws it: seconds before the newest point and
/// the value in °C, oldest first.
pub type Points = Vec<(f64, f64)>;

/// The limit of a part on its chart: where the dashed line goes, and what
/// a screen reader hears of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Limit {
    /// In °C.
    pub value: f64,
    pub words: String,
}

/// The scale while every reading is inside it, in °C.
const BOTTOM: f64 = 30.0;
const TOP: f64 = 100.0;
/// The scale grows by this many degrees at a time.
const STEP: f64 = 10.0;
/// The lines of the grid, in °C.
const GRID: [f64; 3] = [40.0, 60.0, 80.0];
/// A line of the grid closer than this to the limit gives way to it: their
/// labels would overlap.
const NEAR_LIMIT: f64 = 10.0;
/// Room for the dot at the right end and for the line at the top and the
/// bottom.
const DOT: f64 = 4.5;
const RING: f64 = 2.5;
const PAD_Y: f64 = DOT + RING;
/// Between the labels of the scale and the plot.
const LABEL_GAP: f64 = 8.0;
/// Between the plot and the labels of time under it.
const TIME_GAP: f64 = 4.0;
/// The least height of a plot that fills the height it gets.
const FILL_LEAST: i32 = 72;

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

fn range(points: &Points) -> Option<(f64, f64)> {
    let mut values = points.iter().map(|&(_, v)| v);
    let first = values.next()?;
    Some(values.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
}

/// The lowest and highest value for a screen reader: "63–70 °C", or one
/// value when they round to the same degree.
fn range_text(points: &Points) -> String {
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

/// The usual time between two points, in seconds; 1 s with one point.
fn usual_step(points: &Points) -> f64 {
    let mut steps: Vec<f64> = points.windows(2).map(|w| w[0].0 - w[1].0).collect();
    steps.sort_by(f64::total_cmp);
    steps.get(steps.len() / 2).copied().unwrap_or(1.0)
}

/// How much history there is (D80), the title of the card's fact with the
/// range and the start of the description: "Last 30 s", "Last minute",
/// "Last 2 minutes" … "Last 5 minutes", by the time from the step before
/// the oldest point to the newest one.
pub fn period(points: &Points) -> String {
    let covered = points
        .first()
        .map_or(0.0, |&(age, _)| age + usual_step(points));
    let seconds = (covered.round().max(1.0) as u64).min(SPAN.as_secs());
    if seconds < 60 {
        // Translators: the title of the range of a temperature on a card
        // while the app has read it for less than a minute, e.g.
        // "Last 30 s"; {} is the number of seconds.
        return ngettext("Last {} s", "Last {} s", seconds as u32)
            .replace("{}", &seconds.to_string());
    }
    if seconds < 120 {
        // Translators: the title of the range of a temperature on a card
        // while the app has read it for one to two minutes.
        return gettext("Last minute");
    }
    let minutes = seconds / 60;
    // Translators: the title of the range of a temperature on a card, e.g.
    // "Last 2 minutes", up to "Last 5 minutes", the whole history.
    ngettext("Last {} minute", "Last {} minutes", minutes as u32)
        .replace("{}", &minutes.to_string())
}

/// The points between gaps in the samples: a gap is more than three of
/// the usual steps between samples.
fn pieces(points: &Points) -> Vec<&[(f64, f64)]> {
    let usual = usual_step(points);
    let mut out = Vec::new();
    let mut start = 0;
    for (i, pair) in points.windows(2).enumerate() {
        if pair[0].0 - pair[1].0 > 3.0 * usual + 0.5 {
            out.push(&points[start..=i]);
            start = i + 1;
        }
    }
    if start < points.len() {
        out.push(&points[start..]);
    }
    out
}

/// What a screen reader says for the chart (D80): how much history there
/// is with its range, the value now and where it is going, the gaps when
/// there are any, and the limit in words.
pub fn describe(points: &Points, trend: Trend, limit: Option<&str>) -> String {
    let Some(&(_, now)) = points.last() else {
        // Translators: read by screen readers for the history of a
        // temperature before its first reading.
        return gettext("No readings yet");
    };
    let text = match trend {
        // Translators: read by screen readers for the history of a
        // temperature; {period} is e.g. "Last 5 minutes" or "Last 30 s",
        // {range} e.g. "63–70 °C", {value} e.g. "68 °C".
        Trend::Rising => gettext("{period}: {range}. Now {value}, rising."),
        Trend::Falling => gettext("{period}: {range}. Now {value}, falling."),
        Trend::Steady => gettext("{period}: {range}. Now {value}, steady."),
    };
    let mut text = text
        .replace("{period}", &period(points))
        .replace("{range}", &range_text(points))
        .replace("{value}", &units::degrees(now));
    if pieces(points).len() > 1 {
        // Translators: read by screen readers after the history of a
        // temperature that has gaps (the window was hidden, or the computer
        // slept).
        text = format!("{text} {}", gettext("With gaps in the readings."));
    }
    if let Some(limit) = limit {
        text = format!("{text} {limit}");
    }
    text
}

/// The bottom and the top of a scale, in °C.
pub type Scale = (f64, f64);

/// The one scale of every card (D80), for the `values` of all their
/// histories: 30–100 °C, lower or higher in whole 10 °C when a reading
/// needs it, so that the lines of cards side by side match.
pub fn scale(values: impl IntoIterator<Item = f64>) -> Scale {
    let (lo, hi) = values
        .into_iter()
        .fold((BOTTOM, TOP), |(lo, hi), v| (lo.min(v), hi.max(v)));
    ((lo / STEP).floor() * STEP, (hi / STEP).ceil() * STEP)
}

/// The limit when it is inside the scale; outside it there is no line.
fn limit_in((bottom, top): Scale, limit: Option<f64>) -> Option<f64> {
    limit.filter(|l| (bottom..=top).contains(l))
}

/// The lines of the grid inside the scale, without those that give way to
/// the limit.
fn grid((bottom, top): Scale, limit: Option<f64>) -> Vec<f64> {
    GRID.into_iter()
        .filter(|g| *g > bottom && *g < top)
        .filter(|g| limit.is_none_or(|l| (l - g).abs() >= NEAR_LIMIT))
        .collect()
}

/// The labels of time under the line.
fn time_labels() -> (String, String) {
    (
        // Translators: under the left end of the history of a temperature,
        // which always spans five minutes.
        gettext("5 min ago"),
        // Translators: under the right end of the history of a
        // temperature, below the dot of the newest reading.
        gettext("now"),
    )
}

/// Where the parts of the chart stand, in pixels.
struct Frame {
    /// The left end of the plot, right of the labels of the scale.
    left: f64,
    right: f64,
    /// The height of the plot; the labels of time stand under it.
    height: f64,
    scale: Scale,
}

impl Frame {
    fn x(&self, age: f64) -> f64 {
        self.left + (self.right - self.left - DOT - RING) * (1.0 - age / SPAN.as_secs_f64())
    }

    fn y(&self, value: f64) -> f64 {
        let (bottom, top) = self.scale;
        PAD_Y + (top - value) / (top - bottom) * (self.height - 2.0 * PAD_Y)
    }
}

/// The colors of one style.
struct Colors {
    text: gdk::RGBA,
    grid: gdk::RGBA,
    limit: gdk::RGBA,
    limit_text: gdk::RGBA,
    line: gdk::RGBA,
    surface: gdk::RGBA,
}

/// Draws `layout` with its top left corner at (`x`, `top`).
fn put(snapshot: &gtk::Snapshot, layout: &pango::Layout, x: f64, top: f64, color: &gdk::RGBA) {
    snapshot.save();
    snapshot.translate(&point(x, top));
    snapshot.append_layout(layout, color);
    snapshot.restore();
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct HistoryChart {
        pub(super) points: RefCell<Points>,
        pub(super) hot: Cell<bool>,
        pub(super) limit: Cell<Option<f64>>,
        /// The scale of every card; `None` before the first history.
        pub(super) scale: Cell<Option<Scale>>,
        /// The height of the plot; `None` fills the height the chart gets.
        pub(super) height: Cell<Option<i32>>,
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
        /// Its plot and the labels of time under it; a chart that fills
        /// its place asks for a small plot only.
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            match orientation {
                gtk::Orientation::Horizontal => (0, 0, -1, -1),
                _ => {
                    let plot = self.height.get().unwrap_or(FILL_LEAST);
                    let height = plot + self.obj().time_height().ceil() as i32;
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
        glib::Object::builder().property("hexpand", true).build()
    }

    /// The height of the plot, or `None` to fill the height the chart gets
    /// (D80: beside the temperature and the facts in a card of one row).
    pub fn set_height(&self, height: Option<i32>) {
        if self.imp().height.replace(height) != height {
            self.queue_resize();
        }
    }

    /// The history to draw, whether the part is close to its limit, the
    /// limit in °C and the scale of every card.
    pub fn set_view(&self, points: Points, hot: bool, limit: Option<f64>, scale: Scale) {
        let imp = self.imp();
        let changed = *imp.points.borrow() != points
            || imp.hot.get() != hot
            || imp.limit.get() != limit
            || imp.scale.get() != Some(scale);
        if changed {
            imp.points.replace(points);
            imp.hot.set(hot);
            imp.limit.set(limit);
            imp.scale.set(Some(scale));
            self.queue_draw();
        }
    }

    /// The labels of time and the room above them.
    fn time_height(&self) -> f64 {
        let (_, now) = time_labels();
        TIME_GAP + size_of(&layout(self.upcast_ref(), &now, TEXT_SCALE, false)).1
    }

    fn colors(&self) -> Colors {
        let style = adw::StyleManager::default();
        let dark = style.is_dark();
        let contrast = style.is_high_contrast();
        let hex = |light: &str, dark_value: &str| {
            gdk::RGBA::parse(if dark { dark_value } else { light }).unwrap_or(gdk::RGBA::BLACK)
        };
        let fg = self.color();
        let warning = hex("#e5a50a", "#cd9309");
        Colors {
            text: alpha(&fg, if contrast { 1.0 } else { 0.72 }),
            grid: alpha(&fg, if contrast { 0.25 } else { 0.1 }),
            limit: alpha(&warning, 0.8),
            limit_text: hex("#9c6e03", "#f8e45c"),
            line: if self.imp().hot.get() {
                warning
            } else {
                style.accent_color_rgba()
            },
            surface: hex("#ffffff", "#38383c"),
        }
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let points = imp.points.borrow();
        let widget = self.upcast_ref::<gtk::Widget>();
        let colors = self.colors();
        let scale = imp.scale.get().unwrap_or((BOTTOM, TOP));
        let limit = limit_in(scale, imp.limit.get());
        let label = |value: f64| layout(widget, &units::degrees(value), TEXT_SCALE, false);
        // As wide as the widest label of the scale, at least "100 °C", so
        // that the plots of cards side by side line up.
        let margin = [TOP, scale.1]
            .into_iter()
            .chain(limit)
            .map(|value| size_of(&label(value)).0)
            .fold(0.0, f64::max);
        let frame = Frame {
            left: margin + LABEL_GAP,
            right: f64::from(self.width()),
            height: f64::from(self.height()) - self.time_height(),
            scale,
        };
        let grid_stroke = stroke(1.0, &[], false);
        for value in grid(scale, limit) {
            let y = frame.y(value);
            line(
                snapshot,
                (frame.left, y),
                (frame.right, y),
                &colors.grid,
                &grid_stroke,
            );
            let text = label(value);
            let (w, h) = size_of(&text);
            put(snapshot, &text, margin - w, y - h / 2.0, &colors.text);
        }
        if let Some(value) = limit {
            let y = frame.y(value);
            let dashed = stroke(1.25, &[4.0, 4.0], false);
            line(
                snapshot,
                (frame.left, y),
                (frame.right, y),
                &colors.limit,
                &dashed,
            );
            let text = label(value);
            let (w, h) = size_of(&text);
            put(snapshot, &text, margin - w, y - h / 2.0, &colors.limit_text);
        }
        draw_line(snapshot, &frame, &points, &colors);
        let (ago, now) = time_labels();
        let top = frame.height + TIME_GAP;
        put(
            snapshot,
            &layout(widget, &ago, TEXT_SCALE, false),
            frame.left,
            top,
            &colors.text,
        );
        let now = layout(widget, &now, TEXT_SCALE, false);
        put(
            snapshot,
            &now,
            frame.right - size_of(&now).0,
            top,
            &colors.text,
        );
    }
}

/// The line in its pieces with the fill under them, and the dot.
fn draw_line(snapshot: &gtk::Snapshot, frame: &Frame, points: &Points, colors: &Colors) {
    let bottom = frame.height;
    let line_stroke = stroke(2.25, &[], true);
    for piece in pieces(points) {
        let at: Vec<(f64, f64)> = piece
            .iter()
            .map(|&(age, v)| (frame.x(age), frame.y(v)))
            .collect();
        let (Some(&(x0, _)), Some(&(x1, _))) = (at.first(), at.last()) else {
            continue;
        };
        let mut area = vec![(x0, bottom)];
        area.extend(&at);
        area.push((x1, bottom));
        snapshot.push_fill(&polyline(&area), gsk::FillRule::Winding);
        snapshot.append_linear_gradient(
            &gtk::graphene::Rect::new(0.0, 0.0, frame.right as f32, bottom as f32),
            &point(0.0, PAD_Y),
            &point(0.0, bottom),
            &[
                gsk::ColorStop::new(0.0, alpha(&colors.line, 0.16)),
                gsk::ColorStop::new(1.0, alpha(&colors.line, 0.0)),
            ],
        );
        snapshot.pop();
        snapshot.append_stroke(&polyline(&at), &line_stroke, &colors.line);
    }
    if let Some(&(age, value)) = points.last() {
        let center = (frame.x(age), frame.y(value));
        let fill = gsk::FillRule::Winding;
        snapshot.append_fill(&circle(center, DOT + RING), fill, &colors.surface);
        snapshot.append_fill(&circle(center, DOT), fill, &colors.line);
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

    /// One reading a second for `seconds` seconds, at `value`.
    fn steady(seconds: u64, value: f64) -> Points {
        let values: Vec<(u64, f64)> = (0..seconds).map(|s| (s, value)).collect();
        points(&history(&values))
    }

    /// The description: how much history with its range, or one value
    /// when the readings round to one degree; the value now, the trend,
    /// the gaps and the limit.
    #[test]
    fn words_for_a_history() {
        let rising = history(&[(0, 63.2), (1, 64.0), (2, 66.0), (3, 69.8)]);
        let points = points(&rising);
        assert_eq!(points.first(), Some(&(3.0, 63.2)));
        assert_eq!(range_text(&points), "63–70\u{a0}°C");
        assert_eq!(
            describe(&points, rising.trend(), None),
            "Last 4 s: 63–70\u{a0}°C. Now 70\u{a0}°C, rising."
        );
        let quiet = super::points(&history(&[(0, 30.2), (1, 30.4)]));
        assert_eq!(range_text(&quiet), "30\u{a0}°C");
        assert_eq!(
            describe(&Vec::new(), Trend::Steady, None),
            "No readings yet"
        );
        let gaps = super::points(&history(&[(0, 50.0), (1, 50.0), (60, 51.0), (61, 51.0)]));
        assert_eq!(
            describe(&gaps, Trend::Steady, Some("The limit is 80\u{a0}°C.")),
            "Last minute: 50–51\u{a0}°C. Now 51\u{a0}°C, steady. \
             With gaps in the readings. The limit is 80\u{a0}°C."
        );
    }

    /// The title of the range says how much history there is, in seconds
    /// for the first minute, then in whole minutes up to five.
    #[test]
    fn how_much_history() {
        assert_eq!(period(&steady(1, 40.0)), "Last 1 s");
        assert_eq!(period(&steady(30, 40.0)), "Last 30 s");
        assert_eq!(period(&steady(60, 40.0)), "Last minute");
        assert_eq!(period(&steady(119, 40.0)), "Last minute");
        assert_eq!(period(&steady(150, 40.0)), "Last 2 minutes");
        assert_eq!(period(&steady(299, 40.0)), "Last 4 minutes");
        // Full: the next older reading is gone.
        assert_eq!(period(&steady(400, 40.0)), "Last 5 minutes");
    }

    /// One scale for every card: 30–100 °C, lower or higher in whole 10 °C
    /// when a reading of any card needs it; the limit only inside it.
    #[test]
    fn scale_of_every_card() {
        assert_eq!(scale([45.0, 50.0]), (30.0, 100.0));
        assert_eq!(scale([72.0, 88.0]), (30.0, 100.0));
        // A graphics card resting at 29 °C lowers the scale of all cards.
        assert_eq!(scale([41.0, 42.0, 29.0, 38.0]), (20.0, 100.0));
        assert_eq!(scale([90.0, 104.0]), (30.0, 110.0));
        assert_eq!(scale([]), (30.0, 100.0));
        assert_eq!(limit_in((30.0, 100.0), Some(95.0)), Some(95.0));
        assert_eq!(limit_in((30.0, 100.0), Some(100.0)), Some(100.0));
        assert_eq!(limit_in((30.0, 100.0), Some(105.0)), None);
        assert_eq!(limit_in((30.0, 100.0), None), None);
    }

    /// The lines at 40, 60 and 80 °C; one that the limit falls on, or near
    /// enough for their labels to overlap, gives way to it.
    #[test]
    fn grid_and_limit() {
        let normal = (30.0, 100.0);
        assert_eq!(grid(normal, None), [40.0, 60.0, 80.0]);
        assert_eq!(grid(normal, Some(95.0)), [40.0, 60.0, 80.0]);
        assert_eq!(grid(normal, Some(100.0)), [40.0, 60.0, 80.0]);
        assert_eq!(grid(normal, Some(80.0)), [40.0, 60.0]);
        assert_eq!(grid(normal, Some(85.0)), [40.0, 60.0]);
        assert_eq!(grid((40.0, 100.0), None), [60.0, 80.0]);
    }

    /// Time runs from the left end (five minutes ago) to the right end
    /// (now), so a short history stands on the right; a gap in the samples
    /// starts a new piece of the line.
    #[test]
    fn pieces_of_the_line() {
        let mut values: Vec<(u64, f64)> = (0..60).map(|s| (s, 40.0)).collect();
        values.extend((200..260).map(|s| (s, 41.0)));
        let points = points(&history(&values));
        let pieces = pieces(&points);
        assert_eq!(pieces.len(), 2);
        let frame = Frame {
            left: 50.0,
            right: 350.0 + DOT + RING,
            height: 100.0,
            scale: (30.0, 100.0),
        };
        let &(age, value) = pieces[1].last().unwrap();
        assert!((frame.x(age) - 350.0).abs() < 1e-9);
        // 41 °C on 30–100 °C: in the lower sixth of the plot.
        let y = frame.y(value);
        assert!(y > 100.0 - PAD_Y - 86.0 / 6.0 && y < 100.0 - PAD_Y, "{y}");
        // The oldest point, 259 s back, near the left end.
        assert!(frame.x(pieces[0][0].0) < 50.0 + 300.0 * (1.0 - 259.0 / 300.0) + 1e-9);
        // Thirty seconds of history stand in the right tenth.
        let short = steady(30, 40.0);
        assert!(frame.x(short[0].0) > 50.0 + 300.0 * 0.9 - 1.0);
    }
}
