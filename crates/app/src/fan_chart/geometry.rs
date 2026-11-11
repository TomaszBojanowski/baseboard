// SPDX-License-Identifier: GPL-3.0-or-later

//! The geometry of the fan curve chart (docs/PROJEKT.md §7 M2, M2.4):
//! where temperatures and duty cycles go, which grid lines and labels
//! show, the shapes of the curve, the zones, the handles and the
//! operating point. Pure functions in logical pixels, without GTK.

use baseboard_core::fans::curve;
use baseboard_core::fans::driver::CurvePoint;

/// The two sizes of the chart.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Size {
    /// In a connector card: fewer lines and labels, no axis titles.
    #[default]
    Card,
    /// On the curve subpage: axis titles and texts in the zones.
    Large,
}

/// The temperature axis runs from 0 to this, in °C (the range of B7).
pub const TEMP_MAX: f64 = 100.0;
/// The duty-cycle axis runs from 0 to this (0–255 = 0–100 %).
pub const DUTY_MAX: f64 = 255.0;
/// A target closer to the dot than this, in pixels, is not drawn apart.
pub const MIN_TARGET_GAP: f64 = 3.0;
/// Radius of a handle; a run of points is a pill this tall.
pub const HANDLE_RADIUS: f64 = 11.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Overlaps `other`, or comes closer to it than `gap`.
    pub fn near(&self, other: &Rect, gap: f64) -> bool {
        self.x < other.right() + gap
            && other.x < self.right() + gap
            && self.y < other.bottom() + gap
            && other.y < self.bottom() + gap
    }
}

/// The plot area of a widget of the given size, and the mapping of values
/// to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plot {
    pub size: Size,
    pub area: Rect,
}

impl Plot {
    /// The margins leave room for the labels: left for "100 %", below for
    /// "40 °C" (and the axis title on the large chart), above for the
    /// threshold label of the large chart.
    pub fn new(size: Size, width: f64, height: f64) -> Self {
        let (left, right, top, bottom) = match size {
            Size::Card => (42.0, 12.0, 12.0, 26.0),
            Size::Large => (66.0, 22.0, 38.0, 58.0),
        };
        Self {
            size,
            area: Rect {
                x: left,
                y: top,
                width: (width - left - right).max(1.0),
                height: (height - top - bottom).max(1.0),
            },
        }
    }

    /// x of a temperature in °C; outside the axis it stays at the edge.
    pub fn x(&self, temp: f64) -> f64 {
        self.area.x + temp.clamp(0.0, TEMP_MAX) / TEMP_MAX * self.area.width
    }

    /// y of a duty cycle 0–255, not rounded.
    pub fn y(&self, duty: f64) -> f64 {
        self.area.y + self.area.height * (1.0 - duty.clamp(0.0, DUTY_MAX) / DUTY_MAX)
    }

    /// y of a percentage of the duty-cycle axis.
    pub fn y_percent(&self, percent: f64) -> f64 {
        self.y(percent * DUTY_MAX / 100.0)
    }

    /// Temperatures with a vertical grid line.
    pub fn grid_temps(&self) -> Vec<u8> {
        let step = match self.size {
            Size::Card => 20,
            Size::Large => 10,
        };
        (0..=100).step_by(step).collect()
    }

    /// Temperatures with a label under the axis: every 20 °C, on a narrow
    /// card every 50 °C, so that "100 °C" labels do not touch.
    pub fn label_temps(&self) -> Vec<u8> {
        let step = if self.size == Size::Card && self.area.width < 250.0 {
            50
        } else {
            20
        };
        (0..=100).step_by(step).collect()
    }

    /// Percentages with a horizontal line and a label.
    pub fn percents(&self) -> &'static [u8] {
        match self.size {
            Size::Card => &[0, 50, 100],
            Size::Large => &[0, 25, 50, 75, 100],
        }
    }
}

/// The curve as lines to draw.
#[derive(Debug, Clone, PartialEq)]
pub struct CurveShape {
    /// Through the points, straight segments; repeated points once.
    pub line: Vec<(f64, f64)>,
    /// From the left edge to the first point (dashed), when that point
    /// lies above 0 °C: the controller holds its duty cycle there.
    pub lead: Option<[(f64, f64); 2]>,
    /// From the last point to the right edge (dashed), when it lies below
    /// 100 °C.
    pub tail: Option<[(f64, f64); 2]>,
    /// The area below the curve and its ends, closed along the bottom.
    pub area: Vec<(f64, f64)>,
}

pub fn curve_shape(plot: &Plot, points: &[CurvePoint]) -> Option<CurveShape> {
    let first = *points.first()?;
    let last = *points.last()?;
    let mut line: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for p in points {
        let at = (plot.x(f64::from(p.temp)), plot.y(f64::from(p.duty)));
        if line.last() != Some(&at) {
            line.push(at);
        }
    }
    let (left, right) = (plot.x(0.0), plot.x(TEMP_MAX));
    let (start, end) = (line[0], line[line.len() - 1]);
    let lead = (f64::from(first.temp) > 0.0).then_some([(left, start.1), start]);
    let tail = (f64::from(last.temp) < TEMP_MAX).then_some([end, (right, end.1)]);
    let bottom = plot.area.bottom();
    let mut area = vec![(left, bottom), (left, start.1)];
    area.extend(&line);
    area.extend([(right, end.1), (right, bottom)]);
    Some(CurveShape {
        line,
        lead,
        tail,
        area,
    })
}

/// A handle of the large chart: one per run of equal points, numbered like
/// the list of points ("4–7" for a run).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Handle {
    pub x: f64,
    pub y: f64,
    /// Indices of the points, from 0.
    pub first: usize,
    pub last: usize,
}

pub fn handles(plot: &Plot, points: &[CurvePoint]) -> Vec<Handle> {
    curve::runs(points)
        .into_iter()
        .map(|(first, last)| Handle {
            x: plot.x(f64::from(points[first].temp)),
            y: plot.y(f64::from(points[first].duty)),
            first,
            last,
        })
        .collect()
}

/// The band below the minimum duty cycle (B5), if there is a minimum.
pub fn minimum_zone(plot: &Plot, min_percent: u8) -> Option<Rect> {
    (min_percent > 0).then(|| {
        let top = plot.y_percent(f64::from(min_percent));
        Rect {
            x: plot.area.x,
            y: top,
            width: plot.area.width,
            height: plot.area.bottom() - top,
        }
    })
}

/// The band from the emergency threshold to the right edge (B4), if the
/// threshold lies on the axis.
pub fn threshold_band(plot: &Plot, threshold: u8) -> Option<Rect> {
    (f64::from(threshold) < TEMP_MAX).then(|| {
        let left = plot.x(f64::from(threshold));
        Rect {
            x: left,
            y: plot.area.y,
            width: plot.area.right() - left,
            height: plot.area.height,
        }
    })
}

/// The operating point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marker {
    /// The source temperature.
    pub x: f64,
    /// The duty cycle the driver reports now.
    pub y: f64,
    /// The duty cycle the controller moves to, when it lies far enough from
    /// the dot to be seen apart.
    pub target: Option<f64>,
}

/// `temp` in °C, `duty` and `target` 0–255.
pub fn marker(plot: &Plot, temp: f64, duty: f64, target: Option<f64>) -> Marker {
    let y = plot.y(duty);
    Marker {
        x: plot.x(temp),
        y,
        target: target
            .map(|t| plot.y(t))
            .filter(|ty| (ty - y).abs() >= MIN_TARGET_GAP),
    }
}

/// Where a label of `width` × `height` goes next to the operating point:
/// the first of the places around it that stays inside the plot and keeps
/// clear of `obstacles` (handles, texts, the point itself). `None` when
/// no place is free; the label then stays off the chart.
pub fn place_label(
    plot: &Plot,
    point: (f64, f64),
    width: f64,
    height: f64,
    obstacles: &[Rect],
) -> Option<(f64, f64)> {
    let (x, y) = point;
    let area = plot.area;
    let inside = |r: &Rect| {
        r.x >= area.x + 4.0
            && r.right() <= area.right()
            && r.y >= area.y
            && r.bottom() <= area.bottom() - 4.0
    };
    for dy in [
        16.0,
        40.0,
        70.0,
        -16.0 - height,
        -40.0 - height,
        -70.0 - height,
    ] {
        for dx in [18.0, -18.0 - width, 40.0, -40.0 - width] {
            let r = Rect {
                x: x + dx,
                y: y + dy,
                width,
                height,
            };
            if inside(&r) && !obstacles.iter().any(|o| r.near(o, 4.0)) {
                return Some((r.x, r.y));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(points: &[(u8, u8)]) -> Vec<CurvePoint> {
        points
            .iter()
            .map(|&(temp, duty)| CurvePoint { temp, duty })
            .collect()
    }

    /// SYS_FAN1 on MS-7E61: points 4–7 repeat point 4.
    fn sys1() -> Vec<CurvePoint> {
        curve(&[
            (0, 94),
            (5, 96),
            (84, 99),
            (85, 255),
            (85, 255),
            (85, 255),
            (85, 255),
        ])
    }

    /// SYS_FAN3: flat 76 from 40 to 85 °C.
    fn flat() -> Vec<CurvePoint> {
        curve(&[
            (40, 76),
            (55, 76),
            (70, 76),
            (85, 76),
            (85, 76),
            (85, 76),
            (85, 76),
        ])
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn values_map_to_the_plot_area() {
        // A card 400 × 150: plot from x 42 to 388, y 12 to 124.
        let plot = Plot::new(Size::Card, 400.0, 150.0);
        assert_eq!(
            plot.area,
            Rect {
                x: 42.0,
                y: 12.0,
                width: 346.0,
                height: 112.0
            }
        );
        assert!(close(plot.x(0.0), 42.0));
        assert!(close(plot.x(100.0), 388.0));
        assert!(close(plot.x(50.0), 215.0));
        assert!(close(plot.y(0.0), 124.0));
        assert!(close(plot.y(255.0), 12.0));
        assert!(close(plot.y_percent(50.0), 68.0));
        // Outside the axes the values stay at the edges.
        assert!(close(plot.x(-5.0), 42.0));
        assert!(close(plot.x(120.0), 388.0));
        assert!(close(plot.y(300.0), 12.0));
        // A widget too small for its margins still has a plot area.
        let tiny = Plot::new(Size::Large, 10.0, 10.0);
        assert!(tiny.area.width >= 1.0 && tiny.area.height >= 1.0);
    }

    #[test]
    fn grid_and_labels() {
        let card = Plot::new(Size::Card, 400.0, 150.0);
        assert_eq!(card.grid_temps(), [0, 20, 40, 60, 80, 100]);
        assert_eq!(card.label_temps(), [0, 20, 40, 60, 80, 100]);
        assert_eq!(card.percents(), [0, 50, 100]);
        // A narrow card labels every 50 °C.
        let narrow = Plot::new(Size::Card, 280.0, 136.0);
        assert_eq!(narrow.label_temps(), [0, 50, 100]);
        let large = Plot::new(Size::Large, 900.0, 380.0);
        assert_eq!(large.grid_temps().len(), 11);
        assert_eq!(large.percents(), [0, 25, 50, 75, 100]);
    }

    #[test]
    fn repeated_points_are_drawn_once() {
        let plot = Plot::new(Size::Card, 400.0, 150.0);
        let shape = curve_shape(&plot, &sys1()).unwrap();
        // Seven points, four places.
        assert_eq!(shape.line.len(), 4);
        // The first point lies at 0 °C: no dashed part on the left; the
        // last at 85 °C: a dashed part to 100 °C at full duty.
        assert_eq!(shape.lead, None);
        let tail = shape.tail.unwrap();
        assert!(close(tail[0].0, plot.x(85.0)) && close(tail[1].0, plot.x(100.0)));
        assert!(close(tail[0].1, plot.y(255.0)) && close(tail[1].1, tail[0].1));
        // The area runs along the bottom from edge to edge.
        assert_eq!(shape.area.first(), Some(&(plot.x(0.0), plot.area.bottom())));
        assert_eq!(
            shape.area.last(),
            Some(&(plot.x(100.0), plot.area.bottom()))
        );
        assert_eq!(shape.area.len(), shape.line.len() + 4);

        let handles = handles(&plot, &sys1());
        let runs: Vec<_> = handles.iter().map(|h| (h.first, h.last)).collect();
        assert_eq!(runs, [(0, 0), (1, 1), (2, 2), (3, 6)]);
    }

    #[test]
    fn flat_curve() {
        let plot = Plot::new(Size::Large, 900.0, 380.0);
        let shape = curve_shape(&plot, &flat()).unwrap();
        // Four places, all at the same height, and dashed parts on both
        // sides at that height.
        assert_eq!(shape.line.len(), 4);
        let y = plot.y(76.0);
        assert!(shape.line.iter().all(|&(_, ly)| close(ly, y)));
        let lead = shape.lead.unwrap();
        assert!(
            close(lead[0].0, plot.x(0.0)) && close(lead[0].1, y) && close(lead[1].0, plot.x(40.0))
        );
        assert!(shape.tail.is_some());
        // Every point the same: one place, dashed on both sides.
        let one = curve(&[(50, 128); 7]);
        let shape = curve_shape(&plot, &one).unwrap();
        assert_eq!(shape.line.len(), 1);
        assert!(shape.lead.is_some() && shape.tail.is_some());
        assert_eq!(handles(&plot, &one).len(), 1);
        assert_eq!(curve_shape(&plot, &[]), None);
    }

    #[test]
    fn any_number_of_points() {
        let plot = Plot::new(Size::Large, 900.0, 380.0);
        let five = curve(&[(30, 60), (45, 90), (60, 130), (75, 200), (85, 255)]);
        let shape = curve_shape(&plot, &five).unwrap();
        assert_eq!(shape.line.len(), 5);
        assert!(shape.lead.is_some() && shape.tail.is_some());
        let runs: Vec<_> = handles(&plot, &five)
            .iter()
            .map(|h| (h.first, h.last))
            .collect();
        assert_eq!(runs, [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4)]);
        let single = curve(&[(50, 128)]);
        assert_eq!(curve_shape(&plot, &single).unwrap().line.len(), 1);
        assert_eq!(handles(&plot, &single).len(), 1);
    }

    #[test]
    fn zones() {
        let plot = Plot::new(Size::Card, 400.0, 150.0);
        assert_eq!(minimum_zone(&plot, 0), None);
        let zone = minimum_zone(&plot, 30).unwrap();
        assert!(close(zone.bottom(), plot.area.bottom()));
        assert!(close(zone.y, plot.y_percent(30.0)));
        let band = threshold_band(&plot, 95).unwrap();
        assert!(close(band.x, plot.x(95.0)) && close(band.right(), plot.area.right()));
        // A threshold beyond the axis has no band.
        assert_eq!(threshold_band(&plot, 105), None);
    }

    #[test]
    fn marker_shows_a_target_only_apart_from_the_dot() {
        let plot = Plot::new(Size::Card, 400.0, 150.0);
        // Settled: target equals the duty cycle.
        assert_eq!(marker(&plot, 46.0, 98.0, Some(98.0)).target, None);
        // One step apart is under 3 px on a card: not drawn apart.
        assert_eq!(marker(&plot, 46.0, 98.0, Some(99.0)).target, None);
        // Moving from 76 to 98: the target is drawn.
        let m = marker(&plot, 46.0, 76.0, Some(98.0));
        assert!(close(m.target.unwrap(), plot.y(98.0)));
        assert!(close(m.x, plot.x(46.0)) && close(m.y, plot.y(76.0)));
    }

    #[test]
    fn label_keeps_clear_of_obstacles() {
        let plot = Plot::new(Size::Large, 900.0, 380.0);
        let point = (plot.x(46.0), plot.y(98.0));
        // Free: right of the point and below it.
        let (x, y) = place_label(&plot, point, 150.0, 46.0, &[]).unwrap();
        assert!(close(x, point.0 + 18.0) && close(y, point.1 + 16.0));
        // That place taken: another one, still clear of the obstacle.
        let taken = Rect {
            x: point.0 + 10.0,
            y: point.1 + 10.0,
            width: 200.0,
            height: 60.0,
        };
        let (x, y) = place_label(&plot, point, 150.0, 46.0, &[taken]).unwrap();
        let label = Rect {
            x,
            y,
            width: 150.0,
            height: 46.0,
        };
        assert!(!label.near(&taken, 4.0));
        // A chart too small for the label: no place.
        let small = Plot::new(Size::Large, 200.0, 140.0);
        assert_eq!(place_label(&small, (100.0, 60.0), 150.0, 46.0, &[]), None);
    }
}
