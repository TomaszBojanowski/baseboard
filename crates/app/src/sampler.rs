// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the sensors for the Overview (docs/PROJEKT.md §5.1): a worker
//! thread takes a sample every refresh interval while the window is
//! visible and hands the model to the main loop. The source of readings,
//! the fan channels and the check for `compute` statements are made in that
//! thread, and made again when the hwmon devices change (a driver loaded
//! again), without a restart. New options from the settings apply at once.
//! While the Fans page is shown, each sample also has the fan modes, and
//! the curves and their sources, read on entering the page and every 10 s
//! (§7 M2, M2.4); while the Overview is shown, the modes only, for the
//! subtitles of the fans (§5.5.3). Only reads sysfs and the udev database.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use baseboard_core::fans::driver::{Capability, Channel, FanDriver, Mode, SysfsDriver};
use baseboard_core::fans::page::{ChannelReading, CurveRead, SourceRead};
use baseboard_core::labels::UserLabels;
use baseboard_core::overview::{self, Board, Context, Extremes, Group, RowId};
use baseboard_core::profile::Profile;
use baseboard_core::sensors::{self, Device, SensorSource};
use baseboard_core::summary::FanWatch;
use baseboard_core::udev::{NoUdev, Udev};
use gtk::glib;

use crate::sensors::Choice;

/// The settings that change how the sensors are read (§3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// The time between samples.
    pub interval: Duration,
    /// Names from lm-sensors first (§5.1).
    pub sensors_labels: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(1),
            sensors_labels: false,
        }
    }
}

/// What the worker needs to know; it moves to the worker thread.
#[derive(Debug, Clone)]
pub struct Setup {
    /// The sysfs root: /sys, or a fake tree in a debug build.
    pub root: PathBuf,
    /// The board profile, if one matches the board.
    pub profile: Option<Profile>,
    /// The user's own names for the profile's readings (D22).
    pub labels: Option<UserLabels>,
    /// The board's name for its group.
    pub board_name: Option<String>,
    /// The processor's model for its group.
    pub cpu: Option<String>,
    pub options: Options,
    /// What the first samples read of the fan channels.
    pub reads: FanReads,
}

/// The curves are read again after this long while the Fans page is
/// shown: each read of a point goes to the board's controller.
const CURVES_EVERY: Duration = Duration::from_secs(10);

/// What a sample reads of the fan channels besides speeds and duty
/// cycles, for the page shown. Each mode or curve read goes to the board's
/// controller, so only what the page shows is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FanReads {
    /// Neither modes nor curves (the System page).
    #[default]
    Nothing,
    /// The modes, for the subtitles of the fans on the Overview.
    Modes,
    /// The fan channels for the Fans page: the modes, and the curves and
    /// sources on entering the page and every 10 s.
    Channels,
}

/// Takes samples; lives in the worker thread.
pub struct Sampler {
    setup: Setup,
    devices: Vec<Device>,
    reader: Option<Reader>,
    extremes: Extremes,
    /// How long the CPU fans stand (D65).
    watch: FanWatch,
    /// What to read of the fan channels.
    reads: FanReads,
    /// How many times the reader was made, for the tests.
    made: usize,
}

/// What depends on the hwmon devices.
struct Reader {
    source: Box<dyn SensorSource>,
    choice: Choice,
    channels: Vec<Channel>,
    udev: Box<dyn Udev>,
    /// The curves and sources by N of `pwmN`, and when they were read.
    curves: HashMap<u8, (CurveRead, SourceRead)>,
    curves_read: Option<Instant>,
}

/// One sample for the main loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Sample {
    /// The model with every row, also the hidden ones.
    pub groups: Vec<Group>,
    pub status: Status,
    /// The fan channels of the profile, while the Fans page is shown and
    /// the board's device is there.
    pub fans: Option<Vec<ChannelReading>>,
    /// The CPU fans that stand (D65), also when their rows are hidden.
    pub stopped: Vec<RowId>,
}

/// How the sample was read, for the notes on the pages (§5.1, §5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// libsensors, or raw sysfs and why.
    pub source: Choice,
    /// The name of the profile's hwmon device when it is not there once,
    /// e.g. with the driver not loaded; `None` with it or without a profile.
    pub missing_chip: Option<String>,
}

impl Sampler {
    pub fn new(setup: Setup) -> Self {
        Self {
            reads: setup.reads,
            setup,
            devices: Vec::new(),
            reader: None,
            extremes: Extremes::default(),
            watch: FanWatch::default(),
            made: 0,
        }
    }

    /// What to read of the fan channels from the next sample on; turning
    /// the channels on reads the curves again at once.
    pub fn set_reads(&mut self, reads: FanReads) {
        if reads == FanReads::Channels
            && self.reads != FanReads::Channels
            && let Some(reader) = &mut self.reader
        {
            reader.curves_read = None;
        }
        self.reads = reads;
    }

    /// One sample: the model with the minimum and maximum of every row.
    /// The reader is made first, and again when the devices changed: a
    /// device that went away and came back (a driver loaded again) is read
    /// again without a restart.
    pub fn sample(&mut self) -> Sample {
        let devices = sensors::devices(&self.setup.root);
        if self.reader.is_none() || devices != self.devices {
            if self.reader.is_some() {
                glib::g_debug!("baseboard", "hwmon devices changed, reading them again");
            }
            // The old one goes first: only one libsensors at a time.
            self.reader = None;
            self.devices = devices;
            self.made += 1;
        }
        let reader = self.reader.get_or_insert_with(|| Reader::new(&self.setup));
        let chips = reader.source.chips();
        let missing_chip = self
            .setup
            .profile
            .as_ref()
            .map(|profile| profile.hwmon.name.clone())
            .filter(|name| sensors::find(&chips, name).is_none());
        let driver = self
            .setup
            .profile
            .as_ref()
            .and_then(|profile| sensors::find(&chips, &profile.hwmon.name))
            .map(|chip| SysfsDriver::new(&chip.path));
        let fans = match (self.reads, &driver) {
            (FanReads::Channels, Some(driver)) => Some(reader.read_fans(driver)),
            _ => None,
        };
        // The Overview's modes come from the channels when the Fans page
        // read them: each `pwmN_enable` is read once a sample, and the
        // Overview is up to date when the window goes back to it.
        let modes = match (&fans, self.reads, &driver) {
            (Some(fans), ..) => modes_of(fans),
            (None, FanReads::Modes, Some(driver)) => reader.read_modes(driver),
            _ => Vec::new(),
        };
        let context = Context {
            sysfs_root: &self.setup.root,
            udev: reader.udev.as_ref(),
            board: self.setup.profile.as_ref().map(|profile| Board {
                profile,
                channels: &reader.channels,
                modes: &modes,
                name: self.setup.board_name.clone(),
            }),
            cpu: self.setup.cpu.clone(),
            sensors_labels: self.setup.options.sensors_labels,
            labels: self.setup.labels.as_ref(),
        };
        let mut groups = overview::build(&chips, &context);
        self.extremes.update(&mut groups);
        let stopped = self.watch.update(&groups, Instant::now());
        Sample {
            groups,
            status: Status {
                source: reader.choice.clone(),
                missing_chip,
            },
            fans,
            stopped,
        }
    }

    pub fn options(&self) -> Options {
        self.setup.options
    }

    pub fn set_options(&mut self, options: Options) {
        self.setup.options = options;
    }
}

impl Reader {
    fn new(setup: &Setup) -> Self {
        let root = &setup.root;
        let profile = setup.profile.as_ref();
        let (source, choice) = crate::sensors::source(root, profile);
        let channels = profile
            .map(|p| crate::board::fan_channels(root, p))
            .unwrap_or_default();
        // The udev database describes the real /sys only.
        let udev: Box<dyn Udev> = match crate::libudev::for_root(root) {
            Some(udev) => Box::new(Cached::new(udev)),
            None => Box::new(NoUdev),
        };
        Self {
            source,
            choice,
            channels,
            udev,
            curves: HashMap::new(),
            curves_read: None,
        }
    }

    /// The modes of the fan channels, and their curves and sources when
    /// they are due; otherwise the ones read before.
    fn read_fans(&mut self, driver: &SysfsDriver) -> Vec<ChannelReading> {
        let due = self
            .curves_read
            .is_none_or(|read| read.elapsed() >= CURVES_EVERY);
        if due {
            self.curves.clear();
            self.curves_read = Some(Instant::now());
        }
        self.channels
            .iter()
            .map(|channel| {
                let known = self.curves.get(&channel.pwm).cloned();
                let reading = ChannelReading::read(driver, channel, known);
                self.curves
                    .entry(channel.pwm)
                    .or_insert_with(|| (reading.curve.clone(), reading.source.clone()));
                reading
            })
            .collect()
    }

    /// The modes of the channels with `mode`, for the Overview. A mode
    /// that cannot be read is left out: the subtitle then has none.
    fn read_modes(&self, driver: &SysfsDriver) -> Vec<(String, Mode)> {
        self.channels
            .iter()
            .filter(|channel| channel.capabilities.contains(Capability::Mode))
            .filter_map(|channel| {
                let mode = driver.mode(channel.pwm).ok()?;
                Some((channel.key.clone(), mode))
            })
            .collect()
    }
}

/// The modes the Fans page read, for the Overview.
fn modes_of(fans: &[ChannelReading]) -> Vec<(String, Mode)> {
    fans.iter()
        .filter_map(|channel| match channel.mode {
            Some(Ok(mode)) => Some((channel.key.clone(), mode)),
            _ => None,
        })
        .collect()
}

/// Properties of the udev database do not change while a device exists, so
/// each is looked up once per reader.
struct Cached<U> {
    inner: U,
    known: RefCell<HashMap<(PathBuf, String), Option<String>>>,
}

impl<U: Udev> Cached<U> {
    fn new(inner: U) -> Self {
        Self {
            inner,
            known: RefCell::new(HashMap::new()),
        }
    }
}

impl<U: Udev> Udev for Cached<U> {
    fn property(&self, device: &Path, name: &str) -> Option<String> {
        self.known
            .borrow_mut()
            .entry((device.to_owned(), name.to_owned()))
            .or_insert_with(|| self.inner.property(device, name))
            .clone()
    }
}

/// What the main loop tells the worker.
enum Command {
    /// Take samples (the window is visible).
    Run,
    /// Stop taking samples until `Run`.
    Pause,
    /// New settings; a running worker takes a sample with them at once.
    Options(Options),
    /// Another page is shown; a running worker takes a sample at once.
    Reads(FanReads),
}

/// The worker thread; it ends when this is dropped.
pub struct Worker {
    commands: mpsc::Sender<Command>,
}

impl Worker {
    /// Starts taking samples. `deliver` gets each one in the worker thread
    /// and passes it on to the main loop.
    pub fn start(setup: Setup, deliver: impl Fn(Sample) + Send + 'static) -> Self {
        let (commands, received) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("baseboard-sensors".into())
            .spawn(move || {
                let mut sampler = Sampler::new(setup);
                let mut running = true;
                loop {
                    if running {
                        deliver(sampler.sample());
                    }
                    let command = if running {
                        received.recv_timeout(sampler.options().interval)
                    } else {
                        received.recv().map_err(|_| RecvTimeoutError::Disconnected)
                    };
                    match command {
                        Ok(Command::Run) => running = true,
                        Ok(Command::Pause) => running = false,
                        Ok(Command::Options(options)) => sampler.set_options(options),
                        Ok(Command::Reads(reads)) => sampler.set_reads(reads),
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("baseboard: cannot start reading the sensors: {e}");
        }
        Self { commands }
    }

    /// Takes samples while `running`, from now on.
    pub fn set_running(&self, running: bool) {
        let command = if running {
            Command::Run
        } else {
            Command::Pause
        };
        // A worker that is gone has nothing to pause.
        let _ = self.commands.send(command);
    }

    /// New settings, applied at once.
    pub fn set_options(&self, options: Options) {
        let _ = self.commands.send(Command::Options(options));
    }

    /// What to read of the fan channels, for the page shown.
    pub fn set_reads(&self, reads: FanReads) {
        let _ = self.commands.send(Command::Reads(reads));
    }
}

#[cfg(test)]
mod tests {
    use baseboard_core::overview::{GroupKind, Subtitle};
    use baseboard_core::sensors::Kind;

    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures");
    const PROFILE: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/boards/ms-7e61.toml"
    );

    /// A copy of a fake tree, removed at the end.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str, test: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("baseboard-sampler-{}-{test}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            for part in ["class", "devices"] {
                copy(&Path::new(FIXTURES).join(name).join(part), &root.join(part));
            }
            Self(root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            let kind = entry.file_type().unwrap();
            if kind.is_symlink() {
                std::os::unix::fs::symlink(std::fs::read_link(entry.path()).unwrap(), &target)
                    .unwrap();
            } else if kind.is_dir() {
                copy(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    fn duties(groups: &[Group]) -> Vec<Option<f64>> {
        let fans = groups.iter().find(|g| g.kind == GroupKind::Fans).unwrap();
        fans.rows
            .iter()
            .map(|r| match r.subtitle {
                Subtitle::Header { duty, .. } => duty,
                _ => None,
            })
            .collect()
    }

    /// The nct6683 driver loaded again, from Fedora's to the one with
    /// pwm13–16 (v1): the device folder is made anew under the same
    /// `hwmon0`, and the next sample reads it again, with the duty cycles
    /// of SYS 1–4 and the ranges kept. A device that goes away goes from
    /// the page too.
    #[test]
    fn devices_that_change_are_read_again() {
        let tree = Tree::new("sysfs-ms7e61-fedora-7.3.3", "reload");
        let mut sampler = Sampler::new(setup(&tree, Options::default()));
        let first = sampler.sample().groups;
        assert_eq!(duties(&first)[2..6], [None; 4]);
        let _ = sampler.sample();
        assert_eq!(sampler.made, 1, "nothing changed, nothing made again");

        let hwmon = tree.0.join("devices/platform/nct6683.2592/hwmon/hwmon0");
        std::fs::remove_dir_all(&hwmon).unwrap();
        copy(
            &Path::new(FIXTURES).join("sysfs-ms7e61-v1/devices/platform/nct6683.2592/hwmon/hwmon7"),
            &hwmon,
        );
        std::fs::write(hwmon.join("fan1_input"), "900\n").unwrap();
        let second = sampler.sample().groups;
        assert_eq!(sampler.made, 2);
        assert_eq!(
            duties(&second)[2..6],
            [Some(98.0), Some(108.0), Some(76.0), Some(102.0)]
        );
        // The CPU fan read 890 rpm before and 900 after: the range spans
        // both drivers.
        let cpu_fan = &second
            .iter()
            .find(|g| g.kind == GroupKind::Fans)
            .unwrap()
            .rows[0];
        assert_eq!(cpu_fan.range, Some((890.0, 900.0)));

        std::fs::remove_file(tree.0.join("class/hwmon/hwmon7")).unwrap();
        let third = sampler.sample().groups;
        assert_eq!(sampler.made, 3);
        let memory = third.iter().find(|g| g.kind == GroupKind::Memory).unwrap();
        assert_eq!(memory.rows.len(), 1);
        assert!(memory.rows.iter().all(|r| r.kind == Kind::Temp));
    }

    /// While the Fans page is shown, every sample has the modes, and the
    /// curves read on entering the page are kept for 10 s; leaving and
    /// entering it reads them again. Without the page nothing is read.
    #[test]
    fn fan_channels_while_the_page_is_shown() {
        let tree = Tree::new("sysfs-ms7e61-hwmon-next", "fans");
        let mut sampler = Sampler::new(setup(&tree, Options::default()));
        assert_eq!(sampler.sample().fans, None);
        sampler.set_reads(FanReads::Channels);
        let first = sampler.sample().fans.unwrap();
        let keys: Vec<_> = first.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(
            keys,
            ["cpu1", "pump1", "sys1", "sys2", "sys3", "sys4", "ezconn"]
        );
        let sys1 = |fans: &[ChannelReading]| fans.iter().find(|c| c.key == "sys1").unwrap().clone();
        let point1 = |reading: &ChannelReading| reading.curve.clone().unwrap().unwrap()[0].duty;
        assert_eq!(sys1(&first).mode, Some(Ok(Mode::Auto)));
        assert_eq!(point1(&sys1(&first)), 94);

        let hwmon = tree.0.join("devices/platform/nct6683.2592/hwmon/hwmon3");
        std::fs::write(hwmon.join("pwm16_enable"), "1\n").unwrap();
        std::fs::write(hwmon.join("pwm16_auto_point1_pwm"), "50\n").unwrap();
        let second = sampler.sample().fans.unwrap();
        assert_eq!(sys1(&second).mode, Some(Ok(Mode::Manual)), "at once");
        assert_eq!(point1(&sys1(&second)), 94, "not before 10 s");

        sampler.set_reads(FanReads::Nothing);
        assert_eq!(sampler.sample().fans, None);
        sampler.set_reads(FanReads::Channels);
        let third = sampler.sample().fans.unwrap();
        assert_eq!(point1(&sys1(&third)), 50, "read again on entering");
    }

    fn modes(groups: &[Group]) -> Vec<Option<Mode>> {
        let fans = groups.iter().find(|g| g.kind == GroupKind::Fans).unwrap();
        fans.rows
            .iter()
            .map(|r| match r.subtitle {
                Subtitle::Header { mode, .. } => mode,
                _ => None,
            })
            .collect()
    }

    /// While the Overview is shown, every sample has the modes of the
    /// channels with `mode` in the fans' subtitles, and nothing else of the
    /// channels (§5.5.3). On the Fans page the Overview gets the modes the
    /// page read; on the System page there are none.
    #[test]
    fn fan_modes_while_the_overview_is_shown() {
        let tree = Tree::new("sysfs-ms7e61-hwmon-next", "modes");
        let mut sampler = Sampler::new(Setup {
            reads: FanReads::Modes,
            ..setup(&tree, Options::default())
        });
        // CPU, pump, SYS 1–4, JAF_1 (no `pwm4_enable`).
        let auto = Some(Mode::Auto);
        let first = sampler.sample();
        assert_eq!(first.fans, None, "no curves");
        assert_eq!(
            modes(&first.groups),
            [auto, auto, auto, auto, auto, auto, None],
            "from the first sample"
        );

        let hwmon = tree.0.join("devices/platform/nct6683.2592/hwmon/hwmon3");
        std::fs::write(hwmon.join("pwm16_enable"), "1\n").unwrap();
        let manual = Some(Mode::Manual);
        let second = sampler.sample();
        assert_eq!(
            modes(&second.groups),
            [auto, auto, manual, auto, auto, auto, None],
            "at once"
        );

        sampler.set_reads(FanReads::Channels);
        let fans_page = sampler.sample();
        assert!(fans_page.fans.is_some());
        assert_eq!(modes(&fans_page.groups), modes(&second.groups));

        sampler.set_reads(FanReads::Nothing);
        let system_page = sampler.sample();
        assert_eq!(system_page.fans, None);
        assert_eq!(modes(&system_page.groups), [None; 7]);
    }

    /// Fedora's driver has no `pwmN_enable`: no modes, nothing to read.
    #[test]
    fn no_fan_modes_without_the_attribute() {
        let tree = Tree::new("sysfs-ms7e61-fedora-7.3.3", "no-modes");
        let mut sampler = Sampler::new(Setup {
            reads: FanReads::Modes,
            ..setup(&tree, Options::default())
        });
        assert_eq!(modes(&sampler.sample().groups), [None; 7]);
    }

    fn kinds(groups: &[Group]) -> Vec<GroupKind> {
        groups.iter().map(|g| g.kind).collect()
    }

    /// The board's device goes away (the driver unloaded): the other chips
    /// stay, the status names the missing chip for the note on the page.
    /// When it comes back, its readings return by themselves.
    #[test]
    fn board_chip_goes_and_comes_back() {
        let tree = Tree::new("sysfs-ms7e61-fedora-7.3.3", "missing");
        let mut sampler = Sampler::new(setup(&tree, Options::default()));
        let before = sampler.sample();
        assert_eq!(before.status.missing_chip, None);
        assert!(matches!(before.status.source, Choice::Sysfs { .. }));
        let all = kinds(&before.groups);

        let link = tree.0.join("class/hwmon/hwmon0");
        let target = std::fs::read_link(&link).unwrap();
        std::fs::remove_file(&link).unwrap();
        let without = sampler.sample();
        assert_eq!(without.status.missing_chip.as_deref(), Some("nct6687"));
        assert_eq!(
            kinds(&without.groups),
            [
                GroupKind::Cpu,
                GroupKind::Gpu,
                GroupKind::Disks,
                GroupKind::Memory
            ]
        );

        std::os::unix::fs::symlink(target, &link).unwrap();
        let after = sampler.sample();
        assert_eq!(after.status.missing_chip, None);
        assert_eq!(kinds(&after.groups), all);
        assert_eq!(sampler.made, 3);
    }

    /// No hwmon devices at all, as in a virtual machine: an empty sample,
    /// no error. Without a profile nothing counts as missing.
    #[test]
    fn no_hwmon_devices() {
        let root =
            std::env::temp_dir().join(format!("baseboard-sampler-{}-empty", std::process::id()));
        std::fs::create_dir_all(root.join("class/dmi/id")).unwrap();
        let mut sampler = Sampler::new(Setup {
            root: root.clone(),
            profile: None,
            labels: None,
            board_name: None,
            cpu: None,
            options: Options::default(),
            reads: FanReads::Modes,
        });
        let sample = sampler.sample();
        assert!(sample.groups.is_empty());
        assert_eq!(sample.status.missing_chip, None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn worker_delivers_and_pauses() {
        let tree = Tree::new("sysfs-ms7e61-hwmon-next", "worker");
        let (sent, received) = mpsc::channel();
        let worker = Worker::start(setup(&tree, Options::default()), move |sample| {
            let _ = sent.send(sample.groups.len());
        });
        let interval = Options::default().interval;
        // The first sample comes at once.
        let groups = received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(groups > 0);
        worker.set_running(false);
        // At most one sample was on its way; then nothing.
        let _ = received.recv_timeout(interval / 2);
        assert!(received.recv_timeout(interval * 2).is_err());
        worker.set_running(true);
        assert!(received.recv_timeout(Duration::from_secs(5)).is_ok());
        drop(worker);
    }

    /// New options give a sample at once, without waiting for the
    /// interval, and a new interval holds from then on (§3.2: settings work
    /// without a restart).
    #[test]
    fn options_apply_at_once() {
        let tree = Tree::new("sysfs-ms7e61-hwmon-next", "options");
        let (sent, received) = mpsc::channel();
        let slow = Options {
            interval: Duration::from_secs(60),
            sensors_labels: false,
        };
        let worker = Worker::start(setup(&tree, slow), move |sample| {
            let _ = sent.send(sample.groups.len());
        });
        let soon = Duration::from_secs(5);
        assert!(received.recv_timeout(soon).is_ok(), "the first sample");
        assert!(
            received.recv_timeout(Duration::from_millis(1500)).is_err(),
            "nothing before the interval"
        );
        worker.set_options(Options {
            sensors_labels: true,
            ..slow
        });
        assert!(received.recv_timeout(soon).is_ok(), "a sample at once");
        worker.set_options(Options {
            interval: Duration::from_millis(200),
            sensors_labels: true,
        });
        for _ in 0..3 {
            assert!(received.recv_timeout(soon).is_ok());
        }
        drop(worker);
    }

    fn setup(tree: &Tree, options: Options) -> Setup {
        Setup {
            root: tree.0.clone(),
            profile: Some(profile_ms7e61()),
            labels: None,
            board_name: None,
            cpu: None,
            options,
            reads: FanReads::Nothing,
        }
    }

    fn profile_ms7e61() -> Profile {
        baseboard_core::profile::load(Path::new(PROFILE)).unwrap()
    }
}
