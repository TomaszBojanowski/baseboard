// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the data of the System page (docs/PROJEKT.md §5.4) in a worker
//! thread: each time the page is entered, and again while it is shown when
//! the set of hwmon devices changes (a driver loaded or unloaded). Only
//! reads files and the udev database.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use baseboard_core::profile::Profile;
use baseboard_core::sensors;
use baseboard_core::system::{self, Paths, SystemInfo};
use baseboard_core::udev::{NoUdev, Udev};
use gtk::glib;

/// How often the hwmon devices are compared while the page is shown.
const CHECK: Duration = Duration::from_secs(2);

/// Reads the data for the sysfs `root`, with the real /proc and /etc; in a
/// Flatpak sandbox with what it shows of the host (§10.3).
pub fn read(root: &Path, profile: Option<&Profile>) -> SystemInfo {
    let libudev = crate::libudev::for_root(root);
    let udev: &dyn Udev = match &libudev {
        Some(udev) => udev,
        None => &NoUdev,
    };
    let paths = match crate::config::sandbox() {
        Some(sandbox) => Paths::flatpak(root, sandbox.host_etc),
        None => Paths::system(root),
    };
    system::read(&paths, udev, profile)
}

/// What the main loop tells the worker.
enum Command {
    /// The page is shown and the window visible. `reread`: read even when
    /// the hwmon devices did not change, because the page was entered.
    Active { reread: bool },
    /// Nothing to read until `Active`.
    Inactive,
}

/// The worker thread; it ends when this is dropped.
pub struct Worker {
    commands: mpsc::Sender<Command>,
}

impl Worker {
    /// Starts the thread, idle until [`set_active`](Self::set_active).
    /// `deliver` gets each reading in the worker thread.
    pub fn start(
        root: PathBuf,
        profile: Option<Profile>,
        deliver: impl Fn(SystemInfo) + Send + 'static,
    ) -> Self {
        Self::start_checking(root, profile, CHECK, deliver)
    }

    fn start_checking(
        root: PathBuf,
        profile: Option<Profile>,
        check: Duration,
        deliver: impl Fn(SystemInfo) + Send + 'static,
    ) -> Self {
        let (commands, received) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("baseboard-system".into())
            .spawn(move || {
                let mut active = false;
                let mut devices = None;
                loop {
                    let command = if active {
                        received.recv_timeout(check)
                    } else {
                        received.recv().map_err(|_| RecvTimeoutError::Disconnected)
                    };
                    match command {
                        Ok(Command::Active { reread }) => {
                            active = true;
                            if reread {
                                devices = None;
                            }
                        }
                        Ok(Command::Inactive) => active = false,
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }
                    if !active {
                        continue;
                    }
                    let now = sensors::devices(&root);
                    if devices.as_ref() != Some(&now) {
                        let start = Instant::now();
                        let info = read(&root, profile.as_ref());
                        glib::g_debug!(
                            "baseboard",
                            "system information read in {} ms; nct6683 {:?}",
                            start.elapsed().as_millis(),
                            info.fan_driver
                        );
                        deliver(info);
                        devices = Some(now);
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("baseboard: cannot start reading the system information: {e}");
        }
        Self { commands }
    }

    /// Reads while `active`: the page is shown and the window visible.
    /// `reread` reads again at once, also without a change of devices.
    pub fn set_active(&self, active: bool, reread: bool) {
        let command = if active {
            Command::Active { reread }
        } else {
            Command::Inactive
        };
        // A worker that is gone has nothing to do.
        let _ = self.commands.send(command);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// This machine through libudev, where there is one: the data is read
    /// without errors and the memory, when the database has it, has slots.
    #[test]
    fn reads_this_machine() {
        let info = read(Path::new("/sys"), None);
        match &info.memory {
            Some(slots) => assert!(!slots.is_empty()),
            None => eprintln!("no memory data in the udev database here, slots not checked"),
        }
        assert!(
            info.sensors_config.unwrap_or_default().is_empty(),
            "no profile, no chip"
        );
        assert_eq!(info.fan_driver.level, None);
    }

    /// A fake sysfs with one hwmon device, removed at the end.
    struct Tree(PathBuf);

    impl Tree {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("baseboard-reader-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let device = root.join("devices/platform/chip/hwmon/hwmon0");
            std::fs::create_dir_all(&device).unwrap();
            std::fs::write(device.join("name"), "chip\n").unwrap();
            std::fs::create_dir_all(root.join("class/hwmon")).unwrap();
            std::os::unix::fs::symlink(&device, root.join("class/hwmon/hwmon0")).unwrap();
            Self(root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A reading on entering the page, none while nothing changes, one
    /// when a hwmon device goes, none while inactive, and one on entering
    /// again.
    #[test]
    fn reads_on_entry_and_on_device_changes() {
        let tree = Tree::new();
        let check = Duration::from_millis(50);
        let (sent, received) = mpsc::channel();
        let worker = Worker::start_checking(tree.0.clone(), None, check, move |info| {
            let _ = sent.send(info.hwmon);
        });
        let wait = Duration::from_secs(5);
        assert!(received.recv_timeout(check * 4).is_err(), "idle at first");

        worker.set_active(true, true);
        assert_eq!(received.recv_timeout(wait).unwrap(), ["chip"]);
        assert!(received.recv_timeout(check * 4).is_err(), "no change");

        std::fs::remove_file(tree.0.join("class/hwmon/hwmon0")).unwrap();
        assert!(received.recv_timeout(wait).unwrap().is_empty());

        worker.set_active(false, false);
        std::thread::sleep(check * 2);
        while received.try_recv().is_ok() {}
        assert!(received.recv_timeout(check * 4).is_err(), "inactive");

        worker.set_active(true, false);
        assert!(received.recv_timeout(check * 4).is_err(), "no change");
        worker.set_active(true, true);
        assert!(received.recv_timeout(wait).unwrap().is_empty());
    }
}
