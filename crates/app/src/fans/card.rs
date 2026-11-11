// SPDX-License-Identifier: GPL-3.0-or-later

//! The card of one header on the Fans page (docs/PROJEKT.md §7 M2, M2.4):
//! icon, name and header, speed and duty cycle; the mode switch, which in
//! 0.2.0 only shows the mode, or a box for a header nobody checked; the
//! chart of the curve, the duty cycle of manual mode, a bar, or a note;
//! a footer with "Show Curve". The card itself does nothing when
//! activated, but takes the keyboard focus, so that a screen reader reads
//! it.
//!
//! Speeds and duty cycles are inscriptions, whose size does not depend on
//! their text: a new value every second redraws them without laying out
//! the window again.

use adw::prelude::*;
use baseboard_core::fans::driver::Mode;
use baseboard_core::fans::page::{Body, Card, CardKind, Control};
use baseboard_core::profile::FanRole;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{UNIT_SPACE, duty_to_percent};
use gettextrs::gettext;
use gtk::{glib, pango};

use super::{Layout, PageContext, percent, source_name};
use crate::fan_chart::{ChartCurve, FanChart, Reading, Size};
use crate::overview;

mod imp {
    use gtk::subclass::prelude::*;

    use super::*;

    /// The box of a card. Tab stops on the card first and then on its
    /// controls, as on a row of a list; Shift+Tab from the first control
    /// goes back to the card.
    #[derive(Default)]
    pub struct CardBox;

    #[glib::object_subclass]
    impl ObjectSubclass for CardBox {
        const NAME: &'static str = "BaseboardFanCard";
        type Type = super::CardBox;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Group);
        }
    }

    impl ObjectImpl for CardBox {}
    impl BoxImpl for CardBox {}

    impl WidgetImpl for CardBox {
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            let obj = self.obj();
            let forward = matches!(
                direction,
                gtk::DirectionType::TabForward
                    | gtk::DirectionType::Down
                    | gtk::DirectionType::Right
            );
            if obj.has_focus() {
                // On the card: forward into its controls, back out of it.
                return forward && self.parent_focus(direction);
            }
            if obj.focus_child().is_none() {
                // Entering: forward onto the card; backward onto its last
                // control, or the card.
                if forward || !self.parent_focus(direction) {
                    return obj.grab_focus();
                }
                return true;
            }
            // In a control: to the next one, or back onto the card.
            self.parent_focus(direction) || (!forward && obj.grab_focus())
        }
    }
}

glib::wrapper! {
    pub struct CardBox(ObjectSubclass<imp::CardBox>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl CardBox {
    fn new() -> Self {
        glib::Object::new()
    }
}

/// What decides whether a card is made again rather than updated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Shape {
    key: String,
    low: bool,
    /// Positions of the switch; 0 for the box of an unchecked header.
    positions: u8,
    body: u8,
}

impl Shape {
    pub(super) fn of(card: &Card) -> Self {
        let (low, positions, body) = match &card.kind {
            CardKind::Low => (true, 0, 0),
            CardKind::Full { control, body } => (
                false,
                match control {
                    Control::Switch { positions, .. } => *positions,
                    Control::Unchecked => 0,
                },
                match body {
                    Body::Chart(_) => 1,
                    Body::Manual => 2,
                    Body::NoCurve => 3,
                    Body::CurveUnreadable => 4,
                },
            ),
        };
        Self {
            key: card.key.clone(),
            low,
            positions,
            body,
        }
    }
}

/// The main part of a card.
enum BodyWidgets {
    None,
    Chart(FanChart),
    Manual {
        panel: gtk::Box,
        value: gtk::Inscription,
        scale: gtk::Scale,
    },
    Level {
        bar: gtk::LevelBar,
        caption: gtk::Inscription,
    },
    Note,
}

pub(super) struct CardWidgets {
    pub(super) card: gtk::Box,
    head: gtk::Grid,
    name: gtk::Label,
    subtitle: gtk::Label,
    values: gtk::Box,
    rpm: gtk::Inscription,
    duty: gtk::Label,
    switch: Option<adw::ToggleGroup>,
    unchecked_empty: Option<gtk::Label>,
    body: BodyWidgets,
    empty: gtk::Box,
    foot: gtk::Box,
    foot_note: gtk::Label,
    button: gtk::Button,
}

fn caption(text: &str, soft: bool) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .natural_wrap_mode(gtk::NaturalWrapMode::Word)
        .css_classes(["caption"])
        .build();
    if soft {
        label.add_css_class("fan-soft");
    }
    label
}

/// An icon with a text next to it, for the notes of a card.
fn note_line(text: &str) -> (gtk::Box, gtk::Label) {
    let line = gtk::Box::builder()
        .spacing(9)
        .css_classes(["fan-note"])
        .build();
    let icon = gtk::Image::from_icon_name("help-about-symbolic");
    icon.update_property(&[gtk::accessible::Property::Label("")]);
    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
    let label = caption(text, false);
    label.set_hexpand(true);
    line.append(&icon);
    line.append(&label);
    (line, label)
}

/// A value in large type with its unit in normal type, as in the header
/// line and in manual mode; `chars` wide, so that a new value does not lay
/// out the window again.
fn big_value(css: &str, chars: u32) -> gtk::Inscription {
    gtk::Inscription::builder()
        .min_chars(chars)
        .nat_chars(chars)
        .xalign(1.0)
        .css_classes([css, "numeric"])
        .build()
}

/// The markup of a large value: the number, and the unit at `unit_size`
/// percent of it, bold and dimmed.
fn big_markup(number: &str, unit: &str, unit_size: u32) -> String {
    format!(
        "{}<span size=\"{unit_size}%\" weight=\"bold\" alpha=\"55%\">{UNIT_SPACE}{}</span>",
        glib::markup_escape_text(number),
        glib::markup_escape_text(unit)
    )
}

fn set_markup(inscription: &gtk::Inscription, markup: &str) {
    // The inscription keeps the text without markup; compare that.
    let plain = markup_text(markup);
    if inscription.text().as_deref() != Some(plain.as_str()) {
        inscription.set_markup(Some(markup));
    }
}

/// The text of `markup` without its tags.
fn markup_text(markup: &str) -> String {
    pango::parse_markup(markup, '\0')
        .map(|(_, text, _)| text.to_string())
        .unwrap_or_default()
}

impl CardWidgets {
    pub(super) fn new(card: &Card, layout: Layout) -> Self {
        // Not activatable, but it takes the keyboard focus, so that the
        // keyboard goes from card to card and a screen reader reads each.
        let box_: gtk::Box = CardBox::new().upcast();
        box_.set_orientation(gtk::Orientation::Vertical);
        box_.set_spacing(12);
        box_.set_focusable(true);
        box_.add_css_class("card");
        box_.add_css_class("fan-card");

        // The header line: icon, name and subtitle, speed and duty cycle.
        let head = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(4)
            .build();
        let icon_box = gtk::Box::builder()
            .css_classes(["fan-icon"])
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .build();
        let icon = gtk::Image::builder()
            .icon_name(match card.role {
                FanRole::Pump => "baseboard-pump-symbolic",
                _ => "baseboard-fan-symbolic",
            })
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        icon_box.append(&icon);
        let texts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .valign(gtk::Align::Center)
            .hexpand(true)
            .build();
        let name = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .natural_wrap_mode(gtk::NaturalWrapMode::Word)
            .css_classes(["heading"])
            .build();
        let subtitle = caption("", true);
        texts.append(&name);
        texts.append(&subtitle);
        let values = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .build();
        // "1024 rpm": the unit at the size of normal text.
        let rpm = big_value("fan-rpm", 7);
        // A label as wide as its text: it changes only with the duty cycle,
        // not with every sample.
        let duty = gtk::Label::builder()
            .xalign(1.0)
            .css_classes(["caption", "fan-soft", "numeric"])
            .build();
        values.append(&rpm);
        values.append(&duty);
        head.attach(&icon_box, 0, 0, 1, 1);
        head.attach(&texts, 1, 0, 1, 1);
        head.attach(&values, 2, 0, 1, 1);
        box_.append(&head);

        // The mode switch, or the box of a header nobody checked.
        let mut switch = None;
        let mut unchecked_empty = None;
        if let CardKind::Full { control, .. } = &card.kind {
            match control {
                Control::Switch { positions, .. } => {
                    let group = adw::ToggleGroup::builder()
                        .homogeneous(true)
                        .can_shrink(false)
                        .hexpand(true)
                        .sensitive(false)
                        .build();
                    group.add_css_class("fan-mode");
                    let names = [
                        ("auto", gettext("Automatic BIOS")),
                        ("manual", gettext("Manual")),
                        ("curve", gettext("Curve")),
                    ];
                    for (name, label) in names.into_iter().take(usize::from(*positions)) {
                        group.add(adw::Toggle::builder().name(name).label(label).build());
                    }
                    // Translators: the name of the mode switch of a fan,
                    // for screen readers.
                    group
                        .upcast_ref::<gtk::Widget>()
                        .update_property(&[gtk::accessible::Property::Label(&gettext("Mode"))]);
                    box_.append(&group);
                    switch = Some(group);
                }
                Control::Unchecked => {
                    let frame = gtk::Box::builder()
                        .spacing(10)
                        .css_classes(["fan-unchecked"])
                        .build();
                    let icon = gtk::Image::from_icon_name("help-about-symbolic");
                    icon.set_accessible_role(gtk::AccessibleRole::Presentation);
                    let lines = gtk::Box::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .spacing(2)
                        .valign(gtk::Align::Center)
                        .hexpand(true)
                        .build();
                    lines.append(&caption(
                        // Translators: in place of the mode switch of a fan
                        // header whose channel nobody has checked on this
                        // board.
                        &gettext("This header was not checked on this board."),
                        false,
                    ));
                    let empty = caption(&empty_text(), true);
                    lines.append(&empty);
                    frame.append(&icon);
                    frame.append(&lines);
                    box_.append(&frame);
                    unchecked_empty = Some(empty);
                }
            }
        }

        // The main part.
        let body = match &card.kind {
            CardKind::Low => {
                if card.duty.is_some() {
                    let line = gtk::Box::builder().spacing(12).build();
                    let bar = gtk::LevelBar::builder()
                        .min_value(0.0)
                        .max_value(100.0)
                        .hexpand(true)
                        .valign(gtk::Align::Center)
                        .build();
                    // Added, not set: the bar keeps its own classes.
                    bar.add_css_class("fan-level");
                    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_LOW));
                    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_HIGH));
                    bar.remove_offset_value(Some(gtk::LEVEL_BAR_OFFSET_FULL));
                    let caption = gtk::Inscription::builder()
                        .min_chars(12)
                        .nat_chars(16)
                        .css_classes(["caption", "numeric"])
                        .build();
                    line.append(&bar);
                    line.append(&caption);
                    box_.append(&line);
                    BodyWidgets::Level { bar, caption }
                } else {
                    BodyWidgets::None
                }
            }
            CardKind::Full { body, .. } => match body {
                Body::Chart(_) => {
                    let chart = FanChart::new(Size::Card, false);
                    box_.append(&chart);
                    BodyWidgets::Chart(chart)
                }
                Body::Manual => {
                    // As high as a chart, the contents in the middle.
                    let panel = gtk::Box::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .css_classes(["fan-manual"])
                        .build();
                    let inner = gtk::Box::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .spacing(2)
                        .vexpand(true)
                        .valign(gtk::Align::Center)
                        .build();
                    let value = big_value("fan-manual-value", 5);
                    value.set_xalign(0.0);
                    inner.append(&value);
                    // Translators: under the duty cycle of a fan in manual
                    // mode, e.g. "45 %".
                    inner.append(&caption(&gettext("duty cycle"), true));
                    let scale = gtk::Scale::builder()
                        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 100.0, 1.0, 10.0, 0.0))
                        .sensitive(false)
                        .margin_top(18)
                        .build();
                    inner.append(&scale);
                    panel.append(&inner);
                    box_.append(&panel);
                    BodyWidgets::Manual {
                        panel,
                        value,
                        scale,
                    }
                }
                Body::NoCurve | Body::CurveUnreadable => {
                    let text = if matches!(body, Body::NoCurve) {
                        // Translators: on a fan card when the kernel driver
                        // can switch modes but does not report the curves.
                        gettext(
                            "A curve in the board's controller sets the speed. \
                             This driver does not show it.",
                        )
                    } else {
                        gettext("The curve could not be read from the board's controller.")
                    };
                    let (line, _) = note_line(&text);
                    box_.append(&line);
                    BodyWidgets::Note
                }
            },
        };

        // No speed since the start: the header may be empty.
        let (empty, empty_label) = note_line("");
        empty_label.set_label(&format!(
            "{} {}",
            empty_text(),
            // Translators: after "No speed since the app started: the
            // header may be empty."; Readings is the page of the
            // Preferences dialog.
            gettext("You can hide it in Preferences, on the Readings page.")
        ));
        empty.set_visible(false);
        box_.append(&empty);

        // The footer: a note, and the button that opens the curve.
        // At the bottom of a card as high as its neighbour.
        let foot = gtk::Box::builder()
            .spacing(8)
            .vexpand(true)
            .valign(gtk::Align::End)
            .css_classes(["fan-foot"])
            .build();
        let foot_note = caption("", true);
        foot_note.set_hexpand(true);
        let button_content = gtk::Box::builder().spacing(6).build();
        // Translators: a button on a fan card that opens its curve.
        button_content.append(&gtk::Label::new(Some(&gettext("Show Curve"))));
        button_content.append(&gtk::Image::from_icon_name("go-next-symbolic"));
        let button = gtk::Button::builder()
            .child(&button_content)
            .halign(gtk::Align::End)
            .hexpand(true)
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .action_name("win.show-curve")
            .action_target(&card.key.to_variant())
            .build();
        foot.append(&foot_note);
        foot.append(&button);
        box_.append(&foot);

        let widgets = Self {
            card: box_,
            head,
            name,
            subtitle,
            values,
            rpm,
            duty,
            switch,
            unchecked_empty,
            body,
            empty,
            foot,
            foot_note,
            button,
        };
        widgets.set_layout(layout);
        widgets
    }

    /// Narrow: the name gets the whole line and the speed goes under it;
    /// the chart is lower.
    pub(super) fn set_layout(&self, layout: Layout) {
        let narrow = layout == Layout::Narrow;
        // Whole names of the modes where there is room; a long one may be
        // shortened in a narrow window.
        if let Some(switch) = &self.switch {
            switch.set_can_shrink(narrow);
        }
        let attached = |row| {
            self.head
                .child_at(if row == 0 { 2 } else { 1 }, row)
                .is_some_and(|w| w == self.values)
        };
        if narrow && !attached(1) {
            self.head.remove(&self.values);
            self.head.attach(&self.values, 1, 1, 1, 1);
        } else if !narrow && !attached(0) {
            self.head.remove(&self.values);
            self.head.attach(&self.values, 2, 0, 1, 1);
        }
        self.values.set_orientation(if narrow {
            gtk::Orientation::Horizontal
        } else {
            gtk::Orientation::Vertical
        });
        self.values.set_spacing(if narrow { 12 } else { 0 });
        self.values.set_halign(if narrow {
            gtk::Align::Start
        } else {
            gtk::Align::End
        });
        self.duty.set_xalign(if narrow { 0.0 } else { 1.0 });
        self.rpm.set_xalign(if narrow { 0.0 } else { 1.0 });
        self.duty.set_valign(if narrow {
            gtk::Align::Baseline
        } else {
            gtk::Align::Fill
        });
        if let BodyWidgets::Chart(chart) = &self.body {
            chart.set_compact(narrow);
        }
        // As high as a chart, so that the grid of cards stays even.
        if let BodyWidgets::Manual { panel, .. } = &self.body {
            panel.set_height_request(if narrow { 136 } else { 150 });
        }
    }

    pub(super) fn update(&self, card: &Card, context: &PageContext) {
        let numbers = context.numbers;
        let name = overview::name(&card.row.name, numbers);
        set_label(&self.name, &name);
        let subtitle = subtitle(card, context);
        set_label(&self.subtitle, &subtitle);

        let speed = numbers.value(Kind::Fan, card.row.value);
        let (number, unit) = speed.split_once(UNIT_SPACE).unwrap_or((&speed, ""));
        // The unit at the size of normal text: 100 / 175.
        set_markup(&self.rpm, &big_markup(number, unit, 57));
        let quiet = card.maybe_empty();
        if quiet != self.card.has_css_class("quiet") {
            if quiet {
                self.card.add_css_class("quiet");
            } else {
                self.card.remove_css_class("quiet");
            }
        }
        let still = card.row.value == 0.0;
        if still != self.rpm.has_css_class("fan-still") {
            if still {
                self.rpm.add_css_class("fan-still");
            } else {
                self.rpm.remove_css_class("fan-still");
            }
        }

        let duty = card.duty.map(|d| percent(duty_to_percent(d)));
        let moving = card.moving_to().map(|t| percent(duty_to_percent(t)));
        let duty_line = match (&card.kind, &duty, &moving) {
            // A low card shows its duty cycle under the header line, and a
            // card in manual mode in large type.
            (CardKind::Low, ..) => String::new(),
            (
                CardKind::Full {
                    body: Body::Manual, ..
                },
                ..,
            ) => String::new(),
            (_, Some(duty), Some(target)) => {
                // Translators: the duty cycle of a fan moving to the one
                // its curve gives, e.g. "duty 30 % → 38 %".
                gettext("duty {duty} → {target}")
                    .replace("{duty}", duty)
                    .replace("{target}", target)
            }
            // Translators: the duty cycle of a fan, e.g. "duty 49 %".
            (_, Some(duty), None) => gettext("duty {duty}").replace("{duty}", duty),
            _ => String::new(),
        };
        set_label(&self.duty, &duty_line);
        self.duty.set_visible(!duty_line.is_empty());

        let mode = match &card.kind {
            CardKind::Full {
                control: Control::Switch { mode, .. },
                ..
            } => *mode,
            _ => None,
        };
        if let Some(switch) = &self.switch {
            let name = match mode {
                Some(Mode::Auto) => Some("auto"),
                Some(Mode::Manual) => Some("manual"),
                None => None,
            };
            if switch.active_name().as_deref() != name {
                switch.set_active_name(name);
            }
        }
        let empty = card.maybe_empty();
        if let Some(label) = &self.unchecked_empty {
            label.set_visible(empty);
        }
        self.empty.set_visible(
            empty && self.unchecked_empty.is_none() && !matches!(card.kind, CardKind::Low),
        );

        match &self.body {
            BodyWidgets::Chart(chart) => {
                if let Some(data) = card.chart() {
                    let source = data
                        .own_source
                        .as_ref()
                        .or(context.source)
                        .map(|s| source_name(s, numbers))
                        .unwrap_or_default();
                    chart.set_curve(Some(ChartCurve {
                        name: name.clone(),
                        source,
                        points: data.points.clone(),
                        min_percent: card.min_percent,
                        threshold: data.threshold,
                        needs_full: data.needs_full,
                    }));
                    chart.set_reading(
                        data.millidegrees
                            .zip(card.duty)
                            .map(|(millidegrees, duty)| Reading { millidegrees, duty }),
                    );
                }
            }
            BodyWidgets::Manual { value, scale, .. } => {
                let percent = card.duty.map(duty_to_percent);
                let number = percent.map(|p| p.to_string()).unwrap_or_default();
                // The unit at half the size: 150 / 300.
                set_markup(value, &big_markup(&number, "%", 50));
                scale.set_value(f64::from(percent.unwrap_or(0)));
            }
            BodyWidgets::Level { bar, caption } => {
                let percent = card.duty.map(duty_to_percent).unwrap_or(0);
                bar.set_value(f64::from(percent));
                let text = duty
                    .as_deref()
                    .map(|d| gettext("duty {duty}").replace("{duty}", d))
                    .unwrap_or_default();
                set_inscription(caption, &text);
                bar.update_property(&[gtk::accessible::Property::Label(&text)]);
            }
            BodyWidgets::None | BodyWidgets::Note => {}
        }

        // The footer: why the duty cycle changes, or what 0 % does.
        let note = if moving.is_some() {
            // Translators: under a fan card while the fan moves to the duty
            // cycle of its curve.
            gettext("In automatic mode the controller changes the duty cycle gradually.")
        } else if mode == Some(Mode::Manual) && card.min_percent == 0 {
            // Translators: under a fan card in manual mode; {duty} is "0 %".
            gettext("{duty} stops the fan").replace("{duty}", &percent(0))
        } else {
            String::new()
        };
        set_label(&self.foot_note, &note);
        self.foot_note.set_visible(!note.is_empty());
        let button = card.chart().is_some();
        self.button.set_visible(button);
        // Translators: read by screen readers for the button that opens
        // the curve of a fan; {name} is the fan, e.g. "CPU fan 1".
        let label = gettext("Show the curve of {name}").replace("{name}", &name);
        self.button
            .update_property(&[gtk::accessible::Property::Label(&label)]);
        self.foot.set_visible(button || !note.is_empty());

        // For screen readers the card is one item: its name, and what it
        // shows as a list.
        let mut parts = vec![subtitle, speed];
        if !duty_line.is_empty() {
            parts.push(duty_line);
        }
        if let Some(mode) = mode {
            parts.push(
                // Translators: read by screen readers on a fan card, e.g.
                // "mode: Automatic BIOS".
                gettext("mode: {mode}").replace(
                    "{mode}",
                    &match mode {
                        Mode::Auto => gettext("Automatic BIOS"),
                        Mode::Manual => gettext("Manual"),
                    },
                ),
            );
        }
        if self.unchecked_empty.is_some() {
            parts.push(gettext("This header was not checked on this board."));
        }
        if empty {
            parts.push(empty_text());
        }
        let description = parts.join(", ");
        self.card.update_property(&[
            gtk::accessible::Property::Label(&name),
            gtk::accessible::Property::Description(&description),
        ]);
    }
}

/// "No speed since the app started: the header may be empty."
fn empty_text() -> String {
    // Translators: on a fan card when the fan has not turned since
    // Baseboard started; nothing tells an empty header from a fan that
    // does not start.
    gettext("No speed since the app started: the header may be empty.")
}

/// The second line of a card: the header, the minimum where something
/// could be controlled, "header not tested" where no box says it, and the
/// source when it is not the page's.
fn subtitle(card: &Card, context: &PageContext) -> String {
    let mut parts = vec![card.header.clone()];
    let full = !matches!(card.kind, CardKind::Low);
    if full && card.min_percent > 0 {
        // Translators: the lowest duty cycle of a fan, e.g. "minimum 30 %".
        parts.push(gettext("minimum {duty}").replace("{duty}", &percent(card.min_percent)));
    }
    let boxed = matches!(
        card.kind,
        CardKind::Full {
            control: Control::Unchecked,
            ..
        }
    );
    if !card.verified && !boxed {
        // Translators: nobody has yet checked on this board that the
        // reading belongs to this header.
        parts.push(gettext("header not tested"));
    }
    if let Some(own) = card.chart().and_then(|c| c.own_source.as_ref()) {
        // Translators: the temperature a fan curve follows, when it is not
        // the one most curves follow, e.g. "source: System temperature".
        parts.push(
            gettext("source: {source}").replace("{source}", &source_name(own, context.numbers)),
        );
    }
    parts.join(" · ")
}

fn set_label(label: &gtk::Label, text: &str) {
    if label.label() != text {
        label.set_label(text);
    }
}

fn set_inscription(inscription: &gtk::Inscription, text: &str) {
    if inscription.text().as_deref() != Some(text) {
        inscription.set_text(Some(text));
    }
}
