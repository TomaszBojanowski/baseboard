// SPDX-License-Identifier: GPL-3.0-or-later

//! Logic shared by the Baseboard app and its system helper: board profiles,
//! the sensor model and the validation of fan settings.
//!
//! The helper runs as root, so this crate has no user interface, no
//! translated text and no unsafe code. Both programs compute the same
//! results from it; the helper always has the final say.

pub mod board;
pub mod fans;
pub mod labels;
pub mod overview;
pub mod profile;
pub mod report;
pub mod sensors;
pub mod summary;
pub mod system;
pub mod udev;
pub mod units;
