// SPDX-License-Identifier: GPL-3.0-or-later

//! The udev database, behind a trait so that tests can use a fake one. The
//! app implements it with libudev (`app::libudev`, D21).

use std::path::Path;

/// Properties of devices in the udev database.
pub trait Udev {
    /// A property of a device in the database, e.g.
    /// `ID_MODEL_FROM_DATABASE`. `device` is the sysfs path relative to the
    /// sysfs root, as in [`crate::sensors::Chip::device`].
    fn property(&self, device: &Path, name: &str) -> Option<String>;
}

/// No udev database: a fake sysfs tree, or no libudev.
pub struct NoUdev;

impl Udev for NoUdev {
    fn property(&self, _device: &Path, _name: &str) -> Option<String> {
        None
    }
}
