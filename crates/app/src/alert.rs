// SPDX-License-Identifier: GPL-3.0-or-later

//! Warnings in plain words (D63, D64, D67): the texts of the problems of
//! the status sentence on the Overview and the System page, and the
//! warning box on top of the Fans page for a stopped CPU fan (D65).

use adw::prelude::*;
use baseboard_core::sensors::Kind;
use baseboard_core::summary::{HotPart, OutsideRail, Problem};
use baseboard_core::units::Numbers;
use gettextrs::{gettext, ngettext};

use crate::overview;

/// The warning about a stopped CPU fan (D65): on the Overview, on its
/// fans tile, on the Fans page and in the status row of the System page.
pub fn cpu_fan_stopped() -> String {
    // Translators: a CPU fan was spinning and shows 0 rpm now, on the
    // Overview, the Fans page and the System page.
    gettext("CPU fan stopped")
}

/// Why the app says the CPU fan stopped, under the warning on the Fans page.
pub fn cpu_fan_stopped_why() -> String {
    // Translators: under "CPU fan stopped" on the Fans page.
    gettext("It was spinning earlier and has shown 0 rpm for at least 10 seconds.")
}

/// The status sentence about a rail outside its tolerance:
/// its title with the value, and the normal range with advice that does not
/// overstate a reading of the board (D64).
pub fn rail_warning(rail: &OutsideRail, numbers: &Numbers) -> (String, String) {
    let value = numbers.value_of(Kind::Voltage, rail.value, Some(rail.nominal));
    let title = if rail.too_low() {
        // Translators: {rail} is a supply rail, e.g. "+12 V", and {value}
        // what the board reads, e.g. "11.20 V".
        gettext("{rail} below normal: {value}")
    } else {
        // Translators: {rail} is a supply rail, e.g. "+12 V", and {value}
        // what the board reads, e.g. "12.80 V".
        gettext("{rail} above normal: {value}")
    }
    .replace("{rail}", &overview::name(&rail.name, numbers))
    .replace("{value}", &value);
    let range = numbers.range_of(Kind::Voltage, rail.low, rail.high, Some(rail.nominal));
    let text = gettext(
        // Translators: under the warning about a supply rail; {range} is
        // the normal range, e.g. "11.40–12.60 V".
        "The normal range is {range}. Board readings can be inaccurate; if the computer is \
         unstable, consider checking the power supply.",
    )
    .replace("{range}", &range);
    (title, text)
}

/// A temperature close to its limit, as the status sentence says it (D67):
/// the processor in its own words (it slows down at its limit, and the
/// fans may already run at full speed), any other part by its name.
pub fn hot_warning(part: &HotPart, fans_at_full: bool, numbers: &Numbers) -> (String, String) {
    let value = crate::history_chart::degrees(part.value);
    let limit = crate::history_chart::degrees(part.limit);
    if part.processor {
        // Translators: the status sentence on top of the Overview; {value}
        // is e.g. "93 °C".
        let title = gettext("Processor is very hot: {value}").replace("{value}", &value);
        let text = if fans_at_full {
            // Translators: under "Processor is very hot"; {limit} is the
            // threshold of the board profile, e.g. "95 °C".
            gettext(
                "At {limit} it starts to slow down so as not to overheat. The fans run at full speed.",
            )
        } else {
            gettext("At {limit} it starts to slow down so as not to overheat.")
        };
        (title, text.replace("{limit}", &limit))
    } else {
        // Translators: the status sentence on top of the Overview; {name}
        // is a reading, e.g. "Composite", and {value} e.g. "87 °C".
        let title = gettext("Very hot: {name}, {value}")
            .replace("{name}", &overview::name(&part.name, numbers))
            .replace("{value}", &value);
        // Translators: under "Very hot"; {limit} is e.g. "94 °C".
        let text = gettext("The limit is {limit}.").replace("{limit}", &limit);
        (title, text)
    }
}

// The status sentence on the Overview and the status row of the System
// page say the same (D67).

pub fn everything_fine() -> String {
    // Translators: the status sentence on top of the Overview and the
    // System page.
    gettext("Everything is fine")
}

/// A board without a profile, or a system that only reads the fans.
pub fn readings_work() -> String {
    // Translators: the status sentence on top of the Overview and the
    // System page on a board without a profile.
    gettext("Readings work")
}

/// Under the most important problem: how many more there are.
pub fn more_problems(more: u32) -> String {
    // Translators: under the most important problem on the Overview and the
    // System page.
    ngettext("And {} more problem", "And {} more problems", more).replace("{}", &more.to_string())
}

/// A problem of the status sentence: its title and text.
pub fn problem(problem: &Problem, fans_at_full: bool, numbers: &Numbers) -> (String, String) {
    match problem {
        Problem::CpuFanStopped => (cpu_fan_stopped(), cpu_fan_stopped_why()),
        Problem::Hot(part) => hot_warning(part, fans_at_full, numbers),
        Problem::Rail(rail) => rail_warning(rail, numbers),
    }
}

/// The warning box: hidden until there is something to say.
pub struct Alert {
    pub widget: gtk::Box,
    title: gtk::Label,
    text: gtk::Label,
}

impl Alert {
    pub fn new() -> Self {
        let widget = gtk::Box::builder()
            .spacing(12)
            .visible(false)
            .accessible_role(gtk::AccessibleRole::Alert)
            .css_classes(["alert-bar"])
            .build();
        let icon = gtk::Image::builder()
            .icon_name("dialog-warning-symbolic")
            .valign(gtk::Align::Start)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["warning"])
            .build();
        let texts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();
        let title = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .natural_wrap_mode(gtk::NaturalWrapMode::Word)
            .css_classes(["heading"])
            .build();
        let text = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .natural_wrap_mode(gtk::NaturalWrapMode::Word)
            .visible(false)
            .build();
        texts.append(&title);
        texts.append(&text);
        widget.append(&icon);
        widget.append(&texts);
        widget.update_relation(&[
            gtk::accessible::Relation::LabelledBy(&[title.upcast_ref()]),
            gtk::accessible::Relation::DescribedBy(&[text.upcast_ref()]),
        ]);
        Self {
            widget,
            title,
            text,
        }
    }

    /// Shows `title` and `text` (none when empty); `None` hides the box.
    pub fn set(&self, message: Option<(&str, &str)>) {
        let Some((title, text)) = message else {
            self.widget.set_visible(false);
            return;
        };
        if self.title.label() != title {
            self.title.set_label(title);
        }
        if self.text.label() != text {
            self.text.set_label(text);
        }
        self.text.set_visible(!text.is_empty());
        self.widget.set_visible(true);
    }
}

#[cfg(test)]
mod tests {
    use baseboard_core::overview::Name;
    use baseboard_core::profile::VoltageRole;

    use super::*;

    /// A rail below its range says so with the value and the range, and
    /// does not tell the user that the power supply is broken (D64).
    #[test]
    fn rail_outside_its_range() {
        let numbers = Numbers::new(",");
        let low = OutsideRail {
            name: Name::Voltage(VoltageRole::P12v),
            value: 11.2,
            nominal: 12.0,
            low: 11.4,
            high: 12.6,
        };
        let (title, text) = rail_warning(&low, &numbers);
        assert_eq!(title, "+12\u{a0}V below normal: 11,20\u{a0}V");
        assert!(
            text.starts_with("The normal range is 11,40–12,60\u{a0}V. Board readings can be"),
            "{text}"
        );
        let high = OutsideRail { value: 12.8, ..low };
        assert!(rail_warning(&high, &numbers).0.contains("above"));
    }
}
