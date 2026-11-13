// SPDX-License-Identifier: GPL-3.0-or-later

//! The model of the Overview page (docs/PROJEKT.md §5.1): groups, rows,
//! names, subtitles and the rating of voltages. It has no user-visible
//! text: names are roles and keys, which the app turns into translated
//! text with gettext. Device facts that the readings do not carry come
//! from sysfs attributes and from the udev database ([`Udev`]).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::fans::driver::{Capability, Channel, Mode};
use crate::labels::UserLabels;
use crate::profile::{FanRole, Profile, TempRole, Voltage, VoltageRole};
use crate::sensors::{self, Chip, Kind, Reading};
use crate::system::{self, attribute};
use crate::udev::Udev;

/// The groups, in the order the page lists them in one column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKind {
    Cpu,
    Board,
    Fans,
    Voltages,
    Gpu,
    Disks,
    Memory,
    Other,
}

/// One group of rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub kind: GroupKind,
    /// What the group is about, not translated: the processor's model, the
    /// board's name, or the graphics card's name as sysfs or the database
    /// give it (§5.1). The app shortens the first and the last.
    pub description: Option<String>,
    pub rows: Vec<Row>,
    /// Only in Other: the voltages of the board's device that the profile
    /// does not name (VSB, AVSB, VTT, VBAT, VREF on MS-7E61), shown folded
    /// as "Auxiliary voltages".
    pub auxiliary: Vec<Row>,
}

/// The minimum and maximum of every row since the start (§5.1), kept
/// across refreshes by [`RowId`].
#[derive(Debug, Clone, Default)]
pub struct Extremes(HashMap<RowId, (f64, f64)>);

impl Extremes {
    /// Takes the values of `groups` in and sets the `range` of every row.
    /// A value that could not be read (NaN) leaves the range as it was.
    pub fn update(&mut self, groups: &mut [Group]) {
        let rows = groups
            .iter_mut()
            .flat_map(|g| g.rows.iter_mut().chain(g.auxiliary.iter_mut()));
        for row in rows {
            if !row.value.is_finite() {
                row.range = self.0.get(&row.id).copied();
                continue;
            }
            let range = self
                .0
                .entry(row.id.clone())
                .or_insert((row.value, row.value));
            range.0 = range.0.min(row.value);
            range.1 = range.1.max(row.value);
            row.range = Some(*range);
        }
    }
}

/// Identifies a row across refreshes, e.g. for its minimum and maximum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RowId {
    /// The `name` of the hwmon device.
    pub chip: String,
    pub device: Option<PathBuf>,
    /// The sysfs channel, e.g. `fan16`.
    pub channel: String,
}

impl RowId {
    /// The row as text for the hidden readings in the settings: chip,
    /// channel and device, e.g. `nct6687/fan16@devices/platform/nct6683.2592`.
    /// It holds across restarts, as the device path does, unlike `hwmonN`.
    pub fn key(&self) -> String {
        match &self.device {
            Some(device) => format!("{}/{}@{}", self.chip, self.channel, device.display()),
            None => format!("{}/{}", self.chip, self.channel),
        }
    }
}

/// What the user turned off or on in the settings (§5.1), by
/// [`RowId::key`]: rows turned off, and rows hidden by default
/// ([`hidden_by_default`]) that were turned on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visibility {
    pub hidden: HashSet<String>,
    pub shown: HashSet<String>,
}

impl Visibility {
    /// Whether the row with `id` is on the page.
    pub fn shows(&self, id: &RowId) -> bool {
        let key = id.key();
        !self.hidden.contains(&key) && (!hidden_by_default(id) || self.shown.contains(&key))
    }
}

/// Rows left off the page until the user turns them on (D42): the
/// temperature sensors 1–8 of an NVMe drive, `temp2`–`temp9` of the `nvme`
/// driver. The driver reads them from the drive's SMART log without saying
/// where they are, and the drives do not set thresholds for them. The
/// composite temperature, `temp1`, is the one the drive checks against its
/// own limits; it is always shown.
pub fn hidden_by_default(id: &RowId) -> bool {
    id.chip == "nvme"
        && id
            .channel
            .strip_prefix("temp")
            .and_then(|n| n.parse::<u8>().ok())
            .is_some_and(|n| (2..=9).contains(&n))
}

/// The groups with only the rows `visibility` shows, and without groups
/// left empty (§5.1: hidden readings and headers).
pub fn visible(groups: &[Group], visibility: &Visibility) -> Vec<Group> {
    let shown = |rows: &[Row]| -> Vec<Row> {
        rows.iter()
            .filter(|row| visibility.shows(&row.id))
            .cloned()
            .collect()
    };
    groups
        .iter()
        .map(|group| Group {
            rows: shown(&group.rows),
            auxiliary: shown(&group.auxiliary),
            ..group.clone()
        })
        .filter(|group| !group.rows.is_empty() || !group.auxiliary.is_empty())
        .collect()
}

/// One reading on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: RowId,
    pub name: Name,
    /// What the row is: the built-in name, before the user's custom names,
    /// the profile's labels and the labels of lm-sensors replace `name`.
    /// The Overview and its warnings go by it, never by `name` (B1).
    pub role: Name,
    pub subtitle: Subtitle,
    /// The kind of the value, which gives its unit.
    pub kind: Kind,
    /// In the unit of `kind`. For a voltage of the profile, volts on the
    /// rail, scaled exactly once (§5.1).
    pub value: f64,
    /// Only for the voltages of the profile.
    pub rating: Option<Rating>,
    /// The minimum and maximum since the start, in the unit of `value`;
    /// set by [`Extremes::update`].
    pub range: Option<(f64, f64)>,
}

/// Where the name of a row comes from (§5.1). The app makes the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
    /// A fan of the profile: role and number, e.g. "System fan 1".
    Fan { role: FanRole, number: u8 },
    /// A temperature of the profile.
    Temp(TempRole),
    /// A voltage of the profile.
    Voltage(VoltageRole),
    /// The profile's `label`, not translated.
    Label(String),
    /// k10temp `Tctl`.
    CpuTctl,
    /// k10temp `TccdN`.
    CpuChiplet(u8),
    /// amdgpu `edge`.
    GpuChip,
    /// amdgpu `junction`.
    GpuHotspot,
    /// amdgpu `mem`.
    GpuMemory,
    /// amdgpu `vddgfx`.
    GpuCoreVoltage,
    /// amdgpu `PPT`.
    GpuPower,
    /// amdgpu `fan1`.
    GpuFan,
    /// The board's standby rail, `VSB` of the `nct6683` driver.
    StandbyVoltage,
    /// The board's backup battery, `VBAT` of the `nct6683` driver.
    BatteryVoltage,
    /// A disk: its model from sysfs, not translated, and the driver's label
    /// of a sensor other than the composite temperature and the numbered
    /// ones.
    Disk {
        model: String,
        sensor: Option<String>,
    },
    /// A numbered temperature sensor of a disk, "Sensor 1" of the `nvme`
    /// driver: the model and the number (D42).
    DiskSensor { model: String, number: u8 },
    /// A memory module by its slot on the board, e.g. `DIMMA2`, from the
    /// udev database; not translated.
    MemorySlot(String),
    /// A memory module without a slot name, numbered from 1.
    MemoryModule(u8),
    /// A device under a wireless interface.
    WifiCard,
    /// A device under an Ethernet PHY.
    NetworkCard,
    /// The name `sensors` shows (option "labels from lm-sensors").
    Sensors(String),
    /// The driver's label, or else the channel name such as `temp1`.
    Driver(String),
}

/// The second line of a row. The app makes the text.
#[derive(Debug, Clone, PartialEq)]
pub enum Subtitle {
    /// A fan header of the board; its duty cycle (0–255) where the driver
    /// reports it (`duty-read`); whether the mapping was checked; its mode
    /// where the channel has `mode` and the app read it (§5.5.3).
    Header {
        header: String,
        duty: Option<f64>,
        verified: bool,
        mode: Option<Mode>,
    },
    /// The driver ([`chip_label`]) and the channel, e.g. `amdgpu` and
    /// `edge`; an empty channel when the name already is the channel.
    Source { driver: String, channel: String },
    /// A memory module on its SMBus address.
    MemoryModule { driver: String, address: u16 },
    /// A disk's controller, e.g. `nvme0`.
    Disk { controller: String },
}

/// How a voltage of the profile compares with its tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rating {
    /// The profile gives no tolerance for this rail.
    Unrated,
    Rated {
        nominal: f64,
        tolerance_pct: f64,
        /// (value − nominal) / nominal, in percent.
        deviation_pct: f64,
        within: bool,
    },
}

/// The board, when a profile matches it.
pub struct Board<'a> {
    pub profile: &'a Profile,
    /// What the driver allows on the profile's fan channels
    /// (`core::fans::driver`); here `duty-read` and `mode` count.
    pub channels: &'a [Channel],
    /// The modes read in this sample, by the channel's key; empty when the
    /// app does not read them. Only channels with `mode` show theirs.
    pub modes: &'a [(String, Mode)],
    /// The board's name for its group, e.g. `board_name` from DMI.
    pub name: Option<String>,
}

/// Everything the model needs besides the readings.
pub struct Context<'a> {
    /// The sysfs root, for device attributes such as `product_name`.
    pub sysfs_root: &'a Path,
    pub udev: &'a dyn Udev,
    pub board: Option<Board<'a>>,
    /// The processor's model for its group: `model name` from
    /// `/proc/cpuinfo` (`system::cpu_model`).
    pub cpu: Option<String>,
    /// The option "labels from lm-sensors" (§5.1): the name `sensors`
    /// shows comes first.
    pub sensors_labels: bool,
    /// The user's custom names for the readings of the board (D22); they come
    /// before everything else.
    pub labels: Option<&'a UserLabels>,
}

/// The groups of the page, in order, without empty ones. Readings that
/// libsensors ignores are already gone from `chips`; the ones the profile
/// hides are left out here.
pub fn build(chips: &[Chip], context: &Context) -> Vec<Group> {
    let board_path = context.board.as_ref().and_then(|board| {
        sensors::find(chips, &board.profile.hwmon.name).map(|chip| chip.path.clone())
    });
    let slots = memory_slots(chips, context.udev);
    let mut rows = Rows::default();
    let mut modules = 0;
    for chip in chips {
        if let Some(board) = &context.board
            && board_path.as_ref() == Some(&chip.path)
        {
            board_rows(chip, board, context, &mut rows);
            continue;
        }
        match chip.name.as_str() {
            "k10temp" => rows.cpu.extend(generic_rows(chip, context, None)),
            "amdgpu" => rows.gpus.push(Group {
                kind: GroupKind::Gpu,
                description: gpu_name(chip, context),
                rows: generic_rows(chip, context, None),
                auxiliary: Vec::new(),
            }),
            "nvme" => rows.disks.extend(generic_rows(chip, context, None)),
            "spd5118" => {
                modules += 1;
                let name = chip
                    .device
                    .as_deref()
                    .and_then(|device| slots.get(device))
                    .map_or(Name::MemoryModule(modules), |slot| {
                        Name::MemorySlot(slot.clone())
                    });
                rows.memory.extend(generic_rows(chip, context, Some(&name)));
            }
            _ => rows.other.extend(generic_rows(chip, context, None)),
        }
    }
    // Disks by controller number, as on the System page: nvme2 before
    // nvme10, whatever their PCI paths (§5.1). Stable, so the sensors of a
    // disk keep their order.
    rows.disks.sort_by(|a, b| {
        let controller = |row: &Row| match &row.subtitle {
            Subtitle::Disk { controller } => controller.clone(),
            _ => String::new(),
        };
        system::natural_order(&controller(a), &controller(b))
    });
    rows.into_groups(
        context.cpu.clone(),
        context.board.as_ref().and_then(|b| b.name.clone()),
    )
}

/// The slot name of every memory sensor's device, when the udev database
/// makes it unambiguous (`system::spd_slot_names`); else none.
fn memory_slots(chips: &[Chip], udev: &dyn Udev) -> HashMap<PathBuf, String> {
    let devices: Option<Vec<&Path>> = chips
        .iter()
        .filter(|chip| chip.name == "spd5118")
        .map(|chip| chip.device.as_deref())
        .collect();
    match devices {
        Some(devices) if !devices.is_empty() => system::memory(udev)
            .map(|slots| system::spd_slot_names(&devices, &slots))
            .unwrap_or_default(),
        _ => HashMap::new(),
    }
}

/// The part of a hardware database entry the app shows (D23): the name in
/// square brackets, or else the whole entry.
pub fn marketing_name(entry: &str) -> &str {
    entry
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(name, _)| name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or(entry)
}

#[derive(Default)]
struct Rows {
    cpu: Vec<Row>,
    board_cpu: Vec<Row>,
    board: Vec<Row>,
    fans: Vec<Row>,
    voltages: Vec<Row>,
    gpus: Vec<Group>,
    disks: Vec<Row>,
    memory: Vec<Row>,
    other: Vec<Row>,
    board_other: Vec<Row>,
    auxiliary: Vec<Row>,
}

impl Rows {
    fn into_groups(self, cpu: Option<String>, board_name: Option<String>) -> Vec<Group> {
        let group = |kind, description, rows| Group {
            kind,
            description,
            rows,
            auxiliary: Vec::new(),
        };
        let cpu_name = cpu;
        let mut cpu = self.cpu;
        cpu.extend(self.board_cpu);
        let mut other = self.other;
        other.extend(self.board_other);
        let mut groups = vec![
            group(GroupKind::Cpu, cpu_name, cpu),
            group(GroupKind::Board, board_name, self.board),
            group(GroupKind::Fans, None, self.fans),
            group(GroupKind::Voltages, None, self.voltages),
        ];
        groups.extend(self.gpus);
        groups.extend([
            group(GroupKind::Disks, None, self.disks),
            group(GroupKind::Memory, None, self.memory),
            Group {
                auxiliary: self.auxiliary,
                ..group(GroupKind::Other, None, other)
            },
        ]);
        groups.retain(|g| !g.rows.is_empty() || !g.auxiliary.is_empty());
        groups
    }
}

/// The rows of the board's device: fans in header order, temperatures and
/// voltages by role, and the rest of the device under Other.
fn board_rows(chip: &Chip, board: &Board, context: &Context, out: &mut Rows) {
    let profile = board.profile;
    let mut used = HashSet::new();
    for fan in &profile.fans {
        used.insert((Kind::Fan, fan.fan_index));
        if let Some(pwm) = fan.pwm_index {
            used.insert((Kind::Pwm, pwm));
        }
        let Some(reading) = chip.reading(Kind::Fan, fan.fan_index) else {
            continue;
        };
        let has = |capability| {
            board
                .channels
                .iter()
                .any(|c| c.key == fan.key && c.capabilities.contains(capability))
        };
        let duty = fan
            .pwm_index
            .filter(|_| has(Capability::DutyRead))
            .and_then(|pwm| chip.reading(Kind::Pwm, pwm))
            .map(|r| r.value);
        let mode = board
            .modes
            .iter()
            .find(|(key, _)| *key == fan.key)
            .map(|&(_, mode)| mode)
            .filter(|_| has(Capability::Mode));
        let role = Name::Fan {
            role: fan.role,
            number: fan.number,
        };
        out.fans.push(Row {
            id: row_id(chip, reading),
            name: pick_custom(
                reading,
                context,
                |l| l.fan(&fan.key),
                || labelled(&fan.label).unwrap_or_else(|| role.clone()),
            ),
            role,
            subtitle: Subtitle::Header {
                header: fan.header.clone(),
                duty,
                verified: fan.verified,
                mode,
            },
            kind: Kind::Fan,
            value: reading.value,
            rating: None,
            range: None,
        });
    }

    let mut temps: Vec<_> = profile.temps.iter().collect();
    temps.sort_by_key(|t| t.role);
    for temp in temps {
        used.insert((Kind::Temp, temp.input));
        let Some(reading) = chip.reading(Kind::Temp, temp.input) else {
            continue;
        };
        let role = Name::Temp(temp.role);
        let row = Row {
            id: row_id(chip, reading),
            name: pick_custom(
                reading,
                context,
                |l| l.temp(&temp.key),
                || labelled(&temp.label).unwrap_or_else(|| role.clone()),
            ),
            role,
            subtitle: channel_source(chip, reading),
            kind: Kind::Temp,
            value: reading.value,
            rating: None,
            range: None,
        };
        if temp.role == TempRole::Cpu {
            out.board_cpu.push(row);
        } else {
            out.board.push(row);
        }
    }

    let mut voltages: Vec<_> = profile.voltages.iter().collect();
    voltages.sort_by_key(|v| v.role);
    for rail in voltages {
        used.insert((Kind::Voltage, rail.input));
        let Some(reading) = chip.reading(Kind::Voltage, rail.input) else {
            continue;
        };
        let volts = rail.volts(reading);
        let role = Name::Voltage(rail.role);
        out.voltages.push(Row {
            id: row_id(chip, reading),
            name: pick_custom(
                reading,
                context,
                |l| l.voltage(&rail.key),
                || labelled(&rail.label).unwrap_or_else(|| role.clone()),
            ),
            role,
            subtitle: channel_source(chip, reading),
            kind: Kind::Voltage,
            value: volts,
            rating: Some(rate(rail, volts)),
            range: None,
        });
    }

    for reading in &chip.readings {
        if reading.kind == Kind::Pwm
            || used.contains(&(reading.kind, reading.index))
            || hidden(profile, reading)
        {
            continue;
        }
        let role = built_in(chip, reading).unwrap_or_else(|| Name::Driver(driver_label(reading)));
        let name = pick(reading, context, || role.clone());
        let row = Row {
            id: row_id(chip, reading),
            subtitle: source(chip, reading, &name),
            name,
            role,
            kind: reading.kind,
            value: reading.value,
            rating: None,
            range: None,
        };
        if reading.kind == Kind::Voltage {
            out.auxiliary.push(row);
        } else {
            out.board_other.push(row);
        }
    }
}

/// Rows for every reading of a device other than the board's. Duty cycles
/// are left out: only the board's fans show them. `module` is the name of a
/// memory module.
fn generic_rows(chip: &Chip, context: &Context, module: Option<&Name>) -> Vec<Row> {
    let single = chip.readings.iter().filter(|r| r.kind != Kind::Pwm).count() == 1;
    chip.readings
        .iter()
        .filter(|r| r.kind != Kind::Pwm)
        .map(|reading| {
            let role = generic_name(chip, reading, context, module, single);
            let name = pick(reading, context, || role.clone());
            Row {
                id: row_id(chip, reading),
                subtitle: generic_subtitle(chip, reading, &name),
                name,
                role,
                kind: reading.kind,
                value: reading.value,
                rating: None,
                range: None,
            }
        })
        .collect()
}

/// Names 2 and 3 of §5.1: built-in names, else the driver's label.
fn generic_name(
    chip: &Chip,
    reading: &Reading,
    context: &Context,
    module: Option<&Name>,
    single: bool,
) -> Name {
    if let Some(name) = built_in(chip, reading) {
        return name;
    }
    let device = chip.device.as_deref();
    match chip.name.as_str() {
        "nvme" => {
            if let Some(model) = device.and_then(|d| attribute(context.sysfs_root, d, "model")) {
                let label = reading.label.as_deref();
                if let Some(number) = label
                    .and_then(|l| l.strip_prefix("Sensor "))
                    .and_then(|n| n.parse().ok())
                {
                    return Name::DiskSensor { model, number };
                }
                let sensor = (label != Some("Composite")).then(|| driver_label(reading));
                return Name::Disk { model, sensor };
            }
        }
        "spd5118" => {
            if let Some(name) = module {
                return name.clone();
            }
        }
        _ => {}
    }
    // A network device is known by the device above the sensor; with more
    // than one reading the name would not tell them apart.
    let path = device.map(|d| d.to_string_lossy().into_owned());
    if single && let Some(path) = path {
        if path.contains("/ieee80211/") {
            return Name::WifiCard;
        }
        if path.contains("/mdio_bus/") {
            return Name::NetworkCard;
        }
    }
    Name::Driver(driver_label(reading))
}

/// The built-in names of §5.1 for known labels of known drivers.
fn built_in(chip: &Chip, reading: &Reading) -> Option<Name> {
    let label = reading.label.as_deref();
    match (chip.name.as_str(), reading.kind) {
        ("k10temp", Kind::Temp) => match label? {
            "Tctl" => Some(Name::CpuTctl),
            other => other
                .strip_prefix("Tccd")
                .and_then(|n| n.parse().ok())
                .map(Name::CpuChiplet),
        },
        ("amdgpu", Kind::Temp) => match label? {
            "edge" => Some(Name::GpuChip),
            "junction" => Some(Name::GpuHotspot),
            "mem" => Some(Name::GpuMemory),
            _ => None,
        },
        ("amdgpu", Kind::Voltage) => (label? == "vddgfx").then_some(Name::GpuCoreVoltage),
        ("amdgpu", Kind::Power) => (label? == "PPT").then_some(Name::GpuPower),
        ("amdgpu", Kind::Fan) => (reading.index == 1).then_some(Name::GpuFan),
        // Only the rails whose meaning is certain; AVSB, VTT and VREF keep
        // the driver's label.
        (name, Kind::Voltage) if NCT6683_CHIPS.contains(&name) => match label? {
            "VSB" => Some(Name::StandbyVoltage),
            "VBAT" => Some(Name::BatteryVoltage),
            _ => None,
        },
        _ => None,
    }
}

/// The hwmon names of the chips the `nct6683` driver handles.
const NCT6683_CHIPS: [&str; 3] = ["nct6683", "nct6686", "nct6687"];

fn generic_subtitle(chip: &Chip, reading: &Reading, name: &Name) -> Subtitle {
    let last = chip
        .device
        .as_deref()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned());
    match (chip.name.as_str(), last) {
        ("nvme", Some(controller)) => Subtitle::Disk { controller },
        ("spd5118", Some(device)) => match smbus_address(&device) {
            Some(address) => Subtitle::MemoryModule {
                driver: chip_label(&chip.name).to_owned(),
                address,
            },
            None => source(chip, reading, name),
        },
        _ => source(chip, reading, name),
    }
}

/// The chip's name for subtitles, without what drivers add to tell
/// devices apart (§5.1): trailing parts after "_" that hold a digit, and a
/// trailing "hwmon". "r8169_0_d00:00" → "r8169", "ath12k_hwmon" → "ath12k",
/// "iwlwifi_1" → "iwlwifi"; "acpi_fan" and "nct6687" stay.
pub fn chip_label(name: &str) -> &str {
    let mut end = name.len();
    while let Some(cut) = name[..end].rfind('_') {
        let part = &name[cut + 1..end];
        if part == "hwmon" || part.chars().any(|c| c.is_ascii_digit()) {
            end = cut;
        } else {
            break;
        }
    }
    &name[..end]
}

/// The address of an I2C device named like `8-0051` (bus 8, 0x51).
fn smbus_address(device: &str) -> Option<u16> {
    let (_, address) = device.split_once('-')?;
    u16::from_str_radix(address, 16).ok()
}

/// The graphics card's name: `product_name` of the device, else the udev
/// database entry, unchanged (the app shortens it, D23).
fn gpu_name(chip: &Chip, context: &Context) -> Option<String> {
    system::device_name(context.sysfs_root, context.udev, chip.device.as_deref()?)
}

/// The name `sensors` shows when the option is on and libsensors gave one;
/// otherwise the default.
fn pick(reading: &Reading, context: &Context, default: impl FnOnce() -> Name) -> Name {
    match &reading.sensors_label {
        Some(label) if context.sensors_labels => Name::Sensors(label.clone()),
        _ => default(),
    }
}

/// For the profile's readings: the user's custom name first (D22), then as
/// [`pick`].
fn pick_custom<'a>(
    reading: &Reading,
    context: &Context<'a>,
    custom: impl FnOnce(&'a UserLabels) -> Option<&'a str>,
    default: impl FnOnce() -> Name,
) -> Name {
    match context.labels.and_then(custom) {
        Some(label) => Name::Label(label.to_owned()),
        None => pick(reading, context, default),
    }
}

fn labelled(label: &Option<String>) -> Option<Name> {
    label.clone().map(Name::Label)
}

/// The rating of a rail. The limit itself is within the tolerance, also
/// when rounding puts it a hair outside (0.95 × 12 is 11.399999… V).
/// A rail whose value could not be read is not rated.
fn rate(rail: &Voltage, volts: f64) -> Rating {
    match (rail.nominal, rail.tolerance_pct) {
        (Some(nominal), Some(tolerance_pct)) if volts.is_finite() => {
            let deviation_pct = (volts - nominal) / nominal * 100.0;
            Rating::Rated {
                nominal,
                tolerance_pct,
                deviation_pct,
                within: deviation_pct.abs() <= tolerance_pct + 1e-9,
            }
        }
        _ => Rating::Unrated,
    }
}

fn hidden(profile: &Profile, reading: &Reading) -> bool {
    let hidden = &profile.hidden;
    let list = match reading.kind {
        Kind::Fan => &hidden.fan,
        Kind::Temp => &hidden.temp,
        Kind::Voltage => &hidden.voltage,
        Kind::Pwm => &hidden.pwm,
        Kind::Power => return false,
    };
    list.contains(&reading.index)
}

fn channel_name(reading: &Reading) -> String {
    format!("{}{}", reading.kind.prefix(), reading.index)
}

fn driver_label(reading: &Reading) -> String {
    reading
        .label
        .clone()
        .unwrap_or_else(|| channel_name(reading))
}

/// Driver and channel name, for rows the profile names (`nct6687 · temp2`).
fn channel_source(chip: &Chip, reading: &Reading) -> Subtitle {
    Subtitle::Source {
        driver: chip_label(&chip.name).to_owned(),
        channel: channel_name(reading),
    }
}

/// Where a reading comes from, without repeating its name (§5.1): the
/// driver's label (`amdgpu · edge`); the channel when the name is that
/// label or holds it (`nct6687 · in9` for VSB); only the driver when the
/// name is the channel itself (an empty `channel`).
fn source(chip: &Chip, reading: &Reading, name: &Name) -> Subtitle {
    let channel = channel_name(reading);
    let named_by = match name {
        Name::Driver(text) => Some(text.as_str()),
        Name::StandbyVoltage | Name::BatteryVoltage => reading.label.as_deref(),
        _ => None,
    };
    let shown = match named_by {
        Some(text) if text == channel => String::new(),
        Some(_) => channel,
        None => driver_label(reading),
    };
    Subtitle::Source {
        driver: chip_label(&chip.name).to_owned(),
        channel: shown,
    }
}

fn row_id(chip: &Chip, reading: &Reading) -> RowId {
    RowId {
        chip: chip.name.clone(),
        device: chip.device.clone(),
        channel: channel_name(reading),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udev::NoUdev;

    fn reading(kind: Kind, index: u8, label: Option<&str>) -> Reading {
        Reading {
            kind,
            index,
            label: label.map(Into::into),
            value: 40.0,
            scaled: false,
            sensors_label: None,
        }
    }

    fn chip(name: &str, device: &str, readings: Vec<Reading>) -> Chip {
        Chip {
            name: name.into(),
            device: Some(device.into()),
            path: PathBuf::from("/sys/class/hwmon").join(name),
            readings,
        }
    }

    fn context<'a>(root: &'a Path, udev: &'a dyn Udev) -> Context<'a> {
        Context {
            sysfs_root: root,
            udev,
            board: None,
            cpu: None,
            sensors_labels: false,
            labels: None,
        }
    }

    #[test]
    fn marketing_name_from_the_brackets() {
        assert_eq!(
            marketing_name("Navi 44 [Radeon RX 9050 / 9060 XT]"),
            "Radeon RX 9050 / 9060 XT"
        );
        assert_eq!(
            marketing_name("GA104 [GeForce RTX 3070] (rev a1)"),
            "GeForce RTX 3070"
        );
        assert_eq!(marketing_name("Raphael"), "Raphael");
        assert_eq!(marketing_name("Odd [ ] entry"), "Odd [ ] entry");
        assert_eq!(marketing_name("Unclosed [name"), "Unclosed [name");
    }

    #[test]
    fn smbus_addresses() {
        assert_eq!(smbus_address("8-0051"), Some(0x51));
        assert_eq!(smbus_address("10-0053"), Some(0x53));
        assert_eq!(smbus_address("spd"), None);
    }

    #[test]
    fn network_cards_by_the_device_above() {
        let wifi = chip(
            "ath12k_hwmon",
            "devices/pci0000:00/0000:0e:00.0/ieee80211/phy0",
            vec![reading(Kind::Temp, 1, None)],
        );
        let phy = chip(
            "r8169_0_d00:00",
            "devices/pci0000:00/0000:0d:00.0/mdio_bus/r8169-0-d00/r8169-0-d00:00",
            vec![reading(Kind::Temp, 1, None)],
        );
        // Two readings: the name would not tell them apart.
        let two = chip(
            "mt7921_phy0",
            "devices/pci0000:00/0000:0e:00.0/ieee80211/phy0",
            vec![reading(Kind::Temp, 1, None), reading(Kind::Temp, 2, None)],
        );
        let root = Path::new("/nonexistent");
        let groups = build(&[wifi, phy, two], &context(root, &NoUdev));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, GroupKind::Other);
        let names: Vec<_> = groups[0].rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(
            names,
            [
                Name::WifiCard,
                Name::NetworkCard,
                Name::Driver("temp1".into()),
                Name::Driver("temp2".into())
            ]
        );
        // Subtitles: the chip without the parts drivers add to tell
        // devices apart, and no channel that the name already says.
        let subtitles: Vec<_> = groups[0].rows.iter().map(|r| r.subtitle.clone()).collect();
        let source = |driver: &str, channel: &str| Subtitle::Source {
            driver: driver.into(),
            channel: channel.into(),
        };
        assert_eq!(
            subtitles,
            [
                source("ath12k", "temp1"),
                source("r8169", "temp1"),
                source("mt7921", ""),
                source("mt7921", "")
            ]
        );
    }

    /// Rule of §5.1: the parts after "_" with a digit, and "hwmon", go.
    #[test]
    fn chip_labels() {
        for (name, label) in [
            ("r8169_0_d00:00", "r8169"),
            ("ath12k_hwmon", "ath12k"),
            ("iwlwifi_1", "iwlwifi"),
            ("mt7921_phy0", "mt7921"),
            ("hidpp_battery_0", "hidpp_battery"),
            ("acpi_fan", "acpi_fan"),
            ("nct6687", "nct6687"),
            ("k10temp", "k10temp"),
            ("spd5118", "spd5118"),
        ] {
            assert_eq!(chip_label(name), label, "{name}");
        }
    }

    /// Disks by controller number (§5.1): nvme2 before nvme10, although
    /// the PCI path of nvme10 sorts first.
    #[test]
    fn disks_by_controller_number() {
        let disk = |path: &str| {
            chip(
                "nvme",
                path,
                vec![reading(Kind::Temp, 1, Some("Composite"))],
            )
        };
        let chips = [
            disk("devices/pci0000:00/0000:00:01.0/0000:01:00.0/nvme/nvme10"),
            disk("devices/pci0000:00/0000:00:02.0/0000:02:00.0/nvme/nvme2"),
            disk("devices/pci0000:00/0000:00:03.0/0000:03:00.0/nvme/nvme0"),
        ];
        let groups = build(&chips, &context(Path::new("/nonexistent"), &NoUdev));
        let controllers: Vec<_> = groups[0]
            .rows
            .iter()
            .map(|r| match &r.subtitle {
                Subtitle::Disk { controller } => controller.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(controllers, ["nvme0", "nvme2", "nvme10"]);
    }

    #[test]
    fn gpu_name_from_product_name_first() {
        let root = std::env::temp_dir().join(format!("baseboard-overview-{}", std::process::id()));
        let device = Path::new("devices/pci0000:00/0000:03:00.0");
        std::fs::create_dir_all(root.join(device)).unwrap();
        struct Database;
        impl Udev for Database {
            fn property(&self, _: &Path, name: &str) -> Option<String> {
                (name == "ID_MODEL_FROM_DATABASE").then(|| "Navi 44 [Radeon RX 9060 XT]".into())
            }
        }
        let gpu = || {
            chip(
                "amdgpu",
                device.to_str().unwrap(),
                vec![reading(Kind::Temp, 1, Some("edge"))],
            )
        };
        let name = |udev: &dyn Udev| {
            build(&[gpu()], &context(&root, udev))[0]
                .description
                .clone()
        };
        assert_eq!(
            name(&Database).as_deref(),
            Some("Navi 44 [Radeon RX 9060 XT]")
        );
        assert_eq!(name(&NoUdev), None);
        std::fs::write(root.join(device).join("product_name"), "  \n").unwrap();
        assert_eq!(
            name(&Database).as_deref(),
            Some("Navi 44 [Radeon RX 9060 XT]")
        );
        std::fs::write(
            root.join(device).join("product_name"),
            "AMD Radeon RX 9060 XT\n",
        )
        .unwrap();
        assert_eq!(name(&Database).as_deref(), Some("AMD Radeon RX 9060 XT"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn one_group_per_graphics_card() {
        let root = Path::new("/nonexistent");
        let cards = [
            chip(
                "amdgpu",
                "devices/a",
                vec![reading(Kind::Temp, 1, Some("edge"))],
            ),
            chip(
                "amdgpu",
                "devices/b",
                vec![reading(Kind::Temp, 1, Some("edge"))],
            ),
        ];
        let groups = build(&cards, &context(root, &NoUdev));
        assert_eq!(
            groups.iter().map(|g| g.kind).collect::<Vec<_>>(),
            [GroupKind::Gpu, GroupKind::Gpu]
        );
    }
}
