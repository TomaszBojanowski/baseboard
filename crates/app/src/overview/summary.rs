// SPDX-License-Identifier: GPL-3.0-or-later

//! The top of the Overview page (docs/PROJEKT.md §5.1, D64), from
//! `core::summary`: a warning about a rail outside its tolerance, three
//! large numbers in the style of the Fans page, and a short list (power,
//! fans, memory, drives, every reading). A number or a row opens the page
//! "All Sensors" on its group; the fans row opens the Fans page.

use adw::prelude::*;
use baseboard_core::overview::GroupKind;
use baseboard_core::sensors::Kind;
use baseboard_core::summary::{HeroKind, OutsideRail, Summary, Warmest};
use baseboard_core::units::Numbers;
use gettextrs::{gettext, ngettext, pgettext};

use crate::alert::{self, Alert};

/// The key of a group in the action `win.show-sensors`; an empty key opens
/// the page at its top.
pub fn group_key(kind: GroupKind) -> &'static str {
    match kind {
        GroupKind::Cpu => "cpu",
        GroupKind::Board => "board",
        GroupKind::Fans => "fans",
        GroupKind::Voltages => "voltages",
        GroupKind::Gpu => "gpu",
        GroupKind::Disks => "disks",
        GroupKind::Memory => "memory",
        GroupKind::Other => "other",
    }
}

/// The group of a key of `win.show-sensors`; `None` for the top.
pub fn group_of_key(key: &str) -> Option<GroupKind> {
    [
        GroupKind::Cpu,
        GroupKind::Board,
        GroupKind::Fans,
        GroupKind::Voltages,
        GroupKind::Gpu,
        GroupKind::Disks,
        GroupKind::Memory,
        GroupKind::Other,
    ]
    .into_iter()
    .find(|kind| group_key(*kind) == key)
}

fn hero_name(kind: HeroKind) -> String {
    match kind {
        HeroKind::Cpu => gettext("Processor"),
        HeroKind::Gpu => gettext("Graphics card"),
        HeroKind::Board => gettext("Mainboard"),
    }
}

/// A temperature in whole degrees, as the large number of the Fans page.
fn whole_degrees(value: f64) -> String {
    if value.is_finite() {
        format!("{}", value.round() as i64)
    } else {
        "–".to_owned()
    }
}

/// One large number: a flat button over its name, value and unit.
struct HeroWidgets {
    kind: HeroKind,
    button: gtk::Button,
    value: gtk::Label,
}

impl HeroWidgets {
    fn new(kind: HeroKind) -> Self {
        let name = gtk::Label::builder()
            .label(hero_name(kind))
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["hero-label"])
            .build();
        let value = gtk::Label::builder()
            .css_classes(["hero-value", "numeric"])
            .build();
        let unit = gtk::Label::builder()
            .label("°C")
            .valign(gtk::Align::Start)
            .css_classes(["hero-unit"])
            .build();
        let number = gtk::Box::builder().halign(gtk::Align::Center).build();
        number.append(&value);
        number.append(&unit);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Start)
            .build();
        content.append(&name);
        content.append(&number);
        let button = gtk::Button::builder()
            .child(&content)
            .visible(false)
            .css_classes(["flat", "hero-button"])
            .build();
        button.update_property(&[gtk::accessible::Property::Description(&gettext(
            // Translators: read by screen readers for a large number on the
            // Overview, which opens the page "All Sensors" on its group.
            "Shows every reading of the group",
        ))]);
        Self {
            kind,
            button,
            value,
        }
    }
}

/// A row of the short list: a title, a value and an arrow, and an icon
/// before them for the power rails and the fans.
struct SummaryRow {
    row: adw::ActionRow,
    icon: Option<gtk::Image>,
    value: gtk::Label,
}

impl SummaryRow {
    fn new(list: &gtk::ListBox, title: &str, icon: bool) -> Self {
        let row = adw::ActionRow::builder()
            .use_markup(false)
            .title(title)
            .activatable(true)
            .visible(false)
            .build();
        // Put before the title only while it shows: a hidden prefix would
        // still move the title.
        let icon = icon.then(|| {
            gtk::Image::builder()
                .valign(gtk::Align::Center)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build()
        });
        let value = gtk::Label::builder()
            .valign(gtk::Align::Center)
            .css_classes(["numeric"])
            .build();
        row.add_suffix(&value);
        row.add_suffix(&chevron());
        list.append(&row);
        Self { row, icon, value }
    }

    fn opens(self, action: &str, target: Option<&str>) -> Self {
        self.row.set_action_name(Some(action));
        if let Some(target) = target {
            self.row.set_action_target_value(Some(&target.to_variant()));
        }
        self
    }

    /// Shows `value` and `subtitle` (none when empty); the icon warns or
    /// says that all is well, or is not there.
    fn show(&self, value: &str, subtitle: &str, icon: Option<bool>) {
        if self.value.label() != value {
            self.value.set_label(value);
        }
        if self.row.subtitle().as_deref().unwrap_or_default() != subtitle {
            self.row.set_subtitle(subtitle);
        }
        if let Some(image) = &self.icon {
            match (icon.is_some(), image.parent().is_some()) {
                (true, false) => self.row.add_prefix(image),
                (false, true) => self.row.remove(image),
                _ => {}
            }
            match icon {
                Some(true) => {
                    image.set_icon_name(Some("dialog-warning-symbolic"));
                    image.set_css_classes(&["warning"]);
                }
                Some(false) => {
                    image.set_icon_name(Some("object-select-symbolic"));
                    image.set_css_classes(&["success"]);
                }
                None => {}
            }
        }
        // The title with the value; the subtitle is the description.
        let label = if value.is_empty() {
            self.row.title().to_string()
        } else {
            format!("{}: {value}", self.row.title())
        };
        self.row
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Label(&label)]);
        self.row.set_visible(true);
    }
}

fn chevron() -> gtk::Image {
    gtk::Image::builder()
        .icon_name("go-next-symbolic")
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .css_classes(["dimmed"])
        .build()
}

/// The widgets of the top of the page.
pub struct SummaryWidgets {
    alert: Alert,
    heroes_box: gtk::Box,
    heroes: Vec<HeroWidgets>,
    power: SummaryRow,
    fans: SummaryRow,
    memory: SummaryRow,
    disks: SummaryRow,
}

impl SummaryWidgets {
    /// Fills `column` (the warning on top), `heroes`, `list` and `all`.
    pub fn new(
        column: &gtk::Box,
        heroes: &gtk::Box,
        list: &gtk::ListBox,
        all: &gtk::ListBox,
    ) -> Self {
        let alert = Alert::new();
        column.prepend(&alert.widget);
        let heroes_widgets: Vec<HeroWidgets> = [HeroKind::Cpu, HeroKind::Gpu, HeroKind::Board]
            .into_iter()
            .map(HeroWidgets::new)
            .collect();
        for hero in &heroes_widgets {
            heroes.append(&hero.button);
        }
        let show = "win.show-sensors";
        let power = SummaryRow::new(list, &gettext("Power"), true)
            .opens(show, Some(group_key(GroupKind::Voltages)));
        let fans = SummaryRow::new(list, &gettext("Fans"), true).opens("win.show-fans", None);
        let memory = SummaryRow::new(list, &pgettext("system memory", "Memory"), false)
            .opens(show, Some(group_key(GroupKind::Memory)));
        let disks = SummaryRow::new(list, &gettext("Disks"), false)
            .opens(show, Some(group_key(GroupKind::Disks)));
        let every = SummaryRow::new(all, &gettext("All sensors"), false).opens(show, Some(""));
        every.show(
            "",
            &gettext("Every reading with its source and its range since the app started"),
            None,
        );
        list.update_property(&[gtk::accessible::Property::Label(&gettext(
            // Translators: read by screen readers for the short list of the
            // Overview.
            "Summary",
        ))]);
        Self {
            alert,
            heroes_box: heroes.clone(),
            heroes: heroes_widgets,
            power,
            fans,
            memory,
            disks,
        }
    }

    /// Smaller numbers in a narrow window.
    pub fn set_narrow(&self, narrow: bool) {
        if narrow {
            self.heroes_box.add_css_class("narrow");
        } else {
            self.heroes_box.remove_css_class("narrow");
        }
    }

    pub fn update(&self, summary: &Summary, numbers: &Numbers) {
        for widgets in &self.heroes {
            let hero = summary.heroes.iter().find(|h| h.kind == widgets.kind);
            widgets.button.set_visible(hero.is_some());
            let Some(hero) = hero else {
                continue;
            };
            let value = whole_degrees(hero.value);
            if widgets.value.label() != value {
                widgets.value.set_label(&value);
                let text = numbers.value(Kind::Temp, hero.value.round());
                widgets
                    .button
                    .update_property(&[gtk::accessible::Property::Label(&format!(
                        "{}: {text}",
                        hero_name(hero.kind)
                    ))]);
            }
            let key = group_key(hero.group).to_variant();
            if widgets.button.action_target_value().as_ref() != Some(&key) {
                widgets.button.set_action_name(Some("win.show-sensors"));
                widgets.button.set_action_target_value(Some(&key));
            }
        }
        self.heroes_box.set_visible(!summary.heroes.is_empty());

        match &summary.power {
            None => self.power.row.set_visible(false),
            Some(rails) => self.show_power(rails, numbers),
        }
        let first = summary.power.as_deref().and_then(<[_]>::first);
        match first {
            Some(rail) => {
                let (title, text) = alert::rail_warning(rail, numbers);
                self.alert.set(Some((&title, &text)));
            }
            None => self.alert.set(None),
        }

        match summary.fans {
            None => self.fans.row.set_visible(false),
            Some(fans) => {
                let count = u32::try_from(fans.turning).unwrap_or(u32::MAX);
                // Translators: how many fans turn, on the Overview.
                let value =
                    ngettext("{} running", "{} running", count).replace("{}", &count.to_string());
                if fans.cpu_stopped {
                    self.fans
                        .show(&value, &alert::cpu_fan_stopped(), Some(true));
                } else {
                    self.fans.show(&value, "", None);
                }
            }
        }

        show_warmest(&self.memory, summary.memory, numbers, |count| {
            // Translators: under the warmest memory module on the Overview.
            ngettext("Warmest of {} module", "Warmest of {} modules", count)
        });
        show_warmest(&self.disks, summary.disks, numbers, |count| {
            // Translators: under the warmest drive on the Overview.
            ngettext("Warmest of {} drive", "Warmest of {} drives", count)
        });
    }

    fn show_power(&self, rails: &[OutsideRail], numbers: &Numbers) {
        match rails {
            [] => self.power.show(&gettext("In order"), "", Some(false)),
            [rail] => self.power.show(
                &numbers.value_of(Kind::Voltage, rail.value, Some(rail.nominal)),
                &alert::rail_outside(rail, numbers),
                Some(true),
            ),
            more => {
                let count = u32::try_from(more.len()).unwrap_or(u32::MAX);
                let text = ngettext(
                    "{} voltage outside the normal range",
                    "{} voltages outside the normal range",
                    count,
                )
                .replace("{}", &count.to_string());
                self.power.show("", &text, Some(true));
            }
        }
    }

    /// The warning shown on top, if any (tests).
    #[cfg(test)]
    pub fn alert(&self) -> Option<(String, String)> {
        self.alert.shown()
    }
}

fn show_warmest(
    row: &SummaryRow,
    warmest: Option<Warmest>,
    numbers: &Numbers,
    of: impl Fn(u32) -> String,
) {
    let Some(warmest) = warmest else {
        row.row.set_visible(false);
        return;
    };
    let count = u32::try_from(warmest.count).unwrap_or(u32::MAX);
    let subtitle = if count > 1 {
        of(count).replace("{}", &count.to_string())
    } else {
        String::new()
    };
    row.show(&numbers.value(Kind::Temp, warmest.value), &subtitle, None);
}

#[cfg(test)]
mod tests {
    use baseboard_core::overview::Name;
    use baseboard_core::profile::VoltageRole;
    use baseboard_core::summary::{Fans, Hero};

    use super::*;

    #[test]
    fn keys_of_groups() {
        for kind in [GroupKind::Cpu, GroupKind::Gpu, GroupKind::Disks] {
            assert_eq!(group_of_key(group_key(kind)), Some(kind));
        }
        assert_eq!(group_of_key(""), None);
        assert_eq!(whole_degrees(68.2), "68");
        assert_eq!(whole_degrees(f64::NAN), "–");
    }

    /// The numbers, the rows and the warning for a sample with a low
    /// +12 V rail and a stopped CPU fan. Needs a display.
    #[test]
    fn top_of_the_page() {
        crate::gtk_test::run("top_of_the_page", || {
            let numbers = Numbers::new(",");
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let heroes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            let list = gtk::ListBox::new();
            let all = gtk::ListBox::new();
            let widgets = SummaryWidgets::new(&column, &heroes, &list, &all);
            let rail = OutsideRail {
                name: Name::Voltage(VoltageRole::P12v),
                value: 11.2,
                nominal: 12.0,
                low: 11.4,
                high: 12.6,
            };
            let summary = Summary {
                heroes: vec![Hero {
                    kind: HeroKind::Cpu,
                    value: 68.2,
                    group: GroupKind::Cpu,
                }],
                power: Some(vec![rail]),
                fans: Some(Fans {
                    turning: 5,
                    cpu_stopped: true,
                }),
                memory: Some(Warmest {
                    value: 30.0,
                    count: 2,
                }),
                disks: None,
            };
            widgets.update(&summary, &numbers);
            assert_eq!(widgets.heroes[0].value.label(), "68");
            assert!(widgets.heroes[0].button.is_visible());
            assert!(!widgets.heroes[1].button.is_visible());
            assert_eq!(
                widgets.heroes[0].button.action_target_value(),
                Some("cpu".to_variant())
            );
            let (title, _) = widgets.alert().unwrap();
            assert_eq!(
                title,
                "Voltage +12\u{a0}V below the normal range: 11,20\u{a0}V"
            );
            assert_eq!(widgets.power.value.label(), "11,20\u{a0}V");
            assert_eq!(widgets.fans.value.label(), "5 running");
            assert_eq!(
                widgets.fans.row.subtitle().as_deref(),
                Some("CPU fan stopped")
            );
            assert_eq!(
                widgets.memory.row.subtitle().as_deref(),
                Some("Warmest of 2 modules")
            );
            assert!(!widgets.disks.row.is_visible());

            // All well: no warning, power in order, no fan subtitle.
            let calm = Summary {
                power: Some(Vec::new()),
                fans: Some(Fans {
                    turning: 5,
                    cpu_stopped: false,
                }),
                ..summary
            };
            widgets.update(&calm, &numbers);
            assert_eq!(widgets.alert(), None);
            assert_eq!(widgets.power.value.label(), "In order");
            assert_eq!(widgets.fans.row.subtitle().as_deref(), Some(""));
        });
    }
}
