// SPDX-License-Identifier: GPL-3.0-or-later

//! The text of "Copy system information" (docs/PROJEKT.md §5.4): plain
//! English for bug reports, also upstream, so it is never translated. It
//! has the fields of §5.4 that M1 knows. Never in it: serial numbers and
//! MAC addresses (the model has none, see `system`), removable media and
//! the running programs (§1.3).

use std::fmt::Write;

use crate::board::{Detection, Dmi, code_label};
use crate::system::{
    Disk, GraphicsCard, Manufacturer, MemoryModule, MemorySlot, ModuleOrigin, NetworkCard,
    SystemInfo, short_cpu_name,
};

/// What the text is made of.
pub struct Report<'a> {
    /// The version of Baseboard.
    pub version: &'a str,
    pub dmi: &'a Dmi,
    pub detection: &'a Detection,
    pub info: &'a SystemInfo,
}

const UNKNOWN: &str = "unknown";

impl Report<'_> {
    /// The text, one field per line, sections apart by an empty line.
    pub fn text(&self) -> String {
        let (dmi, info) = (self.dmi, self.info);
        let mut text = Text(format!("Baseboard {}\n", self.version));

        text.heading("System");
        text.field("OS", info.os.as_deref());
        text.field("Kernel", info.kernel.as_deref());

        text.heading("Board");
        text.field("Vendor", dmi.board_vendor.as_deref());
        text.field(
            "Model",
            dmi.board_name.as_deref().or(dmi.product_name.as_deref()),
        );
        text.field("Revision", dmi.board_version.as_deref());
        text.field(
            "Board code",
            dmi.board_code().map(|code| code_label(&code)).as_deref(),
        );
        text.field(
            "PCB revision",
            dmi.pcb_revision().map(|r| r.to_string()).as_deref(),
        );
        text.field("Profile", Some(&profile(self.detection)));

        text.heading("BIOS");
        text.field("Vendor", info.bios.vendor.as_deref());
        text.field("Version", info.bios.version.as_deref());
        text.field("Date", info.bios.date.as_deref());
        text.field("Release", info.bios.release.as_deref());

        text.heading("CPU");
        text.field("Model", info.cpu.as_deref().map(short_cpu_name));
        text.field("cpufreq driver", info.cpufreq_driver.as_deref());

        text.heading("Memory");
        match &info.memory {
            Some(slots) => {
                for slot in slots {
                    text.field(&slot_name(slot), Some(&slot_text(slot)));
                }
            }
            None => text.item("no data"),
        }

        text.heading("Graphics");
        text.items(info.graphics.iter().map(graphics_text));

        text.heading("Disks");
        text.items(info.disks.iter().filter(|d| !d.removable).map(disk_text));

        text.heading("Network");
        text.items(info.network.iter().map(network_text));

        text.heading("Sensors");
        let hwmon = info.hwmon.join(", ");
        text.field(
            "hwmon devices",
            Some(if hwmon.is_empty() { "none" } else { &hwmon }),
        );
        let driver = &info.fan_driver;
        let loaded = match driver.origin {
            Some(ModuleOrigin::InTree) => "loaded (in-tree)",
            Some(ModuleOrigin::OutOfTree) => "loaded (out-of-tree)",
            None => "loaded",
        };
        let state = match (driver.loaded, driver.device) {
            (false, _) => "not loaded".to_owned(),
            (true, false) if self.detection.profile().is_some() => {
                format!("{loaded}, board device not found")
            }
            (true, _) => loaded.to_owned(),
        };
        text.field("nct6683", Some(&state));
        let level = match (driver.level, self.detection.profile()) {
            (Some(level), _) => level.as_str(),
            (None, None) => "no profile",
            (None, Some(_)) => "board device not found",
        };
        text.field("Fan control", Some(level));
        text.0
    }
}

struct Text(String);

impl Text {
    fn heading(&mut self, title: &str) {
        let _ = write!(self.0, "\n{title}\n");
    }

    fn field(&mut self, name: &str, value: Option<&str>) {
        let _ = writeln!(self.0, "  {name}: {}", value.unwrap_or(UNKNOWN));
    }

    fn item(&mut self, item: &str) {
        let _ = writeln!(self.0, "  {item}");
    }

    /// One line per item, or "none".
    fn items(&mut self, items: impl Iterator<Item = String>) {
        let mut any = false;
        for item in items {
            self.item(&item);
            any = true;
        }
        if !any {
            self.item("none");
        }
    }
}

/// The matched profile, or why there is none.
fn profile(detection: &Detection) -> String {
    match detection {
        Detection::Supported(profile) => profile.id.clone(),
        Detection::UnverifiedRevision {
            profile,
            revision: Some(revision),
        } => format!(
            "{}, PCB revision {revision} not verified (read only)",
            profile.id
        ),
        Detection::UnverifiedRevision {
            profile,
            revision: None,
        } => format!("{}, unknown PCB revision (read only)", profile.id),
        Detection::OtherModel { code, .. } => {
            format!("none, the profiles for MS-{code} are for other models (generic mode)")
        }
        Detection::Generic => "none (generic mode)".to_owned(),
    }
}

fn slot_name(slot: &MemorySlot) -> String {
    slot.locator
        .clone()
        .unwrap_or_else(|| format!("Slot {}", slot.index))
}

fn slot_text(slot: &MemorySlot) -> String {
    let Some(module) = &slot.module else {
        return "empty".to_owned();
    };
    let mut size = binary_size(module.size_bytes);
    if let Some(kind) = &module.kind {
        size = format!("{size} {kind}");
    }
    let mut parts = vec![size];
    parts.extend(speed(module));
    parts.extend(module.part_number.as_ref().map(|p| format!("part {p}")));
    // The raw JEDEC code is here only, not on the System page (D37).
    parts.extend(module.manufacturer.as_ref().map(|maker| match maker {
        Manufacturer::Name(name) => format!("maker {name}"),
        Manufacturer::JedecId(id) => format!("maker JEDEC {id}"),
    }));
    parts.join("; ")
}

/// The configured speed, and the highest one when it differs.
fn speed(module: &MemoryModule) -> Option<String> {
    match (module.configured_speed_mts, module.max_speed_mts) {
        (Some(set), Some(max)) if set != max => Some(format!("{set} MT/s (max {max} MT/s)")),
        (Some(set), _) => Some(format!("{set} MT/s")),
        (None, Some(max)) => Some(format!("max {max} MT/s")),
        (None, None) => None,
    }
}

fn graphics_text(card: &GraphicsCard) -> String {
    let mut parts = vec![card.name.clone().unwrap_or_else(|| UNKNOWN.to_owned())];
    if let Some(id) = card.pci_id {
        let mut pci = format!("PCI {:04x}:{:04x}", id.vendor, id.device);
        if let Some((vendor, device)) = id.subsystem {
            let _ = write!(pci, " (subsystem {vendor:04x}:{device:04x})");
        }
        parts.push(pci);
    }
    parts.extend(card.vbios.as_ref().map(|v| format!("VBIOS {v}")));
    parts.join(", ")
}

fn disk_text(disk: &Disk) -> String {
    let mut parts = vec![
        disk.model.clone().unwrap_or_else(|| disk.name.clone()),
        decimal_size(disk.size_bytes),
    ];
    parts.extend(disk.firmware.as_ref().map(|f| format!("firmware {f}")));
    parts.join(", ")
}

fn network_text(card: &NetworkCard) -> String {
    let name = card.name.as_deref().unwrap_or(UNKNOWN);
    match &card.driver {
        Some(driver) => format!("{name}, driver {driver}"),
        None => format!("{name}, no driver"),
    }
}

/// Memory in binary units: "16 GiB", "1.5 GiB", "512 MiB".
fn binary_size(bytes: u64) -> String {
    const GIB: u64 = 1 << 30;
    const MIB: u64 = 1 << 20;
    if bytes >= GIB {
        if bytes.is_multiple_of(GIB) {
            format!("{} GiB", bytes / GIB)
        } else {
            format!("{:.1} GiB", bytes as f64 / GIB as f64)
        }
    } else {
        format!("{} MiB", bytes / MIB)
    }
}

/// Disks in decimal units with one decimal: "2.0 TB", "61.5 GB".
fn decimal_size(bytes: u64) -> String {
    let bytes = bytes as f64;
    let (value, unit) = if bytes >= 1e12 {
        (bytes / 1e12, "TB")
    } else if bytes >= 1e9 {
        (bytes / 1e9, "GB")
    } else {
        (bytes / 1e6, "MB")
    };
    format!("{value:.1} {unit}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(binary_size(16 << 30), "16 GiB");
        assert_eq!(binary_size(3 << 29), "1.5 GiB");
        assert_eq!(binary_size(512 << 20), "512 MiB");
        assert_eq!(decimal_size(2_000_398_934_016), "2.0 TB");
        assert_eq!(decimal_size(61_524_148_224), "61.5 GB");
        assert_eq!(decimal_size(500_000_000), "500.0 MB");
    }

    #[test]
    fn speeds() {
        let module = |set, max| MemoryModule {
            size_bytes: 1,
            kind: None,
            max_speed_mts: max,
            configured_speed_mts: set,
            part_number: None,
            manufacturer: None,
        };
        assert_eq!(
            speed(&module(Some(6000), Some(6000))).as_deref(),
            Some("6000 MT/s")
        );
        assert_eq!(
            speed(&module(Some(4800), Some(6000))).as_deref(),
            Some("4800 MT/s (max 6000 MT/s)")
        );
        assert_eq!(
            speed(&module(None, Some(6000))).as_deref(),
            Some("max 6000 MT/s")
        );
        assert_eq!(speed(&module(None, None)), None);
    }
}
