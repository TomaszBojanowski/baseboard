// SPDX-License-Identifier: GPL-3.0-or-later

//! The texts of the readings of the model (`core::overview`): the titles of
//! the groups, the names and subtitles of the rows and the ratings of the
//! rails, through gettext, with numbers through `core::units`. The Overview,
//! "All Sensors", the Fans page, the Preferences and the warnings use them.

use baseboard_core::fans::driver::Mode;
use baseboard_core::overview::{self, Group, GroupKind, Name, Rating, Row, Subtitle};
use baseboard_core::profile::{FanRole, TempRole, VoltageRole};
use baseboard_core::sensors::Kind;
use baseboard_core::system;
use baseboard_core::units::Numbers;
use gettextrs::{gettext, pgettext};

/// The title of a group.
pub fn group_title(kind: GroupKind) -> String {
    match kind {
        GroupKind::Cpu => gettext("Processor"),
        GroupKind::Board => gettext("Motherboard"),
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
pub(super) fn short_name(name: &Name, numbers: &Numbers) -> String {
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

#[cfg(test)]
mod tests {
    use baseboard_core::overview::RowId;

    use super::*;

    fn row(subtitle: Subtitle, rating: Option<Rating>) -> Row {
        Row {
            id: RowId {
                chip: "nct6687".into(),
                device: Some("devices/platform/nct6683.2592".into()),
                channel: "in1".into(),
            },
            name: Name::Driver("in1".into()),
            role: Name::Driver("in1".into()),
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
