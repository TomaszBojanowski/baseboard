// SPDX-License-Identifier: GPL-3.0-or-later

//! The fan curve chart (docs/PROJEKT.md §5.5.4, §7 M2, M2.4): a widget
//! that draws a curve of the board's controller with its grid and axes,
//! the zone below the minimum duty cycle, the emergency threshold of the
//! source and the operating point, live, on the page "Curve Details".
//!
//! - The curve is straight segments, because the controller interpolates
//!   linearly (`core::fans::curve`); outside the end points it is dashed.
//! - The operating point shows the duty cycle the driver reports now; when
//!   the controller is still moving to the target of the curve, an open
//!   ring marks the target, joined by a dashed line.
//! - Grid, axes, zones and the curve are one render node, kept until the
//!   size, the curve or the style changes; a new reading draws only the
//!   operating point, and an unchanged one draws nothing.
//! - The point moves smoothly (`AdwTimedAnimation`, so not at all with
//!   animations turned off) when it moves more than 2 px.
//! - Colors: the curve and its points in the system's accent color, the
//!   operating point in the green of the app icon, the threshold in the
//!   error color of libadwaita; zones also have a pattern or a dashed line
//!   and a text, so nothing depends on color alone. The chart draws its
//!   own shapes, so it cannot read colors from CSS: these are the values
//!   of libadwaita 1.10 for the light and the dark style.
//! - For the screen reader the chart is an image with a description: the
//!   points, the temperature and duty cycle now, the target, the minimum
//!   and the threshold.

mod draw;
mod geometry;
pub mod swatch;

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::fans::curve;
use baseboard_core::fans::driver::CurvePoint;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{self, Numbers, duty_to_percent};
use gettextrs::gettext;
use gtk::{gdk, glib, gsk};

use draw::{
    Palette, TEXT_SCALE, alpha, circle, draw_area_below, draw_axis_labels, draw_curve_line,
    draw_dot, draw_full_at, draw_grid, draw_guides, draw_text, draw_zone_texts, draw_zones, layout,
    line, rect, rounded, size_of, stroke,
};
use geometry::{Plot, Rect};

/// What the chart draws apart from the live reading.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartCurve {
    /// The fan's name, for the screen reader.
    pub name: String,
    /// The name of the source temperature, as on the Overview.
    pub source: String,
    pub points: Vec<CurvePoint>,
    /// The minimum duty cycle in percent (B5); 0 without one.
    pub min_percent: u8,
    /// The emergency threshold of the source in °C (B4).
    pub threshold: Option<u8>,
    /// The curve must reach 100 % by the threshold (B14: CPU and pump).
    pub needs_full: bool,
    /// The curve sets the duty cycle now. In manual mode it does not: the
    /// chart shows the curve the controller keeps, without a target.
    pub in_effect: bool,
}

/// The live reading: the source temperature and the duty cycle the driver
/// reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub millidegrees: i64,
    pub duty: u8,
}

/// The operating point as drawn: temperature in °C, duty cycle and target
/// 0–255. It moves from one reading to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shown {
    temp: f64,
    duty: f64,
    target: f64,
}

impl Shown {
    fn between(from: Self, to: Self, t: f64) -> Self {
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Self {
            temp: mix(from.temp, to.temp),
            duty: mix(from.duty, to.duty),
            target: mix(from.target, to.target),
        }
    }
}

/// Told whether the large chart labels the operating point.
type NowLabelCallback = Box<dyn Fn(bool)>;

/// Moves shorter than this, in pixels, jump instead of moving smoothly.
const SMALLEST_MOVE: f64 = 2.0;
const MOVE_MS: u32 = 250;

/// The static part of the chart for one size of the widget, in two layers:
/// the guides of the operating point go between them, its dot on top.
struct Cache {
    width: i32,
    height: i32,
    generation: u64,
    /// Grid, axes, zones, the curve.
    back: Option<gsk::RenderNode>,
    /// The points of the curve.
    front: Option<gsk::RenderNode>,
    /// Places the label of the operating point must keep clear of.
    obstacles: Vec<Rect>,
}

/// The description for the screen reader: whole sentences, each translated
/// on its own (§9).
pub fn describe(curve: &ChartCurve, reading: Option<Reading>, numbers: &Numbers) -> String {
    let mut sentences = vec![
        // Translators: {source} is a temperature, e.g. "Processor (board sensor)".
        gettext("Temperature source: {source}.").replace("{source}", &curve.source),
    ];
    for (first, last) in curve::runs(&curve.points) {
        let p = curve.points[first];
        let text = if first == last {
            // Translators: a point of a fan curve, e.g. "Point 3: 60 °C, 35 %."
            gettext("Point {number}: {temperature}, {duty}.")
                .replace("{number}", &(first + 1).to_string())
        } else {
            // The same words as the list of points on the curve page.
            gettext("Points {first} to {last}: {temperature}, {duty}.")
                .replace("{first}", &(first + 1).to_string())
                .replace("{last}", &(last + 1).to_string())
        };
        sentences.push(
            text.replace("{temperature}", &units::degrees(f64::from(p.temp)))
                .replace("{duty}", &units::percent(duty_to_percent(p.duty))),
        );
    }
    if let Some(reading) = reading {
        let temp = numbers.value(Kind::Temp, reading.millidegrees as f64 / 1000.0);
        sentences.push(
            // Translators: the source temperature and the fan's duty cycle
            // now, e.g. "Now 46.0 °C, duty cycle 38 %."
            gettext("Now {temperature}, duty cycle {duty}.")
                .replace("{temperature}", &temp)
                .replace("{duty}", &units::percent(duty_to_percent(reading.duty))),
        );
        if let Some(target) = curve::expected_duty(&curve.points, reading.millidegrees)
            .filter(|_| curve.in_effect)
            .filter(|&t| duty_to_percent(t) != duty_to_percent(reading.duty))
        {
            sentences.push(
                // Translators: the controller changes the duty cycle
                // gradually; {duty} is where it is heading, e.g. "40 %".
                gettext("The fan is moving to {duty}.")
                    .replace("{duty}", &units::percent(duty_to_percent(target))),
            );
        }
    }
    if curve.min_percent > 0 {
        sentences.push(
            // Translators: the lowest duty cycle allowed for this fan, e.g. "30 %".
            gettext("Minimum duty cycle: {duty}.")
                .replace("{duty}", &units::percent(curve.min_percent)),
        );
    }
    if let Some(threshold) = curve.threshold {
        sentences.push(
            // Translators: the emergency temperature of the source, e.g. "95 °C".
            gettext("Emergency threshold: {temperature}.")
                .replace("{temperature}", &units::degrees(f64::from(threshold))),
        );
    }
    sentences.join(" ")
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FanChart {
        pub(super) curve: RefCell<Option<ChartCurve>>,
        pub(super) reading: Cell<Option<Reading>>,
        pub(super) shown: Cell<Option<Shown>>,
        pub(super) from: Cell<Option<Shown>>,
        pub(super) to: Cell<Option<Shown>>,
        pub(super) animation: OnceCell<adw::TimedAnimation>,
        pub(super) cache: RefCell<Option<Cache>>,
        /// Grows when the curve or the style changes.
        pub(super) generation: Cell<u64>,
        pub(super) description: RefCell<String>,
        pub(super) numbers: OnceCell<Numbers>,
        pub(super) style_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        /// The font and the text size, whose changes draw the texts again.
        pub(super) font_handlers: RefCell<Vec<glib::SignalHandlerId>>,
        /// Whether the large chart labelled the operating point last time,
        /// and who wants to know.
        pub(super) now_label: Cell<Option<bool>>,
        pub(super) on_now_label: RefCell<Option<NowLabelCallback>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FanChart {
        const NAME: &'static str = "BaseboardFanChart";
        type Type = super::FanChart;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("fan-chart");
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for FanChart {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            let style = adw::StyleManager::default();
            let restyle = |obj: &super::FanChart| {
                let obj = obj.downgrade();
                move |_: &adw::StyleManager| {
                    if let Some(obj) = obj.upgrade() {
                        obj.invalidate();
                    }
                }
            };
            *self.style_handlers.borrow_mut() = vec![
                style.connect_dark_notify(restyle(&obj)),
                style.connect_high_contrast_notify(restyle(&obj)),
                style.connect_accent_color_rgba_notify(restyle(&obj)),
            ];
            if let Some(settings) = gtk::Settings::default() {
                *self.font_handlers.borrow_mut() = ["gtk-font-name", "gtk-xft-dpi"]
                    .into_iter()
                    .map(|name| {
                        let obj = obj.downgrade();
                        settings.connect_notify_local(Some(name), move |_, _| {
                            if let Some(obj) = obj.upgrade() {
                                obj.invalidate();
                            }
                        })
                    })
                    .collect();
            }
            let weak = obj.downgrade();
            let target = adw::CallbackAnimationTarget::new(move |value| {
                if let Some(obj) = weak.upgrade() {
                    obj.step(value);
                }
            });
            let animation = adw::TimedAnimation::new(&*obj, 0.0, 1.0, MOVE_MS, target);
            animation.set_easing(adw::Easing::EaseOutCubic);
            let _ = self.animation.set(animation);
        }

        fn dispose(&self) {
            let style = adw::StyleManager::default();
            for handler in self.style_handlers.take() {
                style.disconnect(handler);
            }
            if let Some(settings) = gtk::Settings::default() {
                for handler in self.font_handlers.take() {
                    settings.disconnect(handler);
                }
            }
        }
    }

    impl WidgetImpl for FanChart {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (min, natural) = match orientation {
                gtk::Orientation::Horizontal => (300, 900),
                // The chart keeps the shape of the mockups. Its minimum is
                // the same, because a scrolled page gives its children
                // their minimum height.
                _ if for_size > 0 => {
                    let height = (f64::from(for_size) * 0.43).clamp(280.0, 390.0) as i32;
                    (height, height)
                }
                _ => (280, 390),
            };
            (min, natural, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct FanChart(ObjectSubclass<imp::FanChart>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FanChart {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The curve to draw, or nothing.
    pub fn set_curve(&self, curve: Option<ChartCurve>) {
        if *self.imp().curve.borrow() == curve {
            return;
        }
        if let Some(curve) = &curve {
            // Translators: the name of the chart for the screen reader;
            // {name} is a fan, e.g. "CPU fan 1".
            let label = gettext("Curve of {name}").replace("{name}", &curve.name);
            self.update_property(&[gtk::accessible::Property::Label(&label)]);
        }
        self.imp().curve.replace(curve);
        // The target depends on the curve.
        let reading = self.imp().reading.get();
        self.imp().reading.set(None);
        self.imp().shown.set(None);
        self.invalidate();
        self.set_reading(reading);
    }

    /// The live reading. The same reading again draws nothing; a new one
    /// moves the operating point.
    pub fn set_reading(&self, reading: Option<Reading>) {
        let imp = self.imp();
        if imp.reading.get() == reading && (reading.is_none() || imp.shown.get().is_some()) {
            return;
        }
        imp.reading.set(reading);
        self.update_description();
        let next = reading.and_then(|r| {
            let curve = imp.curve.borrow();
            let curve = curve.as_ref()?;
            // Without a curve in effect the target is the duty cycle itself:
            // nothing to move to.
            let target = curve::expected_duty(&curve.points, r.millidegrees)
                .filter(|_| curve.in_effect)
                .unwrap_or(r.duty);
            Some(Shown {
                temp: r.millidegrees as f64 / 1000.0,
                duty: f64::from(r.duty),
                target: f64::from(target),
            })
        });
        let animation = imp.animation.get();
        match (imp.shown.get(), next) {
            // Each smooth move redraws the window 30 times at 120 Hz, so
            // only a far one is smooth (D57).
            (Some(now), Some(next)) if self.is_mapped() && self.moves_far(now, next) => {
                imp.from.set(Some(now));
                imp.to.set(Some(next));
                if let Some(animation) = animation {
                    animation.reset();
                    animation.play();
                }
            }
            _ => {
                if let Some(animation) = animation {
                    animation.pause();
                }
                imp.shown.set(next);
                self.queue_draw();
            }
        }
    }

    /// One step of the move between two readings.
    fn step(&self, value: f64) {
        let imp = self.imp();
        if let (Some(from), Some(to)) = (imp.from.get(), imp.to.get()) {
            imp.shown.set(Some(Shown::between(from, to, value)));
            self.queue_draw();
        }
    }

    fn moves_far(&self, from: Shown, to: Shown) -> bool {
        let plot = Plot::new(f64::from(self.width()), f64::from(self.height()));
        let dx = (plot.x(from.temp) - plot.x(to.temp)).abs();
        let dy = (plot.y(from.duty) - plot.y(to.duty)).abs();
        let dt = (plot.y(from.target) - plot.y(to.target)).abs();
        dx.max(dy).max(dt) > SMALLEST_MOVE
    }

    /// The static part is drawn again: new curve or new style.
    fn invalidate(&self) {
        let imp = self.imp();
        imp.generation.set(imp.generation.get() + 1);
        self.queue_draw();
    }

    fn numbers(&self) -> &Numbers {
        self.imp()
            .numbers
            .get_or_init(|| Numbers::new(&crate::i18n::decimal_separator()))
    }

    /// The description changes only when its text does, so a screen
    /// reader is not told about every second.
    fn update_description(&self) {
        let imp = self.imp();
        let text = match imp.curve.borrow().as_ref() {
            Some(curve) => describe(curve, imp.reading.get(), self.numbers()),
            None => String::new(),
        };
        if *imp.description.borrow() != text {
            self.update_property(&[gtk::accessible::Property::Description(&text)]);
            imp.description.replace(text);
        }
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let (width, height) = (self.width(), self.height());
        if imp.curve.borrow().is_none() || width <= 0 || height <= 0 {
            return;
        }
        let palette = Palette::of(self.upcast_ref());
        let plot = Plot::new(f64::from(width), f64::from(height));
        let fresh = imp.cache.borrow().as_ref().is_some_and(|c| {
            c.width == width && c.height == height && c.generation == imp.generation.get()
        });
        if !fresh {
            let back = gtk::Snapshot::new();
            let front = gtk::Snapshot::new();
            let obstacles = self.draw_static(&back, &front, &plot, &palette);
            imp.cache.replace(Some(Cache {
                width,
                height,
                generation: imp.generation.get(),
                back: back.to_node(),
                front: front.to_node(),
                obstacles,
            }));
        }
        let cache = imp.cache.borrow();
        let Some(cache) = cache.as_ref() else {
            return;
        };
        let shown = imp.shown.get();
        let marker = shown.map(|s| geometry::marker(&plot, s.temp, s.duty, Some(s.target)));
        if let Some(node) = &cache.back {
            snapshot.append_node(node);
        }
        if let Some(marker) = &marker {
            draw_guides(snapshot, &plot, &palette, marker);
        }
        if let Some(node) = &cache.front {
            snapshot.append_node(node);
        }
        let mut labelled = false;
        if let (Some(shown), Some(marker)) = (shown, &marker) {
            draw_dot(snapshot, &palette, marker);
            labelled =
                self.draw_now_label(snapshot, &plot, &palette, shown, marker, &cache.obstacles);
        }
        if imp.now_label.replace(Some(labelled)) != Some(labelled) {
            // Told after drawing, as the legend changes the layout.
            let chart = self.downgrade();
            glib::idle_add_local_once(move || {
                if let Some(chart) = chart.upgrade()
                    && let Some(f) = chart.imp().on_now_label.borrow().as_ref()
                {
                    f(labelled);
                }
            });
        }
    }

    /// Calls `f` whenever the large chart starts or stops labelling the
    /// operating point (a narrow chart has no room); the page then shows
    /// the values in a legend.
    pub fn connect_now_label(&self, f: impl Fn(bool) + 'static) {
        self.imp().on_now_label.replace(Some(Box::new(f)));
    }

    /// Grid, axes, zones, the curve and its points. Returns the places of
    /// the texts, which the label of the operating point keeps clear of.
    fn draw_static(
        &self,
        snapshot: &gtk::Snapshot,
        front: &gtk::Snapshot,
        plot: &Plot,
        palette: &Palette,
    ) -> Vec<Rect> {
        let widget: &gtk::Widget = self.upcast_ref();
        let curve = self.imp().curve.borrow();
        let Some(curve) = curve.as_ref() else {
            return Vec::new();
        };
        draw_grid(snapshot, plot, palette);
        draw_axis_labels(widget, snapshot, plot, palette, &curve.source);
        draw_zones(snapshot, plot, palette, curve);
        let shape = geometry::curve_shape(plot, &curve.points);
        if let Some(shape) = &shape {
            draw_area_below(snapshot, plot, palette, shape);
        }
        // Texts of the zones, below the curve line so that they never hide
        // it.
        let obstacles = draw_zone_texts(widget, snapshot, plot, palette, curve);
        if let Some(shape) = &shape {
            draw_curve_line(snapshot, palette, shape);
        }
        draw_full_at(snapshot, plot, palette, curve);
        // The points: a dot for each run of equal points.
        for h in &geometry::handles(plot, &curve.points) {
            let dot = circle((h.x, h.y), 4.5);
            front.append_fill(&dot, gsk::FillRule::Winding, &palette.accent);
            front.append_stroke(&dot, &stroke(2.0, &[], false), &palette.surface);
        }
        obstacles
    }

    /// "Now 46.0 °C · 38 %", and "target 40 %" while the fan moves to it,
    /// where it covers nothing; without a free place, no label (the
    /// description and the legend of the page have the values). Says
    /// whether it drew the label.
    fn draw_now_label(
        &self,
        snapshot: &gtk::Snapshot,
        plot: &Plot,
        palette: &Palette,
        shown: Shown,
        marker: &geometry::Marker,
        obstacles: &[Rect],
    ) -> bool {
        let widget: &gtk::Widget = self.upcast_ref();
        let (x, y) = (marker.x, marker.y);
        let moving = marker.target.is_some();
        let mut obstacles = obstacles.to_vec();
        obstacles.push(Rect {
            x: x - 14.0,
            y: y - 14.0,
            width: 28.0,
            height: 28.0,
        });
        if let Some(ty) = marker.target {
            obstacles.push(Rect {
                x: x - 10.0,
                y: ty - 10.0,
                width: 20.0,
                height: 20.0,
            });
        }
        let Some(reading) = self.imp().reading.get() else {
            return false;
        };
        let numbers = self.numbers();
        // Translators: the label of the operating point on a fan curve,
        // e.g. "Now 46.0 °C · 38 %".
        let first = gettext("Now {temperature} · {duty}")
            .replace(
                "{temperature}",
                &numbers.value(Kind::Temp, reading.millidegrees as f64 / 1000.0),
            )
            .replace("{duty}", &units::percent(duty_to_percent(reading.duty)));
        let first = layout(widget, &first, TEXT_SCALE, true);
        let second = moving.then(|| {
            let target = shown.target.round().clamp(0.0, 255.0) as u8;
            let percent = units::percent(duty_to_percent(target));
            // Translators: under the label of the operating point, where
            // the fan is heading, e.g. "target 40 %".
            let text = gettext("target {duty}").replace("{duty}", &percent);
            layout(widget, &text, TEXT_SCALE, false)
        });
        // Lines 17 px apart as in the mockups, more with larger text.
        let (w1, h1) = size_of(&first);
        let w2 = second.as_ref().map_or(0.0, |l| size_of(l).0);
        let line_height = h1.max(17.0);
        let lines = if second.is_some() { 2.0 } else { 1.0 };
        let width = w1.max(w2) + 30.0;
        let height = 12.0 + lines * line_height;
        let Some((bx, by)) = geometry::place_label(plot, (x, y), width, height, &obstacles) else {
            return false;
        };
        let r = Rect {
            x: bx,
            y: by,
            width,
            height,
        };
        let path = rounded(&r, 8.0);
        snapshot.append_outset_shadow(
            &gsk::RoundedRect::from_rect(rect(&r), 8.0),
            &gdk::RGBA::new(0.0, 0.0, 0.024, 0.14),
            0.0,
            1.0,
            0.0,
            2.0,
        );
        snapshot.append_fill(&path, gsk::FillRule::Winding, &palette.surface);
        snapshot.append_stroke(&path, &stroke(1.0, &[], false), &alpha(&palette.fg, 0.16));
        let baseline = by + 4.0 + line_height;
        snapshot.append_fill(
            &circle((bx + 13.0, baseline - 4.0), 4.0),
            gsk::FillRule::Winding,
            &palette.live,
        );
        draw_text(snapshot, &first, bx + 23.0, baseline, &palette.fg);
        if let Some(second) = &second {
            draw_text(
                snapshot,
                second,
                bx + 23.0,
                baseline + line_height,
                &alpha(&palette.fg, palette.text),
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys1() -> ChartCurve {
        let mut points: Vec<CurvePoint> = [(0, 94), (5, 96), (84, 99), (85, 255)]
            .iter()
            .map(|&(temp, duty)| CurvePoint { temp, duty })
            .collect();
        points.extend([points[3]; 3]);
        ChartCurve {
            name: "System fan 1".into(),
            source: "Processor (board sensor)".into(),
            points,
            min_percent: 0,
            threshold: Some(95),
            needs_full: false,
            in_effect: true,
        }
    }

    #[test]
    fn description_has_points_reading_and_threshold() {
        let numbers = Numbers::new(".");
        let text = describe(
            &sys1(),
            Some(Reading {
                millidegrees: 46_000,
                duty: 98,
            }),
            &numbers,
        );
        assert_eq!(
            text,
            "Temperature source: Processor (board sensor). Point 1: 0\u{a0}°C, 37\u{a0}%. \
             Point 2: 5\u{a0}°C, 38\u{a0}%. Point 3: 84\u{a0}°C, 39\u{a0}%. \
             Points 4 to 7: 85\u{a0}°C, 100\u{a0}%. Now 46.0\u{a0}°C, duty cycle 38\u{a0}%. \
             Emergency threshold: 95\u{a0}°C."
        );
    }

    #[test]
    fn description_while_the_fan_moves() {
        let numbers = Numbers::new(",");
        let mut curve = sys1();
        curve.min_percent = 30;
        let text = describe(
            &curve,
            Some(Reading {
                millidegrees: 46_000,
                duty: 76,
            }),
            &numbers,
        );
        assert!(
            text.contains("Now 46,0\u{a0}°C, duty cycle 30\u{a0}%."),
            "{text}"
        );
        assert!(text.contains("The fan is moving to 38\u{a0}%."), "{text}");
        assert!(text.contains("Minimum duty cycle: 30\u{a0}%."), "{text}");
        // Without a reading, no sentence about now.
        assert!(!describe(&curve, None, &numbers).contains("Now"));
        // In manual mode the curve sets nothing: no target.
        curve.in_effect = false;
        let reading = Some(Reading {
            millidegrees: 46_000,
            duty: 76,
        });
        assert!(!describe(&curve, reading, &numbers).contains("moving"));
    }

    /// In manual mode the chart shows the fixed duty cycle as the point
    /// now, without a target. Needs a display.
    #[test]
    fn no_target_in_manual_mode() {
        crate::gtk_test::run("no_target_in_manual_mode", || {
            let chart = FanChart::new();
            let reading = Some(Reading {
                millidegrees: 46_000,
                duty: 115,
            });
            chart.set_curve(Some(sys1()));
            chart.set_reading(reading);
            let shown = chart.imp().shown.get().unwrap();
            assert_eq!(shown.target, 98.0);
            let mut manual = sys1();
            manual.in_effect = false;
            chart.set_curve(Some(manual));
            let shown = chart.imp().shown.get().unwrap();
            assert_eq!((shown.duty, shown.target), (115.0, 115.0));
        });
    }

    #[test]
    fn description_of_five_points() {
        let mut curve = sys1();
        curve.points = [(30, 60), (45, 90), (60, 130), (75, 200), (85, 255)]
            .iter()
            .map(|&(temp, duty)| CurvePoint { temp, duty })
            .collect();
        let text = describe(&curve, None, &Numbers::new("."));
        assert!(text.contains("Point 5: 85\u{a0}°C, 100\u{a0}%."), "{text}");
        assert!(
            !text.contains("Point 6") && !text.contains("Points"),
            "{text}"
        );
    }

    #[test]
    fn shown_moves_between_readings() {
        let from = Shown {
            temp: 40.0,
            duty: 76.0,
            target: 98.0,
        };
        let to = Shown {
            temp: 50.0,
            duty: 98.0,
            target: 98.0,
        };
        assert_eq!(Shown::between(from, to, 0.0), from);
        assert_eq!(Shown::between(from, to, 1.0), to);
        assert_eq!(Shown::between(from, to, 0.5).temp, 45.0);
    }
}
