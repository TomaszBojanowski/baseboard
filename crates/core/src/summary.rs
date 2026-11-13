// SPDX-License-Identifier: GPL-3.0-or-later

//! The Overview page as a model (docs/PROJEKT.md §5.1, D67): a card for
//! the processor, the graphics card and the board, the state of the power
//! rails and of the fans, the warmest memory module and disk, and the
//! status sentence on top by one rule (`state`), which the System page
//! uses too. And the stopped CPU fan (D65), which the Fans and System
//! pages show as well, and whether the fans speed up. No user-visible
//! text: the app makes it.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::heat::Hot;
use crate::overview::{Group, GroupKind, Name, Rating, Row, RowId, Subtitle};
use crate::profile::{FanRole, TempRole};
use crate::sensors::Kind;

/// What a card on top stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroKind {
    /// The processor: the first temperature of its group (Tctl on AMD).
    Cpu,
    /// The first graphics card: the first temperature of its group.
    Gpu,
    /// The board: the system temperature of the profile.
    Board,
}

impl HeroKind {
    /// The group of "All Sensors" the card opens.
    pub fn group(self) -> GroupKind {
        match self {
            Self::Cpu => GroupKind::Cpu,
            Self::Gpu => GroupKind::Gpu,
            Self::Board => GroupKind::Board,
        }
    }
}

/// The temperature of a card.
#[derive(Debug, Clone, PartialEq)]
pub struct Hero {
    pub kind: HeroKind,
    /// The reading, for its history.
    pub id: RowId,
    /// In °C; NaN when it could not be read.
    pub value: f64,
    /// The highest value since the app started.
    pub highest: f64,
}

/// The CPU fan on the processor's card (with a profile only: without one
/// the app does not know which fan it is).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuFan {
    /// In rpm.
    pub value: f64,
    /// 0–255, where the driver reports it.
    pub duty: Option<f64>,
    /// It stopped spinning (D65).
    pub stopped: bool,
}

impl CpuFan {
    /// It runs at full duty cycle.
    pub fn at_full(&self) -> bool {
        self.duty.is_some_and(|duty| duty >= 254.5)
    }
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

/// The fans on their tile.
#[derive(Debug, Clone, PartialEq)]
pub struct Fans {
    /// Fans of the list that spin now.
    pub spinning: usize,
    /// The slowest and the fastest of them, in rpm.
    pub speeds: Option<(f64, f64)>,
    /// Every fan in the order of the headers, in rpm, and whether it is a
    /// stopped CPU fan: the bars of the tile.
    pub bars: Vec<(f64, bool)>,
    /// A CPU fan stopped spinning (D65).
    pub cpu_stopped: bool,
    /// The fans speed up ([`SpeedWatch`]).
    pub speeding: bool,
}

/// The warmest of a kind of device, e.g. of the memory modules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Warmest {
    /// In °C.
    pub value: f64,
    /// How many devices it is the warmest of.
    pub count: usize,
}

/// The Overview page.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// In the order processor, graphics card, board; only those found.
    pub heroes: Vec<Hero>,
    /// The rated rails: `Some` with an empty list when all are within
    /// their tolerance; `None` without rated rails (no profile).
    pub power: Option<Vec<OutsideRail>>,
    pub cpu_fan: Option<CpuFan>,
    /// What the first graphics card draws, in watts.
    pub gpu_power: Option<f64>,
    /// `None` without fans and without a stopped CPU fan.
    pub fans: Option<Fans>,
    pub memory: Option<Warmest>,
    pub disks: Option<Warmest>,
}

/// The page from the groups it shows; `stopped` are the CPU fans that
/// stopped spinning ([`FanWatch`]), also when their rows are hidden, and
/// `speeding` says whether the fans speed up ([`SpeedWatch`]).
pub fn summary(groups: &[Group], stopped: &[RowId], speeding: bool) -> Summary {
    let group = |kind: GroupKind| groups.iter().find(|g| g.kind == kind);
    fn first(group: &Group, kind: Kind) -> Option<&Row> {
        group.rows.iter().find(|r| r.kind == kind)
    }
    fn board_temp(board: &Group) -> Option<&Row> {
        board
            .rows
            .iter()
            .find(|r| r.role == Name::Temp(TempRole::System))
    }
    let heroes = [
        (
            HeroKind::Cpu,
            group(GroupKind::Cpu).and_then(|g| first(g, Kind::Temp)),
        ),
        (
            HeroKind::Gpu,
            group(GroupKind::Gpu).and_then(|g| first(g, Kind::Temp)),
        ),
        (
            HeroKind::Board,
            group(GroupKind::Board).and_then(board_temp),
        ),
    ]
    .into_iter()
    .filter_map(|(kind, row)| {
        let row = row?;
        let highest = row.range.map_or(row.value, |(_, max)| max.max(row.value));
        Some(Hero {
            kind,
            id: row.id.clone(),
            value: row.value,
            highest,
        })
    })
    .collect();

    let power = group(GroupKind::Voltages).and_then(|voltages| {
        let rated: Vec<&Row> = voltages
            .rows
            .iter()
            .filter(|r| matches!(r.rating, Some(Rating::Rated { .. })))
            .collect();
        (!rated.is_empty()).then(|| rated.into_iter().filter_map(outside).collect())
    });

    let cpu_fan = group(GroupKind::Fans)
        .and_then(|fans| {
            fans.rows.iter().find(|r| {
                matches!(
                    r.role,
                    Name::Fan {
                        role: FanRole::Cpu,
                        ..
                    }
                )
            })
        })
        .map(|row| CpuFan {
            value: row.value,
            duty: match row.subtitle {
                Subtitle::Header { duty, .. } => duty.filter(|d| d.is_finite()),
                _ => None,
            },
            stopped: stopped.contains(&row.id),
        });

    let fans = group(GroupKind::Fans)
        .map(|fans| {
            let speeds: Vec<f64> = fans
                .rows
                .iter()
                .map(|r| r.value)
                .filter(|v| v.is_finite() && *v > 0.0)
                .collect();
            Fans {
                spinning: speeds.len(),
                speeds: speeds
                    .iter()
                    .copied()
                    .reduce(f64::min)
                    .zip(speeds.iter().copied().reduce(f64::max)),
                bars: fans
                    .rows
                    .iter()
                    .map(|r| (r.value, stopped.contains(&r.id)))
                    .collect(),
                cpu_stopped: !stopped.is_empty(),
                speeding,
            }
        })
        .or_else(|| {
            (!stopped.is_empty()).then_some(Fans {
                spinning: 0,
                speeds: None,
                bars: Vec::new(),
                cpu_stopped: true,
                speeding,
            })
        });

    Summary {
        heroes,
        power,
        cpu_fan,
        gpu_power: group(GroupKind::Gpu)
            .and_then(|g| first(g, Kind::Power))
            .map(|r| r.value),
        fans,
        memory: group(GroupKind::Memory).and_then(|g| warmest(&g.rows, |_| true)),
        disks: group(GroupKind::Disks).and_then(|g| {
            // A disk once, by its composite temperature, not its sensors.
            warmest(&g.rows, |r| {
                matches!(r.role, Name::Disk { sensor: None, .. })
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

/// What the status sentence warns about.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// A CPU fan stopped spinning (D65).
    CpuFanStopped,
    /// A temperature close to its limit ([`crate::heat`]).
    Hot(HotPart),
    /// A rated rail outside its tolerance (D64).
    Rail(OutsideRail),
}

/// A temperature close to its limit, with what the sentence says of it.
#[derive(Debug, Clone, PartialEq)]
pub struct HotPart {
    pub name: Name,
    /// Its group, which the warning opens.
    pub group: GroupKind,
    /// A temperature of the processor, which slows down at its limit.
    pub processor: bool,
    /// In °C.
    pub value: f64,
    pub limit: f64,
}

/// The status sentence on top of the Overview and the status row of the
/// System page (D67).
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// The most important first: a stopped CPU fan, then temperatures
    /// closest to their limit, then rails.
    Problems(Vec<Problem>),
    /// No problem, but no board profile: the readings work, without the
    /// names and ratings a profile gives.
    NoProfile,
    Fine,
}

/// The state of the page: `groups` has every row (a hidden temperature can
/// be close to its limit too), `hot` comes from [`crate::heat::HeatWatch`],
/// and `profile` says whether a board profile matches.
pub fn state(summary: &Summary, groups: &[Group], hot: &[Hot], profile: bool) -> State {
    let mut problems = Vec::new();
    if summary.fans.as_ref().is_some_and(|fans| fans.cpu_stopped) {
        problems.push(Problem::CpuFanStopped);
    }
    for hot in hot {
        let found = groups.iter().find_map(|group| {
            group
                .rows
                .iter()
                .chain(&group.auxiliary)
                .find(|row| row.id == hot.id)
                .map(|row| (group.kind, row))
        });
        if let Some((group, row)) = found {
            problems.push(Problem::Hot(HotPart {
                name: row.name.clone(),
                group,
                processor: group == GroupKind::Cpu || row.role == Name::Temp(TempRole::Cpu),
                value: hot.value,
                limit: hot.limit,
            }));
        }
    }
    problems.extend(summary.power.iter().flatten().cloned().map(Problem::Rail));
    if !problems.is_empty() {
        State::Problems(problems)
    } else if profile {
        State::Fine
    } else {
        State::NoProfile
    }
}

/// How long a CPU fan has to be stopped before the app says so (D65).
pub const STOPPED_FOR: Duration = Duration::from_secs(10);

/// A longer pause between samples (the window was hidden) starts the count
/// again: nothing is known of the time in between.
const LONGEST_GAP: Duration = Duration::from_secs(20);

/// Watches the CPU fans across samples for D65: a fan stopped when its
/// header is checked in the board profile, it spun earlier since the app
/// started, its duty cycle is above zero, and it shows 0 rpm for at least
/// [`STOPPED_FOR`]. An empty header (never spun) and a fan the curve
/// stopped (0 %) are no alarm.
#[derive(Debug, Clone, Default)]
pub struct FanWatch {
    /// Since when each CPU fan does not spin.
    since: HashMap<RowId, Instant>,
    last: Option<Instant>,
}

impl FanWatch {
    /// Takes the sample of `now` (every row, also the hidden ones, with
    /// their ranges since the start) and gives the CPU fans that stopped.
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
            if !not_spinning(row) {
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
            row.role,
            Name::Fan {
                role: FanRole::Cpu,
                ..
            }
        )
        && matches!(row.subtitle, Subtitle::Header { verified: true, .. })
}

/// A change of the duty cycle people see, in points of percent: the Fans
/// page shows a fan moving to its curve by this much, smaller moves only go
/// to screen readers (D62); the Overview says the fans speed up when one
/// rose by this much (D67).
pub const VISIBLE_CHANGE: u8 = 5;

/// How far back [`SpeedWatch`] looks.
pub const SPEEDING_FOR: Duration = Duration::from_secs(10);

/// Watches the duty cycles of the fan headers across samples, for the
/// Overview, which reads no curves (each read goes to the board's
/// controller): the fans speed up while the duty cycle of one of them is
/// [`VISIBLE_CHANGE`] points or more above its lowest of the last
/// [`SPEEDING_FOR`].
#[derive(Debug, Clone, Default)]
pub struct SpeedWatch {
    /// The duty cycles of each header, 0–255, oldest first.
    duties: HashMap<RowId, VecDeque<(Instant, f64)>>,
    last: Option<Instant>,
}

impl SpeedWatch {
    /// Takes the sample of `now` and says whether the fans speed up.
    pub fn update(&mut self, groups: &[Group], now: Instant) -> bool {
        if self
            .last
            .is_some_and(|last| now.saturating_duration_since(last) > LONGEST_GAP)
        {
            self.duties.clear();
        }
        self.last = Some(now);
        let percent = |duty: f64| duty * 100.0 / 255.0;
        let mut speeding = false;
        let mut seen = HashSet::new();
        let rows = groups
            .iter()
            .flat_map(|g| g.rows.iter().chain(&g.auxiliary));
        for row in rows {
            let Subtitle::Header {
                duty: Some(duty), ..
            } = row.subtitle
            else {
                continue;
            };
            if !duty.is_finite() {
                continue;
            }
            seen.insert(row.id.clone());
            let duties = self.duties.entry(row.id.clone()).or_default();
            duties.push_back((now, duty));
            while duties
                .front()
                .is_some_and(|&(at, _)| now.saturating_duration_since(at) > SPEEDING_FOR)
            {
                duties.pop_front();
            }
            let lowest = duties.iter().map(|&(_, d)| d).fold(duty, f64::min);
            speeding |= percent(duty) - percent(lowest) >= f64::from(VISIBLE_CHANGE);
        }
        self.duties.retain(|id, _| seen.contains(id));
        speeding
    }
}

/// 0 rpm now at a duty cycle above zero, after spinning earlier.
fn not_spinning(row: &Row) -> bool {
    let duty = match row.subtitle {
        Subtitle::Header { duty: Some(d), .. } => d,
        _ => return false,
    };
    let spun = row.range.is_some_and(|(_, max)| max > 0.0);
    row.value == 0.0 && duty > 0.0 && spun
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
            role: name.clone(),
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
                    row("power1", Name::GpuPower, Kind::Power, 12.0),
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

    /// The cards: the processor's first temperature, the graphics card's
    /// first temperature (not its fan), the board's system temperature,
    /// each with the highest value since the start; the CPU fan and what
    /// the graphics card draws.
    #[test]
    fn cards() {
        let mut groups = page();
        groups[0].rows[0].range = Some((40.0, 78.0));
        let summary = summary(&groups, &[], false);
        assert_eq!(
            summary.heroes,
            [
                Hero {
                    kind: HeroKind::Cpu,
                    id: id("temp1"),
                    value: 68.2,
                    highest: 78.0,
                },
                Hero {
                    kind: HeroKind::Gpu,
                    id: id("temp1"),
                    value: 30.0,
                    highest: 30.0,
                },
                Hero {
                    kind: HeroKind::Board,
                    id: id("temp2"),
                    value: 37.5,
                    highest: 37.5,
                },
            ]
        );
        assert_eq!(HeroKind::Board.group(), GroupKind::Board);
        assert_eq!(
            summary.cpu_fan,
            Some(CpuFan {
                value: 890.0,
                duty: Some(124.0),
                stopped: false
            })
        );
        assert!(!summary.cpu_fan.unwrap().at_full());
        assert_eq!(summary.gpu_power, Some(12.0));
        let stopped = super::summary(&groups, &[id("fan1")], false);
        assert!(stopped.cpu_fan.unwrap().stopped);
        // Without a graphics card and a profile, only the processor.
        let groups: Vec<Group> = page()
            .into_iter()
            .filter(|g| !matches!(g.kind, GroupKind::Gpu | GroupKind::Board))
            .collect();
        let kinds: Vec<HeroKind> = super::summary(&groups, &[], false)
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
        assert_eq!(summary(&page(), &[], false).power, Some(Vec::new()));
        let mut groups = page();
        groups[3].rows[0] = rail(11.2, false);
        let power = summary(&groups, &[], false).power.unwrap();
        assert_eq!(power.len(), 1);
        let rail = &power[0];
        assert!(rail.too_low());
        assert!((rail.low - 11.4).abs() < 1e-9 && (rail.high - 12.6).abs() < 1e-9);
        groups[3].rows.remove(0);
        assert_eq!(summary(&groups, &[], false).power, None);
    }

    /// Fans that spin; the warmest module and disk, a disk once by its
    /// composite temperature.
    #[test]
    fn fans_memory_and_disks() {
        let summary = summary(&page(), &[], false);
        assert_eq!(
            summary.fans,
            Some(Fans {
                spinning: 1,
                speeds: Some((890.0, 890.0)),
                bars: vec![(890.0, false), (0.0, false)],
                cpu_stopped: false,
                speeding: false,
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
        let stopped = super::summary(&page(), &[id("fan1")], false);
        assert!(stopped.fans.unwrap().cpu_stopped);
        let speeding = super::summary(&page(), &[], true);
        assert!(speeding.fans.unwrap().speeding);
    }

    /// The fans speed up while a duty cycle is 5 points of percent or more
    /// above its lowest of the last 10 s: a smaller rise, a fall and a
    /// steady duty cycle are not; nor two samples a minute apart.
    #[test]
    fn fans_speeding_up() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let fan = |duty: f64| {
            [group(
                GroupKind::Fans,
                vec![cpu_fan(890.0, 900.0, Some(duty), true)],
            )]
        };
        let mut w = SpeedWatch::default();
        assert!(!w.update(&fan(100.0), at(0)));
        assert!(!w.update(&fan(110.0), at(1)), "3.9 points");
        assert!(w.update(&fan(113.0), at(2)), "5.1 points");
        assert!(w.update(&fan(140.0), at(5)));
        assert!(
            w.update(&fan(140.0), at(11)),
            "100 at 0 s is gone, 110 at 1 s is not"
        );
        assert!(!w.update(&fan(140.0), at(16)), "steady for 10 s");
        assert!(!w.update(&fan(90.0), at(17)), "slowing down");
        assert!(!w.update(&fan(90.0), at(18)));
        // Hidden for a minute: nothing is known of the time in between.
        assert!(!w.update(&fan(200.0), at(80)));
        // A row without a duty cycle is no fan to watch.
        let rpm_only = [group(
            GroupKind::Fans,
            vec![cpu_fan(890.0, 900.0, None, true)],
        )];
        assert!(!w.update(&rpm_only, at(81)));
        assert!(w.duties.is_empty());
    }

    /// The status sentence (D67): the most important problem first (a
    /// stopped CPU fan, the temperature closest to its limit, a rail),
    /// "the readings work" without a profile, else all fine.
    #[test]
    fn state_of_the_page() {
        let groups = page();
        let fine = summary(&groups, &[], false);
        assert_eq!(state(&fine, &groups, &[], true), State::Fine);
        assert_eq!(state(&fine, &groups, &[], false), State::NoProfile);

        let mut low = groups.clone();
        low[3].rows[0] = rail(11.2, false);
        let stopped = summary(&low, &[id("fan1")], false);
        let hot = [Hot {
            id: id("temp3"),
            value: 92.0,
            limit: 95.0,
        }];
        let State::Problems(problems) = state(&stopped, &low, &hot, false) else {
            panic!("no problems");
        };
        assert_eq!(problems.len(), 3);
        assert_eq!(problems[0], Problem::CpuFanStopped);
        // temp3 is in the processor's group (and a chiplet of the board's
        // group too: the first row with the id wins).
        let Problem::Hot(part) = &problems[1] else {
            panic!("{:?}", problems[1]);
        };
        assert_eq!(
            (part.group, part.processor, part.value, part.limit),
            (GroupKind::Cpu, true, 92.0, 95.0)
        );
        assert!(matches!(&problems[2], Problem::Rail(rail) if rail.too_low()));
        // A board temperature that is not the processor's.
        let board = [Hot {
            id: id("temp2"),
            value: 76.0,
            limit: 80.0,
        }];
        let State::Problems(problems) = state(&fine, &groups, &board, true) else {
            panic!("no problems");
        };
        assert!(matches!(&problems[0], Problem::Hot(part)
            if part.group == GroupKind::Board && !part.processor));
    }

    fn watch(fan: Row, watch: &mut FanWatch, at: Instant) -> Vec<RowId> {
        watch.update(&[group(GroupKind::Fans, vec![fan])], at)
    }

    /// D65: a CPU fan that spun and does not spin at a duty cycle above zero
    /// is stopped after 10 s, not before; spinning again ends it.
    #[test]
    fn stopped_cpu_fan() {
        let start = Instant::now();
        let at = |s: u64| start + Duration::from_secs(s);
        let mut w = FanWatch::default();
        let still = || cpu_fan(0.0, 900.0, Some(124.0), true);
        assert!(watch(cpu_fan(890.0, 900.0, Some(124.0), true), &mut w, at(0)).is_empty());
        assert!(watch(still(), &mut w, at(1)).is_empty());
        assert!(watch(still(), &mut w, at(6)).is_empty());
        assert_eq!(watch(still(), &mut w, at(11)), [id("fan1")]);
        assert_eq!(watch(still(), &mut w, at(12)), [id("fan1")]);
        // It spins again: no alarm, and the count starts over.
        assert!(watch(cpu_fan(700.0, 900.0, Some(124.0), true), &mut w, at(13)).is_empty());
        assert!(watch(still(), &mut w, at(14)).is_empty());
        assert!(watch(still(), &mut w, at(23)).is_empty());
        assert_eq!(watch(still(), &mut w, at(24)), [id("fan1")]);
    }

    /// No false alarms (D65): an empty header that never spun, a fan the
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
        assert!(quiet(|| cpu_fan(0.0, 0.0, Some(124.0), true)), "never spun");
        assert!(quiet(|| cpu_fan(0.0, 900.0, Some(0.0), true)), "0 %");
        assert!(
            quiet(|| cpu_fan(0.0, 900.0, Some(124.0), false)),
            "not checked"
        );
        assert!(quiet(|| cpu_fan(0.0, 900.0, None, true)), "no duty cycle");
        let mut pump = cpu_fan(0.0, 900.0, Some(124.0), true);
        pump.role = Name::Fan {
            role: FanRole::Pump,
            number: 1,
        };
        let mut w = FanWatch::default();
        assert!(watch(pump.clone(), &mut w, at(0)).is_empty());
        assert!(watch(pump, &mut w, at(15)).is_empty(), "only CPU fans");
        // Hidden for a minute: the two samples say nothing of the time in
        // between.
        let mut w = FanWatch::default();
        let still = || cpu_fan(0.0, 900.0, Some(124.0), true);
        assert!(watch(still(), &mut w, at(0)).is_empty());
        assert!(watch(still(), &mut w, at(60)).is_empty());
        assert_eq!(watch(still(), &mut w, at(70)), [id("fan1")]);
    }
}
