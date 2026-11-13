// SPDX-License-Identifier: GPL-3.0-or-later

//! The Overview model (docs/PROJEKT.md §5.1) on the three fake sysfs trees:
//! grouping, order, hidden readings, voltage tolerances and names. The
//! readings come from raw sysfs; duty-read needs only the file, so the
//! trees are read in place.

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

use baseboard_core::fans::driver::{self, Mode};
use baseboard_core::labels::{self, UserLabels};
use baseboard_core::overview::{
    self, Board, Context, Group, GroupKind, Name, Rating, Row, Subtitle, Visibility,
};
use baseboard_core::profile::{self, FanRole, Profile, TempRole, VoltageRole};
use baseboard_core::sensors::{self, Chip, Kind, SensorSource, SysfsSource};
use baseboard_core::summary::{self, FanWatch, HeroKind};
use baseboard_core::system;
use baseboard_core::udev::Udev;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
const PROFILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/boards/ms-7e61.toml"
);
const TREES: [&str; 3] = [
    "sysfs-ms7e61-fedora-7.3.3",
    "sysfs-ms7e61-hwmon-next",
    "sysfs-ms7e61-v1",
];
const GPU_ENTRY: &str = "Navi 44 [Radeon RX 9050 / 9060 XT]";
const BOARD_NAME: &str = "MAG B850M MORTAR WIFI (MS-7E61)";
const CPU_MODEL: &str = "AMD Ryzen 7 9700X 8-Core Processor";

/// The udev database of this machine, for the graphics card only.
struct Database;

impl Udev for Database {
    fn property(&self, device: &Path, name: &str) -> Option<String> {
        (device.ends_with("0000:03:00.0") && name == "ID_MODEL_FROM_DATABASE")
            .then(|| GPU_ENTRY.to_owned())
    }
}

fn profile() -> Profile {
    profile::load(Path::new(PROFILE)).unwrap()
}

fn chips(tree: &str) -> Vec<Chip> {
    SysfsSource::new(&Path::new(FIXTURES).join(tree)).chips()
}

/// The model of a tree with the given profile (or none) and readings.
fn build(
    tree: &str,
    profile: Option<&Profile>,
    chips: &[Chip],
    sensors_labels: bool,
) -> Vec<Group> {
    build_with(tree, profile, chips, sensors_labels, &Database, None)
}

fn build_with(
    tree: &str,
    profile: Option<&Profile>,
    chips: &[Chip],
    sensors_labels: bool,
    udev: &dyn Udev,
    labels: Option<&UserLabels>,
) -> Vec<Group> {
    build_with_modes(tree, profile, chips, sensors_labels, udev, labels, &[])
}

fn build_with_modes(
    tree: &str,
    profile: Option<&Profile>,
    chips: &[Chip],
    sensors_labels: bool,
    udev: &dyn Udev,
    labels: Option<&UserLabels>,
    modes: &[(String, Mode)],
) -> Vec<Group> {
    let root = Path::new(FIXTURES).join(tree);
    let channels = profile
        .map(|p| driver::channels(p, &sensors::find(chips, "nct6687").unwrap().path))
        .unwrap_or_default();
    let context = Context {
        sysfs_root: &root,
        udev,
        board: profile.map(|profile| Board {
            profile,
            channels: &channels,
            modes,
            name: Some(BOARD_NAME.into()),
        }),
        cpu: Some(CPU_MODEL.into()),
        sensors_labels,
        labels,
    };
    overview::build(chips, &context)
}

fn model(tree: &str) -> Vec<Group> {
    build(tree, Some(&profile()), &chips(tree), false)
}

fn group(groups: &[Group], kind: GroupKind) -> &Group {
    groups.iter().find(|g| g.kind == kind).unwrap()
}

fn names(group: &Group) -> Vec<Name> {
    group.rows.iter().map(|r| r.name.clone()).collect()
}

fn values(group: &Group) -> Vec<f64> {
    group.rows.iter().map(|r| r.value).collect()
}

fn source(driver: &str, channel: &str) -> Subtitle {
    Subtitle::Source {
        driver: driver.into(),
        channel: channel.into(),
    }
}

#[test]
fn groups_in_page_order() {
    for tree in TREES {
        let groups = model(tree);
        let kinds: Vec<_> = groups.iter().map(|g| g.kind).collect();
        assert_eq!(
            kinds,
            [
                GroupKind::Cpu,
                GroupKind::Board,
                GroupKind::Fans,
                GroupKind::Voltages,
                GroupKind::Gpu,
                GroupKind::Disks,
                GroupKind::Memory,
                GroupKind::Other,
            ],
            "{tree}"
        );
        assert_eq!(
            group(&groups, GroupKind::Cpu).description.as_deref(),
            Some(CPU_MODEL)
        );
        assert_eq!(
            group(&groups, GroupKind::Board).description.as_deref(),
            Some(BOARD_NAME)
        );
        assert_eq!(
            group(&groups, GroupKind::Gpu).description.as_deref(),
            Some(GPU_ENTRY)
        );
        // Duty cycles are never rows of their own.
        assert!(
            groups
                .iter()
                .flat_map(|g| &g.rows)
                .all(|r| r.kind != Kind::Pwm)
        );
    }
}

#[test]
fn processor_and_board_temperatures() {
    for tree in TREES {
        let groups = model(tree);
        let cpu = group(&groups, GroupKind::Cpu);
        // k10temp first, then the board's sensor of the processor.
        assert_eq!(
            names(cpu),
            [
                Name::CpuTctl,
                Name::CpuChiplet(1),
                Name::Temp(TempRole::Cpu)
            ],
            "{tree}"
        );
        assert_eq!(values(cpu), [68.25, 69.125, 68.0]);
        assert_eq!(cpu.rows[0].subtitle, source("k10temp", "Tctl"));
        assert_eq!(cpu.rows[2].subtitle, source("nct6687", "temp1"));

        let board = group(&groups, GroupKind::Board);
        assert_eq!(
            names(board),
            [
                Name::Temp(TempRole::System),
                Name::Temp(TempRole::Vrm),
                Name::Temp(TempRole::Chipset)
            ]
        );
        assert_eq!(values(board), [37.5, 36.0, 37.0]);
        let subtitles: Vec<_> = board.rows.iter().map(|r| r.subtitle.clone()).collect();
        assert_eq!(
            subtitles,
            [
                source("nct6687", "temp2"),
                source("nct6687", "temp3"),
                source("nct6687", "temp4")
            ]
        );
    }
}

#[test]
fn fans_in_header_order_with_duty_where_readable() {
    for tree in TREES {
        let groups = model(tree);
        let fans = group(&groups, GroupKind::Fans);
        let fan = |role, number| Name::Fan { role, number };
        assert_eq!(
            names(fans),
            [
                fan(FanRole::Cpu, 1),
                fan(FanRole::Pump, 1),
                fan(FanRole::Sys, 1),
                fan(FanRole::Sys, 2),
                fan(FanRole::Sys, 3),
                fan(FanRole::Sys, 4),
                fan(FanRole::Aux, 1),
            ],
            "{tree}"
        );
        // fan1, fan2, fan16, fan15, fan14, fan13, fan4; fan3 is hidden.
        assert_eq!(values(fans), [890.0, 0.0, 948.0, 777.0, 753.0, 1013.0, 0.0]);
        let channels: Vec<_> = fans.rows.iter().map(|r| r.id.channel.as_str()).collect();
        assert_eq!(
            channels,
            ["fan1", "fan2", "fan16", "fan15", "fan14", "fan13", "fan4"]
        );
        let headers: Vec<_> = fans
            .rows
            .iter()
            .map(|r| match &r.subtitle {
                Subtitle::Header {
                    header,
                    duty,
                    verified,
                    mode: None,
                } => (header.as_str(), *duty, *verified),
                other => panic!("{other:?}"),
            })
            .collect();
        // SYS 1–4 have a duty cycle only where pwm13–16 exist.
        let sys = if tree.contains("fedora") {
            [None; 4]
        } else {
            [Some(98.0), Some(108.0), Some(76.0), Some(102.0)]
        };
        assert_eq!(
            headers,
            [
                ("CPU_FAN1", Some(124.0), true),
                ("PUMP_SYS1", Some(255.0), false),
                ("SYS_FAN1", sys[0], true),
                ("SYS_FAN2", sys[1], true),
                ("SYS_FAN3", sys[2], true),
                ("SYS_FAN4", sys[3], true),
                ("JAF_1", Some(153.0), false),
            ],
            "{tree}"
        );
    }
}

/// The mode shows where the channel has `mode` and the app read it
/// (§5.5.3): not without `pwmN_enable` (Fedora's driver, JAF_1 everywhere),
/// not for channels it did not read, not for keys outside the profile.
#[test]
fn fan_modes_where_the_channel_has_mode() {
    let read = [
        ("cpu1", Mode::Manual),
        ("sys1", Mode::Auto),
        ("sys4", Mode::Manual),
        ("ezconn", Mode::Auto),
        ("nowhere", Mode::Manual),
    ]
    .map(|(key, mode)| (key.to_owned(), mode));
    for tree in TREES {
        let profile = profile();
        let chips = chips(tree);
        let groups = build_with_modes(tree, Some(&profile), &chips, false, &Database, None, &read);
        let modes: Vec<_> = group(&groups, GroupKind::Fans)
            .rows
            .iter()
            .map(|r| match &r.subtitle {
                Subtitle::Header { mode, .. } => *mode,
                other => panic!("{other:?}"),
            })
            .collect();
        let expected = if tree.contains("fedora") {
            [None; 7]
        } else {
            // CPU, pump, SYS 1–4, JAF_1.
            [
                Some(Mode::Manual),
                None,
                Some(Mode::Auto),
                None,
                None,
                Some(Mode::Manual),
                None,
            ]
        };
        assert_eq!(modes, expected, "{tree}");
    }
}

#[test]
fn voltages_rated_from_the_profile() {
    for tree in TREES {
        let groups = model(tree);
        let voltages = group(&groups, GroupKind::Voltages);
        assert_eq!(
            names(voltages),
            [
                Name::Voltage(VoltageRole::P12v),
                Name::Voltage(VoltageRole::P5v),
                Name::Voltage(VoltageRole::P3v3),
                Name::Voltage(VoltageRole::Vcore),
                Name::Voltage(VoltageRole::Soc),
                Name::Voltage(VoltageRole::Vddio),
            ],
            "{tree}"
        );
        // Raw sysfs: the profile's multipliers (×12, ×5, ×2) apply once.
        assert_eq!(
            values(voltages),
            [1.008 * 12.0, 0.992 * 5.0, 3.312, 1.344, 1.2, 0.672 * 2.0]
        );
        let ratings: Vec<_> = voltages.rows.iter().map(|r| r.rating.unwrap()).collect();
        for (rating, (nominal, deviation)) in
            ratings.iter().zip([(12.0, 0.8), (5.0, -0.8), (3.3, 0.364)])
        {
            match rating {
                Rating::Rated {
                    nominal: n,
                    tolerance_pct,
                    deviation_pct,
                    within,
                } => {
                    assert_eq!((*n, *tolerance_pct, *within), (nominal, 5.0, true));
                    assert!((deviation_pct - deviation).abs() < 0.001, "{deviation_pct}");
                }
                Rating::Unrated => panic!("{nominal} V is rated"),
            }
        }
        assert_eq!(&ratings[3..], [Rating::Unrated; 3]);
    }
}

#[test]
fn tolerance_limits() {
    let tree = TREES[0];
    let rated = |raw: f64| {
        let mut chips = chips(tree);
        let nct = chips.iter_mut().find(|c| c.name == "nct6687").unwrap();
        nct.readings
            .iter_mut()
            .find(|r| r.kind == Kind::Voltage && r.index == 0)
            .unwrap()
            .value = raw;
        let groups = build(tree, Some(&profile()), &chips, false);
        group(&groups, GroupKind::Voltages).rows[0].rating.unwrap()
    };
    let within = |rating: Rating| match rating {
        Rating::Rated { within, .. } => within,
        Rating::Unrated => panic!(),
    };
    // 11.4 V is exactly −5 %; 11.28 V is −6 %; 12.72 V is +6 %.
    assert!(within(rated(0.95)));
    assert!(!within(rated(0.94)));
    assert!(!within(rated(1.06)));
}

#[test]
fn hidden_readings_and_the_rest_of_the_board() {
    for tree in TREES {
        let groups = model(tree);
        let ids: Vec<(String, String)> = groups
            .iter()
            .flat_map(|g| &g.rows)
            .filter(|r| r.id.chip == "nct6687")
            .map(|r| (r.id.chip.clone(), r.id.channel.clone()))
            .collect();
        for hidden in ["fan3", "temp5", "in5", "in6", "in7"] {
            assert!(!ids.iter().any(|(_, c)| c == hidden), "{tree} {hidden}");
        }
        // in9–in13 are not in the profile: under Other, as the auxiliary
        // voltages of the board. Names only where the meaning is certain
        // (VSB, VBAT); the subtitle gives the channel, not the label again.
        let other = group(&groups, GroupKind::Other);
        assert!(other.rows.is_empty(), "{tree}");
        let auxiliary: Vec<_> = other.auxiliary.iter().map(|r| r.name.clone()).collect();
        assert_eq!(
            auxiliary,
            [
                Name::StandbyVoltage,
                Name::Driver("AVSB".into()),
                Name::Driver("VTT".into()),
                Name::BatteryVoltage,
                Name::Driver("VREF".into())
            ]
        );
        let subtitles: Vec<_> = other.auxiliary.iter().map(|r| r.subtitle.clone()).collect();
        assert_eq!(
            subtitles,
            ["in9", "in10", "in11", "in12", "in13"].map(|c| source("nct6687", c))
        );
        let values: Vec<_> = other.auxiliary.iter().map(|r| r.value).collect();
        assert_eq!(values, [3.344, 3.328, 2.032, 3.088, 1.824]);
        // Only the board's device has auxiliary voltages.
        assert!(
            groups
                .iter()
                .all(|g| g.kind == GroupKind::Other || g.auxiliary.is_empty())
        );
    }
}

/// The range of a row is its minimum and maximum since the start, also in
/// the auxiliary voltages.
#[test]
fn ranges_since_the_start() {
    let tree = TREES[0];
    let profile = profile();
    let mut extremes = overview::Extremes::default();
    let mut sample = |cpu_fan: f64, vref: f64| {
        let mut chips = chips(tree);
        let nct = chips.iter_mut().find(|c| c.name == "nct6687").unwrap();
        for reading in &mut nct.readings {
            match (reading.kind, reading.index) {
                (Kind::Fan, 1) => reading.value = cpu_fan,
                (Kind::Voltage, 13) => reading.value = vref,
                _ => {}
            }
        }
        let mut groups = build(tree, Some(&profile), &chips, false);
        extremes.update(&mut groups);
        let fan = group(&groups, GroupKind::Fans).rows[0].range;
        let aux = group(&groups, GroupKind::Other).auxiliary[4].range;
        (fan, aux)
    };
    assert_eq!(
        sample(890.0, 1.824),
        (Some((890.0, 890.0)), Some((1.824, 1.824)))
    );
    assert_eq!(
        sample(870.0, 1.840),
        (Some((870.0, 890.0)), Some((1.824, 1.840)))
    );
    assert_eq!(
        sample(900.0, 1.832),
        (Some((870.0, 900.0)), Some((1.824, 1.840)))
    );
    // Before an update a row has no range.
    let groups = build(tree, Some(&profile), &chips(tree), false);
    assert!(
        groups
            .iter()
            .flat_map(|g| &g.rows)
            .all(|r| r.range.is_none())
    );
}

#[test]
fn graphics_card_disks_and_memory() {
    let groups = model(TREES[0]);
    let gpu = group(&groups, GroupKind::Gpu);
    assert_eq!(
        names(gpu),
        [
            Name::GpuChip,
            Name::GpuHotspot,
            Name::GpuMemory,
            Name::GpuFan,
            Name::GpuCoreVoltage,
            Name::GpuPower
        ]
    );
    assert_eq!(gpu.rows[0].subtitle, source("amdgpu", "edge"));
    assert_eq!(gpu.rows[5].subtitle, source("amdgpu", "PPT"));
    assert_eq!(gpu.description.as_deref(), Some(GPU_ENTRY));

    let disks = group(&groups, GroupKind::Disks);
    let first = |controller: &str| {
        disks
            .rows
            .iter()
            .find(|r| {
                r.subtitle
                    == Subtitle::Disk {
                        controller: controller.into(),
                    }
            })
            .unwrap()
            .name
            .clone()
    };
    assert_eq!(
        first("nvme0"),
        Name::Disk {
            model: "CT2000P3PSSD8".into(),
            sensor: None
        }
    );
    assert_eq!(
        first("nvme1"),
        Name::Disk {
            model: "WD_BLACK SN7100 2TB".into(),
            sensor: None
        }
    );
    // Without lm-sensors' `ignore`, the numbered sensors of a disk are in the
    // model too, by number; the page leaves them out by default (D42).
    let sensor = disks
        .rows
        .iter()
        .find(|r| {
            matches!(
                &r.name,
                Name::DiskSensor { model, number: 1 } if model == "CT2000P3PSSD8"
            )
        })
        .unwrap();
    assert!(overview::hidden_by_default(&sensor.id));

    let memory = group(&groups, GroupKind::Memory);
    assert_eq!(
        names(memory),
        [Name::MemoryModule(1), Name::MemoryModule(2)]
    );
    let addresses: Vec<_> = memory
        .rows
        .iter()
        .map(|r| match &r.subtitle {
            Subtitle::MemoryModule { address, .. } => *address,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(addresses, [0x51, 0x53]);
}

/// The memory slots of this machine in the udev database (DIMMA1–DIMMB2)
/// with modules in the slots given, and a serial number for each module.
/// The other devices as in [`Database`]. Records every property asked for.
struct Memory {
    filled: Vec<usize>,
    locators: [&'static str; 4],
    asked: std::cell::RefCell<Vec<String>>,
}

impl Memory {
    fn new(filled: &[usize], locators: [&'static str; 4]) -> Self {
        Self {
            filled: filled.to_vec(),
            locators,
            asked: Default::default(),
        }
    }
}

impl Udev for Memory {
    fn property(&self, device: &Path, name: &str) -> Option<String> {
        self.asked.borrow_mut().push(name.to_owned());
        if device != Path::new(system::DMI_DEVICE) {
            return Database.property(device, name);
        }
        let (index, field) = name.strip_prefix("MEMORY_DEVICE_")?.split_once('_')?;
        let index: usize = index.parse().ok()?;
        let locator = self.locators.get(index)?;
        let filled = self.filled.contains(&index);
        match field {
            "LOCATOR" => Some((*locator).to_owned()),
            "PRESENT" if !filled => Some("0".into()),
            "SIZE" if filled => Some("17179869184".into()),
            "SERIAL_NUMBER" if filled => Some(format!("FAKESERIAL{index}")),
            _ => None,
        }
    }
}

/// Memory sensors are named by their slot when the SPD addresses match the
/// filled slots one to one (§5.1); otherwise they are numbered.
#[test]
fn memory_modules_by_slot() {
    let tree = TREES[0];
    let slots = ["DIMMA1", "DIMMA2", "DIMMB1", "DIMMB2"];
    let names_with = |udev: &Memory| {
        let groups = build_with(tree, Some(&profile()), &chips(tree), false, udev, None);
        assert!(!format!("{groups:?}").contains("FAKESERIAL"));
        names(group(&groups, GroupKind::Memory))
    };
    let numbered = [Name::MemoryModule(1), Name::MemoryModule(2)];

    // This machine: modules in DIMMA2 and DIMMB2, SPD on 0x51 and 0x53.
    let this = Memory::new(&[1, 3], slots);
    assert_eq!(
        names_with(&this),
        [
            Name::MemorySlot("DIMMA2".into()),
            Name::MemorySlot("DIMMB2".into())
        ]
    );
    assert!(
        this.asked
            .borrow()
            .iter()
            .all(|name| !name.contains("SERIAL"))
    );
    // A third module without a sensor.
    assert_eq!(names_with(&Memory::new(&[1, 2, 3], slots)), numbered);
    // 0x53 points at an empty slot.
    assert_eq!(names_with(&Memory::new(&[1, 2], slots)), numbered);
    // Two slots with the same name.
    assert_eq!(
        names_with(&Memory::new(&[1, 3], ["A", "DIMM", "B", "DIMM"])),
        numbered
    );
    // No memory in the database: `graphics_card_disks_and_memory`.
}

/// The option "labels from lm-sensors" puts the name `sensors` shows
/// first, where libsensors gave one.
#[test]
fn labels_from_lm_sensors_when_the_option_is_on() {
    let tree = TREES[0];
    let mut chips = chips(tree);
    let mut label = |chip: &str, kind: Kind, index: u8, text: &str| {
        let chip = chips.iter_mut().find(|c| c.name == chip).unwrap();
        let reading = chip
            .readings
            .iter_mut()
            .find(|r| r.kind == kind && r.index == index)
            .unwrap();
        reading.sensors_label = Some(text.into());
    };
    label("nct6687", Kind::Temp, 1, "CPU");
    label("nct6687", Kind::Fan, 16, "SYS Fan 1");
    label("nct6687", Kind::Voltage, 0, "+12V");
    label("k10temp", Kind::Temp, 1, "CPU (Tctl)");
    let profile = profile();
    let on = build(tree, Some(&profile), &chips, true);
    let off = build(tree, Some(&profile), &chips, false);
    let sensors = |s: &str| Name::Sensors(s.into());

    let cpu = group(&on, GroupKind::Cpu);
    assert_eq!(
        names(cpu),
        [sensors("CPU (Tctl)"), Name::CpuChiplet(1), sensors("CPU")]
    );
    assert_eq!(
        group(&on, GroupKind::Fans).rows[2].name,
        sensors("SYS Fan 1")
    );
    assert_eq!(
        group(&on, GroupKind::Voltages).rows[0].name,
        sensors("+12V")
    );
    // Without labels from libsensors, the option changes nothing.
    assert_eq!(
        group(&on, GroupKind::Fans).rows[3].name,
        Name::Fan {
            role: FanRole::Sys,
            number: 2
        }
    );
    // Off: the default names.
    assert_eq!(
        names(group(&off, GroupKind::Cpu)),
        [
            Name::CpuTctl,
            Name::CpuChiplet(1),
            Name::Temp(TempRole::Cpu)
        ]
    );

    // The user's own names come before both (D22); switching the option
    // back and forth changes the other names only.
    let own = labels::parse(
        "schema = 1\n[fan]\nsys1 = \"Front\"\n[temp]\ncpu = \"Socket\"\n",
        &profile,
    )
    .unwrap();
    for sensors_labels in [true, false, true] {
        let groups = build_with(
            tree,
            Some(&profile),
            &chips,
            sensors_labels,
            &Database,
            Some(&own),
        );
        let fans = group(&groups, GroupKind::Fans);
        assert_eq!(fans.rows[2].name, Name::Label("Front".into()));
        assert_eq!(
            group(&groups, GroupKind::Cpu).rows[2].name,
            Name::Label("Socket".into())
        );
        let tctl = &group(&groups, GroupKind::Cpu).rows[0].name;
        if sensors_labels {
            assert_eq!(tctl, &sensors("CPU (Tctl)"));
        } else {
            assert_eq!(tctl, &Name::CpuTctl);
        }
    }
}

/// The Overview and its warnings go by what a row is, not by its name
/// (B1): with the user's own names, and with the names of lm-sensors, a
/// stopped CPU fan still warns (D65), "Board" keeps its number and the
/// disks are counted one each, by their composite temperature.
#[test]
fn summary_does_not_go_by_names() {
    let tree = "sysfs-ms7e61-v1";
    let profile = profile();
    let mut chips = chips(tree);
    for chip in &mut chips {
        for reading in &mut chip.readings {
            let label = match (chip.name.as_str(), reading.kind, reading.index) {
                ("nct6687", Kind::Fan, 1) => Some("CPU Fan"),
                ("nct6687", Kind::Temp, 2) => Some("System"),
                ("nvme", Kind::Temp, 1) => Some("Composite"),
                _ => None,
            };
            reading.sensors_label = label.map(str::to_owned);
        }
    }
    let own = labels::parse(
        "schema = 1\n[fan]\ncpu1 = \"Tower cooler\"\n[temp]\nsystem = \"Case\"\n",
        &profile,
    )
    .unwrap();
    for (labels, sensors_labels) in [(Some(&own), false), (None, true)] {
        let mut groups = build_with(
            tree,
            Some(&profile),
            &chips,
            sensors_labels,
            &Database,
            labels,
        );
        let top = summary::summary(&groups, &[]);
        assert!(
            top.heroes.iter().any(|h| h.kind == HeroKind::Board),
            "no number for the board with names {labels:?}, lm-sensors {sensors_labels}"
        );
        assert_eq!(top.disks.map(|d| d.count), Some(2));

        // The CPU fan turned earlier and stands for 11 s.
        let cpu_fan = group_mut(&mut groups, GroupKind::Fans)
            .rows
            .iter_mut()
            .find(|r| r.id.channel == "fan1")
            .unwrap();
        cpu_fan.value = 0.0;
        cpu_fan.range = Some((0.0, 890.0));
        let mut watch = FanWatch::default();
        let start = Instant::now();
        watch.update(&groups, start);
        let stopped = watch.update(&groups, start + Duration::from_secs(11));
        assert_eq!(
            stopped.len(),
            1,
            "names {labels:?}, lm-sensors {sensors_labels}"
        );
    }
}

fn group_mut(groups: &mut [Group], kind: GroupKind) -> &mut Group {
    groups.iter_mut().find(|g| g.kind == kind).unwrap()
}

/// Hidden readings and headers (§5.1): a row goes by its key, a group with
/// no rows left goes too, and the folded auxiliary voltages can be hidden
/// one by one. The keys do not depend on `hwmonN`.
#[test]
fn hidden_readings() {
    let tree = TREES[2];
    let all = model(tree);
    let key = |kind: GroupKind, index: usize| group(&all, kind).rows[index].id.key();
    let pump = key(GroupKind::Fans, 1);
    assert_eq!(pump, "nct6687/fan2@devices/platform/nct6683.2592");
    let other = group(&all, GroupKind::Other);
    let vsb = other.auxiliary[0].id.key();
    let memory: Vec<String> = group(&all, GroupKind::Memory)
        .rows
        .iter()
        .map(|r| r.id.key())
        .collect();
    assert_eq!(
        memory[0],
        "spd5118/temp1@devices/pci0000:00/0000:00:14.0/i2c-8/8-0051"
    );

    let hidden: HashSet<String> = [pump.clone(), vsb]
        .into_iter()
        .chain(memory.iter().cloned())
        .collect();
    let shown = overview::visible(
        &all,
        &Visibility {
            hidden,
            ..Default::default()
        },
    );
    let fans = group(&shown, GroupKind::Fans);
    assert_eq!(fans.rows.len(), group(&all, GroupKind::Fans).rows.len() - 1);
    assert!(fans.rows.iter().all(|r| r.id.key() != pump));
    assert_eq!(
        group(&shown, GroupKind::Other).auxiliary.len(),
        other.auxiliary.len() - 1
    );
    assert!(shown.iter().all(|g| g.kind != GroupKind::Memory));
    assert_eq!(shown.len(), all.len() - 1);
    // Nothing turned off and every row hidden by default turned on: the
    // same model.
    let every: HashSet<String> = rows(&all)
        .filter(|r| overview::hidden_by_default(&r.id))
        .map(|r| r.id.key())
        .collect();
    let all_on = Visibility {
        shown: every,
        ..Default::default()
    };
    assert_eq!(overview::visible(&all, &all_on), all);
}

/// Every row of the model, the folded ones too.
fn rows(groups: &[Group]) -> impl Iterator<Item = &Row> {
    groups
        .iter()
        .flat_map(|g| g.rows.iter().chain(&g.auxiliary))
}

/// D42: the numbered temperature sensors of an NVMe drive are off until the
/// user turns them on, the composite temperature is always on, and turning
/// a row off wins over turning it on. Natively and in Flatpak alike: the
/// rule needs no lm-sensors configuration (the trees are raw sysfs).
#[test]
fn disk_sensors_hidden_by_default() {
    let all = model(TREES[0]);
    let disks = group(&all, GroupKind::Disks);
    let (extra, composite): (Vec<&Row>, Vec<&Row>) = disks
        .rows
        .iter()
        .partition(|r| overview::hidden_by_default(&r.id));
    assert!(!extra.is_empty());
    assert!(
        extra
            .iter()
            .all(|r| matches!(r.name, Name::DiskSensor { .. }))
    );
    assert!(!composite.is_empty());
    assert!(composite.iter().all(|r| r.id.channel == "temp1"));

    // By default only the composite temperatures.
    let page = overview::visible(&all, &Visibility::default());
    assert_eq!(group(&page, GroupKind::Disks).rows.len(), composite.len());

    // One sensor turned on.
    let key = extra[0].id.key();
    let on = Visibility {
        shown: HashSet::from([key.clone()]),
        ..Default::default()
    };
    let page = overview::visible(&all, &on);
    let disks_on = &group(&page, GroupKind::Disks).rows;
    assert_eq!(disks_on.len(), composite.len() + 1);
    assert!(disks_on.iter().any(|r| r.id.key() == key));

    // Turned on and off: off wins.
    let both = Visibility {
        hidden: HashSet::from([key.clone()]),
        shown: HashSet::from([key]),
    };
    assert!(!both.shows(&extra[0].id));

    // No other device has rows hidden by default.
    for tree in TREES {
        assert!(
            rows(&model(tree))
                .filter(|r| overview::hidden_by_default(&r.id))
                .all(|r| r.id.chip == "nvme" && r.id.channel != "temp1")
        );
    }
}

#[test]
fn profile_label_replaces_the_role_name() {
    let tree = TREES[0];
    let mut profile = profile();
    profile.fans[2].label = Some("Front intake".into());
    let groups = build(tree, Some(&profile), &chips(tree), false);
    assert_eq!(
        group(&groups, GroupKind::Fans).rows[2].name,
        Name::Label("Front intake".into())
    );
}

/// Without a profile, the board's device is just another device: under
/// Other, named by the driver, nothing hidden, no duty cycles.
#[test]
fn without_a_profile() {
    let tree = TREES[1];
    let groups = build(tree, None, &chips(tree), false);
    let kinds: Vec<_> = groups.iter().map(|g| g.kind).collect();
    assert_eq!(
        kinds,
        [
            GroupKind::Cpu,
            GroupKind::Gpu,
            GroupKind::Disks,
            GroupKind::Memory,
            GroupKind::Other
        ]
    );
    let other = group(&groups, GroupKind::Other);
    assert!(other.rows.iter().all(|r| r.id.chip == "nct6687"));
    assert!(other.rows.iter().any(|r| r.id.channel == "temp5"));
    assert!(other.rows.iter().any(|r| r.id.channel == "fan3"));
    // Named by its channel: the subtitle does not say it again.
    assert_eq!(
        other
            .rows
            .iter()
            .find(|r| r.id.channel == "fan16")
            .unwrap()
            .subtitle,
        source("nct6687", "")
    );
    assert_eq!(
        other
            .rows
            .iter()
            .find(|r| r.id.channel == "temp1")
            .unwrap()
            .name,
        Name::Driver("AMD TSI Addr 98h".into())
    );
    // The processor group has only k10temp.
    assert_eq!(
        names(group(&groups, GroupKind::Cpu)),
        [Name::CpuTctl, Name::CpuChiplet(1)]
    );
}
