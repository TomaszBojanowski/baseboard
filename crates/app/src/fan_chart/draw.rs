// SPDX-License-Identifier: GPL-3.0-or-later

//! The drawing of the fan curve chart (docs/PROJEKT.md §7 M2, M2.4) with
//! GSK and Pango: the colors of the style, the shapes and texts, the grid
//! and the axes, the zones, the curve and the operating point. The widget
//! in `fan_chart.rs` puts them together and keeps the static part.

use adw::prelude::*;
use baseboard_core::units;
use gettextrs::gettext;
use gtk::{gdk, graphene, gsk};

use super::ChartCurve;
use super::geometry::{self, Plot, Rect};
use crate::paint::{
    TEXT_SCALE, alpha, circle, draw_text, layout, line, point, polyline, size_of, stroke,
};

/// The colors of one style.
pub(super) struct Palette {
    pub(super) fg: gdk::RGBA,
    pub(super) accent: gdk::RGBA,
    pub(super) surface: gdk::RGBA,
    pub(super) error: gdk::RGBA,
    pub(super) error_band: gdk::RGBA,
    pub(super) live: gdk::RGBA,
    /// Opacity of grid lines and of secondary text; higher with high
    /// contrast.
    pub(super) grid: f32,
    pub(super) text: f32,
}

impl Palette {
    pub(super) fn of(widget: &gtk::Widget) -> Self {
        let style = adw::StyleManager::default();
        let dark = style.is_dark();
        let high_contrast = style.is_high_contrast();
        let hex = |light: &str, dark_value: &str| {
            gdk::RGBA::parse(if dark { dark_value } else { light }).unwrap_or(gdk::RGBA::BLACK)
        };
        Self {
            fg: widget.color(),
            accent: style.accent_color_rgba(),
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

pub(super) fn rect(r: &Rect) -> graphene::Rect {
    graphene::Rect::new(r.x as f32, r.y as f32, r.width as f32, r.height as f32)
}

pub(super) fn rounded(r: &Rect, radius: f64) -> gsk::Path {
    let builder = gsk::PathBuilder::new();
    builder.add_rounded_rect(&gsk::RoundedRect::from_rect(rect(r), radius as f32));
    builder.to_path()
}

/// Under the points of the curve: the guides of the operating point to the
/// axes, and the target when the controller is still moving to it.
pub(super) fn draw_guides(
    snapshot: &gtk::Snapshot,
    plot: &Plot,
    palette: &Palette,
    marker: &geometry::Marker,
) {
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
    if let Some(ty) = marker.target {
        line(
            snapshot,
            (x, y),
            (x, ty),
            &palette.accent,
            &stroke(2.0, &[3.0, 3.0], false),
        );
        let ring = circle((x, ty), 6.0);
        snapshot.append_fill(&ring, gsk::FillRule::Winding, &palette.surface);
        snapshot.append_stroke(&ring, &stroke(2.5, &[], false), &palette.accent);
    }
}

/// Over the points of the curve: the dot of the operating point, with a
/// halo.
pub(super) fn draw_dot(snapshot: &gtk::Snapshot, palette: &Palette, marker: &geometry::Marker) {
    let radius = 6.5;
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

/// The grid and the axes.
pub(super) fn draw_grid(snapshot: &gtk::Snapshot, plot: &Plot, palette: &Palette) {
    let area = plot.area;
    let grid = stroke(1.0, &[], false);
    let grid_color = alpha(&palette.fg, palette.grid);
    let axis_color = alpha(&palette.fg, 0.3);
    for temp in geometry::GRID_TEMPS {
        let x = plot.x(f64::from(temp));
        line(
            snapshot,
            (x, area.y),
            (x, area.bottom()),
            &grid_color,
            &grid,
        );
    }
    for percent in geometry::PERCENTS {
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
}

/// The labels of the axes with their units, and the titles of the axes;
/// `source` is the temperature the curve follows.
pub(super) fn draw_axis_labels(
    widget: &gtk::Widget,
    snapshot: &gtk::Snapshot,
    plot: &Plot,
    palette: &Palette,
    source: &str,
) {
    let area = plot.area;
    let tick = alpha(&palette.fg, palette.text);
    for percent in geometry::PERCENTS {
        let text = layout(widget, &units::percent(percent), TEXT_SCALE, false);
        let (w, _) = size_of(&text);
        let y = plot.y_percent(f64::from(percent));
        draw_text(snapshot, &text, area.x - 7.0 - w, y + 4.0, &tick);
    }
    for temp in geometry::LABEL_TEMPS {
        let text = layout(widget, &units::degrees(f64::from(temp)), TEXT_SCALE, false);
        let (w, _) = size_of(&text);
        draw_text(
            snapshot,
            &text,
            plot.x(f64::from(temp)) - w / 2.0,
            area.bottom() + 20.0,
            &tick,
        );
    }
    // Translators: the title of the vertical axis of a fan curve.
    let title = layout(widget, &gettext("Duty cycle"), TEXT_SCALE, false);
    let (w, _) = size_of(&title);
    snapshot.save();
    snapshot.translate(&point(16.0, area.y + area.height / 2.0));
    snapshot.rotate(-90.0);
    draw_text(snapshot, &title, -w / 2.0, 0.0, &tick);
    snapshot.restore();
    // Translators: the title of the horizontal axis of a fan curve;
    // {source} is a temperature, e.g. "Processor (board sensor)".
    let text = gettext("Temperature: {source}").replace("{source}", source);
    let title = layout(widget, &text, TEXT_SCALE, false);
    let (w, _) = size_of(&title);
    draw_text(
        snapshot,
        &title,
        area.x + area.width / 2.0 - w / 2.0,
        area.bottom() + 48.0,
        &tick,
    );
}

/// The emergency band and the hatched zone below the minimum.
pub(super) fn draw_zones(
    snapshot: &gtk::Snapshot,
    plot: &Plot,
    palette: &Palette,
    curve: &ChartCurve,
) {
    let band = curve
        .threshold
        .and_then(|t| geometry::threshold_band(plot, t));
    if let Some(band) = band {
        snapshot.append_color(&alpha(&palette.error_band, 0.1), &rect(&band));
    }
    let Some(zone) = geometry::minimum_zone(plot, curve.min_percent) else {
        return;
    };
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

/// The area below the curve: the accent color fading downwards.
pub(super) fn draw_area_below(
    snapshot: &gtk::Snapshot,
    plot: &Plot,
    palette: &Palette,
    shape: &geometry::CurveShape,
) {
    let area = plot.area;
    snapshot.push_fill(&polyline(&shape.area), gsk::FillRule::Winding);
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

/// The text of the zone below the minimum and the emergency threshold with
/// its text. Returns where the text of the zone stands.
pub(super) fn draw_zone_texts(
    widget: &gtk::Widget,
    snapshot: &gtk::Snapshot,
    plot: &Plot,
    palette: &Palette,
    curve: &ChartCurve,
) -> Vec<Rect> {
    let area = plot.area;
    let mut obstacles = Vec::new();
    if curve.min_percent > 0 {
        // Translators: the zone of a fan curve below the lowest duty
        // cycle allowed; {duty} is e.g. "30 %".
        let text = gettext("Below the minimum of {duty}")
            .replace("{duty}", &units::percent(curve.min_percent));
        let label = layout(widget, &text, TEXT_SCALE, false);
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
        draw_text(
            snapshot,
            &label,
            r.x + 4.0,
            baseline,
            &alpha(&palette.fg, palette.text),
        );
        obstacles.push(r);
    }
    if let Some(threshold) = curve
        .threshold
        .filter(|t| f64::from(*t) < geometry::TEMP_MAX)
    {
        let x = plot.x(f64::from(threshold));
        line(
            snapshot,
            (x, area.y - 10.0),
            (x, area.bottom()),
            &palette.error,
            &stroke(1.5, &[5.0, 3.0], false),
        );
        let temperature = units::degrees(f64::from(threshold));
        let text = if curve.needs_full {
            // Translators: the emergency temperature of the source on a
            // fan curve that must reach full duty by then; e.g.
            // "Emergency threshold 95 °C: 100 % from there".
            gettext("Emergency threshold {temperature}: {duty} from there")
                .replace("{temperature}", &temperature)
                .replace("{duty}", &units::percent(100))
        } else {
            // Translators: the emergency temperature of the source on a
            // fan curve, e.g. "Emergency threshold 95 °C".
            gettext("Emergency threshold {temperature}").replace("{temperature}", &temperature)
        };
        let label = layout(widget, &text, TEXT_SCALE, true);
        let (w, _) = size_of(&label);
        draw_text(snapshot, &label, x + 4.0 - w, area.y - 16.0, &palette.error);
    }
    obstacles
}

/// The curve: straight segments, dashed outside the end points.
pub(super) fn draw_curve_line(
    snapshot: &gtk::Snapshot,
    palette: &Palette,
    shape: &geometry::CurveShape,
) {
    let width = 2.75;
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

/// Where the curve must reach 100 % (B14): a ring at the threshold.
pub(super) fn draw_full_at(
    snapshot: &gtk::Snapshot,
    plot: &Plot,
    palette: &Palette,
    curve: &ChartCurve,
) {
    let full_at = curve
        .threshold
        .filter(|t| curve.needs_full && f64::from(*t) < geometry::TEMP_MAX);
    if let Some(threshold) = full_at {
        let ring = circle(
            (plot.x(f64::from(threshold)), plot.y(geometry::DUTY_MAX)),
            5.0,
        );
        snapshot.append_fill(&ring, gsk::FillRule::Winding, &palette.surface);
        snapshot.append_stroke(&ring, &stroke(2.0, &[], false), &palette.error);
    }
}
