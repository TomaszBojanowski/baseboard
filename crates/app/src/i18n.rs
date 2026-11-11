// SPDX-License-Identifier: GPL-3.0-or-later

//! Locale and gettext setup, and the decimal separator of `LC_NUMERIC`
//! (D24).
//!
//! An FFI module: `setlocale` changes the process-wide C locale, which is
//! why it is unsafe, and `nl_langinfo` reads it.

#![allow(unsafe_code)]

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The separator of a named locale, without touching the process-wide
    /// one; `None` if the locale is not installed.
    fn separator_of(name: &CStr) -> Option<String> {
        // SAFETY: `name` is NUL-terminated; a null base asks for a new
        // locale object. The result is null or an object we own.
        let locale =
            unsafe { libc::newlocale(libc::LC_NUMERIC_MASK, name.as_ptr(), std::ptr::null_mut()) };
        if locale.is_null() {
            return None;
        }
        // SAFETY: `locale` is a valid locale object until freed below.
        let radix = copy(unsafe { libc::nl_langinfo_l(libc::RADIXCHAR, locale) });
        // SAFETY: frees the object made above, once; `radix` is a copy.
        unsafe { libc::freelocale(locale) };
        Some(radix)
    }

    #[test]
    fn separators_of_real_locales() {
        assert_eq!(separator_of(c"C").as_deref(), Some("."));
        for (name, expected) in [(c"pl_PL.UTF-8", ","), (c"en_US.UTF-8", ".")] {
            match separator_of(name) {
                Some(separator) => assert_eq!(separator, expected, "{name:?}"),
                None => eprintln!("{name:?} not installed, skipped"),
            }
        }
    }

    #[test]
    fn current_locale_of_the_tests() {
        // The test process never calls setlocale, so it runs in "C".
        assert_eq!(decimal_separator(), ".");
    }
}
