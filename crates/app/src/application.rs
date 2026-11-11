// SPDX-License-Identifier: GPL-3.0-or-later

use adw::prelude::*;
use baseboard_core::labels;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config::{self, APP_ID, RESOURCE_BASE_PATH, VERSION};
use crate::sampler::{Options, Setup};
use crate::system::{FileKind, SkippedFile};
use crate::window::BaseboardWindow;

pub fn run() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .resource_base_path(RESOURCE_BASE_PATH)
        .build();
    app.connect_startup(setup_actions);
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        if gio::SettingsSchemaSource::default()
            .and_then(|s| s.lookup(APP_ID, true))
            .is_none()
        {
            eprintln!(
                "baseboard: GSettings schema {APP_ID} not found. \
                 Run from the build directory with `meson devenv -C build ./crates/app/baseboard`, \
                 or install the application."
            );
            app.quit();
            return;
        }
        let window = BaseboardWindow::new(app);
        let (dmi, detection, mut skipped) = crate::board::detect();
        window.show_board(&dmi, &detection);
        let profile = detection.profile().cloned();
        // The user's own names; a wrong file is skipped like a profile.
        let labels = profile.as_ref().and_then(|profile| {
            let dir = config::user_labels_dir();
            let path = labels::path(&dir, profile);
            window.set_labels_file(Some(path.clone()));
            labels::load(&dir, profile)
                .inspect_err(|e| {
                    eprintln!("baseboard: skipping {}: {e}", path.display());
                    skipped.push(SkippedFile::new(
                        path.clone(),
                        FileKind::Labels,
                        &e.to_string(),
                    ));
                })
                .ok()
                .flatten()
        });
        window.start_overview(Setup {
            root: config::sysfs_root(),
            board_name: dmi
                .board_name
                .clone()
                .or_else(|| profile.as_ref().map(|p| p.model.clone())),
            profile,
            labels,
            cpu: baseboard_core::system::cpu_model(std::path::Path::new("/proc")),
            // From the settings, in the window.
            options: Options::default(),
        });
        let skipped_count = skipped.len();
        window.start_system(crate::system::Setup {
            root: config::sysfs_root(),
            dmi,
            detection,
            skipped,
        });
        // Nothing stops: the page names the files, a toast points there.
        window.tell_skipped(skipped_count);
        #[cfg(debug_assertions)]
        debug::prepare(&window);
        window.present();
        #[cfg(debug_assertions)]
        debug::presented(&window);
        #[cfg(debug_assertions)]
        debug::dialog(app, &window);
        #[cfg(debug_assertions)]
        debug::screenshot(app, &window);
    });
    app.run()
}

fn setup_actions(app: &adw::Application) {
    // Through the windows, so that each one saves its size.
    let quit = gio::ActionEntry::builder("quit")
        .activate(|app: &adw::Application, _, _| {
            for window in app.windows() {
                window.close();
            }
        })
        .build();
    let preferences = gio::ActionEntry::builder("preferences")
        .activate(|app: &adw::Application, _, _| {
            if let Some(window) = app.active_window().and_downcast::<BaseboardWindow>() {
                let _ = window.show_preferences();
            }
        })
        .build();
    let about = gio::ActionEntry::builder("about")
        .activate(|app: &adw::Application, _, _| show_about(app))
        .build();
    app.add_action_entries([quit, preferences, about]);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    app.set_accels_for_action("window.close", &["<Control>w"]);
    // app.shortcuts and its <Control>question come from AdwApplication,
    // which loads shortcuts-dialog.ui from the resources.
}

/// Public descriptions of the hardware that Baseboard relies on
/// (docs/PROJEKT.md §2), with their addresses.
fn hardware_sources() -> [String; 4] {
    [
        format!(
            "{} https://docs.kernel.org/hwmon/nct6683.html",
            gettext("The nct6683 driver of Linux and its documentation")
        ),
        format!(
            "{} https://docs.kernel.org/hwmon/sysfs-interface.html",
            gettext("The hwmon sysfs interface of Linux")
        ),
        "lm-sensors https://github.com/lm-sensors/lm-sensors".to_owned(),
        format!(
            "{} https://gitlab.com/CalcProgrammer1/OpenRGB",
            gettext("OpenRGB issue reports on the RGB protocol")
        ),
    ]
}

fn show_about(app: &adw::Application) {
    let dialog = adw::AboutDialog::builder()
        .application_name("Baseboard")
        .application_icon(APP_ID)
        .developer_name("Tomasz Bojanowski")
        .version(VERSION)
        .comments(gettext("Monitor your motherboard sensors"))
        .copyright("© 2026 Tomasz Bojanowski")
        .license_type(gtk::License::Gpl30)
        // Translators: your name and email, one translator per line.
        .translator_credits(gettext("translator-credits"))
        .build();
    // Only once the project has public addresses (config.rs).
    if !config::WEBSITE.is_empty() {
        dialog.set_website(config::WEBSITE);
    }
    if !config::ISSUES.is_empty() {
        dialog.set_issue_url(config::ISSUES);
    }
    let sources = hardware_sources();
    let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
    dialog.add_acknowledgement_section(
        Some(&gettext("Hardware descriptions from public sources")),
        &sources,
    );
    dialog.present(app.active_window().as_ref());
}

/// Debug builds only: settings for looking at the window without a mouse.
/// GNOME Shell does not let other programs take screenshots, so the window
/// renders itself to a PNG.
#[cfg(debug_assertions)]
mod debug {
    use std::path::{Path, PathBuf};

    use adw::prelude::*;
    use gtk::glib;

    use crate::window::{BaseboardWindow, Page};

    fn var(name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }

    /// Before the window is shown: `BASEBOARD_DEBUG_SIZE=1200x800` gives it
    /// that size; `BASEBOARD_DEBUG_SHOW_CONTENT=1` shows the page rather
    /// than the sidebar when the window is narrow;
    /// `BASEBOARD_DEBUG_PAGE=system` opens the System page.
    pub fn prepare(window: &BaseboardWindow) {
        if var("BASEBOARD_DEBUG_PAGE").as_deref() == Some("system") {
            window.select_page(Page::System);
        }
        if let Some((w, h)) = var("BASEBOARD_DEBUG_SIZE").and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        }) {
            window.unmaximize();
            window.set_default_size(w, h);
        }
        if var("BASEBOARD_DEBUG_SHOW_CONTENT").is_some() {
            window.show_content();
        }
    }

    /// After the window is shown: `BASEBOARD_DEBUG_MINIMIZED=1` minimizes
    /// it, to check that reading stops while it cannot be seen.
    /// `BASEBOARD_DEBUG_DIALOG` opens a dialog a second after the window is
    /// shown, when the Overview has its first sample: `preferences`,
    /// `readings` (the second page of the preferences), `about` or
    /// `shortcuts`.
    pub fn dialog(app: &adw::Application, window: &BaseboardWindow) {
        let Some(name) = var("BASEBOARD_DEBUG_DIALOG") else {
            return;
        };
        glib::timeout_add_seconds_local_once(
            1,
            glib::clone!(
                #[weak]
                app,
                #[weak]
                window,
                move || match name.as_str() {
                    "preferences" => {
                        let _ = window.show_preferences();
                    }
                    "readings" => window.show_preferences().set_visible_page_name("readings"),
                    "about" | "shortcuts" => app.activate_action(&name, None),
                    other => eprintln!("baseboard: no dialog {other:?}"),
                }
            ),
        );
    }

    pub fn presented(window: &BaseboardWindow) {
        if var("BASEBOARD_DEBUG_MINIMIZED").is_some() {
            window.minimize();
        }
    }

    /// `BASEBOARD_DEBUG_SCREENSHOT=shot.png` saves the window to a PNG after
    /// `BASEBOARD_DEBUG_SCREENSHOT_DELAY` seconds (default 3) and quits. The
    /// file goes to `BASEBOARD_DEBUG_SCREENSHOT_DIR`, by default
    /// `$XDG_CACHE_HOME/baseboard/`, and never into the source tree. The
    /// window is rendered at the scale of its surface, so the PNG has the
    /// pixels of the screen. A screenshot that cannot be saved quits too, so
    /// that no window is left behind.
    pub fn screenshot(app: &adw::Application, window: &BaseboardWindow) {
        let Some(name) = var("BASEBOARD_DEBUG_SCREENSHOT") else {
            return;
        };
        let path = match screenshot_path(
            &name,
            var("BASEBOARD_DEBUG_SCREENSHOT_DIR").map(PathBuf::from),
            &glib::user_cache_dir(),
            &source_root(),
        ) {
            Ok(path) => path,
            Err(e) => {
                eprintln!("baseboard: no screenshot: {e}");
                app.quit();
                return;
            }
        };
        let delay: u32 = var("BASEBOARD_DEBUG_SCREENSHOT_DELAY")
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        // `BASEBOARD_DEBUG_SCROLL=end`: the page, or the page of an open
        // dialog, scrolled to its end a second before the screenshot.
        if var("BASEBOARD_DEBUG_SCROLL").as_deref() == Some("end") {
            glib::timeout_add_seconds_local_once(
                delay.saturating_sub(1),
                glib::clone!(
                    #[weak]
                    window,
                    move || window.scroll_page_to_end()
                ),
            );
        }
        // `BASEBOARD_DEBUG_COPY=1`: "Copy system information" a second
        // before the screenshot, which then shows the toast.
        if var("BASEBOARD_DEBUG_COPY").is_some() {
            glib::timeout_add_seconds_local_once(
                delay.saturating_sub(1),
                glib::clone!(
                    #[weak]
                    window,
                    move || {
                        let _ = WidgetExt::activate_action(&window, "win.copy-system-info", None);
                    }
                ),
            );
        }
        let window: &gtk::Window = window.upcast_ref();
        glib::timeout_add_seconds_local_once(
            delay,
            glib::clone!(
                #[weak]
                app,
                #[weak]
                window,
                move || {
                    save_png(&window, &path);
                    app.quit();
                }
            ),
        );
    }

    /// The root of the source tree, known at build time.
    fn source_root() -> PathBuf {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        root.canonicalize().unwrap_or(root)
    }

    /// Where a screenshot goes: `name` (a file name; `.png` is added when
    /// missing) in `dir`, or in `<cache>/baseboard` without it. The folder is
    /// created and must lie outside `source_root`.
    fn screenshot_path(
        name: &str,
        dir: Option<PathBuf>,
        cache: &Path,
        source_root: &Path,
    ) -> Result<PathBuf, String> {
        if name.contains('/') || name == "." || name == ".." {
            return Err(format!(
                "{name:?} is not a file name; set the folder with BASEBOARD_DEBUG_SCREENSHOT_DIR"
            ));
        }
        let dir = dir.unwrap_or_else(|| cache.join("baseboard"));
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let dir = dir
            .canonicalize()
            .map_err(|e| format!("{}: {e}", dir.display()))?;
        if dir.starts_with(source_root) {
            return Err(format!(
                "{} is inside the source tree {}",
                dir.display(),
                source_root.display()
            ));
        }
        let file = if name.ends_with(".png") {
            name.to_owned()
        } else {
            format!("{name}.png")
        };
        Ok(dir.join(file))
    }

    fn save_png(window: &gtk::Window, path: &Path) {
        let paintable = gtk::WidgetPaintable::new(Some(window));
        let snapshot = gtk::Snapshot::new();
        let scale = window
            .native()
            .and_then(|n| n.surface())
            .map_or(1.0, |s| s.scale());
        let (dx, dy) = window
            .native()
            .map_or((0.0, 0.0), |n| n.surface_transform());
        snapshot.scale(scale as f32, scale as f32);
        snapshot.translate(&gtk::graphene::Point::new(dx as f32, dy as f32));
        paintable.snapshot(&snapshot, window.width().into(), window.height().into());
        let texture = snapshot.to_node().and_then(|node| {
            let renderer = window.native()?.renderer()?;
            Some(renderer.render_texture(&node, None))
        });
        match texture {
            Some(t) => match t.save_to_png(path) {
                Ok(()) => eprintln!(
                    "baseboard: screenshot saved to {} ({}×{} px, scale {scale})",
                    path.display(),
                    t.width(),
                    t.height()
                ),
                Err(e) => eprintln!("baseboard: cannot save screenshot: {e}"),
            },
            None => eprintln!("baseboard: nothing to render"),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A fresh folder in the system temporary directory for one test.
        fn scratch(test: &str) -> PathBuf {
            let dir =
                std::env::temp_dir().join(format!("baseboard-test-{}-{test}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            dir.canonicalize().unwrap()
        }

        #[test]
        fn default_folder_is_in_the_cache() {
            let base = scratch("cache");
            let path =
                screenshot_path("shot", None, &base.join("cache"), &base.join("src")).unwrap();
            assert_eq!(path, base.join("cache/baseboard/shot.png"));
            assert!(base.join("cache/baseboard").is_dir());
            std::fs::remove_dir_all(base).unwrap();
        }

        #[test]
        fn folder_from_the_environment() {
            let base = scratch("env");
            let path = screenshot_path(
                "shot.png",
                Some(base.join("out")),
                &base.join("cache"),
                &base.join("src"),
            )
            .unwrap();
            assert_eq!(path, base.join("out/shot.png"));
            std::fs::remove_dir_all(base).unwrap();
        }

        #[test]
        fn no_screenshots_in_the_source_tree() {
            let base = scratch("tree");
            let source = base.join("src");
            std::fs::create_dir_all(&source).unwrap();
            let inside = screenshot_path(
                "shot.png",
                Some(source.join("shots")),
                &base.join("cache"),
                &source,
            );
            assert!(inside.is_err());
            std::fs::remove_dir_all(base).unwrap();
        }

        #[test]
        fn only_file_names() {
            let base = scratch("name");
            for name in ["dir/shot.png", ".", ".."] {
                let result = screenshot_path(name, None, &base, &base.join("src"));
                assert!(result.is_err(), "{name}");
            }
            std::fs::remove_dir_all(base).unwrap();
        }
    }
}
