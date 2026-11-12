// SPDX-License-Identifier: GPL-3.0-or-later

//! The small chart in a row of the list of fans (D62): one line and one
//! point "now", in the accent color, with a light fill under the line;
//! no grid, axes, zones or points. The line is the straight segments the
//! controller runs (a spline would show duty cycles the fan never gets,
//! D45), held flat before the first point and after the last one, with
//! the corners rounded a little. In manual mode it is the flat line of the
//! set duty cycle.
//!
//! For screen readers the chart is an image with the full description of
//! the curve (`fan_chart::describe`); the row says the rest.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};

/// Space around the line, so that the point and the rounded line are not
/// cut at the edges.
const MARGIN_X: f64 = 4.0;
const MARGIN_Y: f64 = 5.0;
/// How much the corners of the line are rounded, in pixels.
const ROUND: f64 = 4.0;
const DOT: f64 = 3.5;
const HALO: f64 = 7.0;

/// One piece of the drawn line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Segment {
    Line(f64, f64),
    /// A quadratic curve through a corner: the corner, then the end.
    Quad((f64, f64), (f64, f64)),
}

/// The points of a curve as drawn, in °C and 0–255: repeated points once,
/// held flat to 0 and 100 °C, without the middle points of straight runs.
pub(super) fn line_points(curve: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut points: Vec<(f64, f64)> = Vec::new();
    for &p in curve {
        if points.last() != Some(&p) {
            points.push(p);
        }
    }
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return points;
    };
    if first.0 > 0.0 {
        points.insert(0, (0.0, first.1));
    }
    if last.0 < 100.0 {
        points.push((100.0, last.1));
    }
    let mut kept = Vec::with_capacity(points.len());
    for (i, &p) in points.iter().enumerate() {
        let straight = i > 0 && i + 1 < points.len() && {
            let (a, b) = (points[i - 1], points[i + 1]);
            ((p.1 - a.1) * (b.0 - p.0) - (b.1 - p.1) * (p.0 - a.0)).abs() < 1e-9
        };
        if !straight {
            kept.push(p);
        }
    }
    kept
}

/// `points` in pixels of a chart `width` × `height`: 0–100 °C across,
/// 0–255 up.
pub(super) fn to_pixels(points: &[(f64, f64)], width: f64, height: f64) -> Vec<(f64, f64)> {
    points
        .iter()
        .map(|&(t, d)| (x_of(t, width), y_of(d, height)))
        .collect()
}

fn x_of(temp: f64, width: f64) -> f64 {
    MARGIN_X + temp.clamp(0.0, 100.0) * (width - 2.0 * MARGIN_X) / 100.0
}

fn y_of(duty: f64, height: f64) -> f64 {
    MARGIN_Y + (1.0 - duty.clamp(0.0, 255.0) / 255.0) * (height - 2.0 * MARGIN_Y)
}

/// The line through `points` (pixels) with every corner rounded by at
/// most `radius`, and never by more than half of a segment.
pub(super) fn rounded(points: &[(f64, f64)], radius: f64) -> Vec<Segment> {
    let mut segments = Vec::new();
    for i in 1..points.len() {
        let (x1, y1) = points[i];
        if i + 1 == points.len() {
            segments.push(Segment::Line(x1, y1));
            break;
        }
        let (x0, y0) = points[i - 1];
        let (x2, y2) = points[i + 1];
        let l1 = (x1 - x0).hypot(y1 - y0);
        let l2 = (x2 - x1).hypot(y2 - y1);
        let r = radius.min(l1 / 2.0).min(l2 / 2.0);
        if r <= 0.0 {
            segments.push(Segment::Line(x1, y1));
            continue;
        }
        let before = (x1 + (x0 - x1) * r / l1, y1 + (y0 - y1) * r / l1);
        let after = (x1 + (x2 - x1) * r / l2, y1 + (y2 - y1) * r / l2);
        segments.push(Segment::Line(before.0, before.1));
        segments.push(Segment::Quad((x1, y1), after));
    }
    segments
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Spark {
        pub(super) width: Cell<i32>,
        pub(super) height: Cell<i32>,
        /// The curve in °C and 0–255; empty draws nothing.
        pub(super) curve: RefCell<Vec<(f64, f64)>>,
        /// The point now, in °C and 0–255.
        pub(super) now: Cell<Option<(f64, f64)>>,
        pub(super) style_handlers: RefCell<Vec<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Spark {
        const NAME: &'static str = "BaseboardSpark";
        type Type = super::Spark;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for Spark {
        fn constructed(&self) {
            self.parent_constructed();
            let style = adw::StyleManager::default();
            let redraw = |obj: &super::Spark| {
                let obj = obj.downgrade();
                move |_: &adw::StyleManager| {
                    if let Some(obj) = obj.upgrade() {
                        obj.queue_draw();
                    }
                }
            };
            let obj = self.obj();
            *self.style_handlers.borrow_mut() = vec![
                style.connect_dark_notify(redraw(&obj)),
                style.connect_accent_color_rgba_notify(redraw(&obj)),
            ];
        }

        fn dispose(&self) {
            let style = adw::StyleManager::default();
            for handler in self.style_handlers.take() {
                style.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for Spark {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let size = match orientation {
                gtk::Orientation::Horizontal => self.width.get(),
                _ => self.height.get(),
            };
            (size, size, -1, -1)
        }

        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            self.obj().draw(snapshot);
        }
    }
}

glib::wrapper! {
    pub struct Spark(ObjectSubclass<imp::Spark>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

fn alpha(color: &gdk::RGBA, factor: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        color.red(),
        color.green(),
        color.blue(),
        color.alpha() * factor,
    )
}

fn circle(center: (f64, f64), radius: f64) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.add_circle(
        &graphene::Point::new(center.0 as f32, center.1 as f32),
        radius as f32,
    );
    builder.to_path()
}

impl Spark {
    pub fn new(width: i32, height: i32) -> Self {
        let spark: Self = glib::Object::new();
        spark.set_size(width, height);
        spark
    }

    pub fn set_size(&self, width: i32, height: i32) {
        let imp = self.imp();
        let old = (imp.width.replace(width), imp.height.replace(height));
        if old != (width, height) {
            self.queue_resize();
        }
    }

    /// The curve in °C and 0–255; empty for nothing.
    pub fn set_curve(&self, curve: Vec<(f64, f64)>) {
        if *self.imp().curve.borrow() != curve {
            self.imp().curve.replace(curve);
            self.queue_draw();
        }
    }

    /// The point now, in °C and 0–255.
    pub fn set_now(&self, now: Option<(f64, f64)>) {
        if self.imp().now.replace(now) != now {
            self.queue_draw();
        }
    }

    fn draw(&self, snapshot: &gtk::Snapshot) {
        let imp = self.imp();
        let curve = imp.curve.borrow();
        let points = line_points(&curve);
        if points.len() < 2 {
            return;
        }
        let (width, height) = (f64::from(self.width()), f64::from(self.height()));
        let style = adw::StyleManager::default();
        let accent = style.accent_color_rgba();
        let surface = gdk::RGBA::parse(if style.is_dark() {
            "#38383c"
        } else {
            "#ffffff"
        })
        .unwrap_or(gdk::RGBA::WHITE);

        let pixels = to_pixels(&points, width, height);
        let segments = rounded(&pixels, ROUND);
        let line = gsk::PathBuilder::new();
        let area = gsk::PathBuilder::new();
        let (x0, y0) = pixels[0];
        for builder in [&line, &area] {
            builder.move_to(x0 as f32, y0 as f32);
            for segment in &segments {
                match *segment {
                    Segment::Line(x, y) => builder.line_to(x as f32, y as f32),
                    Segment::Quad((cx, cy), (x, y)) => {
                        builder.quad_to(cx as f32, cy as f32, x as f32, y as f32)
                    }
                }
            }
        }
        let bottom = y_of(0.0, height);
        area.line_to(x_of(100.0, width) as f32, bottom as f32);
        area.line_to(x_of(0.0, width) as f32, bottom as f32);
        area.close();

        // A light fill under the line, fading down.
        snapshot.push_fill(&area.to_path(), gsk::FillRule::Winding);
        snapshot.append_linear_gradient(
            &graphene::Rect::new(0.0, 0.0, width as f32, height as f32),
            &graphene::Point::new(0.0, MARGIN_Y as f32),
            &graphene::Point::new(0.0, bottom as f32),
            &[
                gsk::ColorStop::new(0.0, alpha(&accent, 0.18)),
                gsk::ColorStop::new(1.0, alpha(&accent, 0.0)),
            ],
        );
        snapshot.pop();

        let stroke = gsk::Stroke::new(2.25);
        stroke.set_line_join(gsk::LineJoin::Round);
        stroke.set_line_cap(gsk::LineCap::Round);
        snapshot.append_stroke(&line.to_path(), &stroke, &accent);

        if let Some((temp, duty)) = imp.now.get() {
            let center = (x_of(temp, width), y_of(duty, height));
            let fill = gsk::FillRule::Winding;
            snapshot.append_fill(&circle(center, HALO), fill, &alpha(&accent, 0.18));
            snapshot.append_fill(&circle(center, DOT + 2.0), fill, &surface);
            snapshot.append_fill(&circle(center, DOT), fill, &accent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CPU curve of the BIOS on MS-7E61, as the driver reports it.
    fn cpu() -> Vec<(f64, f64)> {
        vec![
            (0.0, 122.0),
            (75.0, 124.0),
            (80.0, 191.0),
            (85.0, 255.0),
            (85.0, 255.0),
            (85.0, 255.0),
            (85.0, 255.0),
        ]
    }

    #[test]
    fn points_as_drawn() {
        // Repeated points once, held flat to 100 °C.
        assert_eq!(
            line_points(&cpu()),
            [
                (0.0, 122.0),
                (75.0, 124.0),
                (80.0, 191.0),
                (85.0, 255.0),
                (100.0, 255.0)
            ]
        );
        // A flat curve between 40 and 85 °C is one line from 0 to 100 °C.
        let flat = [(40.0, 76.0), (55.0, 76.0), (70.0, 76.0), (85.0, 76.0)];
        assert_eq!(line_points(&flat), [(0.0, 76.0), (100.0, 76.0)]);
        // Manual mode: the two ends.
        assert_eq!(
            line_points(&[(0.0, 115.0), (100.0, 115.0)]),
            [(0.0, 115.0), (100.0, 115.0)]
        );
        assert!(line_points(&[]).is_empty());
    }

    #[test]
    fn pixels_of_a_chart() {
        let pixels = to_pixels(&[(0.0, 255.0), (100.0, 0.0)], 192.0, 36.0);
        assert_eq!(
            pixels,
            [(MARGIN_X, MARGIN_Y), (192.0 - MARGIN_X, 36.0 - MARGIN_Y)]
        );
    }

    /// Corners are rounded by at most the radius and at most half of the
    /// shorter segment; the line ends where it should.
    #[test]
    fn rounded_corners() {
        let segments = rounded(&[(0.0, 10.0), (10.0, 10.0), (10.0, 0.0)], 4.0);
        assert_eq!(
            segments,
            [
                Segment::Line(6.0, 10.0),
                Segment::Quad((10.0, 10.0), (10.0, 6.0)),
                Segment::Line(10.0, 0.0)
            ]
        );
        // A short segment of 2 px rounds by 1 px only.
        let segments = rounded(&[(0.0, 0.0), (2.0, 0.0), (2.0, 10.0)], 4.0);
        assert_eq!(segments[0], Segment::Line(1.0, 0.0));
        // A straight line has nothing to round.
        assert_eq!(
            rounded(&[(0.0, 0.0), (10.0, 0.0)], 4.0),
            [Segment::Line(10.0, 0.0)]
        );
    }
}
