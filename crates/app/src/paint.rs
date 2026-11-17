// SPDX-License-Identifier: GPL-3.0-or-later

//! Drawing with GSK and Pango for the app's own charts (the fan curve and
//! the history of a temperature): colors with less opacity, strokes, lines,
//! circles and texts in the widget's font.

use adw::prelude::*;
use gtk::{gdk, graphene, gsk, pango};

/// The size of the charts' texts, relative to the widget's font.
pub const TEXT_SCALE: f64 = 0.82;

pub fn alpha(color: &gdk::RGBA, factor: f32) -> gdk::RGBA {
    gdk::RGBA::new(
        color.red(),
        color.green(),
        color.blue(),
        color.alpha() * factor,
    )
}

pub fn point(x: f64, y: f64) -> graphene::Point {
    graphene::Point::new(x as f32, y as f32)
}

pub fn stroke(width: f32, dash: &[f32], round: bool) -> gsk::Stroke {
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

pub fn polyline(points: &[(f64, f64)]) -> gsk::Path {
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

pub fn line(
    snapshot: &gtk::Snapshot,
    from: (f64, f64),
    to: (f64, f64),
    color: &gdk::RGBA,
    stroke: &gsk::Stroke,
) {
    snapshot.append_stroke(&polyline(&[from, to]), stroke, color);
}

pub fn circle(center: (f64, f64), radius: f64) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.add_circle(&point(center.0, center.1), radius as f32);
    builder.to_path()
}

/// Text in the widget's font, scaled, with tabular figures.
pub fn layout(widget: &gtk::Widget, text: &str, scale: f64, bold: bool) -> pango::Layout {
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

pub fn size_of(layout: &pango::Layout) -> (f64, f64) {
    let (w, h) = layout.pixel_size();
    (f64::from(w), f64::from(h))
}

/// Draws `layout` from `x`, with its baseline at `baseline`, as text is
/// placed in the mockups.
pub fn draw_text(
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
