// SPDX-License-Identifier: GPL-3.0-or-later

//! The curve model (docs/PROJEKT.md §7 M2, step 2): the duty cycle the
//! board's controller sets for a temperature of the curve source, so that
//! the interface can show the target on the chart and the helper can
//! compare a curve with what the controller does.
//!
//! - Between two points the duty cycle follows a straight line.
//! - Below the first point and above the last one it is the duty cycle of
//!   that point.
//! - The controller rounds the line up to a whole step of 0–255. Seen on
//!   MS-7E61 (§4): every settled sample of six channels over 939 s at
//!   temp1 42–73 °C, with the steps between values exactly where rounding
//!   up puts them. The steep parts above 75 °C are not checked yet.
//! - Points repeated at the end (the BIOS fills points 5–7 with point 4)
//!   and steps (two points at one temperature) make segments of zero width.
//!   The model never interpolates over them: a temperature between two
//!   points always falls on a segment whose ends have different
//!   temperatures. At exactly the temperature of a step, the duty cycle of
//!   the last point at that temperature counts; that edge is an assumption,
//!   not an observation.
//!
//! Temperatures come in millidegrees, as the source reports them, and the
//! arithmetic is in integers, so there is no division by zero and no
//! floating-point edge.

use super::driver::CurvePoint;

/// The duty cycle (0–255) the controller sets at the source temperature
/// `millidegrees`. `None` for an empty curve or one whose temperatures go
/// down somewhere: the controller has no such curve and the driver does
/// not write one.
pub fn expected_duty(points: &[CurvePoint], millidegrees: i64) -> Option<u8> {
    let (start, end) = segment(points, |p| i64::from(p.temp) * 1000 <= millidegrees)?;
    let Some(end) = end else {
        return Some(start.duty);
    };
    let t0 = i64::from(start.temp) * 1000;
    let t1 = i64::from(end.temp) * 1000;
    let d0 = i64::from(start.duty);
    let d1 = i64::from(end.duty);
    // t0 < millidegrees < t1, so t1 - t0 > 0 and the result lies between
    // d0 and d1.
    let duty = d0 + ceil_div((d1 - d0) * (millidegrees - t0), t1 - t0);
    u8::try_from(duty).ok()
}

/// The straight line itself at `temp` °C, without rounding, for drawing.
pub fn line_at(points: &[CurvePoint], temp: f64) -> Option<f64> {
    let (start, end) = segment(points, |p| f64::from(p.temp) <= temp)?;
    let Some(end) = end else {
        return Some(f64::from(start.duty));
    };
    let (t0, t1) = (f64::from(start.temp), f64::from(end.temp));
    let (d0, d1) = (f64::from(start.duty), f64::from(end.duty));
    Some(d0 + (d1 - d0) * (temp - t0) / (t1 - t0))
}

/// Runs of equal consecutive points as (first, last) indices. The
/// controller always has 7 points, and a curve with fewer levels repeats
/// the last one; the interface draws one handle per run ("6–7").
pub fn runs(points: &[CurvePoint]) -> Vec<(usize, usize)> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (i, point) in points.iter().enumerate() {
        match runs.last_mut() {
            Some((_, last)) if points[*last] == *point => *last = i,
            _ => runs.push((i, i)),
        }
    }
    runs
}

/// The segment that holds a temperature: the last point at or below it
/// and the next point, which is above it. `None` as the end means the
/// temperature lies below the first point or at or above the last one.
/// `at_or_below` says whether a point's temperature is at or below the
/// one asked about.
fn segment(
    points: &[CurvePoint],
    at_or_below: impl Fn(&CurvePoint) -> bool,
) -> Option<(CurvePoint, Option<CurvePoint>)> {
    let first = *points.first()?;
    if points.windows(2).any(|w| w[1].temp < w[0].temp) {
        return None;
    }
    let Some(i) = points.iter().rposition(&at_or_below) else {
        return Some((first, None));
    };
    // Temperatures do not go down, so the next point lies above the
    // temperature asked about, and above points[i].
    Some((points[i], points.get(i + 1).copied()))
}

/// a / b rounded up, for b > 0.
fn ceil_div(a: i64, b: i64) -> i64 {
    a.div_euclid(b) + i64::from(a.rem_euclid(b) != 0)
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

    /// Four levels and points 5–7 copying point 4, as the BIOS writes them.
    fn bios(points: [(u8, u8); 4]) -> Vec<CurvePoint> {
        let mut c = curve(&points);
        c.extend([c[3]; 3]);
        c
    }

    /// The curves of MS-7E61 in the fixture trees and §6.3.
    fn cpu() -> Vec<CurvePoint> {
        bios([(0, 122), (75, 124), (80, 191), (85, 255)])
    }
    fn cpu_bios_default() -> Vec<CurvePoint> {
        bios([(0, 51), (45, 89), (65, 178), (80, 255)])
    }
    fn pump() -> Vec<CurvePoint> {
        bios([(35, 255), (40, 255), (70, 255), (90, 255)])
    }
    fn sys1() -> Vec<CurvePoint> {
        bios([(0, 94), (5, 96), (84, 99), (85, 255)])
    }
    fn sys2() -> Vec<CurvePoint> {
        bios([(0, 104), (5, 107), (84, 109), (85, 255)])
    }
    fn sys3() -> Vec<CurvePoint> {
        bios([(40, 76), (55, 76), (70, 76), (85, 76)])
    }

    fn at(points: &[CurvePoint], millidegrees: i64) -> u8 {
        expected_duty(points, millidegrees).unwrap()
    }

    #[test]
    fn matches_the_readings_of_2026_11_11() {
        // temp1 46 °C: CPU_FAN1 124, SYS_FAN1 98, SYS_FAN2 109, SYS_FAN3 76.
        assert_eq!(at(&cpu(), 46_000), 124);
        assert_eq!(at(&sys1(), 46_000), 98);
        assert_eq!(at(&sys2(), 46_000), 109);
        assert_eq!(at(&sys3(), 46_000), 76);
        assert_eq!(at(&pump(), 46_000), 255);
    }

    #[test]
    fn straight_lines_rounded_up() {
        // CPU: 122 → 124 between 0 and 75 °C; the step to 124 comes past
        // 37.5 °C, where the line reaches 123.
        assert_eq!(at(&cpu(), 1), 123);
        assert_eq!(at(&cpu(), 37_500), 123);
        assert_eq!(at(&cpu(), 37_501), 124);
        // A steep part: 124 → 191 between 75 and 80 °C.
        assert_eq!(at(&cpu(), 77_000), 124 + 27); // 26.8 up
        assert_eq!(at(&cpu(), 80_000), 191);
        assert_eq!(at(&cpu(), 82_500), 223);
        // SYS_FAN2: 107 → 109 between 5 and 84 °C, 108 up to 44.5 °C.
        assert_eq!(at(&sys2(), 44_500), 108);
        assert_eq!(at(&sys2(), 44_501), 109);
        // The default CPU curve of the BIOS between 45 and 65 °C.
        assert_eq!(at(&cpu_bios_default(), 46_000), 94); // 93.45 up
        // Points themselves need no rounding.
        assert_eq!(at(&cpu_bios_default(), 65_000), 178);
        assert_eq!(at(&sys1(), 5_000), 96);
    }

    #[test]
    fn outside_the_points_the_end_values() {
        assert_eq!(at(&cpu(), -5_000), 122);
        assert_eq!(at(&cpu(), 0), 122);
        assert_eq!(at(&cpu(), 120_000), 255);
        assert_eq!(at(&sys3(), 20_000), 76);
        assert_eq!(at(&sys3(), 100_000), 76);
        assert_eq!(at(&pump(), 0), 255);
        assert_eq!(at(&cpu_bios_default(), 95_000), 255);
    }

    #[test]
    fn repeated_points_and_steps() {
        // Points 4–7 at 85 °C: exactly there and above, the last point.
        assert_eq!(at(&sys1(), 84_999), 255); // 99 + 155.84 up
        assert_eq!(at(&sys1(), 85_000), 255);
        assert_eq!(at(&sys1(), 85_001), 255);
        // A step: two points at 40 °C. Below it the lower duty cycle, at
        // and above it the higher one.
        let step = curve(&[(20, 50), (40, 60), (40, 100), (60, 100)]);
        assert_eq!(at(&step, 39_999), 60);
        assert_eq!(at(&step, 40_000), 100);
        assert_eq!(at(&step, 50_000), 100);
        // Every point the same.
        let one = curve(&[(50, 128); 7]);
        for t in [-10_000, 49_999, 50_000, 90_000] {
            assert_eq!(at(&one, t), 128);
        }
    }

    #[test]
    fn no_value_for_curves_the_controller_does_not_have() {
        assert_eq!(expected_duty(&[], 40_000), None);
        let down = curve(&[(50, 100), (40, 120)]);
        assert_eq!(expected_duty(&down, 45_000), None);
        assert_eq!(line_at(&down, 45.0), None);
    }

    #[test]
    fn every_temperature_gives_a_value_between_the_neighbours() {
        // From −50 to 150 °C in steps of 0.1 °C, on every known curve: no
        // panic, a value within the curve's range, and with non-decreasing
        // duty cycles never lower at a higher temperature; and never more
        // than one step above the line.
        let curves = [cpu(), cpu_bios_default(), pump(), sys1(), sys2(), sys3()];
        for c in &curves {
            let low = c.iter().map(|p| p.duty).min().unwrap();
            let high = c.iter().map(|p| p.duty).max().unwrap();
            let mut previous = 0;
            for t in (-50_000..=150_000).step_by(100) {
                let duty = at(c, t);
                assert!((low..=high).contains(&duty), "{t}: {duty}");
                assert!(duty >= previous, "{t}: {duty} after {previous}");
                previous = duty;
                let line = line_at(c, t as f64 / 1000.0).unwrap();
                let gap = f64::from(duty) - line;
                assert!((0.0..1.0).contains(&gap), "{t}: {duty} for the line {line}");
            }
        }
    }

    #[test]
    fn line_for_the_chart() {
        let line = line_at(&sys1(), 46.0).unwrap();
        assert!((line - 97.557).abs() < 0.001, "{line}");
        assert_eq!(line_at(&sys1(), 85.0), Some(255.0));
        assert_eq!(line_at(&cpu(), -3.0), Some(122.0));
    }

    #[test]
    fn runs_of_repeated_points() {
        assert_eq!(runs(&sys1()), [(0, 0), (1, 1), (2, 2), (3, 6)]);
        // Same duty cycle at other temperatures is not a repeat.
        assert_eq!(runs(&sys3()), [(0, 0), (1, 1), (2, 2), (3, 6)]);
        let edited = curve(&[
            (35, 51),
            (50, 64),
            (60, 89),
            (68, 127),
            (78, 191),
            (85, 255),
            (85, 255),
        ]);
        assert_eq!(
            runs(&edited),
            [(0, 0), (1, 1), (2, 2), (3, 3), (4, 4), (5, 6)]
        );
        assert_eq!(runs(&[]), []);
    }
}
