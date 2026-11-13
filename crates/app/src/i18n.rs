// SPDX-License-Identifier: GPL-3.0-or-later

//! Locale and gettext setup, and the decimal separator of `LC_NUMERIC`
//! (D24).
//!
//! An FFI module: `setlocale` changes the process-wide C locale, which is
//! why it is unsafe, and `nl_langinfo` reads it.

#![allow(unsafe_code, reason = "an FFI module: setlocale and nl_langinfo")]

use std::ffi::CStr;

use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, setlocale, textdomain};

use crate::config;

/// Sets the locale from the environment and binds the text domain. Must run
/// first in `main`, before any other thread starts.
pub fn init() {
    // SAFETY: called first thing in `main`, before GTK or any other thread
    // exists, so nothing reads the locale while it changes.
    unsafe {
        setlocale(LocaleCategory::LcAll, "");
    }
    // Without the translations the interface stays in English.
    let _ = bindtextdomain(config::GETTEXT_PACKAGE, config::localedir());
    let _ = bind_textdomain_codeset(config::GETTEXT_PACKAGE, "UTF-8");
    let _ = textdomain(config::GETTEXT_PACKAGE);
}

/// The decimal separator of the current `LC_NUMERIC`, e.g. "," for Polish;
/// "." if the locale gives none.
pub fn decimal_separator() -> String {
    // SAFETY: RADIXCHAR is a valid item. The result points to a
    // NUL-terminated string in the locale's data, valid until the locale
    // changes, which happens only in `init`, before any thread starts; it
    // is copied at once.
    let radix = unsafe { libc::nl_langinfo(libc::RADIXCHAR) };
    copy(radix)
}

fn copy(ptr: *const libc::c_char) -> String {
    if ptr.is_null() {
        return ".".to_owned();
    }
    // SAFETY: not null and NUL-terminated (nl_langinfo's contract); read
    // before any further locale call.
    let text = unsafe { CStr::from_ptr(ptr) }
        .to_string_lossy()
        .into_owned();
    if text.is_empty() {
        ".".to_owned()
    } else {
        text
    }
}
