// SPDX-License-Identifier: GPL-3.0-or-later

//! How a test leaves out what this computer cannot do (docs/PROJEKT.md §8,
//! Z12). A tool, library or display the test needs and does not find is a
//! failure under `meson test`, which sets `BASEBOARD_TESTS_STRICT=1`; with
//! plain `cargo test` the test says so and passes. Hardware the computer
//! does not have is never a failure: the test says that it does not apply,
//! and why.

/// Leaves the test out for a missing tool, library or display.
pub fn skip(test: &str, why: &str) {
    if std::env::var_os("BASEBOARD_TESTS_STRICT").is_some_and(|value| value == "1") {
        panic!("{test}: {why}; BASEBOARD_TESTS_STRICT=1 allows no skipped test");
    }
    eprintln!("{test} skipped: {why}");
}

/// Says that the test, or a part of it, does not apply to the hardware of
/// this computer.
pub fn not_applicable(test: &str, why: &str) {
    eprintln!("{test}: not applicable: {why}");
}
