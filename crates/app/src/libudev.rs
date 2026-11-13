// SPDX-License-Identifier: GPL-3.0-or-later

//! The udev database through `dlopen("libudev.so.1")` (D21): properties of
//! devices, such as the model name from the hardware database
//! (`ID_MODEL_FROM_DATABASE`) or the memory slots from SMBIOS
//! (`MEMORY_DEVICE_*` of the DMI device). Which properties are read is up
//! to `core::system` and `core::overview`.
//!
//! One of the FFI modules with unsafe code (with `libsensors` and `i18n`).
//! The declarations follow `libudev.h` of systemd. A udev context is not thread
//! safe, so [`Libudev`] stays on the thread that made it (it is neither
//! `Send` nor `Sync`). It only reads the database.

#![allow(unsafe_code, reason = "an FFI module: libudev through dlopen")]

use std::ffi::{CStr, CString, c_char, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::NonNull;

use baseboard_core::udev::Udev;
use gtk::glib;

const LIBRARY: &CStr = c"libudev.so.1";

/// The udev database for the sysfs `root`: libudev for the real /sys, none
/// for a fake tree, which the database does not describe, or when libudev
/// cannot be loaded.
pub fn for_root(root: &Path) -> Option<Libudev> {
    if root != Path::new("/sys") {
        return None;
    }
    Libudev::load()
        .inspect_err(|e| glib::g_debug!("baseboard", "no udev database: {e}"))
        .ok()
}

/// `struct udev *udev_new(void)`
type NewFn = unsafe extern "C" fn() -> *mut c_void;
/// `struct udev *udev_unref(struct udev *)`
type UnrefFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
/// `struct udev_device *udev_device_new_from_syspath(struct udev *, const char *)`
type DeviceFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
/// `const char *udev_device_get_property_value(struct udev_device *, const char *)`
type PropertyFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> *const c_char;
/// `struct udev_device *udev_device_unref(struct udev_device *)`
type DeviceUnrefFn = unsafe extern "C" fn(*mut c_void) -> *mut c_void;

/// libudev with one udev context.
pub struct Libudev {
    udev: NonNull<c_void>,
    unref: UnrefFn,
    device: DeviceFn,
    property: PropertyFn,
    device_unref: DeviceUnrefFn,
    // Closed after `Drop::drop` has released the context.
    _library: Library,
}

impl Libudev {
    /// Loads libudev and makes a udev context.
    pub fn load() -> Result<Self, String> {
        let library = Library::open(LIBRARY)?;
        let new = library.symbol(c"udev_new")?;
        let unref = library.symbol(c"udev_unref")?;
        let device = library.symbol(c"udev_device_new_from_syspath")?;
        let property = library.symbol(c"udev_device_get_property_value")?;
        let device_unref = library.symbol(c"udev_device_unref")?;
        // SAFETY: each symbol is the libudev function of that name and each
        // type is its C signature in libudev.h. On Linux a function pointer
        // has the size and representation of a data pointer, which dlsym
        // returns.
        let (new, unref, device, property, device_unref) = unsafe {
            (
                std::mem::transmute::<*mut c_void, NewFn>(new.as_ptr()),
                std::mem::transmute::<*mut c_void, UnrefFn>(unref.as_ptr()),
                std::mem::transmute::<*mut c_void, DeviceFn>(device.as_ptr()),
                std::mem::transmute::<*mut c_void, PropertyFn>(property.as_ptr()),
                std::mem::transmute::<*mut c_void, DeviceUnrefFn>(device_unref.as_ptr()),
            )
        };
        // SAFETY: `udev_new` takes no arguments and returns a new context
        // or null.
        let udev = NonNull::new(unsafe { new() }).ok_or("udev_new failed")?;
        Ok(Self {
            udev,
            unref,
            device,
            property,
            device_unref,
            _library: library,
        })
    }
}

impl Udev for Libudev {
    fn property(&self, device: &Path, name: &str) -> Option<String> {
        let syspath = CString::new(Path::new("/sys").join(device).as_os_str().as_bytes()).ok()?;
        let key = CString::new(name).ok()?;
        // SAFETY: the context is alive while `self` is; `syspath` is
        // NUL-terminated. The result is null or a device we own one
        // reference to.
        let dev = NonNull::new(unsafe { (self.device)(self.udev.as_ptr(), syspath.as_ptr()) })?;
        // SAFETY: `dev` is a live device and `key` is NUL-terminated. The
        // result is null or a string owned by the device.
        let value = unsafe { (self.property)(dev.as_ptr(), key.as_ptr()) };
        let copy = (!value.is_null()).then(|| {
            // SAFETY: not null, NUL-terminated, and valid until the device
            // is released below.
            unsafe { CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned()
        });
        // SAFETY: releases the one reference taken above; `value` is not
        // used afterwards.
        unsafe { (self.device_unref)(dev.as_ptr()) };
        copy.filter(|v| !v.is_empty())
    }
}

impl Drop for Libudev {
    fn drop(&mut self) {
        // SAFETY: releases the context made in `load`, once; no device
        // outlives a call of `property`. The library is closed afterwards.
        unsafe { (self.unref)(self.udev.as_ptr()) };
    }
}

/// A library opened with dlopen, closed on drop.
struct Library(NonNull<c_void>);

impl Library {
    fn open(name: &CStr) -> Result<Self, String> {
        // SAFETY: `name` is NUL-terminated. Loading libudev runs only its
        // own and libc's initialisers.
        let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        NonNull::new(handle).map(Self).ok_or_else(dlerror)
    }

    fn symbol(&self, name: &CStr) -> Result<NonNull<c_void>, String> {
        // SAFETY: the handle stays open until drop; `name` is NUL-terminated.
        let symbol = unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) };
        NonNull::new(symbol).ok_or_else(|| format!("libudev lacks {}", name.to_string_lossy()))
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: opened by dlopen and closed once; no pointer into the
        // library is used after this.
        unsafe { libc::dlclose(self.0.as_ptr()) };
    }
}

fn dlerror() -> String {
    // SAFETY: dlerror returns null or a message valid until the next dl*
    // call on this thread; it is copied at once.
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
    //! These read the real udev database; where libudev is missing they
    //! are skipped.

    use super::*;

    #[test]
    fn properties_of_a_real_device() {
        let udev = match Libudev::load() {
            Ok(udev) => udev,
            Err(e) => {
                let why = format!("libudev not available: {e}");
                crate::test_env::skip("properties_of_a_real_device", &why);
                return;
            }
        };
        // The DMI device exists on every PC with udev.
        let dmi = Path::new("devices/virtual/dmi/id");
        if Path::new("/sys").join(dmi).exists() {
            assert_eq!(udev.property(dmi, "SUBSYSTEM").as_deref(), Some("dmi"));
        } else {
            crate::test_env::not_applicable("properties_of_a_real_device", "no DMI device");
        }
        assert_eq!(udev.property(dmi, "BASEBOARD_NO_SUCH_PROPERTY"), None);
        assert_eq!(
            udev.property(Path::new("devices/no/such/device"), "SUBSYSTEM"),
            None
        );
        assert_eq!(
            udev.property(Path::new("devices/nul\0in/path"), "SUBSYSTEM"),
            None
        );
    }
}
