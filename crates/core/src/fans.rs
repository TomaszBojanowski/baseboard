// SPDX-License-Identifier: GPL-3.0-or-later

//! Fan control (docs/PROJEKT.md §5.5): the driver layer, which reads what
//! the `nct6683` driver reports for each channel, and the curve model,
//! which computes what the board's controller does with a curve.

pub mod curve;
pub mod driver;
