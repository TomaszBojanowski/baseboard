// SPDX-License-Identifier: GPL-3.0-or-later

//! A warning on top of a page, in plain words (D63, D64): an icon, a short
//! title and one or two sentences. The Overview shows it for a rail
//! outside its tolerance, the Fans page for a stopped CPU fan (D65).

use adw::prelude::*;
use baseboard_core::sensors::Kind;
use baseboard_core::summary::OutsideRail;
use baseboard_core::units::Numbers;
use gettextrs::gettext;

use crate::overview;

/// The warning about a stopped CPU fan (D65), also in the Fans row of the
/// Overview and in the status row of the System page.
pub fn cpu_fan_stopped() -> String {
    // Translators: a CPU fan turned before and shows 0 rpm now (D65), on the
    // Overview, the Fans page and the System page.
    gettext("CPU fan stopped")
}

/// Why the app says the CPU fan stopped, under the warning on the Fans page.
pub fn cpu_fan_stopped_why() -> String {
    // Translators: under "CPU fan stopped" on the Fans page (D65).
    gettext("It turned before and has shown 0 rpm for at least 10 seconds.")
}

/// A rail outside its tolerance, in a few words: the row of the Overview
/// and the status row of the System page.
pub fn rail_outside(rail: &OutsideRail, numbers: &Numbers) -> String {
    let text = if rail.too_low() {
        // Translators: {rail} is a supply rail, e.g. "+12 V".
        gettext("Voltage {rail} below the normal range")
    } else {
        // Translators: {rail} is a supply rail, e.g. "+12 V".
        gettext("Voltage {rail} above the normal range")
    };
    text.replace("{rail}", &overview::name(&rail.name, numbers))
}

/// The warning on top of the Overview about a rail outside its tolerance:
/// its title with the value, and the normal range with advice that does not
/// overstate a reading of the board (D64).
pub fn rail_warning(rail: &OutsideRail, numbers: &Numbers) -> (String, String) {
    let value = numbers.value_of(Kind::Voltage, rail.value, Some(rail.nominal));
    let title = if rail.too_low() {
        // Translators: {rail} is a supply rail, e.g. "+12 V", and {value}
        // what the board reads, e.g. "11.20 V".
        gettext("Voltage {rail} below the normal range: {value}")
    } else {
        // Translators: {rail} is a supply rail, e.g. "+12 V", and {value}
        // what the board reads, e.g. "12.80 V".
        gettext("Voltage {rail} above the normal range: {value}")
    }
    .replace("{rail}", &overview::name(&rail.name, numbers))
    .replace("{value}", &value);
    let range = numbers.range_of(Kind::Voltage, rail.low, rail.high, Some(rail.nominal));
    let text = gettext(
        // Translators: under the warning about a supply rail; {range} is
        // the normal range, e.g. "11.40–12.60 V".
        "The normal range is {range}. Board readings can be inaccurate; if the computer is \
         unstable, it is worth checking the power supply.",
    )
    .replace("{range}", &range);
    (title, text)
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

    /// The title shown, or `None` while hidden (tests).
    #[cfg(test)]
    pub fn shown(&self) -> Option<(String, String)> {
        self.widget
            .is_visible()
            .then(|| (self.title.label().into(), self.text.label().into()))
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
        assert_eq!(
            rail_outside(&low, &numbers),
            "Voltage +12\u{a0}V below the normal range"
        );
        let (title, text) = rail_warning(&low, &numbers);
        assert_eq!(
            title,
            "Voltage +12\u{a0}V below the normal range: 11,20\u{a0}V"
        );
        assert!(
            text.starts_with("The normal range is 11,40–12,60\u{a0}V. Board readings can be"),
            "{text}"
        );
        let high = OutsideRail { value: 12.8, ..low };
        assert!(rail_outside(&high, &numbers).contains("above"));
    }
}
