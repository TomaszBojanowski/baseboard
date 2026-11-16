// SPDX-License-Identifier: GPL-3.0-or-later

//! Libraries loaded at run time with `dlopen` (docs/PROJEKT.md §3.2, D21),
//! so that the app still starts when one is missing: `libsensors` and
//! `libudev` open theirs here and take their functions from it.
//!
//! One of the FFI modules with unsafe code (with `libsensors`, `libudev`
//! and `i18n`).

#![allow(unsafe_code, reason = "an FFI module: dlopen, dlsym and dlclose")]

use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

/// A library opened with dlopen, closed on drop.
pub struct Library(NonNull<c_void>);

impl Library {
    /// The first of `names` that loads, or why the last one did not.
    pub fn open(names: &[&CStr]) -> Result<Self, String> {
        let mut error = String::from("no library name");
        for name in names {
            // SAFETY: `name` is NUL-terminated. The libraries opened here
            // (libsensors, libudev) run only their own and libc's
            // initialisers.
            let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if let Some(handle) = NonNull::new(handle) {
                return Ok(Self(handle));
            }
            error = dlerror();
        }
        Err(error)
    }

    /// The address of the function or variable `name`, if the library has
    /// it. It is valid while the library is open.
    pub fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>> {
        // SAFETY: the handle stays open until drop; `name` is NUL-terminated.
        NonNull::new(unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: opened by dlopen and closed once; the modules that use a
        // library keep it until they no longer use a pointer into it.
        unsafe { libc::dlclose(self.0.as_ptr()) };
    }
}

/// The message of the last failed dl* call on this thread.
fn dlerror() -> String {
    // SAFETY: dlerror returns null or a message that stays valid until the
    // next dl* call on this thread; it is copied at once.
    let message = unsafe { libc::dlerror() };
    if message.is_null() {
        return "unknown dlopen error".to_owned();
    }
    // SAFETY: not null and NUL-terminated (above).
    unsafe { CStr::from_ptr(message) }
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first name that loads is opened; a library that is not there
    /// gives the reason; a function is found, a name that is none is not.
    #[test]
    fn opens_the_first_library_there() {
        let library = Library::open(&[c"libbaseboard-none.so.0", c"libc.so.6"]).unwrap();
        assert!(library.symbol(c"strlen").is_some());
        assert!(library.symbol(c"baseboard_none").is_none());
        let error = Library::open(&[c"libbaseboard-none.so.0"]).err().unwrap();
        assert!(error.contains("libbaseboard-none.so.0"), "{error}");
        assert_eq!(Library::open(&[]).err().unwrap(), "no library name");
    }
}
