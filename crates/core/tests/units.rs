// SPDX-License-Identifier: GPL-3.0-or-later

//! Numbers and units in Polish and English (docs/PROJEKT.md §5.1, §7 M1):
//! the decimal separator differs, the spaces, signs and decimals do not.

use baseboard_core::profile::VoltageRole;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{MINUS, Numbers, RANGE_DASH, UNIT_SPACE};

fn pl() -> Numbers {
    Numbers::new(",")
}

fn en() -> Numbers {
    Numbers::new(".")
}

/// Both languages, with U+00A0 written as "_" and U+2212 as "~" so that
/// the expected texts show them.
fn both(format: impl Fn(&Numbers) -> String) -> (String, String) {
    let show = |text: String| {
        assert!(!text.contains(' '), "a plain space in {text:?}");
        assert!(!text.contains('-'), "a hyphen in {text:?}");
        text.replace(UNIT_SPACE, "_").replace(MINUS, "~")
    };
    (show(format(&pl())), show(format(&en())))
}

fn pair(pl: &str, en: &str) -> (String, String) {
    (pl.to_owned(), en.to_owned())
}

#[test]
fn readings_with_their_units() {
    let cases = [
        (Kind::Temp, 48.0, "48,0_°C", "48.0_°C"),
        (Kind::Temp, 45.625, "45,6_°C", "45.6_°C"),
        (Kind::Temp, 28.85, "28,9_°C", "28.9_°C"),
        (Kind::Temp, -5.3, "~5,3_°C", "~5.3_°C"),
        (Kind::Temp, -0.04, "0,0_°C", "0.0_°C"),
        (Kind::Voltage, 3.312, "3,31_V", "3.31_V"),
        (Kind::Voltage, 12.096, "12,10_V", "12.10_V"),
        (Kind::Voltage, 2.032, "2,03_V", "2.03_V"),
        // Below 2 V three decimals (D27).
        (Kind::Voltage, 0.944, "0,944_V", "0.944_V"),
        (Kind::Voltage, 1.2, "1,200_V", "1.200_V"),
        (Kind::Voltage, 1.824, "1,824_V", "1.824_V"),
        (Kind::Voltage, 0.215, "0,215_V", "0.215_V"),
        (Kind::Fan, 888.0, "888_rpm", "888_rpm"),
        (Kind::Fan, 1013.0, "1013_rpm", "1013_rpm"),
        (Kind::Power, 13.0, "13,0_W", "13.0_W"),
        (Kind::Power, 12.5, "12,5_W", "12.5_W"),
        // Duty cycles 0–255 in percent (§3.5).
        (Kind::Pwm, 76.0, "30_%", "30_%"),
        (Kind::Pwm, 127.0, "50_%", "50_%"),
        (Kind::Pwm, 255.0, "100_%", "100_%"),
        (Kind::Pwm, 124.0, "49_%", "49_%"),
        (Kind::Pwm, 0.0, "0_%", "0_%"),
    ];
    for (kind, value, pl, en) in cases {
        assert_eq!(
            both(|n| n.value(kind, value)),
            pair(pl, en),
            "{kind:?} {value}"
        );
    }
}

#[test]
fn ranges_have_the_unit_once() {
    assert_eq!(
        both(|n| n.range(Kind::Temp, 39.6, 48.4)),
        pair("39,6–48,4_°C", "39.6–48.4_°C")
    );
    assert_eq!(
        both(|n| n.range(Kind::Fan, 888.0, 890.0)),
        pair("888–890_rpm", "888–890_rpm")
    );
    assert_eq!(
        both(|n| n.range(Kind::Voltage, 12.096, 12.112)),
        pair("12,10–12,11_V", "12.10–12.11_V")
    );
    assert_eq!(
        both(|n| n.range(Kind::Temp, -1.0, 2.0)),
        pair("~1,0–2,0_°C", "~1.0–2.0_°C")
    );
    assert_eq!(
        both(|n| n.range(Kind::Pwm, 76.0, 255.0)),
        pair("30–100_%", "30–100_%")
    );
    assert!(pl().range(Kind::Fan, 1.0, 2.0).contains(RANGE_DASH));
}

/// No range while both ends look the same, also after rounding: it shows
/// once the values part (§5.1).
#[test]
fn no_range_while_the_ends_look_the_same() {
    assert_eq!(both(|n| n.range(Kind::Voltage, 12.1, 12.1)), pair("", ""));
    assert_eq!(both(|n| n.range(Kind::Fan, 0.0, 0.0)), pair("", ""));
    // 12,101 and 12,104 V both show as 12,10 V.
    assert_eq!(
        both(|n| n.range(Kind::Voltage, 12.101, 12.104)),
        pair("", "")
    );
    assert_eq!(
        both(|n| n.range(Kind::Voltage, 12.10, 12.11)),
        pair("12,10–12,11_V", "12.10–12.11_V")
    );
}

/// Voltages below 2 V get three decimals; the nominal voltage decides when
/// there is one, the larger end decides for a range (D27).
#[test]
fn low_voltages_have_three_decimals() {
    assert_eq!(
        both(|n| n.range(Kind::Voltage, 0.656, 1.296)),
        pair("0,656–1,296_V", "0.656–1.296_V")
    );
    // A range across 2 V: both ends like the larger one.
    assert_eq!(
        both(|n| n.range(Kind::Voltage, 1.9, 2.1)),
        pair("1,90–2,10_V", "1.90–2.10_V")
    );
    // A +3.3 V rail reading low keeps two decimals; without a nominal
    // value the reading decides.
    assert_eq!(
        both(|n| n.value_of(Kind::Voltage, 1.98, Some(3.3))),
        pair("1,98_V", "1.98_V")
    );
    assert_eq!(
        both(|n| n.value_of(Kind::Voltage, 1.98, None)),
        pair("1,980_V", "1.980_V")
    );
    assert_eq!(
        both(|n| n.range_of(Kind::Voltage, 1.9, 1.95, Some(3.3))),
        pair("1,90–1,95_V", "1.90–1.95_V")
    );
    // Other kinds do not care.
    assert_eq!(
        both(|n| n.value_of(Kind::Temp, 1.5, None)),
        pair("1,5_°C", "1.5_°C")
    );
}

#[test]
fn rails_are_named_by_their_nominal_voltage() {
    let name = |role: VoltageRole| move |n: &Numbers| n.nominal(role.nominal().unwrap());
    assert_eq!(both(name(VoltageRole::P12v)), pair("+12_V", "+12_V"));
    assert_eq!(both(name(VoltageRole::P5v)), pair("+5_V", "+5_V"));
    assert_eq!(both(name(VoltageRole::P3v3)), pair("+3,3_V", "+3.3_V"));
    assert_eq!(both(|n| n.nominal(-12.0)), pair("~12_V", "~12_V"));
    assert_eq!(VoltageRole::Vcore.nominal(), None);
}

#[test]
fn deviation_is_signed_and_tolerance_has_plus_minus() {
    let cases = [
        (0.8, "+0,8_%", "+0.8_%"),
        (-0.8, "~0,8_%", "~0.8_%"),
        (0.3636, "+0,4_%", "+0.4_%"),
        (0.0, "0,0_%", "0.0_%"),
        (-0.04, "0,0_%", "0.0_%"),
        (-6.0, "~6,0_%", "~6.0_%"),
    ];
    for (percent, pl, en) in cases {
        assert_eq!(both(|n| n.deviation(percent)), pair(pl, en), "{percent}");
    }
    assert_eq!(both(|n| n.tolerance(5.0)), pair("±5_%", "±5_%"));
    assert_eq!(both(|n| n.tolerance(2.5)), pair("±2,5_%", "±2.5_%"));
}

#[test]
fn other_separators_and_an_empty_one() {
    // Whatever LC_NUMERIC gives is used as it is; an empty one is ".".
    assert_eq!(Numbers::new("٫").value(Kind::Temp, 48.0), "48٫0\u{a0}°C");
    assert_eq!(Numbers::new(""), en());
}
