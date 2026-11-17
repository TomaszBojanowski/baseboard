// SPDX-License-Identifier: GPL-3.0-or-later

//! The last five minutes of every temperature (docs/PROJEKT.md §5.1, D67,
//! D80): one point a sample, kept from the start of the app in memory only,
//! never on disk. While the window is hidden there are no samples, so the
//! history has a gap there; only the processor's temperature of the cards
//! is read on its own then. A time the computer slept is a gap too. The
//! cards of the Overview and of the Fans page draw it.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant, SystemTime};

use crate::readings::{Group, RowId};
use crate::sensors::Kind;

/// How far back a history goes.
pub const SPAN: Duration = Duration::from_secs(300);

/// A change of at least this many degrees over [`TREND_OVER`] is a trend.
const TREND_MIN: f64 = 1.0;
const TREND_OVER: Duration = Duration::from_secs(60);

/// A sleep shorter than this is not told apart from a late sample.
const SLEPT_MIN: Duration = Duration::from_secs(3);

/// When a reading was taken: on the monotonic clock, which stands still
/// while the computer sleeps, and on the wall clock, which does not.
#[derive(Debug, Clone, Copy)]
pub struct Moment {
    pub at: Instant,
    pub wall: SystemTime,
}

impl Moment {
    pub fn now() -> Self {
        Self {
            at: Instant::now(),
            wall: SystemTime::now(),
        }
    }
}

/// Where a temperature is going over the last minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Rising,
    Falling,
    Steady,
}

/// The points of one reading, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct History {
    points: VecDeque<(Instant, f64)>,
}

impl History {
    /// Adds the value of `at` (an unreadable one leaves a gap) and forgets
    /// what is older than [`SPAN`].
    pub fn push(&mut self, at: Instant, value: f64) {
        if value.is_finite() {
            self.points.push_back((at, value));
        }
        while self
            .points
            .front()
            .is_some_and(|&(t, _)| at.saturating_duration_since(t) > SPAN)
        {
            self.points.pop_front();
        }
    }

    pub fn points(&self) -> impl Iterator<Item = (Instant, f64)> + '_ {
        self.points.iter().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The newest point.
    pub fn last(&self) -> Option<(Instant, f64)> {
        self.points.back().copied()
    }

    /// The lowest and the highest value.
    pub fn range(&self) -> Option<(f64, f64)> {
        let mut values = self.points.iter().map(|&(_, v)| v);
        let first = values.next()?;
        Some(values.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }

    /// Moves every point `back` in time; a point that would be before the
    /// start of the monotonic clock goes.
    fn shift(&mut self, back: Duration) {
        self.points = self
            .points
            .drain(..)
            .filter_map(|(at, value)| Some((at.checked_sub(back)?, value)))
            .collect();
    }

    /// The newest value against the oldest one of the last minute.
    pub fn trend(&self) -> Trend {
        let Some((now, value)) = self.last() else {
            return Trend::Steady;
        };
        let earlier = self
            .points
            .iter()
            .find(|&&(t, _)| now.saturating_duration_since(t) <= TREND_OVER)
            .map_or(value, |&(_, v)| v);
        let change = value - earlier;
        if change >= TREND_MIN {
            Trend::Rising
        } else if change <= -TREND_MIN {
            Trend::Falling
        } else {
            Trend::Steady
        }
    }
}

/// The histories of every temperature of the samples, by row.
#[derive(Debug, Clone, Default)]
pub struct Histories {
    rows: HashMap<RowId, History>,
    /// When the latest reading was taken.
    last: Option<Moment>,
}

impl Histories {
    /// Adds the temperatures of the sample taken at `now`, also of the
    /// hidden rows; a row that is gone from the sample is forgotten.
    pub fn update(&mut self, groups: &[Group], now: Moment) {
        self.catch_up(now);
        let temps: Vec<_> = groups
            .iter()
            .flat_map(|g| g.rows.iter().chain(&g.auxiliary))
            .filter(|r| r.kind == Kind::Temp)
            .collect();
        self.rows
            .retain(|id, _| temps.iter().any(|row| row.id == *id));
        for row in temps {
            self.rows
                .entry(row.id.clone())
                .or_default()
                .push(now.at, row.value);
        }
    }

    /// Adds one temperature read on its own while the window is not
    /// visible (D80); the other rows keep their histories.
    pub fn push(&mut self, id: &RowId, now: Moment, value: f64) {
        self.catch_up(now);
        self.rows.entry(id.clone()).or_default().push(now.at, value);
    }

    /// Moves the histories back by the time the computer slept since the
    /// latest reading, which the monotonic clock does not count: the line
    /// has a gap there, and what is now older than five minutes goes with
    /// the next reading.
    fn catch_up(&mut self, now: Moment) {
        let Some(last) = self.last.replace(now) else {
            return;
        };
        let monotonic = now.at.saturating_duration_since(last.at);
        let wall = now.wall.duration_since(last.wall).unwrap_or_default();
        if let Some(slept) = wall.checked_sub(monotonic).filter(|s| *s >= SLEPT_MIN) {
            for history in self.rows.values_mut() {
                history.shift(slept);
            }
        }
    }

    pub fn get(&self, id: &RowId) -> Option<&History> {
        self.rows.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::TempRole;
    use crate::readings::{GroupKind, Name, Row, Subtitle};

    fn at(start: Instant, seconds: u64) -> Instant {
        start + Duration::from_secs(seconds)
    }

    #[test]
    fn five_minutes_and_no_more() {
        let start = Instant::now();
        let mut history = History::default();
        for s in 0..=400 {
            history.push(at(start, s), 40.0 + (s % 7) as f64);
        }
        let oldest = history.points().next().unwrap().0;
        assert_eq!(oldest, at(start, 100));
        assert_eq!(history.points().count(), 301);
        assert_eq!(history.range(), Some((40.0, 46.0)));
        // An unreadable value leaves a gap, not a point.
        history.push(at(start, 401), f64::NAN);
        assert_eq!(
            history.last(),
            Some((at(start, 400), 40.0 + (400 % 7) as f64))
        );
    }

    #[test]
    fn trends_over_the_last_minute() {
        let start = Instant::now();
        let series = |values: &[(u64, f64)]| {
            let mut history = History::default();
            for &(s, v) in values {
                history.push(at(start, s), v);
            }
            history.trend()
        };
        assert_eq!(series(&[(0, 60.0), (30, 61.5), (60, 63.0)]), Trend::Rising);
        assert_eq!(series(&[(0, 63.0), (60, 61.9)]), Trend::Falling);
        assert_eq!(series(&[(0, 60.0), (60, 60.5)]), Trend::Steady);
        // Only the last minute counts: an older rise is no trend now.
        assert_eq!(
            series(&[(0, 40.0), (100, 60.0), (160, 60.4)]),
            Trend::Steady
        );
        assert_eq!(History::default().trend(), Trend::Steady);
    }

    fn temp(channel: &str, value: f64) -> Row {
        Row {
            id: RowId {
                chip: "k10temp".into(),
                device: None,
                channel: channel.into(),
            },
            name: Name::Temp(TempRole::Cpu),
            role: Name::Temp(TempRole::Cpu),
            subtitle: Subtitle::Source {
                driver: "k10temp".into(),
                channel: channel.into(),
            },
            kind: Kind::Temp,
            value,
            rating: None,
            range: None,
        }
    }

    fn sample(rows: Vec<Row>) -> Vec<Group> {
        vec![Group {
            kind: GroupKind::Cpu,
            description: None,
            rows,
            auxiliary: Vec::new(),
        }]
    }

    /// `seconds` after `start` on the monotonic clock, and `slept` more on
    /// the wall clock.
    fn moment(start: Moment, seconds: u64, slept: u64) -> Moment {
        Moment {
            at: at(start.at, seconds),
            wall: start.wall + Duration::from_secs(seconds + slept),
        }
    }

    #[test]
    fn every_temperature_of_the_samples() {
        let start = Moment::now();
        let mut histories = Histories::default();
        histories.update(
            &sample(vec![temp("temp1", 60.0), temp("temp3", 50.0)]),
            start,
        );
        histories.update(&sample(vec![temp("temp1", 61.0)]), moment(start, 1, 0));
        let tctl = histories.get(&temp("temp1", 0.0).id).unwrap();
        assert_eq!(tctl.range(), Some((60.0, 61.0)));
        // A row gone from the sample is forgotten.
        assert!(histories.get(&temp("temp3", 0.0).id).is_none());
    }

    /// While the window is not visible the processor's temperature comes
    /// alone (D80): it joins its history, the others keep theirs.
    #[test]
    fn one_reading_on_its_own() {
        let start = Moment::now();
        let mut histories = Histories::default();
        histories.update(
            &sample(vec![temp("temp1", 60.0), temp("temp3", 50.0)]),
            start,
        );
        let tctl = temp("temp1", 0.0).id;
        histories.push(&tctl, moment(start, 1, 0), 62.0);
        histories.push(&tctl, moment(start, 2, 0), 63.0);
        assert_eq!(histories.get(&tctl).unwrap().points().count(), 3);
        let other = histories.get(&temp("temp3", 0.0).id).unwrap();
        assert_eq!(other.points().count(), 1);
    }

    /// A time the computer slept is a gap of that length (D80), though the
    /// monotonic clock stood still; after a sleep longer than five minutes
    /// only the new reading is left. A late sample is no sleep.
    #[test]
    fn a_sleep_is_a_gap() {
        let start = Moment::now();
        let tctl = temp("temp1", 0.0).id;
        let ages = |histories: &Histories| -> Vec<u64> {
            let history = histories.get(&tctl).unwrap();
            let (newest, _) = history.last().unwrap();
            history
                .points()
                .map(|(at, _)| newest.duration_since(at).as_secs())
                .collect()
        };
        let mut histories = Histories::default();
        histories.push(&tctl, start, 60.0);
        histories.push(&tctl, moment(start, 1, 0), 60.0);
        histories.push(&tctl, moment(start, 2, 2), 60.0);
        assert_eq!(ages(&histories), [2, 1, 0], "late by 2 s: no sleep");
        histories.push(&tctl, moment(start, 3, 32), 61.0);
        assert_eq!(ages(&histories), [33, 32, 31, 0], "slept 30 s");
        histories.push(&tctl, moment(start, 4, 632), 62.0);
        assert_eq!(ages(&histories), [0], "slept 10 minutes");
    }
}
