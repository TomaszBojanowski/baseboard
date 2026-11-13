// SPDX-License-Identifier: GPL-3.0-or-later

//! Numbers and units for display (docs/PROJEKT.md §5.1) and the duty-cycle
//! conversion (§3.5, D20). All rules sit here: the precision of each kind
//! of reading, the no-break space before a unit (D25), the minus sign, the
//! range dash. The decimal separator of the locale (`LC_NUMERIC`) comes
//! from the app (D24), so this module has no locale of its own.

use crate::sensors::Kind;

/// Between a number and its unit or the percent sign (D25).
pub const UNIT_SPACE: char = '\u{a0}';
/// The minus sign.
pub const MINUS: char = '\u{2212}';
/// Between the ends of a range.
pub const RANGE_DASH: char = '\u{2013}';

/// Duty cycle in percent → 0–255, cut off like the BIOS does:
/// ⌊p · 255 / 100⌋ (§3.5). Above 100 counts as 100.
pub fn duty_from_percent(percent: u8) -> u8 {
    let percent = u32::from(percent.min(100));
    // At most 255, so the conversion cannot fail.
    u8::try_from(percent * 255 / 100).unwrap_or(u8::MAX)
}

/// Duty cycle 0–255 → percent, to the nearest whole percent, halves up:
/// ⌊v · 100 / 255 + ½⌋ (§3.5). Every whole percent cut off by
/// [`duty_from_percent`] comes back as itself.
pub fn duty_to_percent(duty: u8) -> u8 {
    // ⌊v · 100 / 255 + ½⌋ = ⌊(200 v + 255) / 510⌋, in integers.
    u8::try_from((u32::from(duty) * 200 + 255) / 510).unwrap_or(100)
}

/// How a kind of reading is shown: decimals and unit (§5.1). Duty cycles
/// are shown in percent. `level` decides the decimals of a voltage (D27):
/// three below 2 V (processor and graphics card rails), two above.
fn spec(kind: Kind, level: f64) -> (usize, &'static str) {
    match kind {
        Kind::Temp => (1, "°C"),
        Kind::Fan => (0, "rpm"),
        Kind::Voltage if level.abs() < LOW_VOLTAGE => (3, "V"),
        Kind::Voltage => (2, "V"),
        Kind::Power => (1, "W"),
        Kind::Pwm => (0, "%"),
    }
}

/// A reading whose value cannot be read (§5.1): an en dash.
pub const UNREADABLE: &str = "–";

/// Below this, in volts, a voltage gets three decimals (D27).
const LOW_VOLTAGE: f64 = 2.0;

/// Decimals of a voltage's deviation from its nominal value, in percent.
const DEVIATION_DECIMALS: usize = 1;
/// Most decimals of a nominal voltage or a tolerance; trailing zeros go.
const SHORT_DECIMALS: usize = 2;

/// Formats numbers with the locale's decimal separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Numbers {
    decimal: String,
}

impl Numbers {
    /// `decimal` is the separator of `LC_NUMERIC`, e.g. "," or "."; an
    /// empty one counts as ".".
    pub fn new(decimal: &str) -> Self {
        let decimal = if decimal.is_empty() { "." } else { decimal };
        Self {
            decimal: decimal.to_owned(),
        }
    }

    /// A reading with its unit: "48,0 °C", "3,31 V", "0,976 V", "888 rpm",
    /// "45 %". A duty cycle is given 0–255 and shown in percent.
    pub fn value(&self, kind: Kind, value: f64) -> String {
        self.value_of(kind, value, None)
    }

    /// Like [`value`](Self::value), for a rail with a nominal voltage: the
    /// nominal value, not the reading, decides the decimals, so a rail keeps
    /// them when the reading crosses 2 V.
    pub fn value_of(&self, kind: Kind, value: f64, nominal: Option<f64>) -> String {
        if !value.is_finite() {
            return UNREADABLE.to_owned();
        }
        let (decimals, unit) = spec(kind, nominal.unwrap_or(value));
        format!("{}{UNIT_SPACE}{unit}", self.number(kind, value, decimals))
    }

    /// The range of a reading, with the unit once: "39,6–48,4 °C". Both
    /// ends have the decimals of the larger one.
    pub fn range(&self, kind: Kind, min: f64, max: f64) -> String {
        self.range_of(kind, min, max, None)
    }

    /// Like [`range`](Self::range), for a rail with a nominal voltage.
    /// Empty while both ends look the same ("12,10–12,10 V", "0–0 rpm"):
    /// the range shows once the values part (§5.1).
    pub fn range_of(&self, kind: Kind, min: f64, max: f64, nominal: Option<f64>) -> String {
        if !min.is_finite() || !max.is_finite() {
            return String::new();
        }
        let level = nominal.unwrap_or_else(|| min.abs().max(max.abs()));
        let (decimals, unit) = spec(kind, level);
        let (low, high) = (
            self.number(kind, min, decimals),
            self.number(kind, max, decimals),
        );
        if low == high {
            return String::new();
        }
        format!("{low}{RANGE_DASH}{high}{UNIT_SPACE}{unit}")
    }

    /// The nominal voltage of a rail, which names it (D26): "+12 V",
    /// "+3,3 V", "−12 V".
    pub fn nominal(&self, volts: f64) -> String {
        format!("{}{UNIT_SPACE}V", self.signed(volts, SHORT_DECIMALS, true))
    }

    /// A voltage's deviation from nominal, always signed: "+0,8 %",
    /// "−0,8 %"; zero has no sign.
    pub fn deviation(&self, percent: f64) -> String {
        format!(
            "{}{UNIT_SPACE}%",
            self.signed(percent, DEVIATION_DECIMALS, false)
        )
    }

    /// A tolerance: "±5 %", "±2,5 %".
    pub fn tolerance(&self, percent: f64) -> String {
        format!(
            "±{}{UNIT_SPACE}%",
            self.trimmed(percent.abs(), SHORT_DECIMALS)
        )
    }

    fn number(&self, kind: Kind, value: f64, decimals: usize) -> String {
        let value = match kind {
            Kind::Pwm => f64::from(duty_to_percent(duty(value))),
            _ => value,
        };
        self.plain(value, decimals)
    }

    /// The number with the minus sign where it is below zero after
    /// rounding (no "−0,0").
    fn plain(&self, value: f64, decimals: usize) -> String {
        let digits = self.digits(value.abs(), decimals);
        if value < 0.0 && !is_zero(&digits) {
            format!("{MINUS}{digits}")
        } else {
            digits
        }
    }

    /// With "+" or "−" unless it rounds to zero. `trim` drops trailing
    /// zeros of the decimals.
    fn signed(&self, value: f64, decimals: usize, trim: bool) -> String {
        let digits = if trim {
            self.trimmed(value.abs(), decimals)
        } else {
            self.digits(value.abs(), decimals)
        };
        if is_zero(&digits) {
            digits
        } else if value < 0.0 {
            format!("{MINUS}{digits}")
        } else {
            format!("+{digits}")
        }
    }

    /// At most `decimals` decimals, without trailing zeros: 12 → "12",
    /// 3.30 → "3,3".
    fn trimmed(&self, value: f64, decimals: usize) -> String {
        let text = format!("{value:.decimals$}");
        let text = if text.contains('.') {
            text.trim_end_matches('0').trim_end_matches('.')
        } else {
            &text
        };
        text.replacen('.', &self.decimal, 1)
    }

    fn digits(&self, value: f64, decimals: usize) -> String {
        format!("{value:.decimals$}").replacen('.', &self.decimal, 1)
    }
}

/// A reading in whole units, as the cards and tiles of the Overview and
/// the processor's card of the Fans page show it (D67): "68 °C", "12 W",
/// "1013 rpm". "All Sensors" has the exact values. No decimals, so no
/// separator of the locale; a half rounds away from zero, as people round
/// (92.5 °C is 93 °C), not to the even number as formatting does.
pub fn whole(kind: Kind, value: f64) -> String {
    if !value.is_finite() {
        return UNREADABLE.to_owned();
    }
    let (_, unit) = spec(kind, value);
    format!(
        "{}{UNIT_SPACE}{unit}",
        Numbers::new(".").number(kind, value.round(), 0)
    )
}

/// A duty cycle reading as 0–255.
fn duty(value: f64) -> u8 {
    // Clamped first, so the cast cannot overflow.
    value.round().clamp(0.0, 255.0) as u8
}

fn is_zero(digits: &str) -> bool {
    digits.chars().all(|c| !c.is_ascii_digit() || c == '0')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{CPU_MIN_DUTY_FLOOR, PUMP_MIN_DUTY_FLOOR};

    #[test]
    fn duty_cycle_like_the_bios() {
        assert_eq!(duty_from_percent(30), 76);
        assert_eq!(duty_from_percent(50), 127);
        assert_eq!(duty_from_percent(100), 255);
        assert_eq!(duty_from_percent(0), 0);
        assert_eq!(duty_from_percent(150), 255);
        // The fixed floors of B5.
        assert_eq!(duty_from_percent(CPU_MIN_DUTY_FLOOR), 51);
        assert_eq!(duty_from_percent(PUMP_MIN_DUTY_FLOOR), 102);
        // The other way: tests/units.rs, as the pages show it.
    }

    /// Whole units for the cards (D67): rounded, with the unit after a
    /// non-breaking space, the minus sign, no "−0", a dash when unreadable.
    #[test]
    fn whole_units() {
        assert_eq!(whole(Kind::Power, 12.04), "12\u{a0}W");
        assert_eq!(whole(Kind::Power, 11.6), "12\u{a0}W");
        assert_eq!(whole(Kind::Temp, 68.2), "68\u{a0}°C");
        assert_eq!(whole(Kind::Temp, 92.5), "93\u{a0}°C");
        assert_eq!(whole(Kind::Temp, -4.6), "\u{2212}5\u{a0}°C");
        assert_eq!(whole(Kind::Temp, -0.3), "0\u{a0}°C");
        assert_eq!(whole(Kind::Fan, 1013.4), "1013\u{a0}rpm");
        assert_eq!(whole(Kind::Temp, f64::NAN), UNREADABLE);
    }

    #[test]
    fn every_whole_percent_comes_back() {
        for percent in 0..=100 {
            assert_eq!(
                duty_to_percent(duty_from_percent(percent)),
                percent,
                "{percent} %"
            );
        }
        // And the percent of every duty cycle is the nearest one.
        for duty in 0..=255u8 {
            let exact = f64::from(duty) * 100.0 / 255.0;
            assert!(
                (f64::from(duty_to_percent(duty)) - exact).abs() <= 0.5,
                "{duty}"
            );
        }
    }
}
