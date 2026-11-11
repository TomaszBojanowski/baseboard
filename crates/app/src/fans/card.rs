// SPDX-License-Identifier: GPL-3.0-or-later

//! The card of one header on the Fans page (docs/PROJEKT.md §7 M2, M2.4):
//! icon, name and header, speed and duty cycle; a status box with the mode
//! as text and a lock (0.2.0 changes nothing, D60), or with the note of a
//! header nobody checked; the chart of the curve, the duty cycle of manual
//! mode, a bar, or a note; a footer with "Curve Details". The card itself
//! does nothing when activated, but takes the keyboard focus, so that a
//! screen reader reads it.
//!
//! The header line follows the page's rule (`layout`, D61): name and
//! subtitle on one line each, the values beside or under them by width,
//! never by the values shown. A card with a curve keeps the room of its
//! footer note, so that its height does not change.
//!
//! Speeds are inscriptions, whose size does not depend on their text: a
//! new value every second redraws them without laying out the window
//! again. The speed is as wide as its text, which with tabular digits
//! changes only with the number of digits.

use std::rc::Rc;

use adw::prelude::*;
use baseboard_core::fans::driver::Mode;
use baseboard_core::fans::page::{Body, Card, CardKind, Control};
use baseboard_core::profile::FanRole;
use baseboard_core::sensors::Kind;
use baseboard_core::units::{Numbers, UNIT_SPACE, duty_to_percent};
use gettextrs::gettext;
use gtk::{glib, pango};

use super::layout::{CardHead, HeadNeeds, HeadParts, PageLayout};
use super::slot::StatusSlot;
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
    /// Lays out the header line by the page's rule (D61).
    head: CardHead,
    icon: gtk::Image,
    name: gtk::Label,
    subtitle: gtk::Label,
    rpm: gtk::Inscription,
    duty: gtk::Label,
    /// The card shows a duty cycle under its speed (not a card of a
    /// driver without control, which has a bar).
    duty_line: bool,
    /// The place of the status box: the mode, or the note of an unchecked
    /// header.
    status: Option<StatusSlot>,
    mode: Option<gtk::Label>,
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

/// Makes `inscription` as wide as `markup` in its font, with tabular
/// digits as the `numeric` style draws them. The width changes only with
/// the number of digits, so a new speed rarely lays out the window, and
/// "0 rpm" takes no more room than it needs.
fn fit_width(inscription: &gtk::Inscription, markup: &str) {
    let layout = inscription.create_pango_layout(None);
    layout.set_markup(markup);
    let attributes = layout.attributes().unwrap_or_default();
    attributes.insert(pango::AttrFontFeatures::new("tnum"));
    layout.set_attributes(Some(&attributes));
    let (ink, logical) = layout.pixel_extents();
    let width = (ink.x() + ink.width()).max(logical.x() + logical.width());
    if inscription.width_request() != width {
        inscription.set_width_request(width);
    }
}

/// The text of `markup` without its tags.
fn markup_text(markup: &str) -> String {
    pango::parse_markup(markup, '\0')
        .map(|(_, text, _)| text.to_string())
        .unwrap_or_default()
}

/// One line, never wrapped: the page's layout gives it the room (D61);
/// the ellipsis is only a last resort, with very large text.
fn single_line(css: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .xalign(0.0)
        .single_line_mode(true)
        .ellipsize(pango::EllipsizeMode::End)
        .css_classes(css)
        .build()
}

/// The width of `text` (markup) in the font of `widget`, with tabular
/// digits as the `numeric` style draws them.
fn text_width(widget: &impl IsA<gtk::Widget>, markup: &str) -> i32 {
    let layout = widget.create_pango_layout(None);
    layout.set_markup(markup);
    let attributes = layout.attributes().unwrap_or_default();
    attributes.insert(pango::AttrFontFeatures::new("tnum"));
    layout.set_attributes(Some(&attributes));
    let (ink, logical) = layout.pixel_extents();
    (ink.x() + ink.width()).max(logical.x() + logical.width())
}

impl CardWidgets {
    pub(super) fn new(card: &Card, layout: Layout, page: Rc<PageLayout>) -> Self {
        // Not activatable, but it takes the keyboard focus, so that the
        // keyboard goes from card to card and a screen reader reads each.
        let box_: gtk::Box = CardBox::new().upcast();
        box_.set_orientation(gtk::Orientation::Vertical);
        box_.set_spacing(12);
        box_.set_focusable(true);
        box_.add_css_class("card");
        box_.add_css_class("fan-card");

        // The header line: icon, name and subtitle, speed and duty cycle,
        // laid out by the page's rule (`layout`, D61).
        // The image is the tinted square (style.css): GTK draws the icon in
        // the middle of the image, while a box would put it at its start.
        let icon = gtk::Image::builder()
            .icon_name(match card.role {
                FanRole::Pump => "baseboard-pump-symbolic",
                _ => "baseboard-fan-symbolic",
            })
            .css_classes(["fan-icon"])
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Center)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        let texts = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(3)
            .build();
        let name = single_line(&["heading"]);
        let subtitle = single_line(&["caption", "fan-soft"]);
        texts.append(&name);
        texts.append(&subtitle);
        // "1024 rpm": the unit at the size of normal text. As wide as the
        // text (`fit_width`), not a fixed number of characters.
        let rpm = big_value("fan-rpm", 0);
        // A label as wide as its text: it changes only with the duty cycle,
        // not with every sample. Always there, empty when there is no duty
        // cycle, so that every header line is equally high.
        let duty = gtk::Label::builder()
            .xalign(0.0)
            .css_classes(["caption", "fan-soft", "numeric"])
            .build();
        let head = CardHead::new(
            HeadParts {
                icon: icon.clone().upcast(),
                texts: texts.upcast(),
                rpm: rpm.clone().upcast(),
                duty: duty.clone().upcast(),
            },
            page,
        );
        box_.append(&head);

        // The status box: the mode as text with a lock, as this version
        // changes nothing (D60), or the box of a header nobody checked.
        let mut status = None;
        let mut mode = None;
        let mut unchecked_empty = None;
        if let CardKind::Full { control, .. } = &card.kind {
            let frame = match control {
                Control::Switch { .. } => {
                    let (frame, label) = mode_box();
                    mode = Some(label);
                    frame
                }
                Control::Unchecked => {
                    let (frame, empty) = unchecked_box();
                    unchecked_empty = Some(empty);
                    frame
                }
            };
            let slot = StatusSlot::new(&frame);
            box_.append(&slot);
            status = Some(slot);
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
        // The note takes the room the button leaves, so that it wraps only
        // where the card is narrow; a card with the button always has the
        // note, which keeps the button at the end.
        foot_note.set_hexpand(true);
        let button_content = gtk::Box::builder().spacing(6).build();
        // Translators: a button on a fan card that opens a page with the
        // large chart of its curve and the values of its points.
        button_content.append(&gtk::Label::new(Some(&gettext("Curve Details"))));
        button_content.append(&gtk::Image::from_icon_name("go-next-symbolic"));
        let button = gtk::Button::builder()
            .child(&button_content)
            .halign(gtk::Align::End)
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
            icon,
            name,
            subtitle,
            rpm,
            duty,
            duty_line: !matches!(card.kind, CardKind::Low),
            status,
            mode,
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

    /// Narrow: the chart is lower. The header line follows its own width
    /// (`layout`, D61).
    pub(super) fn set_layout(&self, layout: Layout) {
        let narrow = layout == Layout::Narrow;
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
        let markup = big_markup(number, unit, 57);
        fit_width(&self.rpm, &markup);
        set_markup(&self.rpm, &markup);
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
        // Empty, but in place: every header line is equally high (D61).
        set_label(&self.duty, &duty_line);

        let mode = match &card.kind {
            CardKind::Full {
                control: Control::Switch { mode, .. },
                ..
            } => *mode,
            _ => None,
        };
        if let Some(label) = &self.mode {
            let markup = mode_markup(mode);
            if label.label() != markup {
                label.set_markup(&markup);
                self.status_changed();
            }
        }
        let empty = card.maybe_empty();
        if let Some(label) = &self.unchecked_empty
            && label.is_visible() != empty
        {
            label.set_visible(empty);
            self.status_changed();
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

        // The footer: why the duty cycle changes, or what 0 % does. A card
        // with a curve keeps the room of its note at all times, so that it
        // does not grow when the fan moves to its curve and the button
        // stays in place (D61): the note is only shown or hidden.
        let (note, shown) = if card.chart().is_some() {
            // Translators: under a fan card while the fan moves to the duty
            // cycle of its curve.
            let note =
                gettext("In automatic mode the controller changes the duty cycle gradually.");
            (note, moving.is_some())
        } else if mode == Some(Mode::Manual) && card.min_percent == 0 {
            // Translators: under a fan card in manual mode; {duty} is "0 %".
            (
                gettext("{duty} stops the fan").replace("{duty}", &percent(0)),
                true,
            )
        } else {
            (String::new(), false)
        };
        set_label(&self.foot_note, &note);
        self.foot_note.set_visible(!note.is_empty());
        let opacity = if shown { 1.0 } else { 0.0 };
        if self.foot_note.opacity() != opacity {
            self.foot_note.set_opacity(opacity);
            self.foot_note
                .update_state(&[gtk::accessible::State::Hidden(!shown)]);
        }
        let button = card.chart().is_some();
        self.button.set_visible(button);
        // Translators: read by screen readers for the button that opens
        // the details of the curve of a fan; {name} is the fan, e.g.
        // "CPU fan 1".
        let label = gettext("Curve details: {name}").replace("{name}", &name);
        self.button
            .update_property(&[gtk::accessible::Property::Label(&label)]);
        self.foot.set_visible(button || !note.is_empty());

        // For screen readers the card is one item: its name, and what it
        // shows as a list.
        let mut parts = vec![subtitle, speed];
        if !duty_line.is_empty() {
            parts.push(duty_line);
        }
        match mode {
            Some(mode) => parts.push(
                // Translators: read by screen readers on a fan card, e.g.
                // "mode: Automatic BIOS".
                gettext("mode: {mode}").replace("{mode}", &mode_name(mode)),
            ),
            None if self.mode.is_some() => parts.push(mode_unknown()),
            None => {}
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

    /// What the header line of this card needs for its widest content
    /// (D61): its name and subtitle, a four-digit speed and, where the card
    /// shows one, the duty cycle with a target, both 100 %.
    pub(super) fn needs(&self, numbers: &Numbers) -> HeadNeeds {
        let horizontal = gtk::Orientation::Horizontal;
        let icon = self.icon.measure(horizontal, -1).1;
        let texts = self
            .name
            .measure(horizontal, -1)
            .1
            .max(self.subtitle.measure(horizontal, -1).1);
        let speed = numbers.value(Kind::Fan, 8888.0);
        let (number, unit) = speed.split_once(UNIT_SPACE).unwrap_or((&speed, ""));
        let rpm = text_width(&self.rpm, &big_markup(number, unit, 57));
        let duty = if self.duty_line {
            let full = percent(100);
            let text = gettext("duty {duty} → {target}")
                .replace("{duty}", &full)
                .replace("{target}", &full);
            text_width(&self.duty, &glib::markup_escape_text(&text))
        } else {
            0
        };
        HeadNeeds::of(icon, texts, rpm, duty)
    }

    /// The page's needs changed: the header line lays out again.
    pub(super) fn relayout_head(&self) {
        self.head.queue_resize();
    }

    /// The text of the status box changed: the box next to it measures
    /// again too.
    fn status_changed(&self) {
        if let Some(slot) = &self.status {
            slot.content_changed();
        }
    }
}

/// The status box with the mode as text and a lock (D60); the label gets
/// the mode on update.
pub(super) fn mode_box() -> (gtk::Box, gtk::Label) {
    let frame = gtk::Box::builder()
        .spacing(10)
        .css_classes(["fan-status"])
        .build();
    let label = gtk::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .natural_wrap_mode(gtk::NaturalWrapMode::Word)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .use_markup(true)
        .css_classes(["caption"])
        .build();
    let lock = gtk::Image::builder()
        .icon_name("system-lock-screen-symbolic")
        .valign(gtk::Align::Center)
        .css_classes(["dimmed"])
        .build();
    // Translators: read by screen readers next to the mode of a fan, which
    // this version does not change.
    let cannot = gettext("Cannot be changed");
    lock.update_property(&[gtk::accessible::Property::Label(&cannot)]);
    frame.append(&label);
    frame.append(&lock);
    (frame, label)
}

/// The status box of a header nobody checked on this board; the second
/// label, shown only then, says that the header may be empty.
pub(super) fn unchecked_box() -> (gtk::Box, gtk::Label) {
    let frame = gtk::Box::builder()
        .spacing(10)
        .css_classes(["fan-status"])
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
        // Translators: in place of the mode switch of a fan header whose
        // channel nobody has checked on this board.
        &gettext("This header was not checked on this board."),
        false,
    ));
    let empty = caption(&empty_text(), true);
    lines.append(&empty);
    frame.append(&icon);
    frame.append(&lines);
    (frame, empty)
}

/// The name of a mode, as the mode switch of the editor will name it.
fn mode_name(mode: Mode) -> String {
    match mode {
        Mode::Auto => gettext("Automatic BIOS"),
        Mode::Manual => gettext("Manual"),
    }
}

fn mode_unknown() -> String {
    // Translators: in the status box of a fan card when the kernel driver
    // gives no mode.
    gettext("The mode could not be read")
}

/// The text of the status box: the mode, its name in bold (D60).
fn mode_markup(mode: Option<Mode>) -> String {
    match mode {
        Some(mode) => {
            // Translators: in the status box of a fan card, e.g. "Mode:
            // Automatic BIOS"; {mode} is shown in bold.
            let template = glib::markup_escape_text(&gettext("Mode: {mode}")).to_string();
            let name = format!("<b>{}</b>", glib::markup_escape_text(&mode_name(mode)));
            template.replace("{mode}", &name)
        }
        None => glib::markup_escape_text(&mode_unknown()).to_string(),
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

#[cfg(test)]
mod tests {
    use baseboard_core::fans::driver::CurvePoint;
    use baseboard_core::fans::page::Chart;
    use baseboard_core::overview::{Name, Row, RowId, Subtitle};

    use super::*;

    /// The mode as text with its name in bold, or why there is none (D60).
    #[test]
    fn mode_as_text() {
        assert_eq!(mode_markup(Some(Mode::Auto)), "Mode: <b>Automatic BIOS</b>");
        assert_eq!(mode_markup(Some(Mode::Manual)), "Mode: <b>Manual</b>");
        assert_eq!(mode_markup(None), "The mode could not be read");
    }

    /// The card of a CPU fan in automatic mode at `rpm`, its duty cycle
    /// `duty` moving to `target` (0–255).
    fn cpu_card(rpm: f64, duty: u8, target: u8) -> Card {
        Card {
            key: "cpu".into(),
            role: FanRole::Cpu,
            pwm: Some(1),
            row: Row {
                id: RowId {
                    chip: "nct6687".into(),
                    device: None,
                    channel: "fan1".into(),
                },
                name: Name::Fan {
                    role: FanRole::Cpu,
                    number: 1,
                },
                subtitle: Subtitle::Header {
                    header: "CPU_FAN1".into(),
                    duty: Some(f64::from(duty)),
                    verified: true,
                    mode: Some(Mode::Auto),
                },
                kind: Kind::Fan,
                value: rpm,
                rating: None,
                range: Some((rpm, rpm)),
            },
            header: "CPU_FAN1".into(),
            verified: true,
            min_percent: 30,
            duty: Some(duty),
            kind: CardKind::Full {
                control: Control::Switch {
                    mode: Some(Mode::Auto),
                    positions: 2,
                },
                body: Body::Chart(Box::new(Chart {
                    points: vec![
                        CurvePoint { temp: 30, duty: 77 },
                        CurvePoint {
                            temp: 80,
                            duty: 255,
                        },
                    ],
                    own_source: None,
                    millidegrees: Some(85_000),
                    threshold: Some(95),
                    target: Some(target),
                    needs_full: true,
                })),
            },
        }
    }

    /// Where `widget` is in `card`: x, y, width, height.
    fn place(widget: &impl IsA<gtk::Widget>, card: &gtk::Box) -> (f32, f32, i32, i32) {
        let point = widget
            .compute_point(card, &gtk::graphene::Point::new(0.0, 0.0))
            .unwrap();
        (point.x(), point.y(), widget.width(), widget.height())
    }

    /// The layout of a card follows its width only (D61): at rest and while
    /// its fan moves to 100 % with a four-digit speed, the header line keeps
    /// its arrangement, the name and the subtitle one line each, and the
    /// card its height and the place of its chart and button; the footer
    /// note keeps its room while hidden. Needs a display; skipped without
    /// one.
    #[test]
    fn card_layout_follows_width_only() {
        crate::gtk_test::run("card_layout_follows_width_only", card_layout);
    }

    fn card_layout() {
        let numbers = Numbers::new(".");
        let context = PageContext {
            numbers: &numbers,
            source: None,
        };
        let rest = cpu_card(890.0, 124, 124);
        let moving = cpu_card(1520.0, 178, 255);
        assert_eq!(rest.moving_to(), None);
        assert_eq!(moving.moving_to(), Some(255));

        let page = Rc::new(PageLayout::default());
        let widgets = CardWidgets::new(&rest, Layout::Wide, page.clone());
        widgets.update(&rest, &context);
        let needs = widgets.needs(&numbers);
        page.needs.set(needs);
        widgets.relayout_head();
        let card = widgets.card.clone();
        let lay_out = |width: i32| {
            let (_, height, _, _) = card.measure(gtk::Orientation::Vertical, width);
            card.allocate(width, height, -1, None);
        };
        // The room of the card around its header line.
        lay_out(2 * needs.wide);
        let padding = card.width() - widgets.head.width();

        for head_width in [needs.wide + 20, needs.wide - 1, needs.below - 1] {
            let width = head_width + padding;
            let mut seen = Vec::new();
            for state in [&rest, &moving, &rest] {
                widgets.update(state, &context);
                lay_out(width);
                for label in [&widgets.name, &widgets.subtitle] {
                    assert_eq!(label.layout().line_count(), 1, "{}", label.label());
                    assert!(!label.layout().is_ellipsized(), "{}", label.label());
                }
                seen.push((
                    card.height(),
                    place(&widgets.name, &card),
                    place(&widgets.duty, &card).1,
                    place(&widgets.button, &card),
                    place(&widgets.foot_note, &card),
                ));
            }
            assert!(
                seen.windows(2).all(|w| w[0] == w[1]),
                "{head_width}: {seen:?}"
            );
            // The note is shown only while the fan moves to its curve.
            assert_eq!(widgets.foot_note.opacity(), 0.0);
        }

        // Wide: the values on the right of the name and subtitle; narrower:
        // under them.
        let values_under_texts = |head_width: i32| {
            lay_out(head_width + padding);
            let subtitle = place(&widgets.subtitle, &card);
            place(&widgets.duty, &card).1 >= subtitle.1 + subtitle.3 as f32
        };
        assert!(!values_under_texts(needs.wide));
        assert!(values_under_texts(needs.wide - 1));
        assert!(values_under_texts(needs.below - 1));
    }
}
