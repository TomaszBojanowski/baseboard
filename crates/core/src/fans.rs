// SPDX-License-Identifier: GPL-3.0-or-later

//! Fan control (docs/PROJEKT.md §5.5). For now only the driver layer that
//! detects what the `nct6683` driver allows for each channel.

pub mod driver;
