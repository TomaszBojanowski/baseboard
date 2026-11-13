// SPDX-License-Identifier: GPL-3.0-or-later

//! The data sources of the System page (docs/PROJEKT.md §5.4), read only:
//! the operating system, the BIOS, the processor, memory from the udev
//! database, graphics cards, disks, network cards, the fan driver, the
//! lm-sensors configuration and programs that may also control the fans.
//! No user-visible text: the app makes it.
//!
//! Serial numbers and MAC addresses are never read (§1.3). This module
//! opens only the files and asks only for the udev properties it names, and
//! none of them holds one: no `*_serial`, `product_uuid`, `serial`,
//! `address`, `MEMORY_DEVICE_*_SERIAL_NUMBER` or `ID_NET_NAME_MAC`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use crate::fans::driver::{self, DriverLevel};
use crate::profile::Profile;
use crate::udev::Udev;

/// Where the system's files are. Tests put all of them into a fake tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// `/sys`, or a fake tree.
    pub sys: PathBuf,
    pub proc: PathBuf,
    /// The host's `/etc`, for the lm-sensors files; `None` in a sandbox that
    /// cannot see it (docs/PROJEKT.md §10.3).
    pub etc: Option<PathBuf>,
    /// The `os-release` files to try, in order.
    pub os_release: Vec<PathBuf>,
    /// Whether `proc` shows the host's processes. A sandbox shows only its
    /// own, so the fan programs cannot be checked there.
    pub host_processes: bool,
}

impl Paths {
    /// This system, with sysfs at `sys` (a fake tree in a debug build).
    pub fn system(sys: &Path) -> Self {
        Self {
            sys: sys.to_owned(),
            proc: PathBuf::from("/proc"),
            etc: Some(PathBuf::from("/etc")),
            os_release: vec![
                PathBuf::from("/etc/os-release"),
                PathBuf::from("/usr/lib/os-release"),
            ],
            host_processes: true,
        }
    }

    /// This system seen from a Flatpak sandbox (§10.1): the host's
    /// `os-release` in `/run/host`, its `/etc` in `/run/host/etc` only when
    /// shared (`host_etc`), and a `/proc` of the sandbox's own processes.
    pub fn flatpak(sys: &Path, host_etc: bool) -> Self {
        Self {
            sys: sys.to_owned(),
            proc: PathBuf::from("/proc"),
            etc: host_etc.then(|| PathBuf::from("/run/host/etc")),
            os_release: vec![PathBuf::from("/run/host/os-release")],
            host_processes: false,
        }
    }

    /// Everything under one folder: `<root>/sys`, `<root>/proc`, …
    pub fn under(root: &Path) -> Self {
        Self {
            sys: root.join("sys"),
            proc: root.join("proc"),
            etc: Some(root.join("etc")),
            os_release: vec![root.join("etc/os-release"), root.join("usr/lib/os-release")],
            host_processes: true,
        }
    }
}

/// How Baseboard is installed, for "Copy system information".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Installation {
    #[default]
    Native,
    /// In a Flatpak sandbox, with its runtime, e.g. `org.gnome.Platform 51`.
    Flatpak { runtime: Option<String> },
}

/// The runtime named in the sandbox's `/.flatpak-info`, as "name branch":
/// `runtime=runtime/org.gnome.Platform/x86_64/51` → `org.gnome.Platform 51`.
pub fn flatpak_runtime(info: &str) -> Option<String> {
    let reference = info
        .lines()
        .find_map(|line| line.trim().strip_prefix("runtime="))?;
    let mut parts = reference.split('/');
    let (kind, name, _arch, branch) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    (kind == "runtime" && !name.is_empty()).then(|| format!("{name} {branch}"))
}

/// The only BIOS files this module opens, relative to `class/dmi/id`.
pub const BIOS_FIELDS: [&str; 4] = ["bios_vendor", "bios_version", "bios_date", "bios_release"];

/// The device in the udev database with the memory properties, which the
/// `dmi_memory_id` builtin of systemd fills from SMBIOS type 17.
pub const DMI_DEVICE: &str = "devices/virtual/dmi/id";

/// The only properties of a memory slot this module reads, as
/// `MEMORY_DEVICE_<n>_<field>`. Never the serial number (§1.3).
pub const MEMORY_FIELDS: [&str; 9] = [
    "LOCATOR",
    "PRESENT",
    "SIZE",
    "TYPE",
    "SPEED_MTS",
    "CONFIGURED_SPEED_MTS",
    "PART_NUMBER",
    "MANUFACTURER",
    "MODULE_MANUFACTURER_ID",
];

/// Programs that control fans and could fight with Baseboard (§5.4), by
/// process name (`/proc/<pid>/comm`).
pub const FAN_PROGRAMS: [&str; 4] = ["fancontrol", "coolercontrold", "fan2go", "thinkfan"];

/// The kernel module of the board's EC.
pub const FAN_DRIVER_MODULE: &str = "nct6683";

/// Everything the System page shows besides the board (`board::Dmi` and
/// the detection). The default is a system that can be seen whole and has
/// nothing to show.
#[derive(Debug, Clone, PartialEq)]
pub struct SystemInfo {
    /// `PRETTY_NAME` of `os-release`, else `NAME`.
    pub os: Option<String>,
    /// The kernel release, e.g. `7.3.3-300.fc45.x86_64`.
    pub kernel: Option<String>,
    pub bios: Bios,
    /// The processor's `model name`, unchanged; the pages shorten it
    /// ([`short_cpu_name`]).
    pub cpu: Option<String>,
    /// `scaling_driver` of the first cpufreq policy, e.g. `amd-pstate-epp`.
    pub cpufreq_driver: Option<String>,
    /// The memory slots; `None` when the udev database has none ("no data",
    /// §5.4).
    pub memory: Option<Vec<MemorySlot>>,
    pub graphics: Vec<GraphicsCard>,
    pub disks: Vec<Disk>,
    pub network: Vec<NetworkCard>,
    /// The `name` of every hwmon device, sorted, repeated for devices of one
    /// driver.
    pub hwmon: Vec<String>,
    pub fan_driver: FanDriver,
    /// The lm-sensors files with a `chip` statement for the board's device;
    /// empty without a profile, `None` when the host's `/etc` cannot be
    /// seen (a sandbox, §10.3).
    pub sensors_config: Option<Vec<PathBuf>>,
    /// The programs of [`FAN_PROGRAMS`] that run, sorted, each once; `None`
    /// when the host's processes cannot be seen (a sandbox, §10.3).
    pub fan_programs: Option<Vec<String>>,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self {
            os: None,
            kernel: None,
            bios: Bios::default(),
            cpu: None,
            cpufreq_driver: None,
            memory: None,
            graphics: Vec::new(),
            disks: Vec::new(),
            network: Vec::new(),
            hwmon: Vec::new(),
            fan_driver: FanDriver::default(),
            sensors_config: Some(Vec::new()),
            fan_programs: Some(Vec::new()),
        }
    }
}

/// The BIOS from DMI. A missing or blank file is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Bios {
    pub vendor: Option<String>,
    pub version: Option<String>,
    /// As DMI gives it: MM/DD/YYYY.
    pub date: Option<String>,
    /// `bios_release`, e.g. `5.41`.
    pub release: Option<String>,
}

impl Bios {
    /// Reads the files of [`BIOS_FIELDS`] under `<sys>/class/dmi/id`.
    pub fn read(sys: &Path) -> Self {
        let dir = sys.join("class/dmi/id");
        let field = |name: &str| {
            debug_assert!(BIOS_FIELDS.contains(&name));
            read_text(&dir.join(name))
        };
        Self {
            vendor: field("bios_vendor"),
            version: field("bios_version"),
            date: field("bios_date"),
            release: field("bios_release"),
        }
    }

    /// The date as (year, month, day), from MM/DD/YYYY.
    pub fn ymd(&self) -> Option<(u16, u8, u8)> {
        let mut parts = self.date.as_deref()?.split('/');
        let month: u8 = parts.next()?.parse().ok()?;
        let day: u8 = parts.next()?.parse().ok()?;
        let year: u16 = parts.next()?.parse().ok()?;
        let valid = parts.next().is_none() && (1..=12).contains(&month) && (1..=31).contains(&day);
        valid.then_some((year, month, day))
    }
}

/// One memory slot from SMBIOS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySlot {
    /// n of `MEMORY_DEVICE_<n>_*`: the slot's place in SMBIOS.
    pub index: u8,
    /// The slot's name on the board, e.g. `DIMMA2`.
    pub locator: Option<String>,
    /// `None` for an empty slot.
    pub module: Option<MemoryModule>,
}

/// The module in a slot. Fields that SMBIOS leaves as "Unknown" are `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryModule {
    pub size_bytes: u64,
    /// E.g. `DDR5`.
    pub kind: Option<String>,
    /// The module's highest speed in MT/s.
    pub max_speed_mts: Option<u32>,
    /// The speed it is set to in MT/s.
    pub configured_speed_mts: Option<u32>,
    pub part_number: Option<String>,
    pub manufacturer: Option<Manufacturer>,
}

/// Who made a memory module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Manufacturer {
    /// The name SMBIOS gives.
    Name(String),
    /// SMBIOS gives no name: the raw JEDEC identifier, e.g. "Bank 8, Hex
    /// 0x5D", shown as it is (D8, O14).
    JedecId(String),
}

/// A graphics card: a device with a DRM card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphicsCard {
    /// The device, relative to the sysfs root.
    pub device: PathBuf,
    /// [`device_name`]: `product_name`, else the udev database entry,
    /// unchanged (the app shortens it, D23).
    pub name: Option<String>,
    pub pci_id: Option<PciId>,
    pub driver: Option<String>,
    /// `vbios_version` (amdgpu).
    pub vbios: Option<String>,
}

/// The identifiers of a PCI device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PciId {
    pub vendor: u16,
    pub device: u16,
    /// Subsystem vendor and device, where sysfs gives them.
    pub subsystem: Option<(u16, u16)>,
}

/// A disk: a block device with hardware behind it and a size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disk {
    /// The block device, e.g. `nvme0n1`.
    pub name: String,
    pub model: Option<String>,
    /// `firmware_rev` of an NVMe controller, `rev` of other disks.
    pub firmware: Option<String>,
    pub size_bytes: u64,
    /// The kernel marks the medium removable, e.g. a USB stick.
    pub removable: bool,
}

/// A network card: a PCI device of the network class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkCard {
    /// The device, relative to the sysfs root.
    pub device: PathBuf,
    /// The udev database entry, unchanged (the app shortens it, D23).
    pub name: Option<String>,
    pub driver: Option<String>,
}

/// The state of the board's EC driver (§5.4).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FanDriver {
    /// The module is loaded or built in.
    pub loaded: bool,
    /// Whether the loaded module comes with the kernel; `None` when it is
    /// not loaded or its taint flags cannot be read.
    pub origin: Option<ModuleOrigin>,
    /// The profile's hwmon device is there, once; never without a profile.
    pub device: bool,
    /// What the driver allows on the profile's channels (§5.5.2); only
    /// with the device.
    pub level: Option<DriverLevel>,
}

/// Where a loaded kernel module comes from, by its taint flags in
/// `/proc/modules` (D40). Only shown to the user: what the driver allows is
/// never judged by this (§5.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleOrigin {
    /// Built from the kernel's tree: no `O` flag, or built in.
    InTree,
    /// Built outside the kernel's tree (flag `O`), e.g. a test module.
    OutOfTree,
}

/// Reads everything. Only reads files and the udev database.
pub fn read(paths: &Paths, udev: &dyn Udev, profile: Option<&Profile>) -> SystemInfo {
    let sys = canonical(&paths.sys);
    SystemInfo {
        os: os_name(paths),
        kernel: read_text(&paths.proc.join("sys/kernel/osrelease")),
        bios: Bios::read(&sys),
        cpu: cpu_model(&paths.proc),
        cpufreq_driver: cpufreq_driver(&sys),
        memory: memory(udev),
        graphics: graphics_cards(&sys, udev),
        disks: disks(&sys),
        network: network_cards(&sys, udev),
        hwmon: hwmon_names(&sys),
        fan_driver: fan_driver(&sys, &paths.proc, profile),
        sensors_config: paths.etc.as_deref().map(|etc| {
            profile
                .map(|p| sensors_config(etc, &p.hwmon.name))
                .unwrap_or_default()
        }),
        fan_programs: paths.host_processes.then(|| fan_programs(&paths.proc)),
    }
}

/// `PRETTY_NAME` from the first `os-release` file there is (`/etc`, then
/// `/usr/lib`; in a sandbox the host's in `/run/host`), `NAME` when there is
/// no `PRETTY_NAME`.
pub fn os_name(paths: &Paths) -> Option<String> {
    let text = paths
        .os_release
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())?;
    os_release_value(&text, "PRETTY_NAME").or_else(|| os_release_value(&text, "NAME"))
}

/// The value of `key` in an `os-release` file, without its quotes.
fn os_release_value(text: &str, key: &str) -> Option<String> {
    let value = text.lines().find_map(|line| {
        let (name, value) = line.trim().split_once('=')?;
        (name == key).then_some(value.trim())
    })?;
    let unquoted = ['"', '\'']
        .iter()
        .find_map(|q| value.strip_prefix(*q).and_then(|v| v.strip_suffix(*q)))
        .unwrap_or(value);
    // Shell escapes, as in `\"`.
    let mut text = String::with_capacity(unquoted.len());
    let mut chars = unquoted.chars();
    while let Some(c) = chars.next() {
        text.push(if c == '\\' {
            chars.next().unwrap_or(c)
        } else {
            c
        });
    }
    clean(&text)
}

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

/// The PCI devices of the network class (0x02), sorted.
pub fn network_cards(sys: &Path, udev: &dyn Udev) -> Vec<NetworkCard> {
    let sys = canonical(sys);
    let mut devices: Vec<PathBuf> = entries(&sys.join("bus/pci/devices"))
        .into_iter()
        .filter_map(|(_, path)| relative(&sys, &path))
        .filter(|device| {
            attribute(&sys, device, "class")
                .and_then(|c| u32::from_str_radix(c.trim_start_matches("0x"), 16).ok())
                .is_some_and(|class| class >> 16 == 0x02)
        })
        .collect();
    devices.sort();
    devices
        .into_iter()
        .map(|device| NetworkCard {
            name: device_name(&sys, udev, &device),
            driver: driver_name(&sys, &device),
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

/// Whether the EC driver is loaded and what it allows on the profile's
/// channels. Reads only `name` files, the modes of the `pwm` files and
/// `<proc>/modules`.
pub fn fan_driver(sys: &Path, proc: &Path, profile: Option<&Profile>) -> FanDriver {
    let sys = canonical(sys);
    let hwmon = profile.and_then(|p| hwmon_by_name(&sys, &p.hwmon.name));
    let level = profile
        .zip(hwmon.as_deref())
        .map(|(profile, hwmon)| driver::driver_level(&driver::channels(profile, hwmon)));
    let listing = module_listing(proc, FAN_DRIVER_MODULE);
    let loaded = matches!(listing, Listing::Listed { .. }) || hwmon.is_some();
    let origin = match listing {
        Listing::Listed { out_of_tree: true } => Some(ModuleOrigin::OutOfTree),
        Listing::Listed { out_of_tree: false } => Some(ModuleOrigin::InTree),
        // Not a loadable module, yet its device is there: built in.
        Listing::NotListed if loaded => Some(ModuleOrigin::InTree),
        Listing::NotListed | Listing::Unknown => None,
    };
    FanDriver {
        loaded,
        origin,
        device: hwmon.is_some(),
        level,
    }
}

/// What `/proc/modules` says of one module.
enum Listing {
    /// Loaded as a module; flag `O` among its taint flags means out of the
    /// kernel's tree.
    Listed { out_of_tree: bool },
    /// Not a loaded module. A kernel without loadable modules has no list
    /// at all, which means the same.
    NotListed,
    /// The list cannot be read: the origin stays unknown.
    Unknown,
}

/// The module's line in `<proc>/modules`, e.g.
/// `nct6683 53248 0 - Live 0x0000000000000000 (O)`; its taint flags, if
/// any, close the line in brackets. Read each time, so a module loaded or
/// replaced while the app runs shows as it is now (D40).
fn module_listing(proc: &Path, name: &str) -> Listing {
    let text = match std::fs::read_to_string(proc.join("modules")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Listing::NotListed,
        Err(_) => return Listing::Unknown,
    };
    let Some(line) = text
        .lines()
        .find(|line| line.split_whitespace().next() == Some(name))
    else {
        return Listing::NotListed;
    };
    let flags = line
        .trim_end()
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once('('))
        .map_or("", |(_, flags)| flags);
    Listing::Listed {
        out_of_tree: flags.contains('O'),
    }
}

/// The names of the hwmon devices, sorted.
pub fn hwmon_names(sys: &Path) -> Vec<String> {
    let mut names: Vec<String> = entries(&sys.join("class/hwmon"))
        .into_iter()
        .filter_map(|(_, path)| read_text(&path.join("name")))
        .collect();
    names.sort();
    names
}

/// The only hwmon folder with the given `name`, as in
/// [`crate::sensors::find`].
fn hwmon_by_name(sys: &Path, name: &str) -> Option<PathBuf> {
    let mut found = entries(&sys.join("class/hwmon"))
        .into_iter()
        .filter(|(_, path)| read_text(&path.join("name")).as_deref() == Some(name))
        .filter_map(|(_, path)| std::fs::canonicalize(path).ok());
    let first = found.next()?;
    found.next().is_none().then_some(first)
}

/// The lm-sensors files with a `chip` statement for `chip` (the hwmon name,
/// e.g. `nct6687`): a pattern whose part before the first "-" is that name.
/// The files are the ones libsensors reads: `sensors3.conf` (else
/// `sensors.conf`) and `sensors.d/*` without hidden files, in its order.
pub fn sensors_config(etc: &Path, chip: &str) -> Vec<PathBuf> {
    let main = ["sensors3.conf", "sensors.conf"]
        .iter()
        .map(|name| etc.join(name))
        .find(|path| path.is_file());
    let mut extra: Vec<PathBuf> = entries(&etc.join("sensors.d"))
        .into_iter()
        .filter(|(name, path)| !name.starts_with('.') && path.is_file())
        .map(|(_, path)| path)
        .collect();
    extra.sort();
    main.into_iter()
        .chain(extra)
        .filter(|path| std::fs::read_to_string(path).is_ok_and(|text| names_chip(&text, chip)))
        .collect()
}

/// Whether a configuration has a `chip` statement for `chip`. A line ending
/// in "\" goes on in the next one; "#" starts a comment.
fn names_chip(config: &str, chip: &str) -> bool {
    let mut statement = String::new();
    for line in config.lines() {
        let line = line.split('#').next().unwrap_or_default().trim_end();
        if let Some(start) = line.strip_suffix('\\') {
            statement.push_str(start);
            statement.push(' ');
            continue;
        }
        statement.push_str(line);
        let mut words = statement.split_whitespace();
        if words.next() == Some("chip")
            && words.any(|pattern| pattern.trim_matches('"').split('-').next() == Some(chip))
        {
            return true;
        }
        statement.clear();
    }
    false
}

/// The programs of [`FAN_PROGRAMS`] that run, by `<proc>/<pid>/comm`.
pub fn fan_programs(proc: &Path) -> Vec<String> {
    let running: BTreeSet<String> = entries(proc)
        .into_iter()
        .filter(|(name, _)| name.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(_, path)| read_text(&path.join("comm")))
        .filter(|comm| FAN_PROGRAMS.contains(&comm.as_str()))
        .collect();
    running.into_iter().collect()
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

/// The entries of a folder: (name, path), unsorted; none if it cannot be
/// read.
fn entries(dir: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let entry = entry.ok()?;
            Some((entry.file_name().to_str()?.to_owned(), entry.path()))
        })
        .collect()
}

/// Where `link` leads, relative to `sys` (both canonical).
fn relative(sys: &Path, link: &Path) -> Option<PathBuf> {
    let target = std::fs::canonicalize(link).ok()?;
    target.strip_prefix(sys).ok().map(Path::to_path_buf)
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

/// A sysfs attribute of a device, trimmed; `None` if missing or blank.
pub(crate) fn attribute(sys: &Path, device: &Path, name: &str) -> Option<String> {
    read_text(&sys.join(device).join(name))
}

/// A small text file, trimmed; `None` if missing, unreadable or blank.
fn read_text(path: &Path) -> Option<String> {
    clean(&std::fs::read_to_string(path).ok()?)
}

/// Device names in their natural order: runs of digits compare as
/// numbers, so "nvme2n1" comes before "nvme10n1" and "nvme1" before
/// "sda" (§5.1, §5.4).
pub fn natural_order(a: &str, b: &str) -> std::cmp::Ordering {
    fn runs(s: &str) -> Vec<(bool, &str)> {
        let mut runs = Vec::new();
        let mut start = 0;
        let bytes = s.as_bytes();
        for i in 1..=bytes.len() {
            if i == bytes.len() || bytes[i].is_ascii_digit() != bytes[start].is_ascii_digit() {
                runs.push((bytes[start].is_ascii_digit(), &s[start..i]));
                start = i;
            }
        }
        runs
    }
    if a.is_empty() || b.is_empty() {
        return a.cmp(b);
    }
    let (ra, rb) = (runs(a), runs(b));
    for ((da, sa), (db, sb)) in ra.iter().zip(&rb) {
        let order = if *da && *db {
            let (na, nb) = (sa.trim_start_matches('0'), sb.trim_start_matches('0'));
            na.len().cmp(&nb.len()).then_with(|| na.cmp(nb))
        } else {
            sa.cmp(sb)
        };
        if order.is_ne() {
            return order;
        }
    }
    ra.len().cmp(&rb.len())
}

fn clean(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// A string from SMBIOS, without the placeholders firmware puts in fields
/// it does not know.
fn meaningful(value: &str) -> Option<String> {
    const PLACEHOLDERS: [&str; 6] = [
        "unknown",
        "not specified",
        "not provided",
        "none",
        "undefined",
        "to be filled by o.e.m.",
    ];
    clean(value).filter(|v| !PLACEHOLDERS.contains(&v.to_ascii_lowercase().as_str()))
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

    #[test]
    fn os_release_values() {
        let text = "NAME=\"Fedora Linux\"\nPRETTY_NAME=\"Fedora Linux 45 (Workstation Edition)\"\n\
                    ID=fedora\nQUOTED='single'\nESCAPED=\"say \\\"hi\\\"\"\nEMPTY=\"\"\n";
        assert_eq!(
            os_release_value(text, "PRETTY_NAME").as_deref(),
            Some("Fedora Linux 45 (Workstation Edition)")
        );
        assert_eq!(os_release_value(text, "ID").as_deref(), Some("fedora"));
        assert_eq!(os_release_value(text, "QUOTED").as_deref(), Some("single"));
        assert_eq!(
            os_release_value(text, "ESCAPED").as_deref(),
            Some("say \"hi\"")
        );
        assert_eq!(os_release_value(text, "EMPTY"), None);
        assert_eq!(os_release_value(text, "VERSION"), None);
    }

    #[test]
    fn bios_dates() {
        let bios = |date: &str| Bios {
            date: Some(date.into()),
            ..Bios::default()
        };
        assert_eq!(bios("09/07/2026").ymd(), Some((2026, 9, 7)));
        assert_eq!(bios("13/07/2026").ymd(), None);
        assert_eq!(bios("2026-09-07").ymd(), None);
        assert_eq!(Bios::default().ymd(), None);
    }

    #[test]
    fn chip_statements() {
        let config = "# chip \"nct6687-*\"\nchip \"k10temp-*\"\n  label temp1 \"x\"\n";
        assert!(!names_chip(config, "nct6687"));
        assert!(names_chip("chip \"nct6687-isa-*\"\n", "nct6687"));
        assert!(names_chip(
            "chip \"it87-*\" \\\n  \"nct6687-*\"\n",
            "nct6687"
        ));
        assert!(names_chip("chip nct6687-isa-0a20\n", "nct6687"));
        assert!(!names_chip("chip \"nct6683-*\"\n", "nct6687"));
        assert!(!names_chip("label nct6687 \"x\"\n", "nct6687"));
    }

    #[test]
    fn smbios_placeholders() {
        assert_eq!(meaningful(" Unknown "), None);
        assert_eq!(meaningful("Not Specified"), None);
        assert_eq!(meaningful("To Be Filled By O.E.M."), None);
        assert_eq!(meaningful("DDR5").as_deref(), Some("DDR5"));
    }

    /// Rule of §5.1 and §5.4: numbers in names compare as numbers.
    #[test]
    fn natural_order_of_devices() {
        let mut names = vec![
            "sda", "nvme10n1", "nvme2n1", "nvme0n1", "nvme1n2", "nvme1n1", "sdb",
        ];
        names.sort_by(|a, b| natural_order(a, b));
        assert_eq!(
            names,
            [
                "nvme0n1", "nvme1n1", "nvme1n2", "nvme2n1", "nvme10n1", "sda", "sdb"
            ]
        );
        assert!(natural_order("nvme02", "nvme2").is_eq());
        assert!(natural_order("", "nvme0").is_lt());
    }
}
