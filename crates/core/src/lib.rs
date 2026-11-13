// SPDX-License-Identifier: GPL-3.0-or-later

//! Logic for the Baseboard app and, later, its system helper: board
//! profiles, the sensor model and the fan driver layer.
//!
//! The helper runs as root, so this crate has no user interface, no
//! translated text and no unsafe code. Both programs compute the same
//! results from it; the helper always has the final say.

// Every dependency is used (docs/PROJEKT.md §8, Z4). Not in the workspace
// lints: the integration tests would report the crate's own dependencies.
#![warn(unused_crate_dependencies)]

pub mod board;
pub mod fans;
pub mod heat;
pub mod history;
pub mod labels;
pub mod overview;
pub mod profile;
pub mod report;
pub mod sensors;
pub mod summary;
pub mod system;
pub mod udev;
pub mod units;
