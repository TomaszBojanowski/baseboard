// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiles the interface files and icons into a GResource bundle that the
//! binary includes, so that `cargo build` alone gives a working program.

fn main() {
    glib_build_tools::compile_resources(
        &["src", "../../data"],
        "src/baseboard.gresource.xml",
        "baseboard.gresource",
    );
}
