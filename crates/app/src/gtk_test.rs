// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests with widgets. GTK runs on the one thread that initialized it,
//! while each test has a thread of its own, so the tests hand their
//! widgets work to one thread of GTK's and wait for it. Without a display
//! GTK does not start and the tests are skipped. A warning or an error of
//! GTK or GLib during a test fails it.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread;

use gtk::glib;

type Job = Box<dyn FnOnce() + Send>;

/// The warnings and errors logged since the last test took them.
static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Keeps the warnings and errors for `run` and logs everything as GLib
/// would.
fn record_warnings() {
    glib::log_set_writer_func(|level, fields| {
        if matches!(
            level,
            glib::LogLevel::Warning | glib::LogLevel::Critical | glib::LogLevel::Error
        ) {
            let field = |key| {
                fields
                    .iter()
                    .find(|field| field.key() == key)
                    .and_then(|field| field.value_str())
                    .unwrap_or_default()
            };
            let text = format!("{}: {}", field("GLIB_DOMAIN"), field("MESSAGE"));
            WARNINGS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(text);
        }
        glib::log_writer_default(level, fields)
    });
}

/// The thread of GTK, or `None` without a display.
fn gtk_thread() -> Option<&'static Mutex<mpsc::Sender<Job>>> {
    static THREAD: OnceLock<Option<Mutex<mpsc::Sender<Job>>>> = OnceLock::new();
    THREAD
        .get_or_init(|| {
            let (jobs, receive_jobs) = mpsc::channel::<Job>();
            let (started, receive_started) = mpsc::channel();
            thread::spawn(move || {
                // The tests share one process, which stays in the "C"
                // locale: the texts are the English ones.
                gtk::disable_setlocale();
                record_warnings();
                let ok = gtk::init().is_ok();
                let _ = started.send(ok);
                if ok {
                    for job in receive_jobs {
                        job();
                    }
                }
            });
            receive_started
                .recv()
                .unwrap_or(false)
                .then(|| Mutex::new(jobs))
        })
        .as_ref()
}

/// Runs `test` on the thread of GTK; a failure there fails the test. Says
/// so and returns without a display.
pub fn run(name: &str, test: impl FnOnce() + Send + 'static) {
    let Some(thread) = gtk_thread() else {
        eprintln!("no display: {name} skipped");
        return;
    };
    let (done, receive_done) = mpsc::channel();
    let job: Job = Box::new(move || {
        let result = panic::catch_unwind(AssertUnwindSafe(test));
        let warnings = std::mem::take(&mut *WARNINGS.lock().unwrap_or_else(|e| e.into_inner()));
        let result = match result {
            Ok(()) if !warnings.is_empty() => {
                panic::catch_unwind(|| panic!("GTK or GLib warned: {warnings:#?}"))
            }
            other => other,
        };
        let _ = done.send(result);
    });
    thread
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(job)
        .expect("the thread of GTK is gone");
    if let Err(failure) = receive_done.recv().expect("the thread of GTK is gone") {
        panic::resume_unwind(failure);
    }
}
