// SPDX-License-Identifier: GPL-3.0-or-later

//! The Overview page (docs/PROJEKT.md §5.1, D64): on top a warning, three
//! large numbers and a short list (`summary`); every reading on the page
//! "All Sensors", which the window pushes over it: the groups and rows of
//! the model from `core::overview`, with names, subtitles and ratings
//! through gettext and numbers through `core::units`. A worker thread
//! (`sampler`, started by the window) takes the samples; the page only
//! shows them. The widgets are made again only when the rows change,
//! otherwise their texts are set.

mod summary;

use std::cell::{Cell, OnceCell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::fans::driver::Mode;
use baseboard_core::overview::{
    self, Group, GroupKind, Name, Rating, Row, RowId, Subtitle, Visibility,
};
use baseboard_core::profile::{FanRole, TempRole, VoltageRole};
use baseboard_core::sensors::Kind;
use baseboard_core::system;
use baseboard_core::units::Numbers;
use gettextrs::{gettext, pgettext};
use gtk::{glib, graphene};

use crate::sampler::{Sample, Status};
use summary::SummaryWidgets;

/// The title of a group.
pub fn group_title(kind: GroupKind) -> String {
    match kind {
        GroupKind::Cpu => gettext("Processor"),
        GroupKind::Board => gettext("Mainboard"),
        GroupKind::Fans => gettext("Fans"),
        GroupKind::Voltages => gettext("Voltages"),
        GroupKind::Gpu => gettext("Graphics card"),
        GroupKind::Disks => gettext("Disks"),
        GroupKind::Memory => pgettext("system memory", "Memory"),
        GroupKind::Other => gettext("Other"),
    }
}

/// The text of a reading's name. ATX rails are named by their nominal
/// voltage, formatted like every number (D26).
pub fn name(name: &Name, numbers: &Numbers) -> String {
    let numbered = |text: String, number: &u8| text.replace("{}", &number.to_string());
    match name {
        Name::Fan { role, number } => numbered(
            match role {
                // Translators: {} is the number of the fan in the board profile.
                FanRole::Cpu => gettext("CPU fan {}"),
                // Translators: {} is the number of the pump in the board profile.
                FanRole::Pump => gettext("Pump {}"),
                // Translators: {} is the number of the fan in the board profile.
                FanRole::Sys => gettext("System fan {}"),
                // Translators: {} is the number of the fan in the board profile.
                FanRole::Aux => gettext("Auxiliary fan {}"),
            },
            number,
        ),
        Name::Temp(role) => match role {
            TempRole::Cpu => gettext("Processor (board sensor)"),
            TempRole::System => gettext("System temperature"),
            TempRole::Vrm => gettext("Processor power stage"),
            TempRole::Chipset => gettext("Chipset"),
        },
        Name::Voltage(role) => match role {
            VoltageRole::P12v | VoltageRole::P5v | VoltageRole::P3v3 => role
                .nominal()
                .map(|volts| numbers.nominal(volts))
                .unwrap_or_default(),
            VoltageRole::Vcore => gettext("Processor cores"),
            VoltageRole::Soc => gettext("Processor SoC"),
            VoltageRole::Vddio => gettext("Processor VDDIO"),
        },
        Name::CpuTctl => gettext("Processor (Tctl)"),
        // Translators: {} is the number of the processor's core chiplet (CCD).
        Name::CpuChiplet(number) => numbered(gettext("Chiplet CCD{}"), number),
        Name::GpuChip => gettext("Graphics chip"),
        Name::GpuHotspot => gettext("Hotspot"),
        Name::GpuMemory => pgettext("graphics card", "Memory"),
        Name::GpuCoreVoltage => gettext("Core voltage"),
        Name::GpuPower => gettext("Power draw"),
        Name::GpuFan => gettext("Fan"),
        // Translators: the board's standby supply, which stays on while
        // the computer is off; VSB is the label of the sensor.
        Name::StandbyVoltage => gettext("Standby supply (VSB)"),
        // Translators: the battery that keeps the BIOS settings and the
        // clock; VBAT is the label of the sensor.
        Name::BatteryVoltage => gettext("Backup battery (VBAT)"),
        Name::Disk { model, sensor } => match sensor {
            Some(sensor) => format!("{model} · {sensor}"),
            None => model.clone(),
        },
        Name::DiskSensor { model, number } => {
            // Translators: {} is the number a drive gives one of its
            // temperature sensors, 1 to 8; their place is not documented.
            let sensor = numbered(gettext("Temperature sensor {}"), number);
            format!("{model} · {sensor}")
        }
        // Translators: {} counts the memory modules from 1.
        Name::MemoryModule(number) => numbered(gettext("Memory module {}"), number),
        Name::WifiCard => gettext("Wi-Fi card"),
        Name::NetworkCard => gettext("Network card"),
        Name::MemorySlot(text) | Name::Label(text) | Name::Sensors(text) | Name::Driver(text) => {
            text.clone()
        }
    }
}

/// The short name of an auxiliary voltage, for the folded row's summary.
fn short_name(name: &Name, numbers: &Numbers) -> String {
    match name {
        Name::StandbyVoltage => "VSB".to_owned(),
        Name::BatteryVoltage => "VBAT".to_owned(),
        _ => self::name(name, numbers),
    }
}

/// The second line of a row: the rating of a rated rail, otherwise where
/// the reading comes from.
pub fn subtitle(row: &Row, numbers: &Numbers) -> String {
    if let Some(Rating::Rated {
        tolerance_pct,
        deviation_pct,
        within,
        ..
    }) = row.rating
    {
        return rating_text(tolerance_pct, deviation_pct, within, numbers);
    }
    match &row.subtitle {
        Subtitle::Header {
            header,
            duty,
            verified,
            mode,
        } => {
            let duty = duty.map(|d| numbers.value(Kind::Pwm, d));
            // Only manual mode is named (D59): automatic BIOS is the usual
            // state, as a checked header is.
            let manual = *mode == Some(Mode::Manual);
            let text = match (&duty, verified, manual) {
                // Translators: {header} is a fan header on the board, e.g.
                // CPU_FAN1, and {duty} its duty cycle, e.g. "49 %".
                (Some(_), true, false) => gettext("{header} · duty {duty}"),
                // Translators: nobody has yet checked on this board that
                // the reading belongs to this header. Keep it short: it
                // has to fit in one line with the header and duty cycle.
                (Some(_), false, false) => gettext("{header} · duty {duty} · header not checked"),
                (None, true, false) => return header.clone(),
                (None, false, false) => gettext("{header} · header not checked"),
                // Translators: {mode} is "manual mode" for a fan in manual
                // mode, e.g. "CPU_FAN1 · duty 45 % · manual mode".
                (Some(_), true, true) => gettext("{header} · duty {duty} · {mode}"),
                (Some(_), false, true) => {
                    // Translators: as "{header} · duty {duty} · {mode}", for
                    // a header nobody has checked on this board.
                    gettext("{header} · duty {duty} · {mode} · header not checked")
                }
                // Translators: {mode} is "manual mode" for a fan in manual
                // mode; the driver gives no duty cycle.
                (None, true, true) => gettext("{header} · {mode}"),
                // Translators: as "{header} · {mode}", for a header nobody
                // has checked on this board.
                (None, false, true) => gettext("{header} · {mode} · header not checked"),
            };
            let text = text
                .replace("{header}", header)
                .replace("{duty}", duty.as_deref().unwrap_or_default());
            if manual {
                text.replace("{mode}", &manual_mode())
            } else {
                text
            }
        }
        Subtitle::Source { driver, channel } if channel.is_empty() => driver.clone(),
        Subtitle::Source { driver, channel } => format!("{driver} · {channel}"),
        // Translators: {driver} is a driver name, {address} the SMBus
        // address of a memory module, e.g. 0x51.
        Subtitle::MemoryModule { driver, address } => gettext("{driver} · address {address}")
            .replace("{driver}", driver)
            .replace("{address}", &format!("{address:#04x}")),
        Subtitle::Disk { controller } => controller.clone(),
    }
}

/// Manual mode in the subtitle of a fan (§5.5.3, D59); a line may break
/// around it, not inside it.
fn manual_mode() -> String {
    // Translators: in the subtitle of a fan on the Overview: the fan runs
    // at a fixed duty cycle, as "Manual" on the Fans page.
    gettext("manual mode").replace(' ', "\u{a0}")
}

/// The text of a rated rail; an unrated one shows its source instead.
fn rating_text(tolerance_pct: f64, deviation_pct: f64, within: bool, numbers: &Numbers) -> String {
    let text = if within {
        // Translators: e.g. "Within tolerance ±5 % (+0.8 %)".
        gettext("Within tolerance {tolerance} ({deviation})")
    } else {
        // Translators: e.g. "Outside tolerance ±5 % (−6.0 %)".
        gettext("Outside tolerance {tolerance} ({deviation})")
    };
    text.replace("{tolerance}", &numbers.tolerance(tolerance_pct))
        .replace("{deviation}", &numbers.deviation(deviation_pct))
}

/// The nominal voltage of a rated rail, which sets its decimals.
pub fn nominal(row: &Row) -> Option<f64> {
    match row.rating {
        Some(Rating::Rated { nominal, .. }) => Some(nominal),
        _ => None,
    }
}

/// What decides whether the widgets have to be made again.
#[derive(Debug, Clone, PartialEq)]
struct Shape {
    kind: GroupKind,
    description: Option<String>,
    rows: Vec<RowId>,
    auxiliary: Vec<RowId>,
}

impl Shape {
    fn of(group: &Group) -> Self {
        Self {
            kind: group.kind,
            description: group.description.clone(),
            rows: group.rows.iter().map(|r| r.id.clone()).collect(),
            auxiliary: group.auxiliary.iter().map(|r| r.id.clone()).collect(),
        }
    }
}

/// The widgets of one row.
/// Values and ranges are inscriptions, not labels: their size does not
/// depend on their text, so a new value every second redraws them without
/// laying out the whole window again.
pub(crate) struct RowWidgets {
    pub(crate) row: adw::ActionRow,
    value: gtk::Inscription,
    range: gtk::Inscription,
    icon: Option<gtk::Image>,
    /// The rating the icon shows, to set it only when it changes.
    shown_rating: Cell<Option<Option<bool>>>,
}

impl RowWidgets {
    pub(crate) fn new(rated: bool) -> Self {
        let row = adw::ActionRow::builder().use_markup(false).build();
        // Wide enough for "1020–1022 rpm" and "12,10–12,10 V", so that a
        // narrow window wraps the name rather than cutting the range.
        let range = gtk::Inscription::builder()
            .min_chars(13)
            .nat_chars(14)
            .xalign(1.0)
            .valign(gtk::Align::Center)
            .text_overflow(gtk::InscriptionOverflow::EllipsizeStart)
            .css_classes(["dimmed", "numeric"])
            .build();
        // Translators: the tooltip of the gray range next to a reading on
        // the page "All Sensors"; the line that said it above the readings
        // is gone (D64).
        let tooltip = gettext("Lowest and highest value since the app started");
        range.set_tooltip_text(Some(&tooltip));
        let value = gtk::Inscription::builder()
            .min_chars(8)
            .nat_chars(9)
            .xalign(1.0)
            .valign(gtk::Align::Center)
            .css_classes(["numeric"])
            .build();
        row.add_suffix(&range);
        let icon = rated.then(|| {
            let icon = gtk::Image::new();
            row.add_suffix(&icon);
            icon
        });
        row.add_suffix(&value);
        Self {
            row,
            value,
            range,
            icon,
            shown_rating: Cell::new(None),
        }
    }

    pub(crate) fn update(&self, data: &Row, numbers: &Numbers) {
        let nominal = nominal(data);
        let title = name(&data.name, numbers);
        let subtitle = subtitle(data, numbers);
        let value = numbers.value_of(data.kind, data.value, nominal);
        let range = data
            .range
            .map(|(lo, hi)| numbers.range_of(data.kind, lo, hi, nominal))
            .unwrap_or_default();
        if self.row.title() != title {
            self.row.set_title(&title);
        }
        let same_subtitle = self.row.subtitle().as_deref() == Some(subtitle.as_str());
        let same_value = self.value.text().as_deref() == Some(value.as_str());
        let same_range = self.range.text().as_deref() == Some(range.as_str());
        if same_subtitle && same_value && same_range {
            return;
        }
        if !same_subtitle {
            self.row.set_subtitle(&subtitle);
        }
        if !same_value {
            self.value.set_text(Some(&value));
        }
        if !same_range {
            self.range.set_text(Some(&range));
            // No range while the values have not parted: the subtitle
            // gets the room.
            self.range.set_visible(!range.is_empty());
        }
        // A fan at standstill, or an empty header: the value is dimmed.
        let still = data.kind == Kind::Fan && data.value == 0.0;
        if still != self.value.has_css_class("dimmed") {
            if still {
                self.value.add_css_class("dimmed");
            } else {
                self.value.remove_css_class("dimmed");
            }
        }
        if let Some(icon) = &self.icon {
            let within = match data.rating {
                Some(Rating::Rated { within, .. }) => Some(within),
                _ => None,
            };
            if self.shown_rating.get() != Some(within) {
                self.shown_rating.set(Some(within));
                set_rating_icon(icon, data.rating);
            }
        }
        let description = if range.is_empty() {
            // Translators: read by screen readers after the name of a
            // reading, e.g. "nct6687 · temp2. Now 37.5 °C".
            gettext("{subtitle}. Now {value}")
        } else {
            // Translators: read by screen readers after the name of a
            // reading, e.g. "nct6687 · temp2. Now 37.5 °C, since start
            // 36.0–38.0 °C".
            gettext("{subtitle}. Now {value}, since start {range}")
        }
        .replace("{subtitle}", &subtitle)
        .replace("{value}", &value)
        .replace("{range}", &range);
        self.row
            .upcast_ref::<gtk::Widget>()
            .update_property(&[gtk::accessible::Property::Description(&description)]);
    }
}

/// The warnings of the top of the page in words, for the System page: a
/// stopped CPU fan (D65) first, then each rail outside its tolerance.
fn alerts(top: &baseboard_core::summary::Summary, numbers: &Numbers) -> Vec<String> {
    let mut alerts = Vec::new();
    if top.fans.is_some_and(|fans| fans.cpu_stopped) {
        alerts.push(crate::alert::cpu_fan_stopped());
    }
    alerts.extend(
        top.power
            .iter()
            .flatten()
            .map(|rail| crate::alert::rail_outside(rail, numbers)),
    );
    alerts
}

/// The rating as an icon next to the value, never by color alone: the
/// text is in the subtitle. Unrated rails keep the space empty, so that
/// the values stay aligned.
fn set_rating_icon(icon: &gtk::Image, rating: Option<Rating>) {
    match rating {
        Some(Rating::Rated { within: true, .. }) => {
            icon.set_icon_name(Some("object-select-symbolic"));
            icon.set_css_classes(&["success"]);
            icon.set_opacity(1.0);
            icon.set_tooltip_text(Some(&gettext("Within tolerance")));
            icon.update_state(&[gtk::accessible::State::Hidden(false)]);
        }
        Some(Rating::Rated { within: false, .. }) => {
            icon.set_icon_name(Some("dialog-warning-symbolic"));
            icon.set_css_classes(&["warning"]);
            icon.set_opacity(1.0);
            icon.set_tooltip_text(Some(&gettext("Outside tolerance")));
            icon.update_state(&[gtk::accessible::State::Hidden(false)]);
        }
        _ => {
            icon.set_icon_name(Some("object-select-symbolic"));
            icon.set_opacity(0.0);
            icon.set_tooltip_text(None);
            icon.update_state(&[gtk::accessible::State::Hidden(true)]);
        }
    }
}

/// The folded row of the board's auxiliary voltages.
struct Auxiliary {
    expander: adw::ExpanderRow,
    rows: Vec<RowWidgets>,
}

impl Auxiliary {
    fn new(count: usize) -> Self {
        let expander = adw::ExpanderRow::builder()
            .use_markup(false)
            .title(gettext("Auxiliary voltages"))
            .build();
        let rows = (0..count)
            .map(|_| {
                let row = RowWidgets::new(false);
                expander.add_row(&row.row);
                row
            })
            .collect();
        Self { expander, rows }
    }

    fn update(&self, data: &[Row], numbers: &Numbers) {
        let summary = data
            .iter()
            .map(|r| {
                format!(
                    "{} {}",
                    short_name(&r.name, numbers),
                    numbers.value_of(r.kind, r.value, nominal(r))
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        if self.expander.subtitle() != summary {
            self.expander.set_subtitle(&summary);
        }
        for (widgets, row) in self.rows.iter().zip(data) {
            widgets.update(row, numbers);
        }
    }
}

/// The widgets of one group.
struct GroupWidgets {
    kind: GroupKind,
    widget: adw::PreferencesGroup,
    rows: Vec<RowWidgets>,
    auxiliary: Option<Auxiliary>,
}

/// The device a group is about, as its description shows it: the
/// processor's model without "N-Core Processor", the board's name, the
/// graphics card's name from the brackets of its database entry (D23).
pub fn device_description(group: &Group) -> Option<String> {
    let description = group.description.as_deref()?;
    Some(match group.kind {
        GroupKind::Cpu => system::short_cpu_name(description).to_owned(),
        GroupKind::Gpu => overview::marketing_name(description).to_owned(),
        _ => description.to_owned(),
    })
}

impl GroupWidgets {
    fn new(group: &Group) -> Self {
        let description = match group.kind {
            GroupKind::Fans => Some(gettext("In the order of the headers on the board")),
            GroupKind::Voltages => {
                Some(gettext("Rated against the tolerances in the board profile"))
            }
            _ => device_description(group),
        };
        let widget = adw::PreferencesGroup::builder()
            .title(glib::markup_escape_text(&group_title(group.kind)))
            .build();
        if let Some(description) = description {
            widget.set_description(Some(&glib::markup_escape_text(&description)));
        }
        let rows: Vec<RowWidgets> = group
            .rows
            .iter()
            .map(|row| {
                let widgets = RowWidgets::new(row.rating.is_some());
                widget.add(&widgets.row);
                widgets
            })
            .collect();
        let auxiliary = (!group.auxiliary.is_empty()).then(|| {
            let auxiliary = Auxiliary::new(group.auxiliary.len());
            widget.add(&auxiliary.expander);
            auxiliary
        });
        Self {
            kind: group.kind,
            widget,
            rows,
            auxiliary,
        }
    }

    fn update(&self, group: &Group, numbers: &Numbers) {
        for (widgets, row) in self.rows.iter().zip(&group.rows) {
            widgets.update(row, numbers);
        }
        if let Some(auxiliary) = &self.auxiliary {
            auxiliary.update(&group.auxiliary, numbers);
        }
    }

    /// In two columns: the right one has these, the left one the rest
    /// (memory goes left, to even out the columns; makiety/OPIS.md).
    fn goes_right(&self) -> bool {
        matches!(
            self.kind,
            GroupKind::Voltages | GroupKind::Gpu | GroupKind::Disks | GroupKind::Other
        )
    }
}

type StatusCallback = Box<dyn Fn(&Status)>;

#[derive(Default)]
struct State {
    shape: Vec<Shape>,
    groups: Vec<GroupWidgets>,
    wide: bool,
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/me/bojanowski/Baseboard/overview.ui")]
    pub struct OverviewPage {
        #[template_child]
        pub narrow: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub column: TemplateChild<gtk::Box>,
        #[template_child]
        pub heroes: TemplateChild<gtk::Box>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub all_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub sensors_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub wide: TemplateChild<adw::Breakpoint>,
        #[template_child]
        pub sensors_scrolled: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub left: TemplateChild<gtk::Box>,
        #[template_child]
        pub right: TemplateChild<gtk::Box>,
        pub(super) summary: OnceCell<SummaryWidgets>,
        pub(super) state: RefCell<State>,
        pub(super) numbers: OnceCell<Numbers>,
        /// The latest sample with every row; `None` before the first one.
        pub(super) last: RefCell<Option<Vec<Group>>>,
        /// The CPU fans that stand in the latest sample (D65).
        pub(super) stopped: RefCell<Vec<RowId>>,
        /// The warnings of the latest sample in words (`alerts`).
        pub(super) alerts: RefCell<Vec<String>>,
        /// The rows turned off and on in the settings.
        pub(super) visibility: RefCell<Visibility>,
        /// How the latest sample was read; `None` before the first one.
        pub(super) reading: RefCell<Option<Status>>,
        /// Told when that changes (the window passes it on).
        pub(super) on_status: RefCell<Option<StatusCallback>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for OverviewPage {
        const NAME: &'static str = "BaseboardOverviewPage";
        type Type = super::OverviewPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for OverviewPage {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            let _ = self.summary.set(SummaryWidgets::new(
                &self.column,
                &self.heroes,
                &self.list,
                &self.all_list,
            ));
            self.narrow.connect_apply(glib::clone!(
                #[weak]
                obj,
                move |_| obj.set_narrow(true)
            ));
            self.narrow.connect_unapply(glib::clone!(
                #[weak]
                obj,
                move |_| obj.set_narrow(false)
            ));
            self.wide.connect_apply(glib::clone!(
                #[weak]
                obj,
                move |_| obj.set_wide(true)
            ));
            self.wide.connect_unapply(glib::clone!(
                #[weak]
                obj,
                move |_| obj.set_wide(false)
            ));
        }
    }

    impl WidgetImpl for OverviewPage {}
    impl BinImpl for OverviewPage {}
}

glib::wrapper! {
    pub struct OverviewPage(ObjectSubclass<imp::OverviewPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl OverviewPage {
    /// The rows turned off and on in the settings; the page changes at once.
    pub fn set_visibility(&self, visibility: Visibility) {
        self.imp().visibility.replace(visibility);
        self.display();
    }

    /// The latest sample with every row, also the hidden ones.
    pub fn groups(&self) -> Vec<Group> {
        self.imp().last.borrow().clone().unwrap_or_default()
    }

    pub fn numbers(&self) -> &Numbers {
        self.imp()
            .numbers
            .get_or_init(|| Numbers::new(&crate::i18n::decimal_separator()))
    }

    /// Calls `f` with how the readings are read, whenever that changes.
    pub fn connect_status(&self, f: impl Fn(&Status) + 'static) {
        self.imp().on_status.replace(Some(Box::new(f)));
    }

    /// A new sample from the window's sensor reader.
    pub fn show(&self, sample: Sample) {
        let imp = self.imp();
        imp.last.replace(Some(sample.groups));
        imp.stopped.replace(sample.stopped);
        if imp.reading.borrow().as_ref() != Some(&sample.status) {
            self.show_status(&sample.status);
            if let Some(f) = imp.on_status.borrow().as_ref() {
                f(&sample.status);
            }
            imp.reading.replace(Some(sample.status));
        }
        self.display();
    }

    /// The banner when the board's chip is missing: the other chips are
    /// shown, and the System page says how to check the driver.
    fn show_status(&self, status: &Status) {
        if let Some(chip) = &status.missing_chip {
            self.imp().banner.set_title(
                // Translators: {} is the name of the board's chip, e.g. nct6687.
                &gettext("No readings from the board: its {} chip was not found")
                    .replace("{}", chip),
            );
        }
    }

    /// Shows the latest sample without the hidden rows.
    fn display(&self) {
        let imp = self.imp();
        let (any, groups) = match imp.last.borrow().as_deref() {
            // Still reading the first sample.
            None => return,
            Some(all) => (
                !all.is_empty(),
                overview::visible(all, &imp.visibility.borrow()),
            ),
        };
        // Without any readings the status page says more than the banner.
        let missing = imp
            .reading
            .borrow()
            .as_ref()
            .is_some_and(|status| status.missing_chip.is_some());
        imp.banner.set_revealed(missing && any);
        let numbers = self.numbers();
        let top = baseboard_core::summary::summary(&groups, &imp.stopped.borrow());
        imp.alerts.replace(alerts(&top, numbers));
        if groups.is_empty() {
            let (title, description) = if any {
                (
                    gettext("All readings are hidden"),
                    Some(gettext("Turn them on again in Preferences.")),
                )
            } else {
                (
                    gettext("No sensor readings"),
                    Some(gettext(
                        "This computer reports no hardware sensors (hwmon), \
                         for example in a virtual machine.",
                    )),
                )
            };
            imp.status.set_title(&title);
            imp.status.set_description(description.as_deref());
            imp.stack.set_visible_child_name("status");
            return;
        }
        let shape: Vec<Shape> = groups.iter().map(Shape::of).collect();
        if imp.state.borrow().shape != shape {
            self.rebuild(&groups, shape);
        }
        for (widgets, group) in imp.state.borrow().groups.iter().zip(&groups) {
            widgets.update(group, numbers);
        }
        if let Some(summary) = imp.summary.get() {
            summary.update(&top, numbers);
        }
        imp.stack.set_visible_child_name("summary");
    }

    /// The warnings of the latest sample in words, for the System page:
    /// a stopped CPU fan (D65) and rails outside their tolerance.
    pub fn alerts(&self) -> Vec<String> {
        self.imp().alerts.borrow().clone()
    }

    /// The page "All Sensors" for the window to push, opened on the group
    /// of `key` (`summary::group_key`), or at its top for an empty key.
    pub fn sensors_page(&self, key: &str) -> adw::NavigationPage {
        self.scroll_to_group(summary::group_of_key(key));
        self.imp().sensors_page.clone()
    }

    /// Scrolls "All Sensors" so that the group of `kind` stands on top, once
    /// it is laid out, and moves the keyboard focus into it.
    fn scroll_to_group(&self, kind: Option<GroupKind>) {
        let imp = self.imp();
        let target = kind.and_then(|kind| {
            imp.state
                .borrow()
                .groups
                .iter()
                .find(|g| g.kind == kind)
                .map(|g| g.widget.clone())
        });
        imp.sensors_scrolled.add_tick_callback(move |scrolled, _| {
            let adjustment = scrolled.vadjustment();
            let Some(target) = &target else {
                adjustment.set_value(0.0);
                return glib::ControlFlow::Break;
            };
            // Not laid out yet: the next frame.
            if target.height() == 0 {
                return glib::ControlFlow::Continue;
            }
            // The content under the viewport, which does not scroll.
            let content = scrolled.child().and_then(|viewport| viewport.first_child());
            if let Some(point) =
                content.and_then(|content| target.compute_point(&content, &graphene::Point::zero()))
            {
                adjustment.set_value(f64::from(point.y()));
            }
            target.child_focus(gtk::DirectionType::TabForward);
            glib::ControlFlow::Break
        });
    }

    fn set_narrow(&self, narrow: bool) {
        if let Some(summary) = self.imp().summary.get() {
            summary.set_narrow(narrow);
        }
    }

    fn rebuild(&self, groups: &[Group], shape: Vec<Shape>) {
        let imp = self.imp();
        {
            let mut state = imp.state.borrow_mut();
            for old in state.groups.drain(..) {
                if let Some(column) = old.widget.parent().and_downcast::<gtk::Box>() {
                    column.remove(&old.widget);
                }
            }
            state.groups = groups.iter().map(GroupWidgets::new).collect();
            state.shape = shape;
        }
        self.layout();
    }

    fn set_wide(&self, wide: bool) {
        self.imp().state.borrow_mut().wide = wide;
        self.layout();
    }

    /// Debug builds: scrolls to the end, for a screenshot of the lower
    /// groups.
    #[cfg(debug_assertions)]
    pub fn scroll_to_end(&self) {
        let adjustment = self.imp().scrolled.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }

    /// Puts the groups in one column in the order of §5.1, or in two.
    fn layout(&self) {
        let imp = self.imp();
        let state = imp.state.borrow();
        for group in &state.groups {
            if let Some(column) = group.widget.parent().and_downcast::<gtk::Box>() {
                column.remove(&group.widget);
            }
        }
        let mut right = false;
        for group in &state.groups {
            if state.wide && group.goes_right() {
                imp.right.append(&group.widget);
                right = true;
            } else {
                imp.left.append(&group.widget);
            }
        }
        imp.right.set_visible(right);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(subtitle: Subtitle, rating: Option<Rating>) -> Row {
        Row {
            id: RowId {
                chip: "nct6687".into(),
                device: Some("devices/platform/nct6683.2592".into()),
                channel: "in1".into(),
            },
            name: Name::Driver("in1".into()),
            subtitle,
            kind: Kind::Voltage,
            value: 1.0,
            rating,
            range: None,
        }
    }

    fn source(driver: &str, channel: &str) -> Subtitle {
        Subtitle::Source {
            driver: driver.into(),
            channel: channel.into(),
        }
    }

    /// A rated rail shows its rating; an unrated one, like a temperature,
    /// where it comes from. An empty channel shows the driver alone.
    #[test]
    fn subtitles() {
        let numbers = Numbers::new(".");
        let unrated = row(source("nct6687", "in1"), Some(Rating::Unrated));
        assert_eq!(subtitle(&unrated, &numbers), "nct6687 · in1");
        let rated = row(
            source("nct6687", "in0"),
            Some(Rating::Rated {
                nominal: 12.0,
                tolerance_pct: 5.0,
                deviation_pct: 0.8,
                within: true,
            }),
        );
        assert!(subtitle(&rated, &numbers).starts_with("Within tolerance"));
        assert_eq!(subtitle(&row(source("r8169", ""), None), &numbers), "r8169");
        let header = |duty, verified, mode| {
            let header = Subtitle::Header {
                header: "PUMP_FAN1".into(),
                duty,
                verified,
                mode,
            };
            subtitle(&row(header, None), &numbers)
        };
        assert_eq!(
            header(Some(255.0), false, None),
            "PUMP_FAN1 · duty 100\u{a0}% · header not checked"
        );
        assert_eq!(
            header(Some(255.0), true, None),
            "PUMP_FAN1 · duty 100\u{a0}%"
        );
        assert_eq!(header(None, true, None), "PUMP_FAN1");
        assert_eq!(header(None, false, None), "PUMP_FAN1 · header not checked");
    }

    /// Manual mode (§5.5.3, D59) after the duty cycle, before "header not
    /// tested", never split over two lines; automatic BIOS is not named.
    #[test]
    fn subtitles_with_the_mode() {
        let numbers = Numbers::new(".");
        let header = |duty, verified, mode| {
            let header = Subtitle::Header {
                header: "CPU_FAN1".into(),
                duty,
                verified,
                mode: Some(mode),
            };
            subtitle(&row(header, None), &numbers)
        };
        assert_eq!(
            header(Some(115.0), true, Mode::Manual),
            "CPU_FAN1 · duty 45\u{a0}% · manual\u{a0}mode"
        );
        assert_eq!(
            header(Some(255.0), false, Mode::Manual),
            "CPU_FAN1 · duty 100\u{a0}% · manual\u{a0}mode · header not checked"
        );
        assert_eq!(
            header(None, true, Mode::Manual),
            "CPU_FAN1 · manual\u{a0}mode"
        );
        assert_eq!(
            header(None, false, Mode::Manual),
            "CPU_FAN1 · manual\u{a0}mode · header not checked"
        );
        assert_eq!(
            header(Some(124.0), true, Mode::Auto),
            "CPU_FAN1 · duty 49\u{a0}%"
        );
        assert_eq!(
            header(Some(255.0), false, Mode::Auto),
            "CPU_FAN1 · duty 100\u{a0}% · header not checked"
        );
        assert_eq!(header(None, true, Mode::Auto), "CPU_FAN1");
    }

    /// The folded row of auxiliary voltages names VSB and VBAT short.
    #[test]
    fn short_names_of_auxiliary_voltages() {
        let numbers = Numbers::new(".");
        assert_eq!(short_name(&Name::StandbyVoltage, &numbers), "VSB");
        assert_eq!(short_name(&Name::BatteryVoltage, &numbers), "VBAT");
        assert_eq!(
            short_name(&Name::StandbyVoltage, &numbers),
            name(&Name::Driver("VSB".into()), &numbers)
        );
        assert_eq!(
            name(&Name::StandbyVoltage, &numbers),
            "Standby supply (VSB)"
        );
    }
}
