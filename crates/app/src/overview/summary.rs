// SPDX-License-Identifier: GPL-3.0-or-later

//! The Overview page (docs/PROJEKT.md §5.1, D67), from `core::summary`:
//! the status sentence on top, a card for the processor, the graphics card
//! and the board, tiles for the fans, the memory and the disks, and the
//! row "All sensors". A card or a tile opens the page "All Sensors" on its
//! group, the fans tile the Fans page, the status sentence the place of
//! its problem. Wide, the cards stand in one row and the tiles in another;
//! narrower, one under another (`layout`).

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use baseboard_core::history::{Histories, History, Trend};
use baseboard_core::readings::GroupKind;
use baseboard_core::sensors::Kind;
use baseboard_core::summary::{Hero, HeroKind, OutsideRail, Problem, State, Summary, Warmest};
use baseboard_core::units::{self, Numbers, UNIT_SPACE};
use gettextrs::{gettext, ngettext, pgettext};

use crate::alert;
use crate::history_chart::{self, Limit, Scale};
use crate::layout::Width;
use crate::part_card::{CardView, Fact, PartCard};

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

pub fn hero_name(kind: HeroKind) -> String {
    match kind {
        HeroKind::Cpu => gettext("Processor"),
        HeroKind::Gpu => gettext("Graphics card"),
        HeroKind::Board => gettext("Motherboard"),
    }
}

fn hero_icon(kind: HeroKind) -> &'static str {
    match kind {
        HeroKind::Cpu => "baseboard-processor-symbolic",
        HeroKind::Gpu => "baseboard-graphics-card-symbolic",
        HeroKind::Board => "baseboard-board-symbolic",
    }
}

/// A part close to its limit: a temperature of its group is.
fn is_hot(kind: HeroKind, state: &State) -> bool {
    matches!(state, State::Problems(problems) if problems.iter().any(|problem| {
        matches!(problem, Problem::Hot(part) if part.group == kind.group())
    }))
}

/// What the card of `hero` shows: its temperature and history with the
/// part's `limit` (D80), one number of the part, the range of the history
/// and the highest temperature since the app started, all in whole units
/// (D67); a rail outside its tolerance keeps its decimals, as "11 V" would
/// not say how far it is off.
pub fn card_view(
    hero: &Hero,
    summary: &Summary,
    state: &State,
    histories: &Histories,
    numbers: &Numbers,
    limit: Option<f64>,
) -> CardView {
    let mut facts = Vec::new();
    match hero.kind {
        HeroKind::Cpu => facts.extend(summary.cpu_fan.map(|fan| Fact {
            // Translators: a fact on the processor's card, followed by the
            // speed of the CPU fan, e.g. "890 rpm".
            title: gettext("CPU fan"),
            value: if fan.stopped {
                pgettext("fan state", "Stopped")
            } else {
                units::whole(Kind::Fan, fan.value)
            },
            warn: fan.stopped,
        })),
        HeroKind::Gpu => facts.extend(summary.gpu_power.map(|watts| Fact {
            // Translators: a fact on the graphics card's card, followed by
            // the power it draws, e.g. "12 W".
            title: gettext("Power draw"),
            value: units::whole(Kind::Power, watts),
            warn: false,
        })),
        HeroKind::Board => {
            facts.extend(summary.power.as_deref().map(|rails| power(rails, numbers)))
        }
    }
    let history = histories.get(&hero.id);
    let points = history.map(history_chart::points).unwrap_or_default();
    if let Some(range) = history.and_then(History::range) {
        facts.push(Fact {
            title: history_chart::period(&points),
            value: from_to(Kind::Temp, range),
            warn: false,
        });
    }
    facts.push(Fact {
        // Translators: a fact on a card, followed by a temperature.
        title: gettext("Highest since the app started"),
        value: units::degrees(hero.highest),
        warn: false,
    });
    CardView {
        value: hero.value,
        points,
        trend: history.map_or(Trend::Steady, |h| h.trend()),
        hot: is_hot(hero.kind, state),
        limit: limit.map(|value| Limit {
            value,
            words: alert::limit_words(hero.kind == HeroKind::Cpu, value),
        }),
        scale: cards_scale(summary, histories),
        facts,
        note: None,
    }
}

/// The one scale of the lines of the cards (D80), from the histories of
/// all of them, so that the processor's card on the Fans page has the same
/// as on the Overview.
fn cards_scale(summary: &Summary, histories: &Histories) -> Scale {
    history_chart::scale(
        summary
            .heroes
            .iter()
            .filter_map(|hero| histories.get(&hero.id))
            .flat_map(|history| history.points().map(|(_, value)| value)),
    )
}

/// The rated rails on the board's card.
fn power(rails: &[OutsideRail], numbers: &Numbers) -> Fact {
    let title = gettext("Power");
    match rails {
        [] => Fact {
            title,
            // Translators: on the board's card when every rated rail is
            // within its tolerance.
            value: gettext("Normal"),
            warn: false,
        },
        [rail] => Fact {
            title,
            // Translators: on the board's card for a rail outside its
            // tolerance: the rail and what the board reads, e.g.
            // "+12 V: 11.20 V".
            value: gettext("{rail}: {value}")
                .replace("{rail}", &super::name(&rail.name, numbers))
                .replace(
                    "{value}",
                    &numbers.value_of(Kind::Voltage, rail.value, Some(rail.nominal)),
                ),
            warn: true,
        },
        more => {
            let count = u32::try_from(more.len()).unwrap_or(u32::MAX);
            Fact {
                title,
                value: ngettext(
                    "{} voltage outside the normal range",
                    "{} voltages outside the normal range",
                    count,
                )
                .replace("{}", &count.to_string()),
                warn: true,
            }
        }
    }
}

/// The status sentence: its icon, title and text, and what its row opens
/// (an action and its target), only when it warns.
pub struct StatusText {
    pub icon: &'static str,
    pub class: &'static str,
    pub title: String,
    pub text: String,
    pub opens: Option<(&'static str, Option<String>)>,
}

/// The status sentence of `state` (D67): the most important problem, with
/// how many more there are; the readings working without a profile; or
/// all fine.
pub fn status_text(state: &State, summary: &Summary, numbers: &Numbers) -> StatusText {
    let problems = match state {
        State::Fine => {
            let speeding = summary.fans.as_ref().is_some_and(|fans| fans.speeding);
            return StatusText {
                icon: "baseboard-ok-symbolic",
                class: "success",
                title: alert::everything_fine(),
                text: if speeding {
                    // Translators: under "Everything is fine" on the Overview
                    // while the duty cycle of a fan rises.
                    gettext("Fans are speeding up.")
                } else {
                    gettext("Temperatures, fans and power are normal.")
                },
                opens: None,
            };
        }
        State::NoProfile => {
            return StatusText {
                icon: "help-about-symbolic",
                class: "dimmed",
                title: alert::readings_work(),
                text: gettext(
                    "This board has no profile yet, so some readings have no names or ratings.",
                ),
                opens: None,
            };
        }
        State::Problems(problems) => problems,
    };
    let Some(first) = problems.first() else {
        return status_text(&State::Fine, summary, numbers);
    };
    let (title, mut text) = alert::problem(first, summary.fans_now(), numbers);
    let more = u32::try_from(problems.len() - 1).unwrap_or(u32::MAX);
    if more > 0 {
        text.push('\n');
        text.push_str(&alert::more_problems(more));
    }
    let opens = match first {
        Problem::CpuFanStopped => ("win.show-fans", None),
        Problem::Hot(part) => ("win.show-sensors", Some(group_key(part.group).to_owned())),
        Problem::Rail(_) => (
            "win.show-sensors",
            Some(group_key(GroupKind::Voltages).to_owned()),
        ),
    };
    StatusText {
        icon: "dialog-warning-symbolic",
        class: "warning",
        title,
        text,
        opens: Some(opens),
    }
}

/// The row of the status sentence.
struct StatusRow {
    list: gtk::ListBox,
    row: adw::ActionRow,
    icon: gtk::Image,
    chevron: gtk::Image,
    /// What the warning announced last opens: a screen reader hears each
    /// warning once, not each new value of it.
    announced: RefCell<Option<(&'static str, Option<String>)>>,
}

impl StatusRow {
    fn new() -> Self {
        // One role for good and all: GTK cannot change it once a screen
        // reader has seen it, so a warning is announced instead.
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .accessible_role(gtk::AccessibleRole::Status)
            .css_classes(["boxed-list", "status-list"])
            .build();
        let row = adw::ActionRow::builder()
            .use_markup(false)
            .css_classes(["status-row"])
            .build();
        let icon = gtk::Image::builder()
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["status-icon"])
            .build();
        let chevron = crate::widgets::chevron();
        row.add_prefix(&icon);
        row.add_suffix(&chevron);
        crate::widgets::describe_row(&row, None);
        list.append(&row);
        Self {
            list,
            row,
            icon,
            chevron,
            announced: RefCell::new(None),
        }
    }

    /// What to announce for `status`: the title of a warning other than
    /// the one announced last; nothing for the same warning with a new
    /// value or for a status that is not a warning.
    fn announcement(&self, status: &StatusText) -> Option<String> {
        let mut announced = self.announced.borrow_mut();
        if *announced == status.opens {
            return None;
        }
        announced.clone_from(&status.opens);
        status.opens.as_ref().map(|_| status.title.clone())
    }

    fn show(&self, status: &StatusText) {
        if self.row.title() != status.title {
            self.row.set_title(&status.title);
        }
        if self.row.subtitle().as_deref() != Some(status.text.as_str()) {
            self.row.set_subtitle(&status.text);
        }
        crate::widgets::set_icon(&self.icon, status.icon);
        crate::widgets::set_classes(&self.icon, &["status-icon", status.class]);
        let warning = status.opens.is_some();
        if warning {
            self.list.add_css_class("warning-status");
        } else {
            self.list.remove_css_class("warning-status");
        }
        self.chevron.set_visible(warning);
        self.row.set_activatable(warning);
        match &status.opens {
            Some((action, target)) => {
                // The target before the name (see `PartCard::new`).
                self.row
                    .set_action_target_value(target.as_ref().map(|t| t.to_variant()).as_ref());
                self.row.set_action_name(Some(action));
            }
            None => self.row.set_action_name(None),
        }
        if let Some(words) = self.announcement(status) {
            self.list
                .announce(&words, gtk::AccessibleAnnouncementPriority::High);
        }
    }
}

/// A tile under the cards: an icon, a title, a value and a line under it,
/// and for the fans their speeds as bars.
struct Tile {
    button: gtk::Button,
    badge: gtk::Image,
    icon: &'static str,
    value: gtk::Label,
    sub: gtk::Label,
    bars: Option<gtk::Box>,
}

impl Tile {
    fn new(icon: &'static str, title: &str, bars: bool) -> Self {
        let badge = gtk::Image::builder()
            .icon_name(icon)
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["part-badge"])
            .build();
        let name = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["tile-title"])
            .build();
        let value = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["tile-value", "numeric"])
            .build();
        let sub = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .natural_wrap_mode(gtk::NaturalWrapMode::Word)
            .css_classes(["tile-sub"])
            .build();
        let texts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .build();
        texts.append(&name);
        texts.append(&value);
        texts.append(&sub);
        let content = gtk::Box::builder().spacing(12).build();
        content.append(&badge);
        content.append(&texts);
        let bars = bars.then(|| {
            let bars = gtk::Box::builder()
                .spacing(BARS_GAP)
                .valign(gtk::Align::Center)
                .height_request(BARS_HEIGHT)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build();
            content.append(&bars);
            bars
        });
        let button = gtk::Button::builder()
            .child(&content)
            .visible(false)
            .css_classes(["card", "tile"])
            .build();
        Self {
            button,
            badge,
            icon,
            value,
            sub,
            bars,
        }
    }

    fn opens(self, action: &str, target: Option<&str>) -> Self {
        // The target before the name (see `PartCard::new`).
        if let Some(target) = target {
            self.button
                .set_action_target_value(Some(&target.to_variant()));
        }
        self.button.set_action_name(Some(action));
        self
    }

    fn show(&self, value: &str, sub: &str, warn: bool) {
        if self.value.label() != value {
            self.value.set_label(value);
        }
        if self.sub.label() != sub {
            self.sub.set_label(sub);
        }
        self.sub.set_visible(!sub.is_empty());
        let warning: [&gtk::Widget; 3] = [
            self.sub.upcast_ref(),
            self.badge.upcast_ref(),
            self.button.upcast_ref(),
        ];
        for (widget, class) in warning.into_iter().zip(["warning", "hot", "warning-tile"]) {
            if warn {
                widget.add_css_class(class);
            } else {
                widget.remove_css_class(class);
            }
        }
        crate::widgets::set_icon(
            &self.badge,
            if warn {
                "dialog-warning-symbolic"
            } else {
                self.icon
            },
        );
        let title = self
            .button
            .child()
            .and_then(|content| content.first_child())
            .and_then(|badge| badge.next_sibling())
            .and_then(|texts| texts.first_child())
            .and_downcast::<gtk::Label>()
            .map(|label| label.label().to_string())
            .unwrap_or_default();
        self.button.update_property(&[
            gtk::accessible::Property::Label(&format!("{title}: {value}")),
            gtk::accessible::Property::Description(sub),
        ]);
        self.button.set_visible(true);
    }

    /// The speeds of the fans, a bar each; a stopped CPU fan in the color
    /// of warnings, a fan that does not spin faint.
    fn show_bars(&self, fans: &[(f64, bool)]) {
        let Some(bars) = &self.bars else {
            return;
        };
        let width = bar_width(fans.len());
        let mut bar = bars.first_child();
        let mut count = 0;
        for &(rpm, stopped) in fans {
            let current = bar.clone().unwrap_or_else(|| {
                let new = gtk::Box::builder()
                    .valign(gtk::Align::End)
                    .css_classes(["fan-bar"])
                    .build();
                bars.append(&new);
                new.upcast()
            });
            let share = if rpm.is_finite() { rpm / BARS_RPM } else { 0.0 };
            let height = (share.clamp(0.1, 1.0) * f64::from(BARS_HEIGHT)).round() as i32;
            current.set_size_request(width, height);
            let class = if stopped {
                "warn"
            } else if rpm > 0.0 {
                "on"
            } else {
                "off"
            };
            crate::widgets::set_classes(&current, &["fan-bar", class]);
            bar = current.next_sibling();
            count += 1;
        }
        while let Some(extra) = bars.last_child().filter(|_| count < count_children(bars)) {
            bars.remove(&extra);
        }
    }
}

fn count_children(widget: &gtk::Box) -> usize {
    let mut count = 0;
    let mut child = widget.first_child();
    while let Some(current) = child {
        count += 1;
        child = current.next_sibling();
    }
    count
}

/// The bars of the fans tile: their height, and the speed of a full bar;
/// together as wide as six bars of 10 px, however many fans there are,
/// so that three tiles fit side by side in a window of about 1200 px.
const BARS_HEIGHT: i32 = 40;
const BARS_RPM: f64 = 1600.0;
const BARS_WIDTH: i32 = 82;
const BARS_GAP: i32 = 4;

/// The width of each of `count` bars.
fn bar_width(count: usize) -> i32 {
    let count = i32::try_from(count.max(1)).unwrap_or(i32::MAX);
    ((BARS_WIDTH - BARS_GAP * (count - 1)) / count).max(3)
}

/// The widgets of the page.
pub struct SummaryWidgets {
    status: StatusRow,
    cards_grid: gtk::Grid,
    cards: Vec<(HeroKind, PartCard)>,
    tiles_grid: gtk::Grid,
    fans: Tile,
    memory: Tile,
    disks: Tile,
    width: Cell<Width>,
    /// The cards and tiles shown, as the grids hold them.
    shown: RefCell<(Vec<bool>, Vec<bool>, Option<Width>)>,
}

impl SummaryWidgets {
    /// Fills `column` from the top.
    pub fn new(column: &gtk::Box) -> Self {
        let status = StatusRow::new();
        column.append(&status.list);
        let cards_grid = gtk::Grid::builder()
            .column_spacing(16)
            .row_spacing(16)
            .column_homogeneous(true)
            .build();
        column.append(&cards_grid);
        let cards = [HeroKind::Cpu, HeroKind::Gpu, HeroKind::Board]
            .into_iter()
            .map(|kind| {
                let card = PartCard::new(
                    hero_icon(kind),
                    &hero_name(kind),
                    Some(group_key(kind.group())),
                );
                card.widget.set_visible(false);
                (kind, card)
            })
            .collect();
        let tiles_grid = gtk::Grid::builder()
            .column_spacing(14)
            .row_spacing(14)
            .column_homogeneous(true)
            .build();
        column.append(&tiles_grid);
        let show = "win.show-sensors";
        let fans = Tile::new("baseboard-fan-symbolic", &gettext("Fans"), true)
            .opens("win.show-fans", None);
        let memory = Tile::new(
            "baseboard-memory-symbolic",
            &pgettext("system memory", "Memory"),
            false,
        )
        .opens(show, Some(group_key(GroupKind::Memory)));
        let disks = Tile::new("baseboard-disk-symbolic", &gettext("Disks"), false)
            .opens(show, Some(group_key(GroupKind::Disks)));

        let all = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let every = adw::ActionRow::builder()
            .use_markup(false)
            .title(gettext("All sensors"))
            .subtitle(gettext(
                "Every reading with its source and range since the app started",
            ))
            .activatable(true)
            .build();
        every.add_prefix(
            &gtk::Image::builder()
                .icon_name("view-list-bullet-symbolic")
                .accessible_role(gtk::AccessibleRole::Presentation)
                .css_classes(["dimmed"])
                .build(),
        );
        every.add_suffix(&crate::widgets::chevron());
        crate::widgets::describe_row(&every, None);
        every.set_action_target_value(Some(&"".to_variant()));
        every.set_action_name(Some(show));
        all.append(&every);
        column.append(&all);
        Self {
            status,
            cards_grid,
            cards,
            tiles_grid,
            fans,
            memory,
            disks,
            width: Cell::new(Width::Wide),
            shown: RefCell::new((Vec::new(), Vec::new(), None)),
        }
    }

    pub fn set_width(&self, width: Width) {
        self.width.set(width);
        self.arrange();
    }

    /// Shows the sample; `limit_of` gives the limit a card draws on its
    /// history (D80).
    pub fn update(
        &self,
        summary: &Summary,
        state: &State,
        histories: &Histories,
        numbers: &Numbers,
        limit_of: &dyn Fn(&Hero) -> Option<f64>,
    ) {
        self.status.show(&status_text(state, summary, numbers));
        for (kind, card) in &self.cards {
            let hero = summary.heroes.iter().find(|h| h.kind == *kind);
            card.widget.set_visible(hero.is_some());
            if let Some(hero) = hero {
                let view = card_view(hero, summary, state, histories, numbers, limit_of(hero));
                card.update(&view);
            }
        }
        match &summary.fans {
            Some(fans) => {
                let count = u32::try_from(fans.spinning).unwrap_or(u32::MAX);
                // Translators: how many fans spin, on the Overview.
                let value =
                    ngettext("{} spinning", "{} spinning", count).replace("{}", &count.to_string());
                let sub = if fans.cpu_stopped {
                    alert::cpu_fan_stopped()
                } else if fans.speeding {
                    // Translators: under the number of fans that spin on the
                    // Overview while the duty cycle of one of them rises.
                    pgettext("fans", "Speeding up")
                } else {
                    fans.speeds
                        .map(|speeds| from_to(Kind::Fan, speeds))
                        .unwrap_or_default()
                };
                self.fans.show(&value, &sub, fans.cpu_stopped);
                self.fans.show_bars(&fans.bars);
            }
            None => self.fans.button.set_visible(false),
        }
        show_warmest(&self.memory, summary.memory, |count| {
            // Translators: under the warmest memory module on the Overview.
            ngettext("Warmest of {} module", "Warmest of {} modules", count)
        });
        show_warmest(&self.disks, summary.disks, |count| {
            // Translators: under the warmest disk on the Overview.
            ngettext("Warmest of {} disk", "Warmest of {} disks", count)
        });
        self.arrange();
    }

    /// Puts the cards and tiles shown into their grids: in one row each
    /// when there is room, one under another otherwise.
    fn arrange(&self) {
        let width = self.width.get();
        let cards: Vec<bool> = self
            .cards
            .iter()
            .map(|(_, c)| c.widget.get_visible())
            .collect();
        let tiles: Vec<bool> = self
            .tiles()
            .iter()
            .map(|t| t.button.get_visible())
            .collect();
        let shape = (cards, tiles, Some(width));
        if *self.shown.borrow() == shape {
            return;
        }
        let place = |grid: &gtk::Grid, widgets: Vec<gtk::Widget>| {
            for widget in &widgets {
                if widget.parent().is_some() {
                    grid.remove(widget);
                }
            }
            let shown: Vec<&gtk::Widget> = widgets.iter().filter(|w| w.get_visible()).collect();
            let columns = if width.columns() {
                shown.len().max(1)
            } else {
                1
            };
            for (i, widget) in shown.into_iter().enumerate() {
                let (column, row) = (i % columns, i / columns);
                grid.attach(widget, column as i32, row as i32, 1, 1);
            }
        };
        for (_, card) in &self.cards {
            card.set_width(width);
        }
        place(
            &self.cards_grid,
            self.cards.iter().map(|(_, c)| c.widget.clone()).collect(),
        );
        place(
            &self.tiles_grid,
            self.tiles()
                .iter()
                .map(|t| t.button.clone().upcast())
                .collect(),
        );
        self.shown.replace(shape);
    }

    fn tiles(&self) -> [&Tile; 3] {
        [&self.fans, &self.memory, &self.disks]
    }

    /// The status sentence shown (tests).
    #[cfg(test)]
    pub fn status(&self) -> (String, String) {
        (
            self.status.row.title().into(),
            self.status.row.subtitle().unwrap_or_default().into(),
        )
    }
}

/// "From 750 to 1010 rpm" or "From 72 to 88 °C" in whole units, or one
/// value. The line breaks only before the higher value, and never between
/// a number and its unit: the other spaces are non-breaking.
fn from_to(kind: Kind, (low, high): (f64, f64)) -> String {
    let high = units::whole(kind, high);
    let low = units::whole(kind, low);
    if low == high {
        return high;
    }
    let (low_number, _) = low.split_once(UNIT_SPACE).unwrap_or((&low, ""));
    // Translators: under the number of fans that spin on the Overview, and
    // the range of a temperature on a card; {high} has its unit, e.g. "From
    // 750 to 1010 rpm" or "From 72 to 88 °C". The line may break only
    // before {high}.
    let text = gettext("From {low} to {high}").replace("{low}", low_number);
    let hold = |text: &str| text.replace(' ', &UNIT_SPACE.to_string());
    match text.split_once("{high}") {
        Some((before, after)) => {
            let gap = if before.ends_with(' ') { " " } else { "" };
            format!("{}{gap}{high}{}", hold(before.trim_end()), hold(after))
        }
        None => hold(&text),
    }
}

fn show_warmest(tile: &Tile, warmest: Option<Warmest>, of: impl Fn(u32) -> String) {
    let Some(warmest) = warmest else {
        tile.button.set_visible(false);
        return;
    };
    let count = u32::try_from(warmest.count).unwrap_or(u32::MAX);
    let sub = if count > 1 {
        of(count).replace("{}", &count.to_string())
    } else {
        String::new()
    };
    tile.show(&units::degrees(warmest.value), &sub, false);
}

#[cfg(test)]
mod tests {
    use baseboard_core::profile::VoltageRole;
    use baseboard_core::readings::{Name, RowId};
    use baseboard_core::summary::{CpuFan, Fans, HotPart};

    use super::*;

    #[test]
    fn keys_of_groups() {
        for kind in [GroupKind::Cpu, GroupKind::Gpu, GroupKind::Disks] {
            assert_eq!(group_of_key(group_key(kind)), Some(kind));
        }
        assert_eq!(group_of_key(""), None);
        // The line breaks only before the higher value, which keeps its
        // unit; temperatures as speeds, in whole degrees (D80).
        assert_eq!(
            from_to(Kind::Fan, (750.4, 1013.0)),
            "From\u{a0}750\u{a0}to 1013\u{a0}rpm"
        );
        assert_eq!(from_to(Kind::Fan, (890.0, 890.2)), "890\u{a0}rpm");
        assert_eq!(
            from_to(Kind::Temp, (71.6, 88.4)),
            "From\u{a0}72\u{a0}to 88\u{a0}°C"
        );
        // Six bars of 10 px as on the render, eight narrower in the same room.
        assert_eq!(bar_width(6), 10);
        assert_eq!(bar_width(8), 6);
        assert_eq!(bar_width(30), 3);
    }

    fn rail() -> OutsideRail {
        OutsideRail {
            name: Name::Voltage(VoltageRole::P12v),
            value: 11.2,
            nominal: 12.0,
            low: 11.4,
            high: 12.6,
        }
    }

    fn summary() -> Summary {
        Summary {
            heroes: vec![Hero {
                kind: HeroKind::Cpu,
                id: RowId {
                    chip: "k10temp".into(),
                    device: None,
                    channel: "temp1".into(),
                },
                value: 93.2,
                highest: 93.4,
            }],
            power: Some(vec![rail()]),
            cpu_fan: Some(CpuFan {
                value: 1880.0,
                duty: Some(255.0),
                stopped: false,
            }),
            gpu_power: None,
            fans: Some(Fans {
                spinning: 5,
                speeds: Some((750.0, 1880.0)),
                bars: vec![(1880.0, false), (0.0, false)],
                cpu_stopped: false,
                speeding: false,
            }),
            memory: Some(Warmest {
                value: 30.0,
                count: 2,
            }),
            disks: None,
        }
    }

    /// The cards share one scale of their lines (D80): a graphics card
    /// resting under 30 °C lowers it for the processor's card too, on the
    /// Overview and on the Fans page alike.
    #[test]
    fn one_scale_for_the_cards() {
        use baseboard_core::readings::{Group, Name, Row, Subtitle};
        let mut summary = summary();
        let cpu = summary.heroes[0].id.clone();
        let gpu = RowId {
            chip: "amdgpu".into(),
            device: None,
            channel: "temp1".into(),
        };
        summary.heroes.push(Hero {
            kind: HeroKind::Gpu,
            id: gpu.clone(),
            value: 29.0,
            highest: 29.0,
        });
        let row = |id: &RowId, value: f64| Row {
            id: id.clone(),
            name: Name::GpuChip,
            role: Name::GpuChip,
            subtitle: Subtitle::Source {
                driver: id.chip.clone(),
                channel: id.channel.clone(),
            },
            kind: Kind::Temp,
            value,
            rating: None,
            range: None,
        };
        let sample = |cpu_value: f64, gpu_value: f64| {
            [Group {
                kind: GroupKind::Gpu,
                description: None,
                rows: vec![row(&cpu, cpu_value), row(&gpu, gpu_value)],
                auxiliary: Vec::new(),
            }]
        };
        let mut histories = Histories::default();
        histories.update(&sample(41.0, 35.0), baseboard_core::history::Moment::now());
        assert_eq!(cards_scale(&summary, &histories), (30.0, 100.0));
        histories.update(&sample(42.0, 29.0), baseboard_core::history::Moment::now());
        assert_eq!(cards_scale(&summary, &histories), (20.0, 100.0));
    }

    /// The board's card: "Normal", a rail with its value as on the render,
    /// or how many rails are outside their range.
    #[test]
    fn power_on_the_board_card() {
        let numbers = Numbers::new(",");
        assert_eq!(power(&[], &numbers).value, "Normal");
        let one = power(&[rail()], &numbers);
        assert_eq!(
            (one.value.as_str(), one.warn),
            ("+12\u{a0}V: 11,20\u{a0}V", true)
        );
        assert_eq!(
            power(&[rail(), rail()], &numbers).value,
            "2 voltages outside the normal range"
        );
    }

    fn hot_cpu() -> Problem {
        Problem::Hot(HotPart {
            name: Name::Temp(baseboard_core::profile::TempRole::Cpu),
            group: GroupKind::Cpu,
            processor: true,
            value: 93.2,
            limit: 95.0,
        })
    }

    /// The status sentence by the rule of D67: the text of the 93 °C
    /// render, how many more problems there are, and what the row opens;
    /// the readings working without a profile; all fine.
    #[test]
    fn status_sentences() {
        let numbers = Numbers::new(",");
        let summary = summary();
        let state = State::Problems(vec![hot_cpu(), Problem::Rail(rail())]);
        let status = status_text(&state, &summary, &numbers);
        assert_eq!(status.title, "Processor is very hot: 93\u{a0}°C");
        assert_eq!(
            status.text,
            "At 95\u{a0}°C it slows down to avoid overheating. The fans run at full speed.\nAnd \
             1 more problem"
        );
        assert_eq!(
            status.opens,
            Some(("win.show-sensors", Some("cpu".to_owned())))
        );
        // The sentence about the fans is true: still speeding up at the
        // full duty cycle, nothing without a known duty cycle.
        let alone = State::Problems(vec![hot_cpu()]);
        let mut changed = summary.clone();
        if let Some(fans) = &mut changed.fans {
            fans.speeding = true;
        }
        assert_eq!(
            status_text(&alone, &changed, &numbers).text,
            "At 95\u{a0}°C it slows down to avoid overheating. Fans are speeding up."
        );
        changed.cpu_fan = changed.cpu_fan.map(|fan| CpuFan { duty: None, ..fan });
        assert_eq!(
            status_text(&alone, &changed, &numbers).text,
            "At 95\u{a0}°C it slows down to avoid overheating."
        );
        let stopped = status_text(
            &State::Problems(vec![Problem::CpuFanStopped]),
            &summary,
            &numbers,
        );
        assert_eq!(stopped.title, "CPU fan stopped");
        assert_eq!(stopped.opens, Some(("win.show-fans", None)));
        assert_eq!(
            status_text(&State::NoProfile, &summary, &numbers).title,
            "Readings work"
        );
        let fine = status_text(&State::Fine, &summary, &numbers);
        assert_eq!(fine.title, "Everything is fine");
        assert_eq!(fine.text, "Temperatures, fans and power are normal.");
        assert_eq!(fine.opens, None);
        // Under load, as on the render of 85 °C.
        let mut speeding = summary.clone();
        if let Some(fans) = &mut speeding.fans {
            fans.speeding = true;
        }
        let fine = status_text(&State::Fine, &speeding, &numbers);
        assert_eq!(
            (fine.title.as_str(), fine.text.as_str()),
            ("Everything is fine", "Fans are speeding up.")
        );
    }

    /// The status row keeps the role of a status when a warning comes and
    /// goes (GTK cannot change a role once a screen reader has seen it);
    /// each warning is announced once, not each new value of it. Needs a
    /// display.
    #[test]
    fn status_announces_each_warning_once() {
        crate::gtk_test::run("status_announces_each_warning_once", || {
            let status = |title: &str, opens: Option<(&'static str, Option<String>)>| StatusText {
                icon: "baseboard-ok-symbolic",
                class: "",
                title: title.into(),
                text: String::new(),
                opens,
            };
            let fine = status("Everything is fine", None);
            let cpu = || Some(("win.show-sensors", Some("cpu".to_owned())));
            let hot = status("Processor is very hot: 93\u{a0}°C", cpu());
            let hotter = status("Processor is very hot: 94\u{a0}°C", cpu());
            let stopped = status("CPU fan stopped", Some(("win.show-fans", None)));

            let row = StatusRow::new();
            for shown in [&fine, &hot, &fine] {
                row.show(shown);
                assert_eq!(row.list.accessible_role(), gtk::AccessibleRole::Status);
            }

            let row = StatusRow::new();
            assert_eq!(row.announcement(&fine), None);
            assert_eq!(row.announcement(&hot), Some(hot.title.clone()));
            assert_eq!(row.announcement(&hotter), None);
            assert_eq!(row.announcement(&stopped), Some(stopped.title.clone()));
            assert_eq!(row.announcement(&fine), None);
            assert_eq!(row.announcement(&hot), Some(hot.title.clone()));
        });
    }

    /// The cards, the tiles and the status row for a hot processor with a
    /// low rail, in a window with the actions: a card opens its group;
    /// three cards in a row when wide, one a row when narrower. Needs a
    /// display.
    #[test]
    fn top_of_the_page() {
        crate::gtk_test::run("top_of_the_page", || {
            use std::rc::Rc;

            use gtk::{gio, glib};

            let opened = Rc::new(RefCell::new(Vec::<String>::new()));
            let action = gio::SimpleAction::new("show-sensors", Some(glib::VariantTy::STRING));
            let seen = opened.clone();
            action.connect_activate(move |_, key| {
                seen.borrow_mut()
                    .extend(key.and_then(|k| k.get::<String>()));
            });
            let group = gio::SimpleActionGroup::new();
            group.add_action(&action);
            group.add_action(&gio::SimpleAction::new("show-fans", None));

            let numbers = Numbers::new(",");
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let window = gtk::Window::new();
            window.insert_action_group("win", Some(&group));
            window.set_child(Some(&column));
            let widgets = SummaryWidgets::new(&column);
            let summary = summary();
            let state = State::Problems(vec![hot_cpu(), Problem::Rail(rail())]);
            widgets.update(&summary, &state, &Histories::default(), &numbers, &|_| None);
            assert_eq!(widgets.status().0, "Processor is very hot: 93\u{a0}°C");
            // The sign of the status row in the middle of its two lines
            // (D76), as the icons of other rows.
            let list = &widgets.status.list;
            let (_, height, _, _) = list.measure(gtk::Orientation::Vertical, 800);
            list.allocate(800, height, -1, None);
            let row = widgets.status.row.upcast_ref::<gtk::Widget>();
            let icon = widgets.status.icon.compute_bounds(row).unwrap();
            let middle = icon.y() + icon.height() / 2.0;
            assert!(
                (middle - row.height() as f32 / 2.0).abs() <= 1.0,
                "{middle} in {}",
                row.height()
            );
            let (_, cpu) = &widgets.cards[0];
            assert!(cpu.widget.get_visible() && cpu.widget.has_css_class("hot"));
            assert!(!widgets.cards[1].1.widget.get_visible());
            assert_eq!(
                cpu.facts(),
                [
                    ("CPU fan".to_owned(), "1880\u{a0}rpm".to_owned()),
                    (
                        "Highest since the app started".to_owned(),
                        "93\u{a0}°C".to_owned()
                    ),
                ]
            );
            cpu.widget
                .downcast_ref::<gtk::Button>()
                .unwrap()
                .emit_clicked();
            assert_eq!(*opened.borrow(), ["cpu"]);
            assert_eq!(widgets.fans.value.label(), "5 spinning");
            assert_eq!(
                widgets.fans.sub.label(),
                "From\u{a0}750\u{a0}to 1880\u{a0}rpm"
            );
            assert!(!widgets.disks.button.get_visible());
            // Whole units on the cards and tiles (D67), also for a
            // graphics card that draws 11.6 W.
            assert_eq!(widgets.memory.value.label(), "30\u{a0}°C");
            let mut gpu = summary.clone();
            gpu.gpu_power = Some(11.6);
            gpu.heroes.push(Hero {
                kind: HeroKind::Gpu,
                ..gpu.heroes[0].clone()
            });
            widgets.update(&gpu, &state, &Histories::default(), &numbers, &|_| None);
            assert_eq!(
                widgets.cards[1].1.facts()[0],
                ("Power draw".to_owned(), "12\u{a0}W".to_owned())
            );

            // The fans tile while they speed up, and with a stopped CPU fan
            // in the color of warnings, as on the renders.
            let mut changed = summary.clone();
            if let Some(fans) = &mut changed.fans {
                fans.speeding = true;
            }
            widgets.update(&changed, &state, &Histories::default(), &numbers, &|_| None);
            assert_eq!(widgets.fans.sub.label(), "Speeding up");
            assert!(!widgets.fans.button.has_css_class("warning-tile"));
            if let Some(fans) = &mut changed.fans {
                fans.cpu_stopped = true;
            }
            widgets.update(&changed, &state, &Histories::default(), &numbers, &|_| None);
            assert_eq!(widgets.fans.sub.label(), "CPU fan stopped");
            assert!(widgets.fans.button.has_css_class("warning-tile"));
            widgets.update(&summary, &state, &Histories::default(), &numbers, &|_| None);
            assert!(!widgets.fans.button.has_css_class("warning-tile"));

            // Two tiles in a row when wide, one a row when narrow.
            let row_of = |tile: &Tile| widgets.tiles_grid.query_child(&tile.button).1;
            widgets.set_width(Width::Wide);
            assert_eq!(row_of(&widgets.memory), 0);
            widgets.set_width(Width::Narrow);
            assert_eq!(row_of(&widgets.memory), 1);
            window.destroy();
        });
    }

    /// The same sample again leaves the icons and the style classes of the
    /// status row, the tiles and the bars of the fans as they are
    /// (`widgets::set_icon`, `widgets::set_classes`). Needs a display.
    #[test]
    fn the_same_sample_keeps_icons_and_classes() {
        crate::gtk_test::run("the_same_sample_keeps_icons_and_classes", || {
            use crate::gtk_test::notifications;

            let numbers = Numbers::new(",");
            let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
            let widgets = SummaryWidgets::new(&column);
            let summary = summary();
            let state = State::Problems(vec![hot_cpu(), Problem::Rail(rail())]);
            let update =
                || widgets.update(&summary, &state, &Histories::default(), &numbers, &|_| None);
            update();
            let status = &widgets.status.icon;
            assert_eq!(notifications(status, "icon-name", update), 0);
            assert_eq!(notifications(status, "css-classes", update), 0);
            for tile in [&widgets.fans, &widgets.memory] {
                assert_eq!(notifications(&tile.badge, "icon-name", update), 0);
            }
            let bars = widgets.fans.bars.as_ref().unwrap();
            let mut bar = bars.first_child();
            let mut count = 0;
            while let Some(current) = bar {
                assert_eq!(notifications(&current, "css-classes", update), 0);
                bar = current.next_sibling();
                count += 1;
            }
            assert_eq!(count, 2);
        });
    }

    /// All three cards and tiles, eight fans, fit the narrowest page of
    /// each width (layout.rs) with the margins of the page, in the styles
    /// of the app: 1100, 880 and 600 sp, and the narrowest window of 360
    /// px. Needs a display.
    #[test]
    fn fits_every_width() {
        crate::gtk_test::run("fits_every_width", || {
            crate::gtk_test::in_app_style(|| {
                let numbers = Numbers::new(",");
                let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
                let widgets = SummaryWidgets::new(&column);
                let mut summary = summary();
                let hero = summary.heroes[0].clone();
                for kind in [HeroKind::Gpu, HeroKind::Board] {
                    summary.heroes.push(Hero {
                        kind,
                        ..hero.clone()
                    });
                }
                summary.gpu_power = Some(120.0);
                summary.disks = summary.memory;
                if let Some(fans) = &mut summary.fans {
                    fans.spinning = 8;
                    fans.bars = vec![(1880.0, false); 8];
                }
                let state = State::Problems(vec![hot_cpu(), Problem::Rail(rail())]);
                widgets.update(&summary, &state, &Histories::default(), &numbers, &|_| None);
                let margins = 24;
                for (width, page) in [
                    (Width::Wide, 1100),
                    (Width::Compact, 880),
                    (Width::Medium, 600),
                    (Width::Narrow, 360),
                ] {
                    widgets.set_width(width);
                    let (cards, tiles) = (
                        widgets
                            .cards_grid
                            .measure(gtk::Orientation::Horizontal, -1)
                            .0,
                        widgets
                            .tiles_grid
                            .measure(gtk::Orientation::Horizontal, -1)
                            .0,
                    );
                    assert!(
                        cards.max(tiles) + margins <= page,
                        "{width:?}: cards {cards} px, tiles {tiles} px, page {page} px"
                    );
                }
            })
        });
    }
}
