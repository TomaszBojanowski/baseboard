// SPDX-License-Identifier: GPL-3.0-or-later

//! The System page (docs/PROJEKT.md §5.4, D64), read only. On top one
//! status row ("Everything works", or what needs attention), then a short
//! list of the computer in plain words. Two pages the window pushes over
//! it hold the rest: "Diagnostics" (the drivers and programs around the
//! sensors and fans, the board profile, skipped files, warnings of the
//! readings) and "Technical Details" (the board, BIOS, processor and
//! system, memory, graphics cards, disks and network cards with their
//! codes and versions). The data comes from a worker thread
//! (`system_reader`); the text of "Copy system information" from
//! `core::report`, always in English.

use std::cell::{OnceCell, RefCell};
use std::path::PathBuf;

use adw::prelude::*;
use adw::subclass::prelude::*;
use baseboard_core::board::{Detection, Dmi, code_label};
use baseboard_core::fans::driver::DriverLevel;
use baseboard_core::overview::marketing_name;
use baseboard_core::profile::Profile;
use baseboard_core::report::Report;
use baseboard_core::system::{
    self, Bios, Disk, GraphicsCard, Manufacturer, MemoryModule, MemorySlot, ModuleOrigin,
    NetworkCard, PciId, SystemInfo, short_cpu_name,
};
use gettextrs::{gettext, ngettext, pgettext};
use gtk::{gio, glib};

use crate::config;
use crate::sensors::Choice;
use crate::system_reader::Worker;

/// What the page needs besides the data it reads.
pub struct Setup {
    /// The sysfs root: /sys, or a fake tree in a debug build.
    pub root: PathBuf,
    pub dmi: Dmi,
    pub detection: Detection,
    /// Files left out at start because they are wrong.
    pub skipped: Vec<SkippedFile>,
}

/// A file left out at start, which the app works without (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub kind: FileKind,
    /// Why, as the parser says it; not translated.
    pub reason: String,
}

impl SkippedFile {
    /// `reason` in one line: a TOML error shows the line of the file under
    /// its message, which the page leaves out ("…at line 1, column 1:
    /// missing field `model`").
    pub fn new(path: PathBuf, kind: FileKind, reason: &str) -> Self {
        let parts: Vec<&str> = reason
            .lines()
            .map(str::trim)
            .filter(|line| {
                let quoted = line.starts_with('|')
                    || line
                        .split_once(" |")
                        .is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit()));
                !line.is_empty() && !quoted
            })
            .collect();
        Self {
            path,
            kind,
            reason: parts.join(": "),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A board profile (§6.2).
    Profile,
    /// The user's own names for readings (D22).
    Labels,
}

/// What the page shows besides the data it reads.
#[derive(Debug, Clone, Copy, Default)]
pub struct Extras<'a> {
    /// How the Overview reads the sensors; `None` before its first sample.
    pub source: Option<&'a Choice>,
    pub skipped: &'a [SkippedFile],
}

/// One group of the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub title: String,
    pub description: Option<String>,
    /// Never empty: a section without data has a "No data" line.
    pub lines: Vec<Line>,
}

/// One row: a name and its value, both read by screen readers.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub title: String,
    pub value: String,
    pub style: Style,
    pub status: Option<Status>,
    /// A path or a command, set in a monospace font.
    pub monospace: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// A name and its value next to it, as in "About" of GNOME Settings:
    /// the short list of the computer.
    Value,
    /// A field and its value; the value stands out.
    Property,
    /// A thing and its details, e.g. a disk.
    Item,
    /// "No data" in place of what a section would show.
    NoData,
}

/// An icon before the row; the text says the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warning,
    /// Neither: something to know, e.g. readings straight from sysfs. In a
    /// group of checks every row has an icon, so that the texts line up.
    Info,
}

impl Line {
    fn property(title: String, value: Option<String>) -> Self {
        Self {
            title,
            value: value.unwrap_or_else(no_data),
            style: Style::Property,
            status: None,
            monospace: false,
        }
    }

    fn item(title: String, value: String) -> Self {
        Self {
            style: Style::Item,
            ..Self::property(title, Some(value))
        }
    }

    fn value(title: String, value: Option<String>) -> Self {
        Self {
            style: Style::Value,
            ..Self::property(title, value)
        }
    }

    fn no_data() -> Self {
        Self {
            title: no_data(),
            value: String::new(),
            style: Style::NoData,
            status: None,
            monospace: false,
        }
    }

    fn status(self, status: Option<Status>) -> Self {
        Self { status, ..self }
    }

    fn monospace(self) -> Self {
        Self {
            monospace: true,
            ..self
        }
    }
}

fn no_data() -> String {
    gettext("No data")
}

fn section(title: String, description: Option<String>, mut lines: Vec<Line>) -> Section {
    if lines.is_empty() {
        lines.push(Line::no_data());
    }
    Section {
        title,
        description,
        lines,
    }
}

/// The groups of the three views of the page (D64), in the order of the
/// mockup. Every line is in "Diagnostics" or "Technical Details"; the short
/// list repeats the main ones in plain words.
#[derive(Debug, Clone, PartialEq)]
pub struct Views {
    pub summary: Vec<Section>,
    pub diagnostics: Vec<Section>,
    pub details: Vec<Section>,
}

/// `alerts` are warnings of the readings from the Overview (a rail outside
/// its tolerance, a stopped CPU fan), already in words.
pub fn views(
    dmi: &Dmi,
    detection: &Detection,
    info: &SystemInfo,
    extras: Extras,
    alerts: &[String],
) -> Views {
    let mut details = vec![
        board(dmi),
        bios(&info.bios),
        processor_and_system(info),
        memory(info.memory.as_deref()),
    ];
    details.extend(graphics(&info.graphics));
    details.push(disks(&info.disks));
    details.push(network(&info.network));

    let mut diagnostics = Vec::new();
    if !alerts.is_empty() {
        diagnostics.push(readings(alerts));
    }
    diagnostics.push(sensors_and_fans(detection, info, extras.source));
    diagnostics.push(section(
        gettext("Board profile"),
        None,
        vec![profile_line(detection)],
    ));
    if !extras.skipped.is_empty() {
        diagnostics.push(skipped_files(extras.skipped));
    }

    Views {
        summary: summary(dmi, info),
        diagnostics,
        details,
    }
}

/// The warnings of the readings, as the Overview gives them.
fn readings(alerts: &[String]) -> Section {
    section(
        gettext("Readings"),
        None,
        alerts
            .iter()
            .map(|alert| Line::item(alert.clone(), String::new()).status(Some(Status::Warning)))
            .collect(),
    )
}

/// The short list on top of the page: the computer in plain words, its
/// disks and network cards, and the system (D64).
fn summary(dmi: &Dmi, info: &SystemInfo) -> Vec<Section> {
    let mut computer = vec![
        Line::value(
            gettext("Mainboard"),
            dmi.board_name.clone().or_else(|| dmi.product_name.clone()),
        ),
        Line::value(gettext("BIOS"), bios_version(&info.bios)),
        Line::value(
            gettext("Processor"),
            info.cpu
                .as_deref()
                .map(|cpu| short_cpu_name(cpu).to_owned()),
        ),
        Line::value(
            pgettext("system memory", "Memory"),
            memory_total(info.memory.as_deref()),
        ),
    ];
    if info.graphics.is_empty() {
        computer.push(Line::value(gettext("Graphics card"), None));
    }
    computer.extend(info.graphics.iter().map(|card| {
        Line::value(
            gettext("Graphics card"),
            card.name
                .as_deref()
                .map(|name| marketing_name(name).to_owned()),
        )
    }));
    let disks = info
        .disks
        .iter()
        .map(|disk| {
            let mut size = glib::format_size(disk.size_bytes).to_string();
            if disk.removable {
                size = format!("{size} · {}", gettext("removable medium"));
            }
            Line::value(
                disk.model.clone().unwrap_or_else(|| disk.name.clone()),
                Some(size),
            )
        })
        .collect();
    let network = info
        .network
        .iter()
        .map(|card| {
            let name = card.name.as_deref().map_or_else(
                || gettext("Unknown network card"),
                |name| marketing_name(name).to_owned(),
            );
            Line::value(name, Some(String::new()))
        })
        .collect();
    vec![
        // Translators: the group of the board, processor, memory and
        // graphics on the System page.
        section(gettext("This computer"), None, computer),
        section(gettext("Disks"), None, disks),
        // Translators: the group of the network cards on the System page.
        section(gettext("Network"), None, network),
        section(
            String::new(),
            None,
            vec![Line::value(gettext("System"), info.os.clone())],
        ),
    ]
}

/// All memory in one line: the size, the type and the speed where all
/// modules share them, e.g. "32,0 GiB DDR5 · 6000 MT/s".
fn memory_total(slots: Option<&[MemorySlot]>) -> Option<String> {
    let modules: Vec<&MemoryModule> = slots?.iter().filter_map(|s| s.module.as_ref()).collect();
    let first = modules.first()?;
    let total = modules.iter().map(|m| m.size_bytes).sum();
    let size = glib::format_size_full(total, glib::FormatSizeFlags::IEC_UNITS).to_string();
    let shared = |value: fn(&MemoryModule) -> Option<String>| {
        value(first).filter(|v| modules.iter().all(|m| value(m).as_ref() == Some(v)))
    };
    let mut parts = vec![match shared(|m| m.kind.clone()) {
        Some(kind) => format!("{size} {kind}"),
        None => size,
    }];
    parts.extend(shared(|m| {
        m.configured_speed_mts
            .map(|speed| format!("{speed}\u{a0}MT/s"))
    }));
    Some(parts.join(" · "))
}

/// The state of the page in one row (D64): what needs attention, or that
/// all works; a system that only reads the fans, or a board without a
/// profile, is information, not a problem (D60).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Health {
    Fine,
    Limited(String),
    Attention(Vec<String>),
}

pub fn health(
    detection: &Detection,
    info: &SystemInfo,
    extras: Extras,
    alerts: &[String],
) -> Health {
    let mut problems = alerts.to_vec();
    let driver = info.fan_driver;
    if let Some(profile) = detection.profile() {
        if !driver.loaded {
            problems.push(gettext("The nct6683 driver is not loaded"));
        } else if !driver.device {
            problems.push(
                gettext("The nct6683 driver is loaded but found no {} chip")
                    .replace("{}", &profile.hwmon.name),
            );
        }
    }
    if let Detection::UnverifiedRevision { revision, .. } = detection {
        problems.push(unverified_revision(*revision));
    }
    if info.fan_programs.as_deref().is_some_and(|p| !p.is_empty()) {
        problems.push(gettext("Other fan control programs are running"));
    }
    if !extras.skipped.is_empty() {
        let count = u32::try_from(extras.skipped.len()).unwrap_or(u32::MAX);
        problems.push(
            ngettext(
                "{} file skipped because of errors",
                "{} files skipped because of errors",
                count,
            )
            .replace("{}", &count.to_string()),
        );
    }
    if !problems.is_empty() {
        return Health::Attention(problems);
    }
    match (detection.profile(), driver.level) {
        (None, _) => Health::Limited(no_profile()),
        (Some(_), Some(DriverLevel::ReadOnly)) => Health::Limited(gettext(
            // Translators: under "Sensors work" on the System page when the
            // kernel driver can only read the fans.
            "This system can only read the fans for now",
        )),
        _ => Health::Fine,
    }
}

/// The status row: icon, title and the line under it.
fn status_texts(health: &Health) -> (&'static str, &'static str, String, String) {
    match health {
        Health::Fine => (
            "object-select-symbolic",
            "success",
            gettext("Everything works"),
            gettext("Sensors, driver and configuration are in order"),
        ),
        Health::Limited(why) => (
            "help-about-symbolic",
            "dimmed",
            gettext("Sensors work"),
            why.clone(),
        ),
        Health::Attention(problems) => {
            let first = problems.first().cloned().unwrap_or_default();
            let more = u32::try_from(problems.len().saturating_sub(1)).unwrap_or(u32::MAX);
            let line = if more == 0 {
                first
            } else {
                ngettext(
                    // Translators: under "Needs attention" on the System page:
                    // the first thing, e.g. "CPU fan stopped", and how many
                    // more there are.
                    "{problem}, and {count} more",
                    "{problem}, and {count} more",
                    more,
                )
                .replace("{problem}", &first)
                .replace("{count}", &more.to_string())
            };
            (
                "dialog-warning-symbolic",
                "warning",
                gettext("Needs attention"),
                line,
            )
        }
    }
}

/// The row on top of the page, which opens "Diagnostics".
fn status_row(health: &Health) -> adw::ActionRow {
    let (icon, class, title, subtitle) = status_texts(health);
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(&title)
        .subtitle(&subtitle)
        .activatable(true)
        .action_name("win.show-system-view")
        .action_target(&"diagnostics".to_variant())
        .build();
    let image = gtk::Image::builder()
        .icon_name(icon)
        .css_classes([class])
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    row.add_prefix(&image);
    row.add_suffix(&chevron());
    row
}

/// The row that opens "Technical Details".
fn details_row() -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(gettext("Technical details"))
        .subtitle(gettext(
            "Codes, versions, drivers and identifiers for bug reports",
        ))
        .activatable(true)
        .action_name("win.show-system-view")
        .action_target(&"details".to_variant())
        .build();
    row.add_suffix(&chevron());
    row
}

fn chevron() -> gtk::Image {
    gtk::Image::builder()
        .icon_name("go-next-symbolic")
        .valign(gtk::Align::Center)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .css_classes(["dimmed"])
        .build()
}

/// The files left out at start, which the page names with the reason.
fn skipped_files(files: &[SkippedFile]) -> Section {
    let lines = files
        .iter()
        .map(|file| {
            let kind = match file.kind {
                FileKind::Profile => gettext("Board profile"),
                FileKind::Labels => gettext("Custom names"),
            };
            Line::item(
                crate::preferences::home_relative(&file.path),
                format!("{kind} · {}", file.reason),
            )
            .status(Some(Status::Warning))
        })
        .collect();
    section(
        gettext("Skipped files"),
        Some(gettext(
            "Baseboard left these files out because of errors and works without them.",
        )),
        lines,
    )
}

// The texts about the board's profile, here and on the Fans page.

pub fn unverified_revision(revision: Option<u8>) -> String {
    match revision {
        // Translators: {} is the PCB revision of the board, a digit.
        Some(revision) => gettext("PCB revision {} is not verified: read only")
            .replace("{}", &revision.to_string()),
        None => gettext("Unknown PCB revision: read only"),
    }
}

pub fn other_model(code: &str) -> String {
    // Translators: {} is the four-character board code, e.g. 7E56. Boards
    // with one code can differ in fan headers, and the profiles for this
    // code are for other models.
    gettext("This MS-{} model has a different fan header layout: generic mode").replace("{}", code)
}

pub fn no_profile() -> String {
    gettext("No profile for this board")
}

// Under "Fan control unavailable" in Diagnostics (§5.5.3). No kernel
// version before the changes are in a released kernel (D41).
pub fn no_control() -> String {
    gettext(
        // Translators: in Diagnostics when the kernel driver can only read
        // the fans.
        "The driver in this kernel only reads the fans. \
         Control is coming in a future Linux version.",
    )
}

fn board(dmi: &Dmi) -> Section {
    let code = dmi.board_code().map(|code| code_label(&code));
    let code = match (code, dmi.pcb_revision()) {
        (Some(code), Some(revision)) => Some(
            // Translators: the board code and the PCB revision, e.g.
            // "MS-7E61, revision 1".
            gettext("{code}, revision {revision}")
                .replace("{code}", &code)
                .replace("{revision}", &revision.to_string()),
        ),
        (code, _) => code,
    };
    section(
        gettext("Mainboard"),
        None,
        vec![
            Line::property(
                gettext("Model"),
                dmi.board_name.clone().or_else(|| dmi.product_name.clone()),
            ),
            Line::property(gettext("Manufacturer"), dmi.board_vendor.clone()),
            Line::property(gettext("Board code and PCB revision"), code),
        ],
    )
}

/// The board profile: what it allows as the title, its file or why there
/// is none below.
fn profile_line(detection: &Detection) -> Line {
    let file = |profile: &Profile| {
        // Translators: {} is the file name of the board profile, e.g.
        // ms-7e61.toml.
        gettext("Board profile: {}").replace("{}", &format!("{}.toml", profile.id))
    };
    match detection {
        Detection::Supported(profile) => {
            Line::item(gettext("Verified for this PCB revision"), file(profile))
                .status(Some(Status::Ok))
        }
        Detection::UnverifiedRevision { profile, revision } => {
            let title = match revision {
                // Translators: {} is the PCB revision of the board, a
                // digit. Baseboard only reads the fans then.
                Some(revision) => gettext("Not verified for PCB revision {}: read only")
                    .replace("{}", &revision.to_string()),
                // Translators: Baseboard only reads the fans then.
                None => gettext("PCB revision unknown: read only"),
            };
            Line::item(title, file(profile)).status(Some(Status::Warning))
        }
        Detection::OtherModel { code, .. } => Line::item(
            gettext("No profile for this model: generic mode"),
            // Translators: {} is the board code, e.g. MS-7E56. Boards with
            // one code can differ in fan headers.
            gettext("The profiles for {} are for models with other fan headers")
                .replace("{}", &code_label(code)),
        )
        .status(Some(Status::Info)),
        Detection::Generic => Line::item(
            gettext("No profile for this board: generic mode"),
            gettext("Readings without header names, no fan control"),
        )
        .status(Some(Status::Info)),
    }
}

/// The BIOS version with its date.
fn bios_version(bios: &Bios) -> Option<String> {
    bios.version.as_ref().map(|version| match bios_date(bios) {
        // Translators: the BIOS version and its date, e.g.
        // "1.A68 of 09/07/2026"; the date is written as the locale writes
        // dates.
        Some(date) => gettext("{version} of {date}")
            .replace("{version}", version)
            .replace("{date}", &date),
        None => version.clone(),
    })
}

fn bios(bios: &Bios) -> Section {
    section(
        gettext("BIOS"),
        None,
        vec![
            Line::property(gettext("Version"), bios_version(bios)),
            Line::property(gettext("Manufacturer"), bios.vendor.clone()),
        ],
    )
}

/// The BIOS date as `LC_TIME` writes dates (`%x`, e.g. "07.09.2026" in
/// Polish), like numbers follow `LC_NUMERIC`: a format with month names
/// from the translation would mix two languages when `LANGUAGE` and
/// `LC_TIME` differ.
fn bios_date(bios: &Bios) -> Option<String> {
    let (year, month, day) = bios.ymd()?;
    let date = glib::DateTime::new(
        &glib::TimeZone::utc(),
        year.into(),
        month.into(),
        day.into(),
        0,
        0,
        0.0,
    )
    .ok()?;
    date.format("%x").ok().map(String::from)
}

fn processor_and_system(info: &SystemInfo) -> Section {
    section(
        gettext("Processor and system"),
        None,
        vec![
            Line::property(
                gettext("Processor"),
                info.cpu
                    .as_deref()
                    .map(|cpu| short_cpu_name(cpu).to_owned()),
            ),
            Line::property(
                gettext("Processor frequency driver"),
                info.cpufreq_driver.clone(),
            ),
            Line::property(gettext("System"), info.os.clone()),
            Line::property(gettext("Kernel"), info.kernel.clone()),
        ],
    )
}

fn memory(slots: Option<&[MemorySlot]>) -> Section {
    let lines = slots
        .unwrap_or_default()
        .iter()
        .map(|slot| {
            let name = slot.locator.clone().unwrap_or_else(|| {
                // Translators: {} is the number of a memory slot in SMBIOS.
                gettext("Slot {}").replace("{}", &slot.index.to_string())
            });
            match &slot.module {
                Some(module) => Line::item(name, module_text(module)),
                None => Line::item(name, gettext("Empty")),
            }
        })
        .collect();
    section(pgettext("system memory", "Memory"), None, lines)
}

/// Size and type, speed, part number and maker of a memory module. A raw
/// JEDEC code is left out: it only goes into the copied text (D37).
fn module_text(module: &MemoryModule) -> String {
    let size =
        glib::format_size_full(module.size_bytes, glib::FormatSizeFlags::IEC_UNITS).to_string();
    let mut parts = vec![match &module.kind {
        Some(kind) => format!("{size} {kind}"),
        None => size,
    }];
    parts.extend(speed_text(module));
    parts.extend(module.part_number.clone());
    if let Some(Manufacturer::Name(name)) = &module.manufacturer {
        parts.push(name.clone());
    }
    parts.join(" · ")
}

/// The configured speed, and the highest one when it differs.
fn speed_text(module: &MemoryModule) -> Option<String> {
    let mts = |speed: u32| format!("{speed}\u{a0}MT/s");
    match (module.configured_speed_mts, module.max_speed_mts) {
        (Some(set), Some(max)) if set != max => Some(
            // Translators: the speed a memory module runs at and its highest
            // speed, e.g. "4800 MT/s (max. 6000 MT/s)".
            gettext("{configured} (max. {max})")
                .replace("{configured}", &mts(set))
                .replace("{max}", &mts(max)),
        ),
        (Some(set), _) => Some(mts(set)),
        // Translators: the highest speed of a memory module, e.g.
        // "max. 6000 MT/s".
        (None, Some(max)) => Some(gettext("max. {}").replace("{}", &mts(max))),
        (None, None) => None,
    }
}

/// One group per graphics card, named like on the Overview page.
fn graphics(cards: &[GraphicsCard]) -> Vec<Section> {
    if cards.is_empty() {
        return vec![section(gettext("Graphics card"), None, Vec::new())];
    }
    cards
        .iter()
        .map(|card| {
            let mut lines = Vec::new();
            if let Some(vbios) = &card.vbios {
                lines.push(Line::property(gettext("VBIOS"), Some(vbios.clone())));
            }
            lines.push(Line::property(gettext("Driver"), card.driver.clone()));
            lines.push(Line::property(
                gettext("PCI identifiers"),
                card.pci_id.map(pci_text),
            ));
            section(
                gettext("Graphics card"),
                card.name
                    .as_deref()
                    .map(|name| marketing_name(name).to_owned()),
                lines,
            )
        })
        .collect()
}

fn pci_text(id: PciId) -> String {
    let device = format!("{:04x}:{:04x}", id.vendor, id.device);
    match id.subsystem {
        // Translators: the PCI identifiers of a device, e.g.
        // "1002:7590, subsystem 148c:2437".
        Some((vendor, subsystem)) => gettext("{device}, subsystem {subsystem}")
            .replace("{device}", &device)
            .replace("{subsystem}", &format!("{vendor:04x}:{subsystem:04x}")),
        None => device,
    }
}

fn disks(disks: &[Disk]) -> Section {
    let lines = disks
        .iter()
        .map(|disk| {
            let mut parts = vec![glib::format_size(disk.size_bytes).to_string()];
            parts.extend(disk.firmware.as_ref().map(|firmware| {
                // Translators: the firmware version of a disk, e.g. "firmware P9CR40D".
                gettext("firmware {}").replace("{}", firmware)
            }));
            parts.push(disk.name.clone());
            if disk.removable {
                parts.push(gettext("removable medium"));
            }
            Line::item(
                disk.model.clone().unwrap_or_else(|| disk.name.clone()),
                parts.join(" · "),
            )
        })
        .collect();
    section(gettext("Disks"), None, lines)
}

fn network(cards: &[NetworkCard]) -> Section {
    let lines = cards
        .iter()
        .map(|card| {
            let name = card.name.as_deref().map_or_else(
                || gettext("Unknown network card"),
                |name| marketing_name(name).to_owned(),
            );
            let driver = card.driver.as_ref().map_or_else(
                || gettext("no driver"),
                // Translators: the driver of a device, e.g. "driver r8169".
                |driver| gettext("driver {}").replace("{}", driver),
            );
            Line::item(name, driver)
        })
        .collect();
    section(gettext("Network cards"), None, lines)
}

/// The EC driver and what it allows, how to check the driver when the
/// board's chip is missing, libsensors, the lm-sensors files for the
/// board's chip and other fan control programs.
fn sensors_and_fans(detection: &Detection, info: &SystemInfo, source: Option<&Choice>) -> Section {
    let chip = detection.profile().map(|p| p.hwmon.name.as_str());
    // Without a profile the driver is only information: the board may have
    // no such chip.
    let warning = chip.map(|_| Status::Warning);
    let driver = info.fan_driver;

    let monitor = gettext("Hardware monitor");
    // Whether the module comes with the kernel, after the driver's state;
    // said to be unknown rather than guessed (D40).
    let with_origin = |text: String| match driver.origin {
        Some(ModuleOrigin::InTree) => format!("{text} · {}", gettext("in-tree module")),
        Some(ModuleOrigin::OutOfTree) => format!("{text} · {}", gettext("out-of-tree module")),
        None => format!("{text} · {}", gettext("module origin unknown")),
    };
    let monitor = match (driver.loaded, driver.device, chip) {
        (false, _, _) => {
            Line::item(monitor, gettext("The nct6683 driver is not loaded")).status(warning)
        }
        (true, false, Some(chip)) => Line::item(
            monitor,
            with_origin(
                // Translators: {} is the name of the chip, e.g. nct6687.
                gettext("The nct6683 driver is loaded but found no {} chip").replace("{}", chip),
            ),
        )
        .status(warning),
        (true, _, _) => Line::item(
            monitor,
            with_origin(gettext("The nct6683 driver is loaded")),
        )
        .status(Some(Status::Ok)),
    };
    // With a profile and no chip of the board: commands that show whether
    // the driver is loaded and what it said (read only).
    let check = (chip.is_some() && !(driver.loaded && driver.device)).then(|| {
        Line::item(
            gettext("Checking the driver"),
            "lsmod | grep nct6683\njournalctl -k -g nct6683".to_owned(),
        )
        .monospace()
    });

    let library = gettext("lm-sensors library");
    let library = match source {
        // In a Flatpak sandbox without the host's /etc there is no
        // configuration to apply (§10.2).
        Some(Choice::Libsensors { version }) if info.sensors_config.is_none() => Line::item(
            library,
            // Translators: {} is the version of libsensors, e.g. 3.6.2.
            gettext("Version {}: without the system's configuration, which Flatpak does not share")
                .replace("{}", version),
        ),
        Some(Choice::Libsensors { version }) => Line::item(
            library,
            // Translators: {} is the version of libsensors, e.g. 3.6.0.
            gettext("Version {}: readings with the lm-sensors configuration")
                .replace("{}", version),
        )
        .status(Some(Status::Ok)),
        // Not an error: the readings come straight from sysfs.
        Some(Choice::Sysfs { reason }) => Line::item(
            library,
            format!(
                "{}\n{reason}",
                gettext(
                    "Not used: readings straight from sysfs, without the lm-sensors configuration"
                )
            ),
        ),
        None => Line::property(library, None),
    };

    let config = match (chip, info.sensors_config.as_deref()) {
        // A Flatpak sandbox that cannot see the host's /etc: say so, with
        // the command that shares it, read only (§10.1).
        (_, None) => Line::item(
            gettext("lm-sensors configuration not available in Flatpak"),
            format!(
                "flatpak override --user --filesystem=host-etc:ro {}",
                config::APP_ID
            ),
        )
        .monospace(),
        (None, Some(_)) => Line::property(gettext("lm-sensors configuration"), None),
        (Some(chip), Some([])) => Line::item(
            gettext("lm-sensors configuration"),
            // Translators: {} is the name of the board's chip, e.g. nct6687;
            // "chip" is a statement of the lm-sensors configuration.
            gettext("No file has a chip statement for {}").replace("{}", chip),
        ),
        (Some(_), Some(files)) => Line::item(
            gettext("lm-sensors configuration"),
            files
                .iter()
                .map(|path| host_path(path))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .status(Some(Status::Ok))
        .monospace(),
    };

    let programs = match info.fan_programs.as_deref() {
        // A sandbox sees only its own processes: never a false all-clear.
        None => Line::item(
            gettext("Other fan control programs"),
            gettext("Cannot be checked in Flatpak"),
        ),
        Some([]) => Line::item(
            gettext("No other fan control programs"),
            // Translators: {} lists program names, e.g. "fancontrol, coolercontrold".
            gettext("Checked: {}").replace("{}", &system::FAN_PROGRAMS.join(", ")),
        )
        .status(Some(Status::Ok)),
        Some(running) => Line::item(
            gettext("Other fan control programs are running"),
            // Translators: {} lists program names, e.g. "fancontrol".
            gettext("{}: they may change the fan settings at the same time as Baseboard")
                .replace("{}", &running.join(", ")),
        )
        .status(Some(Status::Warning)),
    };

    let mut lines = vec![monitor];
    lines.extend(check);
    lines.extend([
        fan_control(detection, driver.level, chip),
        library,
        config,
        programs,
    ]);
    // A check that is neither fine nor a problem is information, with its
    // icon, so that every text of the group starts at one place.
    for line in &mut lines {
        line.status.get_or_insert(Status::Info);
    }
    section(gettext("Sensors and fans"), None, lines)
}

/// A file of the host as the user knows it: in a Flatpak sandbox the host's
/// `/etc` is under `/run/host` (§10.1).
fn host_path(path: &std::path::Path) -> String {
    let shown = path.strip_prefix("/run/host").map_or_else(
        |_| path.to_owned(),
        |rest| std::path::Path::new("/").join(rest),
    );
    shown.display().to_string()
}

/// What the driver allows on the board's fans, in words (§5.5.3), without
/// a kernel version.
fn fan_control(detection: &Detection, level: Option<DriverLevel>, chip: Option<&str>) -> Line {
    let warning = chip.map(|_| Status::Warning);
    // Translators: Baseboard cannot control the board's fans here.
    let unavailable = gettext("Fan control unavailable");
    match level {
        None => Line::item(
            unavailable,
            match chip {
                None => no_profile(),
                // Translators: {} is the name of the board's chip, e.g. nct6687.
                Some(chip) => gettext("The board's {} chip was not found").replace("{}", chip),
            },
        )
        .status(warning),
        Some(level) => {
            // The usual state of released kernels, not a problem: the icon
            // of information (D64).
            let Some(words) = level_text(level) else {
                return Line::item(unavailable, no_control());
            };
            if detection.allows_control() {
                Line::item(gettext("Fan control"), words).status(Some(Status::Ok))
            } else {
                Line::item(
                    gettext("Fan control"),
                    // Translators: {} is what the driver allows, e.g. "Manual mode".
                    gettext("{}; read only, because the PCB revision is not verified")
                        .replace("{}", &words),
                )
                .status(Some(Status::Warning))
            }
        }
    }
}

/// What the driver can control, in words (§5.5.3); `None` when it only
/// reads.
fn level_text(level: DriverLevel) -> Option<String> {
    match level {
        DriverLevel::ReadOnly => None,
        DriverLevel::Manual => Some(gettext("Manual mode")),
        DriverLevel::ManualCurveView => Some(gettext("Manual mode and curve preview")),
        DriverLevel::Curves => Some(gettext("Curves in the EC")),
        DriverLevel::CurvesSource => Some(gettext("Curves in the EC with a choice of temperature")),
    }
}

/// The widgets of one group.
fn group_widget(section: &Section) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder()
        .title(glib::markup_escape_text(&section.title))
        .build();
    if let Some(description) = &section.description {
        group.set_description(Some(&glib::markup_escape_text(description)));
    }
    for line in &section.lines {
        group.add(&row(line));
    }
    group
}

/// A row of the page; a line without a value, e.g. a network card in the
/// short list, has its name alone.
fn row(line: &Line) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .use_markup(false)
        .title(&line.title)
        .build();
    match line.style {
        Style::Value => {
            // The value next to the name; it wraps in a narrow window and
            // can be selected and copied.
            if !line.value.is_empty() {
                // In one line while there is room: it only wraps when the
                // row is narrower than the name and the value.
                let value = gtk::Label::builder()
                    .label(&line.value)
                    .wrap(true)
                    .natural_wrap_mode(gtk::NaturalWrapMode::None)
                    .xalign(1.0)
                    .justify(gtk::Justification::Right)
                    .selectable(true)
                    .css_classes(["dimmed"])
                    .build();
                row.add_suffix(&value);
            }
        }
        Style::Property => row.add_css_class("property"),
        Style::Item => {}
        Style::NoData => row.add_css_class("dimmed"),
    }
    if !line.value.is_empty() && line.style != Style::Value {
        row.set_subtitle(&line.value);
    }
    if line.monospace {
        row.add_css_class("monospace-value");
        // Paths and commands can be selected and copied.
        row.set_subtitle_selectable(true);
    }
    if let Some(status) = line.status {
        let (icon, class) = match status {
            Status::Ok => ("object-select-symbolic", "success"),
            Status::Warning => ("dialog-warning-symbolic", "warning"),
            Status::Info => ("help-about-symbolic", "dimmed"),
        };
        let image = gtk::Image::builder()
            .icon_name(icon)
            .css_classes([class])
            .valign(gtk::Align::Center)
            .build();
        // The text says the same.
        image.update_state(&[gtk::accessible::State::Hidden(true)]);
        row.add_prefix(&image);
    }
    // The title is the row's name; its value is read after it.
    row.upcast_ref::<gtk::Widget>()
        .update_property(&[gtk::accessible::Property::Description(&line.value)]);
    row
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/me/bojanowski/Baseboard/system.ui")]
    pub struct SystemPage {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub page: TemplateChild<adw::PreferencesPage>,
        #[template_child]
        pub diagnostics_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub diagnostics: TemplateChild<adw::PreferencesPage>,
        #[template_child]
        pub details_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub details: TemplateChild<adw::PreferencesPage>,
        /// The groups on the page and on its two views, to take them off
        /// again.
        pub(super) groups: RefCell<Vec<(adw::PreferencesPage, adw::PreferencesGroup)>>,
        /// Warnings of the readings from the Overview, in words.
        pub(super) alerts: RefCell<Vec<String>>,
        pub(super) board: OnceCell<(Dmi, Detection)>,
        pub(super) info: RefCell<Option<SystemInfo>>,
        pub(super) worker: RefCell<Option<Worker>>,
        pub(super) copy: OnceCell<gio::SimpleAction>,
        pub(super) source: RefCell<Option<Choice>>,
        pub(super) skipped: OnceCell<Vec<SkippedFile>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SystemPage {
        const NAME: &'static str = "BaseboardSystemPage";
        type Type = super::SystemPage;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SystemPage {}
    impl WidgetImpl for SystemPage {}
    impl BinImpl for SystemPage {}
}

glib::wrapper! {
    pub struct SystemPage(ObjectSubclass<imp::SystemPage>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SystemPage {
    /// Starts the worker thread, idle until the page is shown. `copy` is
    /// the action of "Copy system information", enabled once there is data.
    pub fn start(&self, setup: Setup, copy: &gio::SimpleAction) {
        let imp = self.imp();
        let page = glib::SendWeakRef::from(self.downgrade());
        let worker = Worker::start(
            setup.root,
            setup.detection.profile().cloned(),
            move |info| {
                let page = page.clone();
                glib::idle_add_once(move || {
                    if let Some(page) = page.upgrade() {
                        page.show(info);
                    }
                });
            },
        );
        let _ = imp.board.set((setup.dmi, setup.detection));
        let _ = imp.skipped.set(setup.skipped);
        let _ = imp.copy.set(copy.clone());
        imp.worker.replace(Some(worker));
    }

    /// Reads while `active`: the page is shown and the window visible.
    /// `reread`: the page was entered, so read again at once.
    pub fn set_active(&self, active: bool, reread: bool) {
        if let Some(worker) = self.imp().worker.borrow().as_ref() {
            worker.set_active(active, reread);
        }
    }

    /// The text of "Copy system information", once there is data.
    pub fn report(&self) -> Option<String> {
        let imp = self.imp();
        let (dmi, detection) = imp.board.get()?;
        let info = imp.info.borrow();
        Some(
            Report {
                version: config::VERSION,
                installation: &config::installation(),
                dmi,
                detection,
                info: info.as_ref()?,
            }
            .text(),
        )
    }

    /// Debug builds: scrolls to the end, for a screenshot of the lower
    /// groups.
    #[cfg(debug_assertions)]
    pub fn scroll_to_end(&self) {
        let mut widget = Some(self.imp().page.clone().upcast::<gtk::Widget>());
        while let Some(current) = widget {
            if let Some(scrolled) = current.downcast_ref::<gtk::ScrolledWindow>() {
                let adjustment = scrolled.vadjustment();
                adjustment.set_value(adjustment.upper() - adjustment.page_size());
                return;
            }
            widget = current.first_child();
        }
    }

    /// How the Overview reads the sensors (libsensors or raw sysfs), for
    /// the line about libsensors.
    pub fn set_source(&self, source: Choice) {
        let imp = self.imp();
        if imp.source.borrow().as_ref() == Some(&source) {
            return;
        }
        imp.source.replace(Some(source));
        self.render();
    }

    /// Warnings of the readings from the Overview, in words (D64, D65); the
    /// status row and "Diagnostics" change only when they do.
    pub fn set_alerts(&self, alerts: Vec<String>) {
        let imp = self.imp();
        if *imp.alerts.borrow() == alerts {
            return;
        }
        imp.alerts.replace(alerts);
        self.render();
    }

    /// The view "Diagnostics" for the window to push.
    pub fn diagnostics_page(&self) -> adw::NavigationPage {
        self.imp().diagnostics_page.clone()
    }

    /// The view "Technical Details" for the window to push.
    pub fn details_page(&self) -> adw::NavigationPage {
        self.imp().details_page.clone()
    }

    fn show(&self, info: SystemInfo) {
        let imp = self.imp();
        // The same data: the widgets, and where the page is scrolled, stay.
        if imp.info.borrow().as_ref() == Some(&info) {
            return;
        }
        imp.info.replace(Some(info));
        self.render();
    }

    /// Makes the groups of the page and of its two views again from the
    /// data and the extras, once there is data.
    fn render(&self) {
        let imp = self.imp();
        let Some((dmi, detection)) = imp.board.get() else {
            return;
        };
        let info = imp.info.borrow();
        let Some(info) = info.as_ref() else {
            return;
        };
        let source = imp.source.borrow();
        let extras = Extras {
            source: source.as_ref(),
            skipped: imp.skipped.get().map_or(&[], Vec::as_slice),
        };
        let alerts = imp.alerts.borrow();
        let views = views(dmi, detection, info, extras, &alerts);
        let mut groups = imp.groups.borrow_mut();
        for (page, group) in groups.drain(..) {
            page.remove(&group);
        }
        let mut add = |page: &adw::PreferencesPage, group: adw::PreferencesGroup| {
            page.add(&group);
            groups.push((page.clone(), group));
        };
        let status = adw::PreferencesGroup::new();
        status.add(&status_row(&health(detection, info, extras, &alerts)));
        add(&imp.page, status);
        for section in &views.summary {
            add(&imp.page, group_widget(section));
        }
        let details = adw::PreferencesGroup::new();
        details.add(&details_row());
        add(&imp.page, details);
        for section in &views.diagnostics {
            add(&imp.diagnostics, group_widget(section));
        }
        for section in &views.details {
            add(&imp.details, group_widget(section));
        }
        imp.stack.set_visible_child_name("info");
        if let Some(copy) = imp.copy.get() {
            copy.set_enabled(true);
        }
    }
}

#[cfg(test)]
mod tests {
    //! gettext without a translation gives the English text.

    use baseboard_core::system::FanDriver;

    use super::*;

    /// The groups of both views, "Technical Details" then "Diagnostics".
    fn sections(
        dmi: &Dmi,
        detection: &Detection,
        info: &SystemInfo,
        extras: Extras,
    ) -> Vec<Section> {
        let views = views(dmi, detection, info, extras, &[]);
        views.details.into_iter().chain(views.diagnostics).collect()
    }

    fn titles(sections: &[Section]) -> Vec<&str> {
        sections.iter().map(|s| s.title.as_str()).collect()
    }

    fn section<'a>(sections: &'a [Section], title: &str) -> &'a Section {
        sections.iter().find(|s| s.title == title).unwrap()
    }

    /// Sections without data have a "No data" line, fields without data
    /// say so, and nothing is left out.
    #[test]
    fn sections_without_data() {
        let sections = sections(
            &Dmi::default(),
            &Detection::Generic,
            &SystemInfo::default(),
            Extras::default(),
        );
        assert_eq!(
            titles(&sections),
            [
                "Mainboard",
                "BIOS",
                "Processor and system",
                "Memory",
                "Graphics card",
                "Disks",
                "Network cards",
                "Sensors and fans",
                "Board profile",
            ]
        );
        for title in ["Memory", "Graphics card", "Disks", "Network cards"] {
            assert_eq!(
                section(&sections, title).lines,
                [Line::no_data()],
                "{title}"
            );
        }
        for title in ["Mainboard", "BIOS", "Processor and system"] {
            for line in &section(&sections, title).lines {
                assert!(!line.value.is_empty(), "{title}: {}", line.title);
            }
        }
        let board = section(&sections, "Mainboard");
        assert_eq!(board.lines[0].value, "No data");
        assert_eq!(
            section(&sections, "Board profile").lines[0].title,
            "No profile for this board: generic mode"
        );
        // Without a profile the driver is information, not a warning.
        let drivers = section(&sections, "Sensors and fans");
        assert_eq!(drivers.lines[0].status, Some(Status::Info));
        assert_eq!(titled(drivers, "lm-sensors configuration").value, "No data");
        // Before the Overview's first sample nothing is known of libsensors.
        assert_eq!(titled(drivers, "lm-sensors library").value, "No data");
        // No profile: no advice on a driver the board may not need.
        assert!(
            drivers
                .lines
                .iter()
                .all(|l| l.title != "Checking the driver")
        );
    }

    fn titled<'a>(section: &'a Section, title: &str) -> &'a Line {
        section.lines.iter().find(|l| l.title == title).unwrap()
    }

    fn ms7e61() -> baseboard_core::profile::Profile {
        baseboard_core::profile::load(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/boards/ms-7e61.toml"
        )))
        .unwrap()
    }

    /// The board's chip is missing (driver not loaded, or loaded without
    /// the chip): a warning and the commands to check the driver. With the
    /// chip there, no advice.
    #[test]
    fn missing_board_chip() {
        let supported = Detection::Supported(ms7e61());
        let with = |loaded: bool, device: bool| {
            let info = SystemInfo {
                fan_driver: FanDriver {
                    loaded,
                    origin: loaded.then_some(ModuleOrigin::InTree),
                    device,
                    level: device.then_some(DriverLevel::ReadOnly),
                },
                ..SystemInfo::default()
            };
            sections(&Dmi::default(), &supported, &info, Extras::default())
                .into_iter()
                .find(|s| s.title == "Sensors and fans")
                .unwrap()
        };
        let unloaded = with(false, false);
        assert_eq!(unloaded.lines[0].value, "The nct6683 driver is not loaded");
        assert_eq!(unloaded.lines[0].status, Some(Status::Warning));
        let check = titled(&unloaded, "Checking the driver");
        assert_eq!(
            check.value,
            "lsmod | grep nct6683\njournalctl -k -g nct6683"
        );
        assert!(check.monospace);
        assert_eq!(
            titled(&unloaded, "Fan control unavailable").value,
            "The board's nct6687 chip was not found"
        );
        let no_chip = with(true, false);
        assert!(no_chip.lines[0].value.contains("found no nct6687 chip"));
        assert!(
            no_chip
                .lines
                .iter()
                .any(|l| l.title == "Checking the driver")
        );
        let fine = with(true, true);
        assert!(fine.lines.iter().all(|l| l.title != "Checking the driver"));
    }

    /// The board profile line: the state as the title, the profile's file
    /// or why there is none below it. Verified, a revision not verified or
    /// unknown (read only, warning), another model with the same code and
    /// no profile (generic mode).
    #[test]
    fn profile_states() {
        let profile_line = |detection: Detection| {
            let sections = sections(
                &Dmi::default(),
                &detection,
                &SystemInfo::default(),
                Extras::default(),
            );
            let line = section(&sections, "Board profile").lines[0].clone();
            assert_eq!(line.style, Style::Item);
            (line.title, line.value, line.status)
        };
        let file = "Board profile: ms-7e61.toml".to_owned();
        assert_eq!(
            profile_line(Detection::Supported(ms7e61())),
            (
                "Verified for this PCB revision".into(),
                file.clone(),
                Some(Status::Ok)
            )
        );
        assert_eq!(
            profile_line(Detection::UnverifiedRevision {
                profile: ms7e61(),
                revision: Some(2),
            }),
            (
                "Not verified for PCB revision 2: read only".into(),
                file.clone(),
                Some(Status::Warning)
            )
        );
        assert_eq!(
            profile_line(Detection::UnverifiedRevision {
                profile: ms7e61(),
                revision: None,
            }),
            (
                "PCB revision unknown: read only".into(),
                file,
                Some(Status::Warning)
            )
        );
        assert_eq!(
            profile_line(Detection::OtherModel {
                code: "7E56".into(),
                model: Some("PRO B850-P WIFI".into()),
            }),
            (
                "No profile for this model: generic mode".into(),
                "The profiles for MS-7E56 are for models with other fan headers".into(),
                Some(Status::Info)
            )
        );
        assert_eq!(
            profile_line(Detection::Generic),
            (
                "No profile for this board: generic mode".into(),
                "Readings without header names, no fan control".into(),
                Some(Status::Info)
            )
        );
    }

    /// The board code as the manual writes it, the short processor name as
    /// on the Overview page, and a monospace font only for paths and
    /// commands: not for the kernel or a VBIOS.
    #[test]
    fn board_processor_and_versions() {
        let dmi = Dmi {
            board_name: Some("MAG B850M MORTAR WIFI (MS-7E61)".into()),
            board_version: Some("1.0".into()),
            ..Dmi::default()
        };
        let info = SystemInfo {
            cpu: Some("AMD Ryzen 7 9700X 8-Core Processor".into()),
            cpufreq_driver: Some("amd-pstate-epp".into()),
            kernel: Some("7.3.3-300.fc45.x86_64".into()),
            graphics: vec![GraphicsCard {
                device: "devices/a".into(),
                name: None,
                pci_id: None,
                driver: None,
                vbios: Some("113-APM6744-D01-100".into()),
            }],
            ..SystemInfo::default()
        };
        let sections = sections(&dmi, &Detection::Generic, &info, Extras::default());
        assert_eq!(
            titled(
                section(&sections, "Mainboard"),
                "Board code and PCB revision"
            )
            .value,
            "MS-7E61, revision 1"
        );
        let processor = section(&sections, "Processor and system");
        assert_eq!(titled(processor, "Processor").value, "AMD Ryzen 7 9700X");
        assert_eq!(
            titled(processor, "Processor frequency driver").value,
            "amd-pstate-epp"
        );
        assert!(!titled(processor, "Kernel").monospace);
        assert!(!titled(section(&sections, "Graphics card"), "VBIOS").monospace);
        // Without the revision the code alone.
        let no_revision = Dmi {
            board_version: None,
            ..dmi
        };
        let sections = self::sections(&no_revision, &Detection::Generic, &info, Extras::default());
        assert_eq!(
            titled(
                section(&sections, "Mainboard"),
                "Board code and PCB revision"
            )
            .value,
            "MS-7E61"
        );
    }

    /// libsensors: its version when loaded; when not, the readings come
    /// from sysfs, said without a warning, with the reason.
    #[test]
    fn libsensors_line() {
        let library = |source: &Choice| {
            let sections = sections(
                &Dmi::default(),
                &Detection::Generic,
                &SystemInfo::default(),
                Extras {
                    source: Some(source),
                    skipped: &[],
                },
            );
            titled(section(&sections, "Sensors and fans"), "lm-sensors library").clone()
        };
        let loaded = library(&Choice::Libsensors {
            version: "3.6.0".into(),
        });
        assert_eq!(
            loaded.value,
            "Version 3.6.0: readings with the lm-sensors configuration"
        );
        assert_eq!(loaded.status, Some(Status::Ok));
        let missing = library(&Choice::Sysfs {
            reason: "libsensors.so.5: cannot open shared object file".into(),
        });
        assert!(
            missing
                .value
                .starts_with("Not used: readings straight from sysfs")
        );
        assert!(
            missing
                .value
                .ends_with("libsensors.so.5: cannot open shared object file")
        );
        // Information, with its icon in line with the others (D64).
        assert_eq!(missing.status, Some(Status::Info));
    }

    /// In a Flatpak sandbox (§10.3): the lm-sensors configuration row says
    /// it is not available and gives the command that shares it, the fan
    /// programs cannot be checked (never a green tick), libsensors works
    /// without the system's configuration, and an unknown module origin is
    /// said, not guessed (D40).
    #[test]
    fn rows_in_a_sandbox() {
        let info = SystemInfo {
            sensors_config: None,
            fan_programs: None,
            fan_driver: FanDriver {
                loaded: true,
                origin: None,
                device: true,
                level: Some(DriverLevel::Curves),
            },
            ..SystemInfo::default()
        };
        let source = Choice::Libsensors {
            version: "3.6.2".into(),
        };
        let sections = sections(
            &Dmi::default(),
            &Detection::Supported(ms7e61()),
            &info,
            Extras {
                source: Some(&source),
                skipped: &[],
            },
        );
        let rows = section(&sections, "Sensors and fans");
        let config = titled(rows, "lm-sensors configuration not available in Flatpak");
        assert_eq!(
            config.value,
            "flatpak override --user --filesystem=host-etc:ro me.bojanowski.Baseboard"
        );
        assert!(config.monospace);
        let programs = titled(rows, "Other fan control programs");
        assert_eq!(programs.value, "Cannot be checked in Flatpak");
        assert_eq!(programs.status, Some(Status::Info));
        let library = titled(rows, "lm-sensors library");
        assert!(library.value.contains("Flatpak does not share"));
        assert_eq!(library.status, Some(Status::Info));
        assert_eq!(
            rows.lines[0].value,
            "The nct6683 driver is loaded · module origin unknown"
        );
    }

    /// A file of the host's /etc seen from the sandbox shows as on the host.
    #[test]
    fn host_paths() {
        assert_eq!(
            host_path(std::path::Path::new("/run/host/etc/sensors.d/a.conf")),
            "/etc/sensors.d/a.conf"
        );
        assert_eq!(
            host_path(std::path::Path::new("/etc/sensors3.conf")),
            "/etc/sensors3.conf"
        );
    }

    /// Wrong files are named with the reason, in a group of their own; with
    /// none, there is no such group.
    #[test]
    fn skipped_files() {
        let skipped = [
            SkippedFile {
                path: "/etc/baseboard/boards/broken.toml".into(),
                kind: FileKind::Profile,
                reason: "invalid profile: missing field `id`".into(),
            },
            SkippedFile {
                path: "/nonexistent/ms-7e61.toml".into(),
                kind: FileKind::Labels,
                reason: "invalid labels: [fan] \"sys9\" is not a key of the profile".into(),
            },
        ];
        let with = |skipped: &[SkippedFile]| {
            sections(
                &Dmi::default(),
                &Detection::Generic,
                &SystemInfo::default(),
                Extras {
                    source: None,
                    skipped,
                },
            )
        };
        let sections = with(&skipped);
        let files = section(&sections, "Skipped files");
        assert_eq!(files.lines[0].title, "/etc/baseboard/boards/broken.toml");
        assert_eq!(
            files.lines[0].value,
            "Board profile · invalid profile: missing field `id`"
        );
        assert!(files.lines[1].value.starts_with("Custom names · "));
        assert!(
            files
                .lines
                .iter()
                .all(|l| l.status == Some(Status::Warning))
        );
        assert!(with(&[]).iter().all(|s| s.title != "Skipped files"));
    }

    /// The fan control line in words for every driver level, and when the
    /// board's chip is missing; never a kernel version.
    #[test]
    fn fan_control_in_words() {
        let profile = baseboard_core::profile::load(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/boards/ms-7e61.toml"
        )))
        .unwrap();
        let supported = Detection::Supported(profile.clone());
        let line = |level| fan_control(&supported, level, Some("nct6687"));
        assert_eq!(line(Some(DriverLevel::Curves)).value, "Curves in the EC");
        assert_eq!(line(Some(DriverLevel::Curves)).status, Some(Status::Ok));
        // The usual state of released kernels: no warning.
        let read_only = line(Some(DriverLevel::ReadOnly));
        assert_eq!(read_only.title, "Fan control unavailable");
        assert_eq!(read_only.status, None);
        assert_eq!(read_only.value, no_control());
        // No version number.
        assert!(!read_only.value.chars().any(|c| c.is_ascii_digit()));
        // A missing chip is a problem.
        assert_eq!(line(None).value, "The board's nct6687 chip was not found");
        assert_eq!(line(None).status, Some(Status::Warning));
        let unverified = Detection::UnverifiedRevision {
            profile,
            revision: Some(2),
        };
        let line = fan_control(&unverified, Some(DriverLevel::Manual), Some("nct6687"));
        assert_eq!(line.status, Some(Status::Warning));
        assert!(line.value.starts_with("Manual mode;"));

        let info = SystemInfo {
            fan_driver: FanDriver {
                loaded: true,
                origin: Some(ModuleOrigin::OutOfTree),
                device: true,
                level: Some(DriverLevel::Curves),
            },
            ..SystemInfo::default()
        };
        let sections = sections(&Dmi::default(), &supported, &info, Extras::default());
        let drivers = section(&sections, "Sensors and fans");
        assert_eq!(
            drivers.lines[0].value,
            "The nct6683 driver is loaded · out-of-tree module"
        );
        let in_tree = SystemInfo {
            fan_driver: FanDriver {
                origin: Some(ModuleOrigin::InTree),
                ..info.fan_driver
            },
            ..SystemInfo::default()
        };
        let sections = self::sections(&Dmi::default(), &supported, &in_tree, Extras::default());
        assert_eq!(
            section(&sections, "Sensors and fans").lines[0].value,
            "The nct6683 driver is loaded · in-tree module"
        );
        assert_eq!(
            titled(drivers, "lm-sensors configuration").value,
            "No file has a chip statement for nct6687"
        );
    }

    /// Memory slots, empty ones marked, a removable disk marked, and one
    /// group per graphics card named like on the Overview page.
    #[test]
    fn devices() {
        let module = MemoryModule {
            size_bytes: 16 << 30,
            kind: Some("DDR5".into()),
            max_speed_mts: Some(6000),
            configured_speed_mts: Some(4800),
            part_number: Some("PART".into()),
            manufacturer: Some(Manufacturer::JedecId("Bank 8, Hex 0x5D".into())),
        };
        let info = SystemInfo {
            memory: Some(vec![
                MemorySlot {
                    index: 0,
                    locator: Some("DIMMA1".into()),
                    module: None,
                },
                MemorySlot {
                    index: 1,
                    locator: None,
                    module: Some(module.clone()),
                },
            ]),
            graphics: vec![GraphicsCard {
                device: "devices/a".into(),
                name: Some("Navi 44 [Radeon RX 9050 / 9060 XT]".into()),
                pci_id: None,
                driver: None,
                vbios: None,
            }],
            disks: vec![Disk {
                name: "sda".into(),
                model: Some("Stick".into()),
                firmware: None,
                size_bytes: 1,
                removable: true,
            }],
            ..SystemInfo::default()
        };
        let sections = sections(
            &Dmi::default(),
            &Detection::Generic,
            &info,
            Extras::default(),
        );
        let memory = &section(&sections, "Memory").lines;
        assert_eq!(
            (memory[0].title.as_str(), memory[0].value.as_str()),
            ("DIMMA1", "Empty")
        );
        assert_eq!(memory[1].title, "Slot 1");
        // A raw JEDEC code only goes into the copied text (D37).
        assert!(
            memory[1]
                .value
                .ends_with("DDR5 · 4800\u{a0}MT/s (max. 6000\u{a0}MT/s) · PART")
        );
        let named = MemoryModule {
            manufacturer: Some(Manufacturer::Name("Kingston".into())),
            ..module
        };
        assert!(module_text(&named).ends_with(" · PART · Kingston"));
        let gpu = section(&sections, "Graphics card");
        assert_eq!(gpu.description.as_deref(), Some("Radeon RX 9050 / 9060 XT"));
        assert_eq!(
            gpu.lines
                .iter()
                .map(|l| l.title.as_str())
                .collect::<Vec<_>>(),
            ["Driver", "PCI identifiers"],
            "no VBIOS line without a VBIOS"
        );
        assert!(
            section(&sections, "Disks").lines[0]
                .value
                .ends_with("sda · removable medium")
        );
    }

    /// The short list (D64): the computer in plain words, names and values
    /// side by side; all memory in one line; a disk with its size.
    #[test]
    fn short_list_of_the_computer() {
        let module = |kind: &str, speed| MemoryModule {
            size_bytes: 16 << 30,
            kind: Some(kind.into()),
            max_speed_mts: Some(6000),
            configured_speed_mts: Some(speed),
            part_number: None,
            manufacturer: None,
        };
        let slots = |modules: Vec<MemoryModule>| {
            modules
                .into_iter()
                .enumerate()
                .map(|(index, module)| MemorySlot {
                    index: u8::try_from(index).unwrap(),
                    locator: None,
                    module: Some(module),
                })
                .collect::<Vec<_>>()
        };
        let same = slots(vec![module("DDR5", 6000), module("DDR5", 6000)]);
        assert_eq!(
            memory_total(Some(&same)).as_deref(),
            Some("32.0\u{a0}GiB DDR5 · 6000\u{a0}MT/s")
        );
        let mixed = slots(vec![module("DDR5", 6000), module("DDR4", 3200)]);
        assert_eq!(memory_total(Some(&mixed)).as_deref(), Some("32.0\u{a0}GiB"));
        assert_eq!(memory_total(Some(&[])), None);

        let info = SystemInfo {
            os: Some("Fedora Linux 45".into()),
            cpu: Some("AMD Ryzen 7 9700X 8-Core Processor".into()),
            memory: Some(same),
            disks: vec![Disk {
                name: "nvme0n1".into(),
                model: Some("WD_BLACK SN7100 2TB".into()),
                firmware: Some("7615M0WD".into()),
                size_bytes: 2_000_398_934_016,
                removable: false,
            }],
            ..SystemInfo::default()
        };
        let dmi = Dmi {
            board_name: Some("MAG B850M MORTAR WIFI (MS-7E61)".into()),
            ..Dmi::default()
        };
        let views = views(&dmi, &Detection::Generic, &info, Extras::default(), &[]);
        assert_eq!(
            titles(&views.summary),
            ["This computer", "Disks", "Network", ""]
        );
        let computer = &views.summary[0].lines;
        assert!(computer.iter().all(|l| l.style == Style::Value));
        assert_eq!(computer[0].value, "MAG B850M MORTAR WIFI (MS-7E61)");
        assert_eq!(computer[2].value, "AMD Ryzen 7 9700X");
        assert_eq!(computer[4].title, "Graphics card");
        assert_eq!(computer[4].value, "No data");
        let disk = &views.summary[1].lines[0];
        assert_eq!(
            (disk.title.as_str(), disk.value.as_str()),
            ("WD_BLACK SN7100 2TB", "2.0\u{a0}TB")
        );
        assert_eq!(views.summary[3].lines[0].value, "Fedora Linux 45");
        // The firmware stays in "Technical Details".
        assert!(
            section(&views.details, "Disks").lines[0]
                .value
                .contains("firmware 7615M0WD")
        );
    }

    /// The status row (D64): what needs attention first, with how many more;
    /// a system that only reads the fans and a board without a profile are
    /// information; warnings of the readings go to "Diagnostics" too.
    #[test]
    fn status_of_the_page() {
        let fine = SystemInfo {
            fan_driver: FanDriver {
                loaded: true,
                origin: Some(ModuleOrigin::OutOfTree),
                device: true,
                level: Some(DriverLevel::Curves),
            },
            fan_programs: Some(Vec::new()),
            ..SystemInfo::default()
        };
        let supported = Detection::Supported(ms7e61());
        let health_of = |info: &SystemInfo, alerts: &[String]| {
            health(&supported, info, Extras::default(), alerts)
        };
        assert_eq!(health_of(&fine, &[]), Health::Fine);
        let (_, _, title, line) = status_texts(&health_of(&fine, &[]));
        assert_eq!(
            (title.as_str(), line.as_str()),
            (
                "Everything works",
                "Sensors, driver and configuration are in order"
            )
        );

        let read_only = SystemInfo {
            fan_driver: FanDriver {
                level: Some(DriverLevel::ReadOnly),
                ..fine.fan_driver
            },
            ..fine.clone()
        };
        assert_eq!(
            health_of(&read_only, &[]),
            Health::Limited("This system can only read the fans for now".into())
        );
        assert_eq!(
            health(&Detection::Generic, &fine, Extras::default(), &[]),
            Health::Limited("No profile for this board".into())
        );

        let alerts = ["CPU fan stopped".to_owned()];
        let unloaded = SystemInfo {
            fan_driver: FanDriver::default(),
            ..fine.clone()
        };
        let health = health_of(&unloaded, &alerts);
        assert_eq!(
            health,
            Health::Attention(vec![
                "CPU fan stopped".into(),
                "The nct6683 driver is not loaded".into()
            ])
        );
        let (_, class, title, line) = status_texts(&health);
        assert_eq!(class, "warning");
        assert_eq!(title, "Needs attention");
        assert_eq!(line, "CPU fan stopped, and 1 more");

        let views = views(
            &Dmi::default(),
            &supported,
            &fine,
            Extras::default(),
            &alerts,
        );
        let readings = &views.diagnostics[0];
        assert_eq!(readings.title, "Readings");
        assert_eq!(readings.lines[0].title, "CPU fan stopped");
        assert_eq!(readings.lines[0].status, Some(Status::Warning));
    }
}
