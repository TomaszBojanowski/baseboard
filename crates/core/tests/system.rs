// SPDX-License-Identifier: GPL-3.0-or-later

//! The data of the System page (docs/PROJEKT.md §5.4) and the text of
//! "Copy system information" on a fake system made from the readings of
//! MS-7E61: sysfs, /proc, /etc and a fake udev database. The fake also has
//! serial numbers and MAC addresses where the real system has them; none
//! may reach the model or the text (§1.3).

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use baseboard_core::board::{self, Detection, Dmi};
use baseboard_core::fans::driver::DriverLevel;
use baseboard_core::profile::{self, Profile};
use baseboard_core::report::Report;
use baseboard_core::system::{
    self, Bios, Disk, FanDriver, GraphicsCard, Manufacturer, MemoryModule, MemorySlot,
    ModuleOrigin, NetworkCard, Paths, PciId,
};
use baseboard_core::udev::{NoUdev, Udev};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
const PROFILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/boards/ms-7e61.toml"
);

/// What must never show up in the model.
const SECRETS: [&str; 6] = [
    "FAKE-BOARD-SERIAL",
    "FAKE-UUID",
    "FAKE-NVME-SERIAL",
    "FAKE-USB-SERIAL",
    "FAKE-DIMM-SERIAL",
    "02:00:5e:10:00:01",
];

const GPU: &str = "devices/pci0000:00/0000:00:01.1/0000:01:00.0/0000:02:00.0/0000:03:00.0";
const NIC: &str = "devices/pci0000:00/0000:00:02.1/0000:05:00.0/0000:06:0a.0/0000:0d:00.0";
const NVME: &str = "devices/pci0000:00/0000:00:01.2/0000:04:00.0";
const USB: &str = "devices/pci0000:00/0000:00:02.1/usb1/1-8/host6/target6:0:0/6:0:0:0";
const READER: &str = "devices/pci0000:00/0000:00:02.1/usb1/1-9/host7/target7:0:0/7:0:0:0";

/// A fake system in a temporary folder, removed at the end.
struct System(PathBuf);

impl System {
    fn new(test: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("baseboard-system-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root.canonicalize().unwrap())
    }

    fn file(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn dir(&self, path: &str) {
        std::fs::create_dir_all(self.0.join(path)).unwrap();
    }

    /// A symbolic link at `path` to `target`, both in the fake system.
    fn link(&self, path: &str, target: &str) {
        let path = self.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(self.0.join(target), path).unwrap();
    }

    fn paths(&self) -> Paths {
        Paths::under(&self.0)
    }

    /// MS-7E61 as read on 2026-11-11, with fake serial numbers and a fake
    /// MAC address added.
    fn ms7e61(test: &str) -> Self {
        let s = Self::new(test);
        // DMI: the BIOS and the files with serial numbers.
        for (name, value) in [
            ("board_vendor", "Micro-Star International Co., Ltd."),
            ("board_name", "MAG B850M MORTAR WIFI (MS-7E61)"),
            ("board_version", "1.0"),
            ("product_name", "MS-7E61"),
            ("bios_vendor", "American Megatrends International, LLC."),
            ("bios_version", "1.A68"),
            ("bios_date", "09/07/2026"),
            ("bios_release", "5.41"),
            ("board_serial", "FAKE-BOARD-SERIAL"),
            ("product_serial", "FAKE-BOARD-SERIAL"),
            ("product_uuid", "FAKE-UUID"),
        ] {
            s.file(&format!("sys/class/dmi/id/{name}"), &format!("{value}\n"));
        }

        // The graphics card, with a connector that is not a card.
        for (name, value) in [
            ("vendor", "0x1002"),
            ("device", "0x7590"),
            ("subsystem_vendor", "0x148c"),
            ("subsystem_device", "0x2437"),
            ("class", "0x030000"),
            ("vbios_version", "113-EXT118704-100"),
        ] {
            s.file(&format!("sys/{GPU}/{name}"), &format!("{value}\n"));
        }
        s.dir("sys/bus/pci/drivers/amdgpu");
        s.link(&format!("sys/{GPU}/driver"), "sys/bus/pci/drivers/amdgpu");
        s.dir(&format!("sys/{GPU}/drm/card1/card1-DP-1"));
        s.link(
            &format!("sys/{GPU}/drm/card1/device"),
            &format!("sys/{GPU}"),
        );
        s.link(
            &format!("sys/{GPU}/drm/card1/card1-DP-1/device"),
            &format!("sys/{GPU}/drm/card1"),
        );
        s.link("sys/class/drm/card1", &format!("sys/{GPU}/drm/card1"));
        s.link(
            "sys/class/drm/card1-DP-1",
            &format!("sys/{GPU}/drm/card1/card1-DP-1"),
        );

        // The Ethernet card, with its MAC address in sysfs.
        for (name, value) in [
            ("vendor", "0x10ec"),
            ("device", "0x8126"),
            ("class", "0x020000"),
        ] {
            s.file(&format!("sys/{NIC}/{name}"), &format!("{value}\n"));
        }
        s.file(
            &format!("sys/{NIC}/net/enp13s0/address"),
            "02:00:5e:10:00:01\n",
        );
        s.dir("sys/bus/pci/drivers/r8169");
        s.link(&format!("sys/{NIC}/driver"), "sys/bus/pci/drivers/r8169");

        // An NVMe disk.
        s.file(&format!("sys/{NVME}/class"), "0x010802\n");
        let controller = format!("sys/{NVME}/nvme/nvme0");
        s.file(
            &format!("{controller}/model"),
            "CT2000P3PSSD8                           \n",
        );
        s.file(&format!("{controller}/firmware_rev"), "P9CR40D \n");
        s.file(&format!("{controller}/serial"), "FAKE-NVME-SERIAL\n");
        s.file(&format!("{controller}/nvme0n1/size"), "3907029168\n");
        s.file(&format!("{controller}/nvme0n1/removable"), "0\n");
        s.link(&format!("{controller}/nvme0n1/device"), &controller);
        s.link("sys/block/nvme0n1", &format!("{controller}/nvme0n1"));

        // A USB stick and an empty card reader.
        for (device, block, size, model) in [
            (USB, "sda", "120164352", " SanDisk 3.2Gen1"),
            (READER, "sdb", "0", "Card Reader"),
        ] {
            s.file(&format!("sys/{device}/model"), &format!("{model}\n"));
            s.file(&format!("sys/{device}/rev"), "1.00\n");
            s.file(&format!("sys/{device}/vpd_pg80"), "FAKE-USB-SERIAL\n");
            s.file(
                &format!("sys/{device}/block/{block}/size"),
                &format!("{size}\n"),
            );
            s.file(&format!("sys/{device}/block/{block}/removable"), "1\n");
            s.link(
                &format!("sys/{device}/block/{block}/device"),
                &format!("sys/{device}"),
            );
            s.link(
                &format!("sys/block/{block}"),
                &format!("sys/{device}/block/{block}"),
            );
        }

        // zram: no hardware behind it.
        s.file("sys/devices/virtual/block/zram0/size", "16777216\n");
        s.link("sys/block/zram0", "sys/devices/virtual/block/zram0");

        // The PCI devices.
        for device in [GPU, NIC, NVME] {
            let name = device.rsplit('/').next().unwrap();
            s.link(
                &format!("sys/bus/pci/devices/{name}"),
                &format!("sys/{device}"),
            );
        }

        // cpufreq and the EC driver.
        for policy in [0, 1] {
            s.file(
                &format!("sys/devices/system/cpu/cpufreq/policy{policy}/scaling_driver"),
                "amd-pstate-epp\n",
            );
        }
        // The test module of series v1: out of the kernel's tree (D40).
        s.file(
            "proc/modules",
            "ath12k 663552 1 ath12k_wifi7, Live 0x0000000000000000\n\
             nct6683 53248 0 - Live 0x0000000000000000 (O)\n",
        );
        s.file(
            "sys/devices/platform/nct6683.2592/hwmon/hwmon0/name",
            "nct6687\n",
        );
        s.link(
            "sys/class/hwmon/hwmon0",
            "sys/devices/platform/nct6683.2592/hwmon/hwmon0",
        );

        // /proc: the processor, the kernel and some processes.
        s.file(
            "proc/cpuinfo",
            "processor\t: 0\nvendor_id\t: AuthenticAMD\n\
             model name\t: AMD Ryzen 7 9700X 8-Core Processor\n\n\
             processor\t: 1\nmodel name\t: AMD Ryzen 7 9700X 8-Core Processor\n",
        );
        s.file("proc/sys/kernel/osrelease", "7.3.3-300.fc45.x86_64\n");
        for (pid, comm) in [
            (1, "systemd"),
            (812, "coolercontrold"),
            (900, "fancontrol"),
            (901, "fancontrol"),
        ] {
            s.file(&format!("proc/{pid}/comm"), &format!("{comm}\n"));
        }
        s.file("proc/self/comm", "fancontrol\n");

        // /etc: the system's name and lm-sensors.
        s.file(
            "etc/os-release",
            "NAME=\"Fedora Linux\"\nVERSION_ID=45\n\
             PRETTY_NAME=\"Fedora Linux 45 (Workstation Edition)\"\n",
        );
        s.file("etc/sensors3.conf", "chip \"it87-*\" \"nct6775-*\"\n");
        s.file(
            "etc/sensors.d/msi-b850m-mortar.conf",
            "# MS-7E61\nchip \"nct6687-isa-*\"\n    label in0 \"+12V\"\n",
        );
        s.file("etc/sensors.d/local.conf", "chip \"k10temp-pci-*\"\n");
        s.file("etc/sensors.d/.hidden.conf", "chip \"nct6687-*\"\n");
        s
    }
}

impl Drop for System {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The udev database of MS-7E61 with serial numbers and a MAC address
/// added. Records every property asked for.
#[derive(Default)]
struct Database {
    asked: RefCell<Vec<String>>,
}

impl Udev for Database {
    fn property(&self, device: &Path, name: &str) -> Option<String> {
        self.asked.borrow_mut().push(name.to_owned());
        let device = device.to_str()?;
        let value = match (device, name) {
            (system::DMI_DEVICE, name) => return memory_property(name),
            (GPU, "ID_MODEL_FROM_DATABASE") => "Navi 44 [Radeon RX 9050 / 9060 XT]",
            (NIC, "ID_MODEL_FROM_DATABASE") => "RTL8126 5GbE Controller",
            (NIC, "ID_NET_NAME_MAC") => "enx02005e100001",
            (_, "ID_SERIAL" | "ID_SERIAL_SHORT") => "FAKE-NVME-SERIAL",
            _ => return None,
        };
        Some(value.to_owned())
    }
}

/// The memory properties of MS-7E61: modules in DIMMA2 and DIMMB2.
fn memory_property(name: &str) -> Option<String> {
    let (index, field) = name.strip_prefix("MEMORY_DEVICE_")?.split_once('_')?;
    let index: usize = index.parse().ok()?;
    let locator = ["DIMMA1", "DIMMA2", "DIMMB1", "DIMMB2"].get(index)?;
    let filled = index % 2 == 1;
    let value = match field {
        "LOCATOR" => locator,
        "PRESENT" if !filled => "0",
        "TYPE" | "MANUFACTURER" | "PART_NUMBER" if !filled => "Unknown",
        "SIZE" if filled => "17179869184",
        "TYPE" => "DDR5",
        "SPEED_MTS" | "CONFIGURED_SPEED_MTS" if filled => "6000",
        "MANUFACTURER" => "Unknown",
        "PART_NUMBER" => "IR-6000D564L30S/16G",
        "MODULE_MANUFACTURER_ID" if filled => "Bank 8, Hex 0x5D",
        "SERIAL_NUMBER" if filled => "FAKE-DIMM-SERIAL",
        _ => return None,
    };
    Some(value.to_owned())
}

fn profile() -> Profile {
    profile::load(Path::new(PROFILE)).unwrap()
}

#[test]
fn system_of_ms7e61() {
    let system = System::ms7e61("ms7e61");
    let info = system::read(&system.paths(), &Database::default(), Some(&profile()));

    assert_eq!(
        info.os.as_deref(),
        Some("Fedora Linux 45 (Workstation Edition)")
    );
    assert_eq!(info.kernel.as_deref(), Some("7.3.3-300.fc45.x86_64"));
    assert_eq!(
        info.bios,
        Bios {
            vendor: Some("American Megatrends International, LLC.".into()),
            version: Some("1.A68".into()),
            date: Some("09/07/2026".into()),
            release: Some("5.41".into()),
        }
    );
    assert_eq!(info.bios.ymd(), Some((2026, 9, 7)));
    assert_eq!(
        info.cpu.as_deref(),
        Some("AMD Ryzen 7 9700X 8-Core Processor")
    );
    assert_eq!(info.cpufreq_driver.as_deref(), Some("amd-pstate-epp"));

    let module = MemoryModule {
        size_bytes: 16 << 30,
        kind: Some("DDR5".into()),
        max_speed_mts: Some(6000),
        configured_speed_mts: Some(6000),
        part_number: Some("IR-6000D564L30S/16G".into()),
        manufacturer: Some(Manufacturer::JedecId("Bank 8, Hex 0x5D".into())),
    };
    let slot = |index: u8, locator: &str, module: Option<MemoryModule>| MemorySlot {
        index,
        locator: Some(locator.into()),
        module,
    };
    assert_eq!(
        info.memory,
        Some(vec![
            slot(0, "DIMMA1", None),
            slot(1, "DIMMA2", Some(module.clone())),
            slot(2, "DIMMB1", None),
            slot(3, "DIMMB2", Some(module)),
        ])
    );

    assert_eq!(
        info.graphics,
        [GraphicsCard {
            device: GPU.into(),
            name: Some("Navi 44 [Radeon RX 9050 / 9060 XT]".into()),
            pci_id: Some(PciId {
                vendor: 0x1002,
                device: 0x7590,
                subsystem: Some((0x148c, 0x2437)),
            }),
            driver: Some("amdgpu".into()),
            vbios: Some("113-EXT118704-100".into()),
        }]
    );
    assert_eq!(
        info.disks,
        [
            Disk {
                name: "nvme0n1".into(),
                model: Some("CT2000P3PSSD8".into()),
                firmware: Some("P9CR40D".into()),
                size_bytes: 3_907_029_168 * 512,
                removable: false,
            },
            Disk {
                name: "sda".into(),
                model: Some("SanDisk 3.2Gen1".into()),
                firmware: Some("1.00".into()),
                size_bytes: 120_164_352 * 512,
                removable: true,
            },
        ]
    );
    assert_eq!(
        info.network,
        [NetworkCard {
            device: NIC.into(),
            name: Some("RTL8126 5GbE Controller".into()),
            driver: Some("r8169".into()),
        }]
    );
    // The device has no pwm files here: no channel has `mode`.
    assert_eq!(
        info.fan_driver,
        FanDriver {
            loaded: true,
            origin: Some(ModuleOrigin::OutOfTree),
            device: true,
            level: Some(DriverLevel::ReadOnly),
        }
    );
    assert_eq!(
        info.sensors_config,
        Some(vec![system.0.join("etc/sensors.d/msi-b850m-mortar.conf")])
    );
    assert_eq!(
        info.fan_programs,
        Some(vec!["coolercontrold".into(), "fancontrol".into()])
    );
    assert_eq!(info.hwmon, ["nct6687"]);
}

/// A Flatpak sandbox (§10.3): the host's system name from /run/host, no
/// lm-sensors files without the host's /etc, and no verdict on the fan
/// programs, whose processes the sandbox cannot see.
#[test]
fn seen_from_a_sandbox() {
    let system = System::ms7e61("sandbox");
    let host = system.0.join("run/host");
    system.file("run/host/os-release", "PRETTY_NAME=\"Host Linux 1\"\n");
    let paths = Paths {
        os_release: vec![host.join("os-release")],
        etc: None,
        host_processes: false,
        ..system.paths()
    };
    let info = system::read(&paths, &NoUdev, Some(&profile()));
    assert_eq!(info.os.as_deref(), Some("Host Linux 1"));
    assert_eq!(info.sensors_config, None);
    assert_eq!(info.fan_programs, None);
    // The driver and its origin come from sysfs and /proc/modules, which
    // the sandbox sees.
    assert_eq!(info.fan_driver.origin, Some(ModuleOrigin::OutOfTree));

    // With the host's /etc shared: its lm-sensors files.
    let shared = Paths {
        etc: Some(system.0.join("etc")),
        ..paths
    };
    let info = system::read(&shared, &NoUdev, Some(&profile()));
    assert_eq!(info.sensors_config.map(|files| files.len()), Some(1));
}

/// The runtime named in `/.flatpak-info`, for "Copy system information".
#[test]
fn flatpak_runtime_names() {
    let info = "[Application]\nname=me.bojanowski.Baseboard\n\
                runtime=runtime/org.gnome.Platform/x86_64/51\n";
    assert_eq!(
        system::flatpak_runtime(info).as_deref(),
        Some("org.gnome.Platform 51")
    );
    assert_eq!(system::flatpak_runtime("[Runtime]\nname=x\n"), None);
    assert_eq!(system::flatpak_runtime("runtime=app/x/x86_64/1"), None);
}

/// Serial numbers and MAC addresses in sysfs and in the udev database never
/// reach the model, and the udev properties holding them are never asked
/// for.
#[test]
fn no_serial_numbers_or_mac_addresses() {
    let system = System::ms7e61("private");
    let database = Database::default();
    let info = system::read(&system.paths(), &database, Some(&profile()));
    let text = format!("{info:?}");
    for secret in SECRETS {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    let asked = database.asked.borrow();
    assert!(!asked.is_empty());
    for name in asked.iter() {
        assert!(
            !name.contains("SERIAL") && !name.contains("MAC"),
            "asked for {name}"
        );
    }
    assert!(system::MEMORY_FIELDS.iter().all(|f| !f.contains("SERIAL")));
    assert!(system::BIOS_FIELDS.iter().all(|f| !f.contains("serial")));
}

/// Without a profile there is no board device, level or lm-sensors file;
/// the driver still counts as loaded.
#[test]
fn without_a_profile() {
    let system = System::ms7e61("generic");
    let info = system::read(&system.paths(), &NoUdev, None);
    assert_eq!(
        info.fan_driver,
        FanDriver {
            loaded: true,
            origin: Some(ModuleOrigin::OutOfTree),
            device: false,
            level: None,
        }
    );
    assert_eq!(info.sensors_config, Some(Vec::new()));
    // Without the database: no memory and no names from it.
    assert_eq!(info.memory, None);
    assert_eq!(info.graphics[0].name, None);
    assert_eq!(info.network[0].name, None);
}

/// An empty system gives an empty model, without errors.
#[test]
fn empty_system() {
    let system = System::new("empty");
    let info = system::read(&system.paths(), &NoUdev, Some(&profile()));
    assert_eq!(info.os, None);
    assert_eq!(info.kernel, None);
    assert_eq!(info.bios, Bios::default());
    assert_eq!(info.cpu, None);
    assert_eq!(info.cpufreq_driver, None);
    assert_eq!(info.memory, None);
    assert!(info.graphics.is_empty() && info.disks.is_empty() && info.network.is_empty());
    assert_eq!(
        info.fan_driver,
        FanDriver {
            loaded: false,
            origin: None,
            device: false,
            level: None,
        }
    );
    assert_eq!(info.sensors_config, Some(Vec::new()));
    assert_eq!(info.fan_programs, Some(Vec::new()));
    assert!(info.hwmon.is_empty());
}

/// `os-release` from /usr/lib when /etc has none, and `NAME` without
/// `PRETTY_NAME`; `sensors.conf` when there is no `sensors3.conf`.
#[test]
fn fallback_files() {
    let system = System::new("fallback");
    system.file("usr/lib/os-release", "NAME='Some Linux'\n");
    system.file("etc/sensors.conf", "chip \"nct6687-*\"\n");
    assert_eq!(
        system::os_name(&system.paths()).as_deref(),
        Some("Some Linux")
    );
    assert_eq!(
        system::sensors_config(&system.0.join("etc"), "nct6687"),
        [system.0.join("etc/sensors.conf")]
    );
    system.file("etc/sensors3.conf", "chip \"k10temp-*\"\n");
    assert!(system::sensors_config(&system.0.join("etc"), "nct6687").is_empty());
}

/// Memory slots: a module named by SMBIOS, one with neither a name nor a
/// JEDEC identifier, a slot with size 0 and a slot without `PRESENT`.
#[test]
fn memory_fields() {
    struct Slots;
    impl Udev for Slots {
        fn property(&self, _: &Path, name: &str) -> Option<String> {
            let value = match name {
                "MEMORY_DEVICE_0_LOCATOR" => "DIMM 0",
                "MEMORY_DEVICE_0_SIZE" => "8589934592",
                "MEMORY_DEVICE_0_MANUFACTURER" => "Kingston",
                "MEMORY_DEVICE_0_PART_NUMBER" => "  KF560C30-16  ",
                "MEMORY_DEVICE_1_LOCATOR" => "DIMM 1",
                "MEMORY_DEVICE_1_SIZE" => "8589934592",
                "MEMORY_DEVICE_1_MANUFACTURER" => "Unknown",
                "MEMORY_DEVICE_1_TYPE" => "Unknown",
                "MEMORY_DEVICE_2_LOCATOR" => "DIMM 2",
                "MEMORY_DEVICE_2_SIZE" => "0",
                "MEMORY_DEVICE_3_LOCATOR" => "Not Specified",
                "MEMORY_DEVICE_3_PRESENT" => "0",
                _ => return None,
            };
            Some(value.to_owned())
        }
    }
    let slots = system::memory(&Slots).unwrap();
    assert_eq!(slots.len(), 4);
    let module = |i: usize| slots[i].module.clone();
    assert_eq!(
        module(0).and_then(|m| m.manufacturer),
        Some(Manufacturer::Name("Kingston".into()))
    );
    assert_eq!(
        module(0).and_then(|m| m.part_number).as_deref(),
        Some("KF560C30-16")
    );
    let unnamed = module(1).unwrap();
    assert_eq!((unnamed.manufacturer, unnamed.kind), (None, None));
    assert_eq!(module(2), None);
    assert_eq!((slots[3].locator.clone(), module(3)), (None, None));
}

/// The SPD sensors and the slots: names only when every sensor matches a
/// filled slot of its own on one bus.
#[test]
fn spd_slot_names() {
    let slot = |index: u8, locator: &str, filled: bool| MemorySlot {
        index,
        locator: Some(locator.into()),
        module: filled.then_some(MemoryModule {
            size_bytes: 1,
            kind: None,
            max_speed_mts: None,
            configured_speed_mts: None,
            part_number: None,
            manufacturer: None,
        }),
    };
    let slots = [
        slot(0, "DIMMA1", false),
        slot(1, "DIMMA2", true),
        slot(2, "DIMMB1", false),
        slot(3, "DIMMB2", true),
    ];
    let a = Path::new("devices/pci0000:00/0000:00:14.0/i2c-8/8-0051");
    let b = Path::new("devices/pci0000:00/0000:00:14.0/i2c-8/8-0053");
    let names = system::spd_slot_names(&[a, b], &slots);
    assert_eq!(names.get(a).map(String::as_str), Some("DIMMA2"));
    assert_eq!(names.get(b).map(String::as_str), Some("DIMMB2"));
    // One sensor is missing, or sits on another bus, or has no address.
    assert!(system::spd_slot_names(&[a], &slots).is_empty());
    let other_bus = Path::new("devices/pci0000:00/0000:00:14.0/i2c-9/9-0053");
    assert!(system::spd_slot_names(&[a, other_bus], &slots).is_empty());
    assert!(system::spd_slot_names(&[a, Path::new("devices/spd")], &slots).is_empty());
}

/// The fan driver on the fake trees of the three drivers (§5.5.2): the
/// device is found by its name and Fedora's driver has no `mode`. There is
/// no /proc/modules, as in a kernel without loadable modules: the driver is
/// built in.
#[test]
fn fan_driver_on_the_fake_trees() {
    let tree = Path::new(FIXTURES).join("sysfs-ms7e61-fedora-7.3.3");
    assert_eq!(
        system::fan_driver(&tree, &tree.join("proc"), Some(&profile())),
        FanDriver {
            loaded: true,
            origin: Some(ModuleOrigin::InTree),
            device: true,
            level: Some(DriverLevel::ReadOnly),
        }
    );
}

/// The module's origin from its taint flags in /proc/modules, read each
/// time (D40): `O` is out of the kernel's tree, also with other flags; a
/// line without flags is in-tree, and so is a driver that is not listed
/// while its device is there (built in). An unreadable list leaves the
/// origin unknown rather than guessed. The text of "Copy system
/// information" says which.
#[test]
fn module_origin() {
    let system = System::ms7e61("origin");
    let sys = system.0.join("sys");
    let proc = system.0.join("proc");
    let driver = || system::fan_driver(&sys, &proc, Some(&profile()));
    let origin = || driver().origin;
    let nct6683 = |text: &str| {
        let report = report(&system, &NoUdev, &[profile()]);
        assert!(report.contains(&format!("  nct6683: {text}\n")), "{report}");
    };
    let modules = |text: &str| system.file("proc/modules", text);
    assert_eq!(origin(), Some(ModuleOrigin::OutOfTree));
    nct6683("loaded (out-of-tree)");

    modules("nct6683 53248 0 - Live 0x0000000000000000 (OE)\n");
    assert_eq!(origin(), Some(ModuleOrigin::OutOfTree));

    // Replaced by the kernel's own module while the app runs.
    modules("nct6683 49152 0 - Live 0x0000000000000000\n");
    assert_eq!(origin(), Some(ModuleOrigin::InTree));
    nct6683("loaded (in-tree)");

    // Another module's flags do not count.
    modules("nct6683x 1 0 - Live 0x0 (O)\nnct6683 49152 0 - Live 0x0\n");
    assert_eq!(origin(), Some(ModuleOrigin::InTree));

    // Not listed, device there: built into the kernel.
    modules("ath12k 663552 1 ath12k_wifi7, Live 0x0000000000000000\n");
    assert_eq!(origin(), Some(ModuleOrigin::InTree));

    // Not listed and no device: not loaded.
    let link = sys.join("class/hwmon/hwmon0");
    let target = std::fs::read_link(&link).unwrap();
    std::fs::remove_file(&link).unwrap();
    assert!(!driver().loaded);
    assert_eq!(origin(), None);
    std::os::unix::fs::symlink(target, &link).unwrap();

    // The list cannot be read: loaded (the device is there), origin unknown.
    std::fs::remove_file(proc.join("modules")).unwrap();
    std::fs::create_dir(proc.join("modules")).unwrap();
    assert!(driver().loaded);
    assert_eq!(origin(), None);
    nct6683("loaded (origin unknown)");
}

/// The text of "Copy system information" for the fake MS-7E61, in full.
const REPORT_MS7E61: &str = "\
Baseboard 0.1.0

System
  OS: Fedora Linux 45 (Workstation Edition)
  Kernel: 7.3.3-300.fc45.x86_64
  Installation: native

Board
  Vendor: Micro-Star International Co., Ltd.
  Model: MAG B850M MORTAR WIFI (MS-7E61)
  Revision: 1.0
  Board code: MS-7E61
  PCB revision: 1
  Profile: ms-7e61

BIOS
  Vendor: American Megatrends International, LLC.
  Version: 1.A68
  Date: 09/07/2026
  Release: 5.41

CPU
  Model: AMD Ryzen 7 9700X
  cpufreq driver: amd-pstate-epp

Memory
  DIMMA1: empty
  DIMMA2: 16 GiB DDR5; 6000 MT/s; part IR-6000D564L30S/16G; maker JEDEC Bank 8, Hex 0x5D
  DIMMB1: empty
  DIMMB2: 16 GiB DDR5; 6000 MT/s; part IR-6000D564L30S/16G; maker JEDEC Bank 8, Hex 0x5D

Graphics
  Navi 44 [Radeon RX 9050 / 9060 XT], PCI 1002:7590 (subsystem 148c:2437), VBIOS 113-EXT118704-100

Disks
  CT2000P3PSSD8, 2.0 TB, firmware P9CR40D

Network
  RTL8126 5GbE Controller, driver r8169

Sensors
  hwmon devices: nct6687
  nct6683: loaded (out-of-tree)
  Fan control: read-only
";

/// The text for a system with the board detected from its DMI files.
fn report(system: &System, udev: &dyn Udev, profiles: &[Profile]) -> String {
    let dmi = Dmi::read(&system.0.join("sys"));
    let detection = board::detect(&dmi, profiles);
    let info = system::read(&system.paths(), udev, detection.profile());
    report_of(&dmi, &detection, &info)
}

fn report_of(dmi: &Dmi, detection: &Detection, info: &system::SystemInfo) -> String {
    Report {
        version: "0.1.0",
        installation: &system::Installation::Native,
        dmi,
        detection,
        info,
    }
    .text()
}

/// The way of installation in the text: native, or Flatpak with its
/// runtime (§10.3).
#[test]
fn report_names_the_installation() {
    let system = System::ms7e61("installation");
    let dmi = Dmi::read(&system.0.join("sys"));
    let detection = board::detect(&dmi, &[profile()]);
    let info = system::read(&system.paths(), &NoUdev, detection.profile());
    let text = |installation| {
        Report {
            version: "0.1.0",
            installation: &installation,
            dmi: &dmi,
            detection: &detection,
            info: &info,
        }
        .text()
    };
    let flatpak = text(system::Installation::Flatpak {
        runtime: Some("org.gnome.Platform 51".into()),
    });
    assert!(flatpak.contains("  Installation: Flatpak (org.gnome.Platform 51)\n"));
    let unnamed = text(system::Installation::Flatpak { runtime: None });
    assert!(unnamed.contains("  Installation: Flatpak\n"));
}

/// The full text, the pattern for the format. The USB stick, the running
/// fan programs, serial numbers and the MAC address are not in it.
#[test]
fn report_of_ms7e61() {
    let system = System::ms7e61("report");
    let text = report(&system, &Database::default(), &[profile()]);
    assert_eq!(text, REPORT_MS7E61);
    for absent in SECRETS
        .iter()
        .chain(&["SanDisk", "coolercontrold", "fancontrol"])
    {
        assert!(!text.contains(absent), "{absent}");
    }
}

/// Sections without data say so; nothing is left out.
#[test]
fn report_without_data() {
    let system = System::new("report-empty");
    let text = report(&system, &NoUdev, &[profile()]);
    assert_eq!(
        text,
        "\
Baseboard 0.1.0

System
  OS: unknown
  Kernel: unknown
  Installation: native

Board
  Vendor: unknown
  Model: unknown
  Revision: unknown
  Board code: unknown
  PCB revision: unknown
  Profile: none (generic mode)

BIOS
  Vendor: unknown
  Version: unknown
  Date: unknown
  Release: unknown

CPU
  Model: unknown
  cpufreq driver: unknown

Memory
  no data

Graphics
  none

Disks
  none

Network
  none

Sensors
  hwmon devices: none
  nct6683: not loaded
  Fan control: no profile
"
    );
}

/// A virtual machine: DMI, but no hwmon devices and no EC driver. The
/// System page still has the board and the BIOS (§7, M1 step 12).
#[test]
fn dmi_without_hwmon() {
    let system = System::new("vm");
    for (name, value) in [
        ("board_vendor", "Example Vendor"),
        ("board_name", "Example Board"),
        ("bios_vendor", "Example BIOS"),
        ("bios_version", "1.16.3"),
    ] {
        system.file(&format!("sys/class/dmi/id/{name}"), &format!("{value}\n"));
    }
    let info = system::read(&system.paths(), &NoUdev, None);
    assert_eq!(info.bios.version.as_deref(), Some("1.16.3"));
    assert!(info.hwmon.is_empty());
    assert!(!info.fan_driver.loaded);
    let text = report(&system, &NoUdev, &[profile()]);
    for line in [
        "  Vendor: Example Vendor\n",
        "  Model: Example Board\n",
        "  Profile: none (generic mode)\n",
        "  hwmon devices: none\n",
        "  nct6683: not loaded\n",
    ] {
        assert!(text.contains(line), "{line}");
    }
}

/// The profile line for a revision that is not verified, for another model
/// with the same code, and the driver line without the board's device.
#[test]
fn report_profile_lines() {
    let line = |text: &str, name: &str| {
        text.lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix(&format!("{name}: "))
                    .map(str::to_owned)
            })
            .unwrap()
    };
    let system = System::ms7e61("report-lines");
    system.file("sys/class/dmi/id/board_version", "2.0\n");
    let text = report(&system, &NoUdev, &[profile()]);
    assert_eq!(
        line(&text, "Profile"),
        "ms-7e61, PCB revision 2 not verified (read only)"
    );

    system.file(
        "sys/class/dmi/id/board_name",
        "B850 GAMING PLUS WIFI (MS-7E61)\n",
    );
    let text = report(&system, &NoUdev, &[profile()]);
    assert_eq!(
        line(&text, "Profile"),
        "none, the profiles for MS-7E61 are for other models (generic mode)"
    );
    assert_eq!(line(&text, "Fan control"), "no profile");

    let dmi = Dmi::read(&system.0.join("sys"));
    let info = system::SystemInfo {
        fan_driver: FanDriver {
            loaded: true,
            origin: Some(ModuleOrigin::InTree),
            device: false,
            level: None,
        },
        ..Default::default()
    };
    let text = report_of(&dmi, &Detection::Supported(profile()), &info);
    assert_eq!(
        line(&text, "nct6683"),
        "loaded (in-tree), board device not found"
    );
    assert_eq!(line(&text, "Fan control"), "board device not found");
}
