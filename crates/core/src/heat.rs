// SPDX-License-Identifier: GPL-3.0-or-later

//! Temperatures close to their limit, for the status sentence of the
//! Overview and the status row of the System page (docs/PROJEKT.md §5.1,
//! D67). Every temperature with a limit counts: the emergency threshold of
//! the board profile (B4) for the readings of its chip, else the critical
//! temperature the driver reports (`tempN_crit`). A reading is close when
//! it is [`MARGIN`] or less under its limit in [`READINGS`] samples in a
//! row, and stays so until it falls [`BACK`] more under that.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::overview::{self, Group, RowId};
use crate::profile::Profile;
use crate::sensors::{Chip, Kind};

/// How close to its limit a temperature has to come, in °C.
pub const MARGIN: f64 = 5.0;
/// In how many samples in a row.
pub const READINGS: u8 = 3;
/// How far under the margin it has to fall again, in °C.
pub const BACK: f64 = 3.0;

/// A critical temperature outside this range is not a limit (a driver
/// without one may report 0 or a register's highest value).
const PLAUSIBLE: std::ops::RangeInclusive<f64> = 20.0..=150.0;

/// The limit of every temperature that has one, in °C.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits(HashMap<RowId, f64>);

impl Limits {
    /// Reads the critical temperatures of `chips` once; the profile's
    /// emergency thresholds win for the readings of its chip.
    pub fn read(chips: &[Chip], profile: Option<&Profile>) -> Self {
        let mut limits = HashMap::new();
        for chip in chips {
            for reading in chip.readings.iter().filter(|r| r.kind == Kind::Temp) {
                if let Some(limit) = critical(&chip.path, reading.index) {
                    limits.insert(overview::row_id(chip, reading), limit);
                }
            }
            let Some(profile) = profile.filter(|p| p.hwmon.name == chip.name) else {
                continue;
            };
            for temp in &profile.temps {
                if let (Some(limit), Some(reading)) =
                    (temp.emergency_c, chip.reading(Kind::Temp, temp.input))
                {
                    limits.insert(overview::row_id(chip, reading), f64::from(limit));
                }
            }
        }
        Self(limits)
    }

    pub fn get(&self, id: &RowId) -> Option<f64> {
        self.0.get(id).copied()
    }
}

/// `tempN_crit` of a hwmon folder in °C, when it is there and plausible.
fn critical(folder: &Path, index: u8) -> Option<f64> {
    let text = std::fs::read_to_string(folder.join(format!("temp{index}_crit"))).ok()?;
    let celsius = text.trim().parse::<f64>().ok()? / 1000.0;
    PLAUSIBLE.contains(&celsius).then_some(celsius)
}

/// A temperature close to its limit.
#[derive(Debug, Clone, PartialEq)]
pub struct Hot {
    pub id: RowId,
    /// In °C.
    pub value: f64,
    pub limit: f64,
}

/// Watches the temperatures across samples.
#[derive(Debug, Clone, Default)]
pub struct HeatWatch {
    /// Samples in a row close to the limit, for those not yet hot.
    close: HashMap<RowId, u8>,
    hot: HashSet<RowId>,
}

impl HeatWatch {
    /// Takes a sample (every row, also the hidden ones) and gives the
    /// temperatures close to their limit, closest first.
    pub fn update(&mut self, groups: &[Group], limits: &Limits) -> Vec<Hot> {
        let mut seen = HashSet::new();
        let mut hot = Vec::new();
        let rows = groups
            .iter()
            .flat_map(|g| g.rows.iter().chain(&g.auxiliary))
            .filter(|r| r.kind == Kind::Temp);
        for row in rows {
            let Some(limit) = limits.get(&row.id) else {
                continue;
            };
            seen.insert(row.id.clone());
            // An unreadable value changes nothing.
            if !row.value.is_finite() {
                if self.hot.contains(&row.id) {
                    hot.push(Hot {
                        id: row.id.clone(),
                        value: row.value,
                        limit,
                    });
                }
                continue;
            }
            let close = row.value >= limit - MARGIN;
            if self.hot.contains(&row.id) {
                if row.value <= limit - MARGIN - BACK {
                    self.hot.remove(&row.id);
                }
            } else if close {
                let count = self.close.entry(row.id.clone()).or_insert(0);
                *count += 1;
                if *count >= READINGS {
                    self.close.remove(&row.id);
                    self.hot.insert(row.id.clone());
                }
            } else {
                self.close.remove(&row.id);
            }
            if self.hot.contains(&row.id) {
                hot.push(Hot {
                    id: row.id.clone(),
                    value: row.value,
                    limit,
                });
            }
        }
        self.hot.retain(|id| seen.contains(id));
        self.close.retain(|id, _| seen.contains(id));
        hot.sort_by(|a, b| (a.limit - a.value).total_cmp(&(b.limit - b.value)));
        hot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overview::{GroupKind, Name, Row, Subtitle};
    use crate::profile::TempRole;

    fn id(channel: &str) -> RowId {
        RowId {
            chip: "nct6687".into(),
            device: None,
            channel: channel.into(),
        }
    }

    fn sample(values: &[(&str, f64)]) -> Vec<Group> {
        let rows = values
            .iter()
            .map(|&(channel, value)| Row {
                id: id(channel),
                name: Name::Temp(TempRole::Cpu),
                role: Name::Temp(TempRole::Cpu),
                subtitle: Subtitle::Source {
                    driver: "nct6687".into(),
                    channel: channel.into(),
                },
                kind: Kind::Temp,
                value,
                rating: None,
                range: None,
            })
            .collect();
        vec![Group {
            kind: GroupKind::Board,
            description: None,
            rows,
            auxiliary: Vec::new(),
        }]
    }

    fn limits() -> Limits {
        Limits(HashMap::from([(id("temp1"), 95.0), (id("temp2"), 80.0)]))
    }

    /// 5 °C or less under the limit in 3 samples in a row; back only 3 °C
    /// under that; a reading without a limit never.
    #[test]
    fn close_in_three_samples_and_back_with_a_margin() {
        let mut watch = HeatWatch::default();
        let limits = limits();
        let mut step =
            |cpu: f64| watch.update(&sample(&[("temp1", cpu), ("temp3", 120.0)]), &limits);
        assert!(step(90.0).is_empty());
        assert!(step(91.0).is_empty());
        let hot = step(93.0);
        assert_eq!(
            hot,
            [Hot {
                id: id("temp1"),
                value: 93.0,
                limit: 95.0
            }]
        );
        // Under the margin, but not 3 °C under it: still hot.
        assert_eq!(step(88.0).len(), 1);
        assert!(step(87.0).is_empty());
        // A sample under the margin starts the count again.
        assert!(step(91.0).is_empty());
        assert!(step(85.0).is_empty());
        assert!(step(91.0).is_empty());
        assert!(step(91.0).is_empty());
        assert_eq!(step(91.0).len(), 1);
    }

    /// The closest to its limit first.
    #[test]
    fn closest_first() {
        let mut watch = HeatWatch::default();
        let limits = limits();
        let mut hot = Vec::new();
        for _ in 0..3 {
            hot = watch.update(&sample(&[("temp1", 91.0), ("temp2", 79.0)]), &limits);
        }
        let order: Vec<&str> = hot.iter().map(|h| h.id.channel.as_str()).collect();
        assert_eq!(order, ["temp2", "temp1"]);
    }

    /// The limits of a sysfs tree: the critical temperatures of the drivers
    /// that report a plausible one, and the profile's thresholds for its
    /// chip.
    #[test]
    fn limits_of_the_drivers_and_the_profile() {
        let tree = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/sysfs-ms7e61-v1"
        ));
        let chips = crate::sensors::SensorSource::chips(&crate::sensors::SysfsSource::new(tree));
        let profile =
            crate::profile::parse(include_str!("../../../data/boards/ms-7e61.toml")).unwrap();
        let limits = Limits::read(&chips, Some(&profile));
        let of = |chip: &str, channel: &str| {
            chips.iter().filter(|c| c.name == chip).find_map(|c| {
                let reading = c
                    .readings
                    .iter()
                    .find(|r| r.kind == Kind::Temp && format!("temp{}", r.index) == channel)?;
                limits.get(&overview::row_id(c, reading))
            })
        };
        assert_eq!(of("nct6687", "temp1"), Some(95.0));
        assert_eq!(of("nct6687", "temp2"), Some(80.0));
        // The drives of the tree report 93.85 and 94.85 °C.
        assert!(of("nvme", "temp1").is_some_and(|limit| (93.0..95.0).contains(&limit)));
        assert_eq!(of("k10temp", "temp1"), None);
        assert!(Limits::read(&chips, None).get(&id("temp1")).is_none());
    }
}
