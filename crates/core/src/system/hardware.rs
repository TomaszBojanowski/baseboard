// SPDX-License-Identifier: GPL-3.0-or-later

//! The hardware of the System page (docs/PROJEKT.md §5.4): the processor,
//! memory from the udev database, graphics cards, network cards and disks.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use super::{
    DMI_DEVICE, Disk, GraphicsCard, MEMORY_FIELDS, Manufacturer, MemoryModule, MemorySlot,
    NetworkCard, PciId, attribute, canonical, clean, entries, meaningful, natural_order, read_text,
    relative,
};
use crate::udev::Udev;

/// The first `model name` in `<proc>/cpuinfo`.
pub fn cpu_model(proc: &Path) -> Option<String> {
    let text = std::fs::read_to_string(proc.join("cpuinfo")).ok()?;
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name.trim() == "model name")
            .then(|| clean(value))
            .flatten()
    })
}

/// The processor's name on every page: the model without a trailing
/// "8-Core Processor" ("AMD Ryzen 7 9700X 8-Core Processor" → "AMD Ryzen 7
/// 9700X").
pub fn short_cpu_name(model: &str) -> &str {
    let trimmed = model.trim();
    let Some(rest) = trimmed.strip_suffix("-Core Processor") else {
        return trimmed;
    };
    let digits = rest.len() - rest.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let before = &rest[..rest.len() - digits];
    if digits == 0 || !before.ends_with(' ') || before.trim().is_empty() {
        return trimmed;
    }
    before.trim_end()
}

/// The cpufreq driver of the policy with the lowest number, else of cpu0.
pub fn cpufreq_driver(sys: &Path) -> Option<String> {
    let cpu = sys.join("devices/system/cpu");
    let mut policies: Vec<(u32, PathBuf)> = std::fs::read_dir(cpu.join("cpufreq"))
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let number = entry
                .file_name()
                .to_str()?
                .strip_prefix("policy")?
                .parse()
                .ok()?;
            Some((number, entry.path()))
        })
        .collect();
    policies.sort();
    policies
        .iter()
        .map(|(_, path)| path.join("scaling_driver"))
        .chain([cpu.join("cpu0/cpufreq/scaling_driver")])
        .find_map(|file| read_text(&file))
}

/// The memory slots from the udev database, in SMBIOS order; `None` when
/// it has none.
pub fn memory(udev: &dyn Udev) -> Option<Vec<MemorySlot>> {
    let device = Path::new(DMI_DEVICE);
    let mut slots = Vec::new();
    for index in 0..u8::MAX {
        let field = |name: &str| {
            debug_assert!(MEMORY_FIELDS.contains(&name));
            udev.property(device, &format!("MEMORY_DEVICE_{index}_{name}"))
        };
        let locator = field("LOCATOR");
        let present = field("PRESENT");
        let size = field("SIZE");
        if locator.is_none() && present.is_none() && size.is_none() {
            break;
        }
        let size = size
            .and_then(|s| s.trim().parse::<u64>().ok())
            .filter(|&s| s > 0);
        let module = match (present.as_deref().map(str::trim), size) {
            (Some("0"), _) | (_, None) => None,
            (_, Some(size_bytes)) => Some(MemoryModule {
                size_bytes,
                kind: field("TYPE").and_then(|v| meaningful(&v)),
                max_speed_mts: field("SPEED_MTS").and_then(|v| v.trim().parse().ok()),
                configured_speed_mts: field("CONFIGURED_SPEED_MTS")
                    .and_then(|v| v.trim().parse().ok()),
                part_number: field("PART_NUMBER").and_then(|v| meaningful(&v)),
                manufacturer: field("MANUFACTURER")
                    .and_then(|v| meaningful(&v))
                    .map(Manufacturer::Name)
                    .or_else(|| {
                        field("MODULE_MANUFACTURER_ID")
                            .and_then(|v| meaningful(&v))
                            .map(Manufacturer::JedecId)
                    }),
            }),
        };
        slots.push(MemorySlot {
            index,
            locator: locator.and_then(|v| meaningful(&v)),
            module,
        });
    }
    (!slots.is_empty()).then_some(slots)
}

/// The slot name of each SPD sensor of a memory module (§5.1). The SPD hub
/// of the module in slot i answers on SMBus address 0x50 + i. The names
/// are used only when this is unambiguous for every sensor: all on one
/// bus, as many sensors as filled slots, and each address points at a
/// filled slot with a name of its own. Otherwise the map is empty and the
/// Overview numbers the modules. `sensors` are the devices of the SPD
/// sensors, e.g. `…/i2c-8/8-0051`.
pub fn spd_slot_names(sensors: &[&Path], slots: &[MemorySlot]) -> HashMap<PathBuf, String> {
    let filled: Vec<&MemorySlot> = slots.iter().filter(|s| s.module.is_some()).collect();
    let mut names = HashMap::new();
    let mut buses = BTreeSet::new();
    let mut used = BTreeSet::new();
    for device in sensors {
        let Some((bus, address)) = device
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(i2c_address)
        else {
            return HashMap::new();
        };
        let slot = address
            .checked_sub(0x50)
            .filter(|i| *i < 8)
            .and_then(|i| filled.iter().find(|s| u16::from(s.index) == i));
        let Some(name) = slot.and_then(|s| s.locator.clone()) else {
            return HashMap::new();
        };
        buses.insert(bus);
        if !used.insert(name.clone()) {
            return HashMap::new();
        }
        names.insert(device.to_path_buf(), name);
    }
    if buses.len() > 1 || sensors.len() != filled.len() {
        return HashMap::new();
    }
    names
}

/// Bus and address of an I2C device named like `8-0051`.
fn i2c_address(name: &str) -> Option<(u32, u16)> {
    let (bus, address) = name.split_once('-')?;
    Some((bus.parse().ok()?, u16::from_str_radix(address, 16).ok()?))
}

/// The name of a PCI device: its `product_name` (amdgpu gives one for some
/// cards), else the udev database entry (`ID_MODEL_FROM_DATABASE`, from
/// `pci.ids`), unchanged. `device` is relative to `sys`.
pub fn device_name(sys: &Path, udev: &dyn Udev, device: &Path) -> Option<String> {
    attribute(sys, device, "product_name")
        .or_else(|| udev.property(device, "ID_MODEL_FROM_DATABASE"))
}

/// The devices of the DRM cards (`card0`, not `card0-DP-1`), sorted.
pub fn graphics_cards(sys: &Path, udev: &dyn Udev) -> Vec<GraphicsCard> {
    let sys = canonical(sys);
    let mut devices: Vec<PathBuf> = entries(&sys.join("class/drm"))
        .into_iter()
        .filter(|(name, _)| {
            name.strip_prefix("card")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|(_, path)| relative(&sys, &path.join("device")))
        .collect();
    devices.sort();
    devices.dedup();
    devices
        .into_iter()
        .map(|device| GraphicsCard {
            name: device_name(&sys, udev, &device),
            pci_id: pci_id(&sys, &device),
            driver: driver_name(&sys, &device),
            vbios: attribute(&sys, &device, "vbios_version"),
            device,
        })
        .collect()
}

/// The PCI devices of the network class (0x02), sorted; those of the
/// subclass "other" (0x80) are Wi-Fi cards.
pub fn network_cards(sys: &Path, udev: &dyn Udev) -> Vec<NetworkCard> {
    let sys = canonical(sys);
    let mut devices: Vec<(PathBuf, u32)> = entries(&sys.join("bus/pci/devices"))
        .into_iter()
        .filter_map(|(_, path)| relative(&sys, &path))
        .filter_map(|device| {
            attribute(&sys, &device, "class")
                .and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok())
                .filter(|class| class >> 16 == 0x02)
                .map(|class| (device, class))
        })
        .collect();
    devices.sort();
    devices
        .into_iter()
        .map(|(device, class)| NetworkCard {
            name: device_name(&sys, udev, &device),
            driver: driver_name(&sys, &device),
            wireless: (class >> 8) & 0xff == 0x80,
            device,
        })
        .collect()
}

/// The block devices with hardware behind them (no zram, loop or device
/// mapper) and a size, sorted by name.
pub fn disks(sys: &Path) -> Vec<Disk> {
    let mut blocks = entries(&canonical(sys).join("block"));
    blocks.sort_by(|a, b| natural_order(&a.0, &b.0));
    blocks
        .into_iter()
        .filter_map(|(name, block)| {
            let device = block.join("device");
            if !device.exists() {
                return None;
            }
            let sectors: u64 = read_text(&block.join("size"))?.parse().ok()?;
            if sectors == 0 {
                return None;
            }
            // An NVMe namespace's device is its controller, which names
            // the firmware differently.
            let firmware = if device.join("firmware_rev").exists() {
                "firmware_rev"
            } else {
                "rev"
            };
            Some(Disk {
                model: read_text(&device.join("model")),
                firmware: read_text(&device.join(firmware)),
                size_bytes: sectors.saturating_mul(512),
                removable: read_text(&block.join("removable")).as_deref() == Some("1"),
                name,
            })
        })
        .collect()
}

/// The identifiers of a PCI device from sysfs.
fn pci_id(sys: &Path, device: &Path) -> Option<PciId> {
    let hex = |name: &str| {
        attribute(sys, device, name)
            .and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok())
    };
    Some(PciId {
        vendor: hex("vendor")?,
        device: hex("device")?,
        subsystem: hex("subsystem_vendor").zip(hex("subsystem_device")),
    })
}

/// The name of the driver bound to a device.
fn driver_name(sys: &Path, device: &Path) -> Option<String> {
    let link = std::fs::read_link(sys.join(device).join("driver")).ok()?;
    link.file_name().map(|n| n.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_cpu_names() {
        assert_eq!(
            short_cpu_name("AMD Ryzen 7 9700X 8-Core Processor"),
            "AMD Ryzen 7 9700X"
        );
        assert_eq!(
            short_cpu_name("AMD Ryzen 9 7950X3D 16-Core Processor  "),
            "AMD Ryzen 9 7950X3D"
        );
        for unchanged in [
            "Intel(R) Core(TM) i7-14700K",
            "Some-Core Processor",
            "8-Core Processor",
            "AMD Ryzen 7 9700X8-Core Processor",
        ] {
            assert_eq!(short_cpu_name(unchanged), unchanged);
        }
    }
}
