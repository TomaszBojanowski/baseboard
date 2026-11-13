// SPDX-License-Identifier: GPL-3.0-or-later

//! Baseboard: monitoring and control of motherboard hardware for GNOME.

// Every dependency is used (docs/PROJEKT.md §8, Z4).
#![warn(unused_crate_dependencies)]

mod alert;
mod application;
mod board;
mod config;
mod fan_chart;
mod fans;
#[cfg(test)]
mod gtk_test;
mod i18n;
mod libsensors;
mod libudev;
mod overview;
mod preferences;
mod sampler;
mod sensors;
mod system;
mod system_reader;
#[cfg(test)]
mod test_env;
#[cfg(debug_assertions)]
mod tour;
mod window;

use gtk::{gio, glib};

fn main() -> glib::ExitCode {
    i18n::init();

    gio::resources_register_include!("baseboard.gresource")
        .expect("the resource bundle is built into the program");

    glib::set_application_name("Baseboard");
    application::run()
}
