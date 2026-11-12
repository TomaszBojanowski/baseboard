// SPDX-License-Identifier: GPL-3.0-or-later

//! The top of the Overview page (docs/PROJEKT.md §5.1, D64): a few large
//! numbers, the state of the power rails and of the fans, the warmest
//! memory module and drive. And the stopped CPU fan (D65), which the Fans
//! and System pages show as well. No user-visible text: the app makes it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::overview::{Group, GroupKind, Name, Rating, Row, RowId, Subtitle};
use crate::profile::{FanRole, TempRole};
use crate::sensors::Kind;

/// What a large number on top stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroKind {
    /// The processor: the first temperature of its group (Tctl on AMD).
    Cpu,
    /// The first graphics card: the first temperature of its group.
    Gpu,
    /// The board: the system temperature of the profile.
    Board,
}

/// A large number on top, and the group of the full list it opens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hero {
    pub kind: HeroKind,
    /// In °C; NaN when it could not be read.
    pub value: f64,
    pub group: GroupKind,
}

/// A rated rail outside its tolerance (§5.1).
#[derive(Debug, Clone, PartialEq)]
pub struct OutsideRail {
    pub name: Name,
    /// Volts on the rail.
    pub value: f64,
    pub nominal: f64,
    /// The lowest and highest value within the tolerance.
    pub low: f64,
    pub high: f64,
}

impl OutsideRail {
    /// Below the tolerance rather than above it.
    pub fn too_low(&self) -> bool {
        self.value < self.nominal
    }
}

/// The fans in one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fans {
    /// Fans of the list that turn now.
    pub turning: usize,
    /// A CPU fan stands (D65).
    pub cpu_stopped: bool,
}

/// The warmest of a kind of device, e.g. of the memory modules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warmest {
    /// In °C.
    pub value: f64,
    /// How many devices it is the warmest of.
    pub count: usize,
}

/// The top of the Overview page.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// In the order processor, graphics card, board; only those found.
    pub heroes: Vec<Hero>,
    /// The rated rails: `Some` with an empty list when all are within
    /// their tolerance; `None` without rated rails (no profile).
    pub power: Option<Vec<OutsideRail>>,
    /// `None` without fans and without a stopped CPU fan.
    pub fans: Option<Fans>,
    pub memory: Option<Warmest>,
    pub disks: Option<Warmest>,
}

/// The top of the page from the groups it shows; `stopped` are the CPU
/// fans that stand ([`FanWatch`]), also when their rows are hidden.
pub fn summary(groups: &[Group], stopped: &[RowId]) -> Summary {
    let group = |kind: GroupKind| groups.iter().find(|g| g.kind == kind);
    let first_temp = |group: &Group| {
        group
            .rows
            .iter()
            .find(|r| r.kind == Kind::Temp)
            .map(|r| r.value)
    };
    let mut heroes = Vec::new();
    let mut hero = |kind: HeroKind, group: GroupKind, value: Option<f64>| {
        if let Some(value) = value {
            heroes.push(Hero { kind, value, group });
        }
    };
    hero(
        HeroKind::Cpu,
        GroupKind::Cpu,
        group(GroupKind::Cpu).and_then(first_temp),
    );
    hero(
        HeroKind::Gpu,
        GroupKind::Gpu,
        group(GroupKind::Gpu).and_then(first_temp),
    );
    let board = group(GroupKind::Board).and_then(|board| {
        board
            .rows
            .iter()
            .find(|r| r.name == Name::Temp(TempRole::System))
            .map(|r| r.value)
    });
    hero(HeroKind::Board, GroupKind::Board, board);

    let power = group(GroupKind::Voltages).and_then(|voltages| {
        let rated: Vec<&Row> = voltages
            .rows
            .iter()
            .filter(|r| matches!(r.rating, Some(Rating::Rated { .. })))
            .collect();
        (!rated.is_empty()).then(|| rated.into_iter().filter_map(outside).collect())
    });

    let fans = group(GroupKind::Fans)
        .map(|fans| {
            fans.rows
                .iter()
                .filter(|r| r.value.is_finite() && r.value > 0.0)
                .count()
        })
        .map(|turning| Fans {
            turning,
            cpu_stopped: !stopped.is_empty(),
        })
        .or_else(|| {
            (!stopped.is_empty()).then_some(Fans {
                turning: 0,
                cpu_stopped: true,
            })
        });

    Summary {
        heroes,
        power,
        fans,
        memory: group(GroupKind::Memory).and_then(|g| warmest(&g.rows, |_| true)),
        disks: group(GroupKind::Disks).and_then(|g| {
            // A drive once, by its composite temperature, not its sensors.
            warmest(&g.rows, |r| {
                matches!(r.name, Name::Disk { sensor: None, .. })
            })
            .or_else(|| warmest(&g.rows, |_| true))
        }),
    }
}

/// A rated rail outside its tolerance, with the range it should be in.
fn outside(row: &Row) -> Option<OutsideRail> {
    let Some(Rating::Rated {
        nominal,
        tolerance_pct,
        within: false,
        ..
    }) = row.rating
    else {
        return None;
    };
    let margin = nominal * tolerance_pct / 100.0;
    Some(OutsideRail {
        name: row.name.clone(),
        value: row.value,
        nominal,
        low: nominal - margin,
        high: nominal + margin,
    })
}

/// The warmest of the temperatures of `rows` that `counted` takes.
fn warmest(rows: &[Row], counted: impl Fn(&Row) -> bool) -> Option<Warmest> {
    let temps: Vec<f64> = rows
        .iter()
        .filter(|r| r.kind == Kind::Temp && counted(r))
        .map(|r| r.value)
        .collect();
    let value = temps
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .reduce(f64::max)
        .or_else(|| temps.first().copied())?;
    Some(Warmest {
        value,
        count: temps.len(),
    })
}

/// How long a stopped CPU fan has to stand before the app says so (D65).
pub const STOPPED_FOR: Duration = Duration::from_secs(10);

/// A longer pause between samples (the window was hidden) starts the count
/// again: nothing is known of the time in between.
const LONGEST_GAP: Duration = Duration::from_secs(20);

/// Watches the CPU fans across samples for D65: a fan stands when its
/// header is checked in the board profile, it turned earlier since the app
/// started, its duty cycle is above zero, and it shows 0 rpm for at least
/// [`STOPPED_FOR`]. An empty header (never turned) and a fan the curve
/// stopped (0 %) are no alarm.
#[derive(Debug, Clone, Default)]
pub struct FanWatch {
    /// Since when each CPU fan stands.
    since: HashMap<RowId, Instant>,
    last: Option<Instant>,
}

impl FanWatch {
    /// Takes the sample of `now` (every row, also the hidden ones, with
    /// their ranges since the start) and gives the CPU fans that stand.
    pub fn update(&mut self, groups: &[Group], now: Instant) -> Vec<RowId> {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) > LONGEST_GAP)
        {
            self.since.clear();
        }
        self.last = Some(now);
        let mut stopped = Vec::new();
        let rows = groups
            .iter()
            .flat_map(|g| g.rows.iter().chain(&g.auxiliary));
        for row in rows.filter(|r| watched(r)) {
            if !stands(row) {
                self.since.remove(&row.id);
                continue;
            }
            let since = *self.since.entry(row.id.clone()).or_insert(now);
            if now.saturating_duration_since(since) >= STOPPED_FOR {
                stopped.push(row.id.clone());
            }
        }
        stopped
    }
}

/// A CPU fan on a header checked on this board.
fn watched(row: &Row) -> bool {
    row.kind == Kind::Fan
        && matches!(
            row.name,
            Name::Fan {
                role: FanRole::Cpu,
                ..
            }
        )
        && matches!(row.subtitle, Subtitle::Header { verified: true, .. })
}

/// 0 rpm now at a duty cycle above zero, after turning earlier.
fn stands(row: &Row) -> bool {
    let duty = match row.subtitle {
        Subtitle::Header { duty: Some(d), .. } => d,
        _ => return false,
    };
    let turned = row.range.is_some_and(|(_, max)| max > 0.0);
    row.value == 0.0 && duty > 0.0 && turned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(channel: &str) -> RowId {
        RowId {
            chip: "nct6687".into(),
            device: None,
            channel: channel.into(),
        }
    }

    fn row(channel: &str, name: Name, kind: Kind, value: f64) -> Row {
        Row {
            id: id(channel),
            name,
            subtitle: Subtitle::Source {
                driver: "nct6687".into(),
                channel: channel.into(),
            },
            kind,
            value,
            rating: None,
            range: Some((value, value)),
        }
    }

    fn group(kind: GroupKind, rows: Vec<Row>) -> Group {
        Group {
            kind,
            description: None,
            rows,
            auxiliary: Vec::new(),
        }
    }

    /// The CPU fan: `value` rpm now, `max` the most since the start,
    /// `duty` 0–255, on a header checked or not.
    fn cpu_fan(value: f64, max: f64, duty: Option<f64>, verified: bool) -> Row {
        Row {
            subtitle: Subtitle::Header {
                header: "CPU_FAN1".into(),
                duty,
                verified,
                mode: None,
            },
            range: Some((0.0, max)),
            ..row(
                "fan1",
                Name::Fan {
                    role: FanRole::Cpu,
                    number: 1,
                },
                Kind::Fan,
                value,
            )
        }
    }

    fn rail(value: f64, within: bool) -> Row {
        Row {
            rating: Some(Rating::Rated {
                nominal: 12.0,
                tolerance_pct: 5.0,
                deviation_pct: (value - 12.0) / 12.0 * 100.0,
                within,
            }),
            ..row(
                "in0",
                Name::Voltage(crate::profile::VoltageRole::P12v),
                Kind::Voltage,
                value,
            )
        }
    }

    fn page() -> Vec<Group> {
        vec![
            group(
                GroupKind::Cpu,
                vec![
                    row("temp1", Name::CpuTctl, Kind::Temp, 68.2),
                    row("temp3", Name::CpuChiplet(1), Kind::Temp, 69.1),
                ],
            ),
            group(
                GroupKind::Board,
                vec![
                    row("temp2", Name::Temp(TempRole::System), Kind::Temp, 37.5),
                    row("temp3", Name::Temp(TempRole::Vrm), Kind::Temp, 36.0),
                ],
            ),
            group(
                GroupKind::Fans,
                vec![
                    cpu_fan(890.0, 900.0, Some(124.0), true),
                    row(
                        "fan2",
                        Name::Fan {
                            role: FanRole::Pump,
                            number: 1,
                        },
                        Kind::Fan,
                        0.0,
                    ),
                ],
            ),
            group(
                GroupKind::Voltages,
                vec![
                    rail(12.1, true),
                    row(
                        "in4",
                        Name::Voltage(crate::profile::VoltageRole::Vcore),
                        Kind::Voltage,
                        1.344,
                    ),
                ],
            ),
            group(
                GroupKind::Gpu,
                vec![
                    row("fan1", Name::GpuFan, Kind::Fan, 0.0),
                    row("temp1", Name::GpuChip, Kind::Temp, 30.0),
                ],
            ),
            group(
                GroupKind::Disks,
                vec![
                    row(
                        "temp1",
                        Name::Disk {
                            model: "A".into(),
                            sensor: None,
                        },
                        Kind::Temp,
                        28.9,
                    ),
                    row(
                        "temp2",
                        Name::DiskSensor {
                            model: "A".into(),
                            number: 1,
                        },
                        Kind::Temp,
                        45.0,
                    ),
                    row(
                        "temp1b",
                        Name::Disk {
                            model: "B".into(),
                            sensor: None,
                        },
                        Kind::Temp,
                        27.9,
                    ),
                ],
            ),
            group(
                GroupKind::Memory,
                vec![
                    row("temp1m", Name::MemoryModule(1), Kind::Temp, 29.3),
                    row("temp2m", Name::MemoryModule(2), Kind::Temp, 30.0),
                ],
            ),
        ]
    }

    /// The large numbers: the processor's first temperature, the graphics
    /// card's first temperature (not its fan), the board's system
    /// temperature; each opens its group.
    #[test]
    fn large_numbers() {
        let summary = summary(&page(), &[]);
        assert_eq!(
            summary.heroes,
            [
                Hero {
                    kind: HeroKind::Cpu,
                    value: 68.2,
                    group: GroupKind::Cpu
                },
                Hero {
                    kind: HeroKind::Gpu,
                    value: 30.0,
                    group: GroupKind::Gpu
                },
                Hero {
                    kind: HeroKind::Board,
                    value: 37.5,
                    group: GroupKind::Board
                },
            ]
        );
        // Without a graphics card and a profile, only the processor.
        let groups: Vec<Group> = page()
            .into_iter()
            .filter(|g| !matches!(g.kind, GroupKind::Gpu | GroupKind::Board))
            .collect();
        let kinds: Vec<HeroKind> = super::summary(&groups, &[])
            .heroes
            .iter()
            .map(|h| h.kind)
            .collect();
        assert_eq!(kinds, [HeroKind::Cpu]);
    }

    /// Power: within the tolerance, outside it with the range it should be
    /// in, and nothing without rated rails.
    #[test]
    fn power() {
        assert_eq!(summary(&page(), &[]).power, Some(Vec::new()));
        let mut groups = page();
        groups[3].rows[0] = rail(11.2, false);
        let power = summary(&groups, &[]).power.unwrap();
        assert_eq!(power.len(), 1);
        let rail = &power[0];
        assert!(rail.too_low());
        assert!((rail.low - 11.4).abs() < 1e-9 && (rail.high - 12.6).abs() < 1e-9);
        groups[3].rows.remove(0);
        assert_eq!(summary(&groups, &[]).power, None);
    }

    /// Fans that turn; the warmest module and drive, a drive once by its
    /// composite temperature.
    #[test]
    fn fans_memory_and_disks() {
        let summary = summary(&page(), &[]);
        assert_eq!(
            summary.fans,
            Some(Fans {
                turning: 1,
                cpu_stopped: false
            })
        );
        assert_eq!(
            summary.memory,
            Some(Warmest {
                value: 30.0,
                count: 2
            })
        );
        assert_eq!(
            summary.disks,
            Some(Warmest {
                value: 28.9,
                count: 2
            })
        );
        let stopped = super::summary(&page(), &[id("fan1")]);
        assert!(stopped.fans.unwrap().cpu_stopped);
    }

    fn watch(fan: Row, watch: &mut FanWatch, at: Instant) -> Vec<RowId> {
        watch.update(&[group(GroupKind::Fans, vec![fan])], at)
    }

    /// D65: a CPU fan that turned and stands at a duty cycle above zero is
    /// stopped after 10 s, not before; it turning again ends it.
    #[test]
    fn stopped_cpu_fan() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut w = FanWatch::default();
        let stands = || cpu_fan(0.0, 900.0, Some(124.0), true);
        assert!(watch(cpu_fan(890.0, 900.0, Some(124.0), true), &mut w, at(0)).is_empty());
        assert!(watch(stands(), &mut w, at(1)).is_empty());
        assert!(watch(stands(), &mut w, at(6)).is_empty());
        assert_eq!(watch(stands(), &mut w, at(11)), [id("fan1")]);
        assert_eq!(watch(stands(), &mut w, at(12)), [id("fan1")]);
        // It turns again: no alarm, and the count starts over.
        assert!(watch(cpu_fan(700.0, 900.0, Some(124.0), true), &mut w, at(13)).is_empty());
        assert!(watch(stands(), &mut w, at(14)).is_empty());
        assert!(watch(stands(), &mut w, at(23)).is_empty());
        assert_eq!(watch(stands(), &mut w, at(24)), [id("fan1")]);
    }

    /// No false alarms (D65): an empty header that never turned, a fan the
    /// curve stopped at 0 %, a header nobody checked, no duty cycle, and a
    /// long pause between samples, which starts the count again.
    #[test]
    fn no_false_alarms() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let quiet = |fan: fn() -> Row| {
            let mut w = FanWatch::default();
            (0..=30).all(|s| watch(fan(), &mut w, at(s)).is_empty())
        };
        assert!(
            quiet(|| cpu_fan(0.0, 0.0, Some(124.0), true)),
            "never turned"
        );
        assert!(quiet(|| cpu_fan(0.0, 900.0, Some(0.0), true)), "0 %");
        assert!(
            quiet(|| cpu_fan(0.0, 900.0, Some(124.0), false)),
            "not checked"
        );
        assert!(quiet(|| cpu_fan(0.0, 900.0, None, true)), "no duty cycle");
        let mut pump = cpu_fan(0.0, 900.0, Some(124.0), true);
        pump.name = Name::Fan {
            role: FanRole::Pump,
            number: 1,
        };
        let mut w = FanWatch::default();
        assert!(watch(pump.clone(), &mut w, at(0)).is_empty());
        assert!(watch(pump, &mut w, at(15)).is_empty(), "only CPU fans");
        // Hidden for a minute: the two samples say nothing of the time in
        // between.
        let mut w = FanWatch::default();
        let stands = || cpu_fan(0.0, 900.0, Some(124.0), true);
        assert!(watch(stands(), &mut w, at(0)).is_empty());
        assert!(watch(stands(), &mut w, at(60)).is_empty());
        assert_eq!(watch(stands(), &mut w, at(70)), [id("fan1")]);
    }
}
