// SPDX-License-Identifier: GPL-3.0-or-later

//! The fan curve chart (docs/PROJEKT.md §5.5.4, §7 M2, M2.4): a widget
//! that draws a curve of the board's controller with its grid and axes,
//! the zone below the minimum duty cycle, the emergency threshold of the
//! source and the operating point, live. Two sizes: in a connector card
//! and large (the future curve editor, without dragging for now).
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
//! - Colors: the curve and its handles in the system's accent color, the
//!   operating point in the green of the app icon, the threshold in the
//!   error color of libadwaita; zones also have a pattern or a dashed line
//!   and a text, so nothing depends on color alone. The chart draws its
//!   own shapes, so it cannot read colors from CSS: these are the values
//!   of libadwaita 1.10 for the light and the dark style.
//! - For the screen reader the chart is an image with a description: the
//!   points, the temperature and duty cycle now, the target, the minimum
//!   and the threshold.

mod geometry;
pub mod swatch;

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::fans::curve;
use baseboard_core::fans::driver::CurvePoint;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{Numbers, UNIT_SPACE, duty_to_percent};
use gettextrs::gettext;
use gtk::{gdk, glib, graphene, gsk, pango};

pub use geometry::Size;
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

/// The colors of one style.
struct Palette {
    fg: gdk::RGBA,
    accent: gdk::RGBA,
    accent_text: gdk::RGBA,
    surface: gdk::RGBA,
    error: gdk::RGBA,
    error_band: gdk::RGBA,
    live: gdk::RGBA,
    /// Opacity of grid lines and of secondary text; higher with high
    /// contrast.
    grid: f32,
    text: f32,
}

impl Palette {
    fn of(widget: &gtk::Widget) -> Self {
        let style = adw::StyleManager::default();
        let dark = style.is_dark();
        let high_contrast = style.is_high_contrast();
        let hex = |light: &str, dark_value: &str| {
            gdk::RGBA::parse(if dark { dark_value } else { light }).unwrap_or(gdk::RGBA::BLACK)
        };
        Self {
            fg: widget.color(),
            accent: style.accent_color_rgba(),
            accent_text: style.accent_color().to_standalone_rgba(dark),
            surface: hex("#ffffff", "#38383c"),
            error: hex("#c30000", "#ff938c"),
            error_band: hex("#e01b24", "#c01c28"),
            // The greens of the app icon.
            live: hex("#26a269", "#33d17a"),
            grid: if high_contrast { 0.25 } else { 0.08 },
            text: if high_contrast { 1.0 } else { 0.72 },
        }
    }
}

fn alpha(color: &gdk::RGBA, factor: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        color.red(),
        color.green(),
        color.blue(),
        color.alpha() * factor,
    )
}

fn point(x: f64, y: f64) -> graphene::Point {
    graphene::Point::new(x as f32, y as f32)
}

fn rect(r: &Rect) -> graphene::Rect {
    graphene::Rect::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32)
}

fn stroke(width: f32, dash: &[f32], round: bool) -> gsk::Stroke {
    let stroke = gsk::Stroke::new(width);
    if round {
        stroke.set_line_join(gsk::LineJoin::Round);
        stroke.set_line_cap(gsk::LineCap::Round);
    }
    if !dash.is_empty() {
        stroke.set_dash(dash);
    }
    stroke
}

fn polyline(points: &[(f64, f64)]) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        if i == 0 {
            builder.move_to(x as f32, y as f32);
        } else {
            builder.line_to(x as f32, y as f32);
        }
    }
    builder.to_path()
}

fn line(
    snapshot: &gtk::Snapshot,
    from: (f64, f64),
    to: (f64, f64),
    color: &gdk::RGBA,
    stroke: &gsk::Stroke,
) {
    snapshot.append_stroke(&polyline(&[from, to]), stroke, color);
}

fn circle(center: (f64, f64), radius: f64) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.add_circle(&point(center.0, center.1), radius as f32);
    builder.to_path()
}

fn rounded(r: &Rect, radius: f64) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.add_rounded_rect(&gsk::RoundedRect::from_rect(rect(r), radius as f32));
    builder.to_path()
}

/// Text in the widget's font, scaled, with tabular figures.
fn layout(widget: &gtk::Widget, text: &str, scale: f64, bold: bool) -> pango::Layout {
    let layout = widget.create_pango_layout(Some(text));
    let attrs = pango::AttrList::new();
    attrs.insert(pango::AttrFloat::new_scale(scale));
    attrs.insert(pango::AttrFontFeatures::new("tnum 1"));
    if bold {
        attrs.insert(pango::AttrInt::new_weight(pango::Weight::Bold));
    }
    layout.set_attributes(Some(&attrs));
    layout
}

fn size_of(layout: &pango::Layout) -> (f64, f64) {
    let (w, h) = layout.pixel_size();
    (f64::from(w), f64::from(h))
}

/// Draws `layout` from `x`, with its baseline at `baseline`, as text is
/// placed in the mockups.
fn draw_text(
    snapshot: &gtk::Snapshot,
    layout: &pango::Layout,
    x: f64,
    baseline: f64,
    color: &gdk::RGBA,
) {
    let ascent = f64::from(layout.baseline()) / f64::from(pango::SCALE);
    snapshot.save();
    snapshot.translate(&point(x, baseline - ascent));
    snapshot.append_layout(layout, color);
    snapshot.restore();
}

fn temp_text(temp: u8) -> String {
    format!("{temp}{UNIT_SPACE}°C")
}

fn percent_text(percent: u8) -> String {
    format!("{percent}{UNIT_SPACE}%")
}

/// Under the points of the curve: the guides of the operating point to the
/// axes, and the target when the controller is still moving to it.
fn draw_guides(
    snapshot: &gtk::Snapshot,
    plot: &Plot,
    palette: &Palette,
    marker: &geometry::Marker,
) {
    let large = plot.size == Size::Large;
    let area = plot.area;
    let (x, y) = (marker.x, marker.y);
    let guide = stroke(1.25, &[2.0, 3.0], false);
    let triangle = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| polyline(&[a, b, c, a]);
    line(snapshot, (x, area.bottom()), (x, y), &palette.live, &guide);
    snapshot.append_fill(
        &triangle(
            (x - 5.0, area.bottom() + 1.0),
            (x, area.bottom() - 6.0),
            (x + 5.0, area.bottom() + 1.0),
        ),
        gsk::FillRule::Winding,
        &palette.live,
    );
    if large {
        line(snapshot, (area.x, y), (x, y), &palette.live, &guide);
        snapshot.append_fill(
            &triangle(
                (area.x - 1.0, y - 5.0),
                (area.x + 6.0, y),
                (area.x - 1.0, y + 5.0),
            ),
            gsk::FillRule::Winding,
            &palette.live,
        );
    }
    if let Some(ty) = marker.target {
        line(
            snapshot,
            (x, y),
            (x, ty),
            &palette.accent,
            &stroke(2.0, &[3.0, 3.0], false),
        );
        let ring = circle((x, ty), if large { 6.0 } else { 4.0 });
        snapshot.append_fill(&ring, gsk::FillRule::Winding, &palette.surface);
        snapshot.append_stroke(&ring, &stroke(2.5, &[], false), &palette.accent);
    }
}

/// Over the points of the curve: the dot of the operating point, with a
/// halo.
fn draw_dot(snapshot: &gtk::Snapshot, plot: &Plot, palette: &Palette, marker: &geometry::Marker) {
    let radius = if plot.size == Size::Large { 6.5 } else { 4.5 };
    let center = (marker.x, marker.y);
    snapshot.append_fill(
        &circle(center, radius * 2.0),
        gsk::FillRule::Winding,
        &alpha(&palette.live, 0.2),
    );
    let dot = circle(center, radius);
    snapshot.append_fill(&dot, gsk::FillRule::Winding, &palette.live);
    snapshot.append_stroke(&dot, &stroke(2.5, &[], false), &palette.surface);
}

/// The description for the screen reader: whole sentences, each translated
/// on its own (§9).
pub fn describe(curve: &ChartCurve, reading: Option<Reading>, numbers: &Numbers) -> String {
    let mut sentences = vec![
        // Translators: {source} is a temperature, e.g. "CPU (board sensor)".
        gettext("Temperature source: {source}.").replace("{source}", &curve.source),
    ];
    for (first, last) in curve::runs(&curve.points) {
        let p = curve.points[first];
        let text = if first == last {
            // Translators: a point of a fan curve, e.g. "Point 3: 60 °C, 35 %."
            gettext("Point {number}: {temperature}, {duty}.")
                .replace("{number}", &(first + 1).to_string())
        } else {
            // Translators: repeated points of a fan curve, e.g.
            // "Points 4–7: 85 °C, 100 %."
            gettext("Points {first}–{last}: {temperature}, {duty}.")
                .replace("{first}", &(first + 1).to_string())
                .replace("{last}", &(last + 1).to_string())
        };
        sentences.push(
            text.replace("{temperature}", &temp_text(p.temp))
                .replace("{duty}", &percent_text(duty_to_percent(p.duty))),
        );
    }
    if let Some(reading) = reading {
        let temp = numbers.value(Kind::Temp, reading.millidegrees as f64 / 1000.0);
        sentences.push(
            // Translators: the source temperature and the fan's duty cycle
            // now, e.g. "Now 46.0 °C, duty cycle 38 %."
            gettext("Now {temperature}, duty cycle {duty}.")
                .replace("{temperature}", &temp)
                .replace("{duty}", &percent_text(duty_to_percent(reading.duty))),
        );
        if let Some(target) = curve::expected_duty(&curve.points, reading.millidegrees)
            .filter(|&t| duty_to_percent(t) != duty_to_percent(reading.duty))
        {
            sentences.push(
                // Translators: the controller changes the duty cycle
                // gradually; {duty} is where it is heading, e.g. "40 %".
                gettext("The fan is moving to {duty}.")
                    .replace("{duty}", &percent_text(duty_to_percent(target))),
            );
        }
    }
    if curve.min_percent > 0 {
        sentences.push(
            // Translators: the lowest duty cycle allowed for this fan, e.g. "30 %".
            gettext("Minimum duty cycle: {duty}.")
                .replace("{duty}", &percent_text(curve.min_percent)),
        );
    }
    if let Some(threshold) = curve.threshold {
        sentences.push(
            // Translators: the emergency temperature of the source, e.g. "95 °C".
            gettext("Emergency threshold: {temperature}.")
                .replace("{temperature}", &temp_text(threshold)),
        );
    }
    sentences.join(" ")
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct FanChart {
        pub(super) size: Cell<Size>,
        /// Numbered handles (the large chart of the editor) instead of dots.
        pub(super) handles: Cell<bool>,
        /// A lower card chart, for narrow windows.
        pub(super) compact: Cell<bool>,
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
            match self.size.get() {
                Size::Card => gtk::SizeRequestMode::ConstantSize,
                Size::Large => gtk::SizeRequestMode::HeightForWidth,
            }
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (min, natural) = match (orientation, self.size.get()) {
                // Two cards side by side in a window of the default size.
                (gtk::Orientation::Horizontal, Size::Card) => (220, 360),
                (gtk::Orientation::Horizontal, _) => (300, 900),
                (_, Size::Card) => {
                    let height = if self.compact.get() { 136 } else { 150 };
                    (height, height)
                }
                // The large chart keeps the shape of the mockups. Its
                // minimum is the same, because a scrolled page gives its
                // children their minimum height.
                (_, _) if for_size > 0 => {
                    let height = (f64::from(for_size) * 0.43).clamp(280.0, 390.0) as i32;
                    (height, height)
                }
                (_, _) => (280, 390),
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
    /// A chart of `size`; `handles` draws numbered handles instead of dots
    /// (the large chart of the editor).
    pub fn new(size: Size, handles: bool) -> Self {
        let chart: Self = glib::Object::new();
        chart.imp().size.set(size);
        chart.imp().handles.set(handles);
        chart
    }

    pub fn size(&self) -> Size {
        self.imp().size.get()
    }

    /// A lower card chart (136 px instead of 150), for narrow windows.
    pub fn set_compact(&self, compact: bool) {
        if self.imp().compact.replace(compact) != compact {
            self.queue_resize();
        }
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
            let points = &curve.as_ref()?.points;
            let target = curve::expected_duty(points, r.millidegrees).unwrap_or(r.duty);
            Some(Shown {
                temp: r.millidegrees as f64 / 1000.0,
                duty: f64::from(r.duty),
                target: f64::from(target),
            })
        });
        let animation = imp.animation.get();
        match (imp.shown.get(), next) {
            // Only the large chart moves smoothly: on a card a degree is
            // 3–4 px, and each move redraws the window 30 times at 120 Hz
            // (D57).
            (Some(now), Some(next))
                if imp.size.get() == Size::Large
                    && self.is_mapped()
                    && self.moves_far(now, next) =>
            {
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
        let plot = Plot::new(
            self.imp().size.get(),
            f64::from(self.width()),
            f64::from(self.height()),
        );
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
        let plot = Plot::new(imp.size.get(), f64::from(width), f64::from(height));
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
            draw_dot(snapshot, &plot, &palette, marker);
            if plot.size == Size::Large {
                labelled =
                    self.draw_now_label(snapshot, &plot, &palette, shown, marker, &cache.obstacles);
            }
        }
        if plot.size == Size::Large && imp.now_label.replace(Some(labelled)) != Some(labelled) {
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
    /// the handles and texts, which the label of the operating point keeps
    /// clear of.
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
        let large = plot.size == Size::Large;
        let text_scale = if large { 0.82 } else { 0.75 };
        let area = plot.area;
        let mut obstacles = Vec::new();

        // Grid and axes.
        let grid = stroke(1.0, &[], false);
        let grid_color = alpha(&palette.fg, palette.grid);
        let axis_color = alpha(&palette.fg, 0.3);
        for temp in plot.grid_temps() {
            let x = plot.x(f64::from(temp));
            line(
                snapshot,
                (x, area.y),
                (x, area.bottom()),
                &grid_color,
                &grid,
            );
        }
        for &percent in plot.percents() {
            let y = plot.y_percent(f64::from(percent));
            let color = if percent == 0 {
                &axis_color
            } else {
                &grid_color
            };
            line(snapshot, (area.x, y), (area.right(), y), color, &grid);
        }
        line(
            snapshot,
            (area.x, area.y),
            (area.x, area.bottom()),
            &axis_color,
            &grid,
        );

        // Labels of the axes, with units.
        let tick = alpha(&palette.fg, palette.text);
        for &percent in plot.percents() {
            let text = layout(widget, &percent_text(percent), text_scale, false);
            let (w, _) = size_of(&text);
            let y = plot.y_percent(f64::from(percent));
            draw_text(snapshot, &text, area.x - 7.0 - w, y + 4.0, &tick);
        }
        let below = if large { 20.0 } else { 16.0 };
        for temp in plot.label_temps() {
            let text = layout(widget, &temp_text(temp), text_scale, false);
            let (w, _) = size_of(&text);
            draw_text(
                snapshot,
                &text,
                plot.x(f64::from(temp)) - w / 2.0,
                area.bottom() + below,
                &tick,
            );
        }
        if large {
            // Translators: the title of the vertical axis of a fan curve.
            let title = layout(widget, &gettext("Duty cycle"), text_scale, false);
            let (w, _) = size_of(&title);
            snapshot.save();
            snapshot.translate(&point(16.0, area.y + area.height / 2.0));
            snapshot.rotate(-90.0);
            draw_text(snapshot, &title, -w / 2.0, 0.0, &tick);
            snapshot.restore();
            // Translators: the title of the horizontal axis of a fan curve;
            // {source} is a temperature, e.g. "CPU (board sensor)".
            let text = gettext("Temperature: {source}").replace("{source}", &curve.source);
            let title = layout(widget, &text, text_scale, false);
            let (w, _) = size_of(&title);
            draw_text(
                snapshot,
                &title,
                area.x + area.width / 2.0 - w / 2.0,
                area.bottom() + 48.0,
                &tick,
            );
        }

        // The emergency band and the zone below the minimum.
        let band = curve
            .threshold
            .and_then(|t| geometry::threshold_band(plot, t));
        if let Some(band) = band {
            snapshot.append_color(&alpha(&palette.error_band, 0.1), &rect(&band));
        }
        if let Some(zone) = geometry::minimum_zone(plot, curve.min_percent) {
            // Diagonal lines 6 px apart and 1 px wide, one path.
            snapshot.push_clip(&rect(&zone));
            let builder = gsk::PathBuilder::new();
            let step = 6.0 * std::f64::consts::SQRT_2;
            let mut x = zone.x - zone.height;
            while x < zone.right() {
                builder.move_to(x as f32, zone.bottom() as f32);
                builder.line_to((x + zone.height) as f32, zone.y as f32);
                x += step;
            }
            snapshot.append_stroke(
                &builder.to_path(),
                &stroke(1.0, &[], false),
                &alpha(&palette.fg, 0.2),
            );
            snapshot.pop();
            line(
                snapshot,
                (zone.x, zone.y),
                (zone.right(), zone.y),
                &alpha(&palette.fg, 0.4),
                &stroke(1.0, &[3.0, 3.0], false),
            );
        }

        // The area below the curve: the accent color fading downwards.
        let shape = geometry::curve_shape(plot, &curve.points);
        if let Some(shape) = &shape {
            let area_path = polyline(&shape.area);
            snapshot.push_fill(&area_path, gsk::FillRule::Winding);
            snapshot.append_linear_gradient(
                &rect(&area),
                &point(0.0, area.y),
                &point(0.0, area.bottom()),
                &[
                    gsk::ColorStop::new(0.0, alpha(&palette.accent, 0.30)),
                    gsk::ColorStop::new(1.0, alpha(&palette.accent, 0.04)),
                ],
            );
            snapshot.pop();
        }

        // Texts of the zones, below the curve line so that they never hide
        // it; only on the large chart (a card says the minimum in its
        // subtitle and the threshold in the source row).
        if large && curve.min_percent > 0 {
            // Translators: the zone of a fan curve below the lowest duty
            // cycle allowed; {duty} is e.g. "30 %".
            let text = gettext("Below the minimum of {duty}")
                .replace("{duty}", &percent_text(curve.min_percent));
            let label = layout(widget, &text, text_scale, false);
            let (w, _) = size_of(&label);
            let baseline = area.bottom() - 9.0;
            let r = Rect {
                x: area.x + 4.0,
                y: baseline - 12.0,
                width: w + 8.0,
                height: 18.0,
            };
            snapshot.append_fill(
                &rounded(&r, 4.0),
                gsk::FillRule::Winding,
                &alpha(&palette.surface, 0.9),
            );
            draw_text(snapshot, &label, r.x + 4.0, baseline, &tick);
            obstacles.push(r);
        }
        if let Some(threshold) = curve
            .threshold
            .filter(|t| f64::from(*t) < geometry::TEMP_MAX)
        {
            let x = plot.x(f64::from(threshold));
            let top = if large { area.y - 10.0 } else { area.y };
            line(
                snapshot,
                (x, top),
                (x, area.bottom()),
                &palette.error,
                &stroke(1.5, &[5.0, 3.0], false),
            );
            if large {
                let temperature = temp_text(threshold);
                let text = if curve.needs_full {
                    // Translators: the emergency temperature of the source on
                    // a fan curve that must reach full duty by then; e.g.
                    // "Emergency threshold 95 °C: 100 % from there".
                    gettext("Emergency threshold {temperature}: {duty} from there")
                        .replace("{temperature}", &temperature)
                        .replace("{duty}", &percent_text(100))
                } else {
                    // Translators: the emergency temperature of the source on
                    // a fan curve, e.g. "Emergency threshold 95 °C".
                    gettext("Emergency threshold {temperature}")
                        .replace("{temperature}", &temperature)
                };
                let label = layout(widget, &text, text_scale, true);
                let (w, _) = size_of(&label);
                draw_text(snapshot, &label, x + 4.0 - w, area.y - 16.0, &palette.error);
            }
        }

        // The curve: straight segments, dashed outside the end points.
        if let Some(shape) = &shape {
            let width = if large { 2.75 } else { 2.25 };
            let ends = stroke(width, &[5.0, 5.0], false);
            let ends_color = alpha(&palette.accent, 0.75);
            for part in [shape.lead, shape.tail].into_iter().flatten() {
                line(snapshot, part[0], part[1], &ends_color, &ends);
            }
            snapshot.append_stroke(
                &polyline(&shape.line),
                &stroke(width, &[], true),
                &palette.accent,
            );
        }
        // Where the curve must reach 100 % (B14).
        let full_at = curve
            .threshold
            .filter(|t| large && curve.needs_full && f64::from(*t) < geometry::TEMP_MAX);
        if let Some(threshold) = full_at {
            let ring = circle(
                (plot.x(f64::from(threshold)), plot.y(geometry::DUTY_MAX)),
                5.0,
            );
            snapshot.append_fill(&ring, gsk::FillRule::Winding, &palette.surface);
            snapshot.append_stroke(&ring, &stroke(2.0, &[], false), &palette.error);
        }

        // The points: dots, or numbered handles on the editor's chart.
        let handles = geometry::handles(plot, &curve.points);
        if large && self.imp().handles.get() {
            for h in &handles {
                let text = if h.first == h.last {
                    (h.first + 1).to_string()
                } else {
                    format!("{}–{}", h.first + 1, h.last + 1)
                };
                let label = layout(widget, &text, 0.75, true);
                let (w, _) = size_of(&label);
                let r = geometry::HANDLE_RADIUS;
                let half = if h.first == h.last { r } else { w / 2.0 + 9.0 };
                let shape = Rect {
                    x: h.x - half,
                    y: h.y - r,
                    width: 2.0 * half,
                    height: 2.0 * r,
                };
                let path = rounded(&shape, r);
                front.append_fill(&path, gsk::FillRule::Winding, &palette.surface);
                front.append_stroke(&path, &stroke(2.5, &[], false), &palette.accent);
                draw_text(
                    front,
                    &label,
                    h.x - w / 2.0,
                    h.y + 4.0,
                    &palette.accent_text,
                );
                obstacles.push(Rect {
                    x: shape.x - 4.0,
                    y: shape.y - 4.0,
                    width: shape.width + 8.0,
                    height: shape.height + 8.0,
                });
            }
        } else {
            let (radius, ring) = if large { (4.5, 2.0) } else { (3.0, 1.5) };
            for h in &handles {
                let dot = circle((h.x, h.y), radius);
                front.append_fill(&dot, gsk::FillRule::Winding, &palette.accent);
                front.append_stroke(&dot, &stroke(ring, &[], false), &palette.surface);
            }
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
            .replace("{duty}", &percent_text(duty_to_percent(reading.duty)));
        let first = layout(widget, &first, 0.82, true);
        let second = moving.then(|| {
            let target = shown.target.round().clamp(0.0, 255.0) as u8;
            let percent = percent_text(duty_to_percent(target));
            // Translators: under the label of the operating point, where
            // the fan is heading, e.g. "target 40 %".
            let text = gettext("target {duty}").replace("{duty}", &percent);
            layout(widget, &text, 0.82, false)
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
            source: "CPU (board sensor)".into(),
            points,
            min_percent: 0,
            threshold: Some(95),
            needs_full: false,
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
            "Temperature source: CPU (board sensor). Point 1: 0\u{a0}°C, 37\u{a0}%. \
             Point 2: 5\u{a0}°C, 38\u{a0}%. Point 3: 84\u{a0}°C, 39\u{a0}%. \
             Points 4–7: 85\u{a0}°C, 100\u{a0}%. Now 46.0\u{a0}°C, duty cycle 38\u{a0}%. \
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
