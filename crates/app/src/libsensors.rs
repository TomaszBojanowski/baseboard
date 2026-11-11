// SPDX-License-Identifier: GPL-3.0-or-later

//! libsensors (lm-sensors) through `dlopen` (docs/PROJEKT.md §5.1): the
//! `ignore`, `compute` and `label` statements of /etc/sensors3.conf and
//! /etc/sensors.d. The library is loaded at run time, so the app still
//! starts when it is missing and reads raw sysfs instead.
//!
//! This is the only module of the app with unsafe code. The declarations
//! follow `sensors.h` of lm-sensors 3.6.0. libsensors keeps global state and
//! is not thread safe, so there is at most one [`Libsensors`] at a time and
//! it stays on the thread that made it (it is neither `Send` nor `Sync`).
//! It only reads: `set` statements are never executed.

#![allow(unsafe_code)]

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char, c_int, c_short, c_uint, c_void};
use std::fmt;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, Ordering};

use baseboard_core::sensors::{LmChip, LmFeature};

/// The sonames to try: upstream 3.6 is `.so.5`, Fedora keeps `.so.4`.
const LIBRARIES: [&CStr; 2] = [c"libsensors.so.5", c"libsensors.so.4"];

// The C structures of sensors.h. Not every field is read here.

#[repr(C)]
#[allow(dead_code)]
struct BusId {
    kind: c_short,
    nr: c_short,
}

#[repr(C)]
#[allow(dead_code)]
struct ChipName {
    prefix: *mut c_char,
    bus: BusId,
    addr: c_int,
    path: *mut c_char,
}

#[repr(C)]
#[allow(dead_code)]
struct Feature {
    name: *mut c_char,
    number: c_int,
    kind: c_int,
    first_subfeature: c_int,
    padding1: c_int,
}

#[repr(C)]
#[allow(dead_code)]
struct Subfeature {
    name: *mut c_char,
    number: c_int,
    kind: c_int,
    mapping: c_int,
    flags: c_uint,
}

type InitFn = unsafe extern "C" fn(*mut libc::FILE) -> c_int;
type CleanupFn = unsafe extern "C" fn();
type ChipNameFn = unsafe extern "C" fn(*mut c_char, libc::size_t, *const ChipName) -> c_int;
type DetectedChipsFn = unsafe extern "C" fn(*const ChipName, *mut c_int) -> *const ChipName;
type FeaturesFn = unsafe extern "C" fn(*const ChipName, *mut c_int) -> *const Feature;
type SubfeaturesFn =
    unsafe extern "C" fn(*const ChipName, *const Feature, *mut c_int) -> *const Subfeature;
type LabelFn = unsafe extern "C" fn(*const ChipName, *const Feature) -> *mut c_char;
type ValueFn = unsafe extern "C" fn(*const ChipName, c_int, *mut f64) -> c_int;

/// Why libsensors cannot be used.
#[derive(Debug)]
pub enum Error {
    /// Another [`Libsensors`] exists.
    Busy,
    /// No libsensors library could be loaded.
    Library(String),
    /// The library lacks a function.
    Symbol(&'static CStr),
    /// The configuration file given could not be opened.
    Config(PathBuf),
    /// `sensors_init` failed with this code (it cleans up after itself).
    Init(c_int),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("libsensors is already in use"),
            Self::Library(e) => write!(f, "libsensors not loaded: {e}"),
            Self::Symbol(name) => write!(f, "libsensors lacks {}", name.to_string_lossy()),
            Self::Config(path) => write!(f, "cannot open {}", path.display()),
            Self::Init(code) => write!(f, "sensors_init failed ({code})"),
        }
    }
}

impl std::error::Error for Error {}

/// libsensors, initialised with its configuration.
pub struct Libsensors {
    api: Api,
    version: String,
    /// The chips and features, with labels and the numbers of the input
    /// subfeatures, found on the first call of `chips`; they do not change
    /// until sensors_cleanup.
    layout: RefCell<Option<Vec<ChipLayout>>>,
    // Dropped in this order, after `Drop::drop` has run sensors_cleanup.
    _library: Library,
    _claim: Claim,
}

struct ChipLayout {
    /// Owned by libsensors, valid until sensors_cleanup (in `drop`).
    chip: *const ChipName,
    name: String,
    path: PathBuf,
    features: Vec<FeatureLayout>,
}

struct FeatureLayout {
    name: String,
    label: Option<String>,
    /// The number of `<name>_input` (for power, else `_average`).
    input: Option<c_int>,
}

impl Libsensors {
    /// Loads libsensors and reads the system configuration, or only
    /// `config` instead.
    pub fn load(config: Option<&Path>) -> Result<Self, Error> {
        Self::load_from(&LIBRARIES, config)
    }

    fn load_from(names: &[&CStr], config: Option<&Path>) -> Result<Self, Error> {
        let claim = Claim::take().ok_or(Error::Busy)?;
        let library = Library::open(names)?;
        let api = Api::resolve(&library)?;
        let version = library.version();
        let file = config.map(ConfigFile::open).transpose()?;
        let input = file.as_ref().map_or(ptr::null_mut(), |f| f.0.as_ptr());
        // SAFETY: `init` comes from the open library; `input` is null or a
        // file open for reading that stays open until the call returns.
        // The claim makes this the only user of libsensors' global state.
        let status = unsafe { (api.init)(input) };
        drop(file);
        if status != 0 {
            return Err(Error::Init(status));
        }
        Ok(Self {
            api,
            version,
            layout: RefCell::new(None),
            _library: library,
            _claim: claim,
        })
    }

    /// The version libsensors reports, e.g. "3.6.0".
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The detected chips with their features, after the `ignore`,
    /// `compute` and `label` statements. Everything is copied. Only the
    /// first call reads the labels; later ones read just the values.
    pub fn chips(&self) -> Vec<LmChip> {
        let mut layout = self.layout.borrow_mut();
        let layout = layout.get_or_insert_with(|| self.layout());
        layout
            .iter()
            .map(|chip| LmChip {
                name: chip.name.clone(),
                path: chip.path.clone(),
                features: chip
                    .features
                    .iter()
                    .map(|feature| LmFeature {
                        name: feature.name.clone(),
                        label: feature.label.clone(),
                        value: feature
                            .input
                            .and_then(|number| self.value(chip.chip, number)),
                    })
                    .collect(),
            })
            .collect()
    }

    fn layout(&self) -> Vec<ChipLayout> {
        let mut chips = Vec::new();
        let mut nr: c_int = 0;
        loop {
            // SAFETY: libsensors is initialised while `self` lives; a null
            // match lists every chip; `nr` is the iteration state.
            let chip = unsafe { (self.api.detected_chips)(ptr::null(), &mut nr) };
            // SAFETY: a non-null result points into libsensors' chip list,
            // valid until sensors_cleanup, which only `drop` calls.
            let Some(chip_ref) = (unsafe { chip.as_ref() }) else {
                break;
            };
            // SAFETY: `path` is null or a C string owned by libsensors.
            let Some(path) = (unsafe { text(chip_ref.path) }) else {
                continue;
            };
            chips.push(ChipLayout {
                chip,
                name: self.chip_name(chip_ref),
                path: PathBuf::from(path),
                features: self.features(chip_ref),
            });
        }
        chips
    }

    /// The value of a subfeature after the `compute` statement.
    fn value(&self, chip: *const ChipName, number: c_int) -> Option<f64> {
        let mut value = 0.0;
        // SAFETY: `chip` points into libsensors' chip list, valid until
        // sensors_cleanup, which runs only when `self` is dropped; `number`
        // is one of its subfeature numbers; `value` is a valid place for the
        // result.
        let status = unsafe { (self.api.value)(chip, number, &mut value) };
        (status == 0).then_some(value)
    }

    fn chip_name(&self, chip: &ChipName) -> String {
        let mut buffer = [0 as c_char; 256];
        // SAFETY: the buffer is writable for its whole length; `chip` comes
        // from libsensors and has no wildcards.
        let written = unsafe { (self.api.chip_name)(buffer.as_mut_ptr(), buffer.len(), chip) };
        if written < 0 {
            return String::new();
        }
        // SAFETY: on success the buffer holds a NUL-terminated string (the
        // function works like snprintf and truncates to the buffer).
        unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned()
    }

    fn features(&self, chip: &ChipName) -> Vec<FeatureLayout> {
        let mut features = Vec::new();
        let mut nr: c_int = 0;
        loop {
            // SAFETY: `chip` comes from libsensors; `nr` is the iteration
            // state. Ignored features are skipped by libsensors.
            let feature = unsafe { (self.api.features)(chip, &mut nr) };
            // SAFETY: a non-null result points into libsensors' feature
            // table, valid until sensors_cleanup.
            let Some(feature) = (unsafe { feature.as_ref() }) else {
                break;
            };
            // SAFETY: the name is a C string owned by libsensors.
            let Some(name) = (unsafe { text(feature.name) }) else {
                continue;
            };
            features.push(FeatureLayout {
                label: self.label(chip, feature),
                input: self.input(chip, feature, &name),
                name,
            });
        }
        features
    }

    fn label(&self, chip: &ChipName, feature: &Feature) -> Option<String> {
        // SAFETY: `chip` and `feature` come from libsensors. The result is
        // null or a string allocated with malloc, which the caller owns.
        let label = unsafe { (self.api.label)(chip, feature) };
        // SAFETY: null or a NUL-terminated string (above).
        let copy = unsafe { text(label) };
        // SAFETY: allocated with malloc by libsensors, freed once and not
        // used afterwards; free(NULL) does nothing.
        unsafe { libc::free(label.cast()) };
        copy
    }

    /// The number of the subfeature `<name>_input` (for power, else
    /// `<name>_average`).
    fn input(&self, chip: &ChipName, feature: &Feature, name: &str) -> Option<c_int> {
        let mut wanted = vec![format!("{name}_input")];
        if name.starts_with("power") {
            wanted.push(format!("{name}_average"));
        }
        let mut subfeatures = Vec::new();
        let mut nr: c_int = 0;
        loop {
            // SAFETY: `chip` and `feature` come from libsensors; `nr` is the
            // iteration state.
            let sub = unsafe { (self.api.subfeatures)(chip, feature, &mut nr) };
            // SAFETY: a non-null result points into libsensors' subfeature
            // table, valid until sensors_cleanup.
            let Some(sub) = (unsafe { sub.as_ref() }) else {
                break;
            };
            // SAFETY: the name is a C string owned by libsensors.
            if let Some(sub_name) = unsafe { text(sub.name) } {
                subfeatures.push((sub_name, sub.number));
            }
        }
        wanted.iter().find_map(|w| {
            subfeatures
                .iter()
                .find(|(sub_name, _)| sub_name == w)
                .map(|&(_, number)| number)
        })
    }
}

impl Drop for Libsensors {
    fn drop(&mut self) {
        // SAFETY: libsensors was initialised in `load_from`; nothing it owns
        // outlives `self`, since `chips` copies everything. The library is
        // closed only afterwards, when the fields are dropped.
        unsafe { (self.api.cleanup)() };
    }
}

/// The functions of libsensors that are used.
struct Api {
    init: InitFn,
    cleanup: CleanupFn,
    chip_name: ChipNameFn,
    detected_chips: DetectedChipsFn,
    features: FeaturesFn,
    subfeatures: SubfeaturesFn,
    label: LabelFn,
    value: ValueFn,
}

impl Api {
    fn resolve(library: &Library) -> Result<Self, Error> {
        let init = library.symbol(c"sensors_init")?;
        let cleanup = library.symbol(c"sensors_cleanup")?;
        let chip_name = library.symbol(c"sensors_snprintf_chip_name")?;
        let detected_chips = library.symbol(c"sensors_get_detected_chips")?;
        let features = library.symbol(c"sensors_get_features")?;
        let subfeatures = library.symbol(c"sensors_get_all_subfeatures")?;
        let label = library.symbol(c"sensors_get_label")?;
        let value = library.symbol(c"sensors_get_value")?;
        // SAFETY: each symbol is the libsensors function of that name and
        // each type is its C signature in sensors.h (3.6.0). On Linux a
        // function pointer has the size and representation of a data
        // pointer, which dlsym returns.
        unsafe {
            Ok(Self {
                init: std::mem::transmute::<*mut c_void, InitFn>(init.as_ptr()),
                cleanup: std::mem::transmute::<*mut c_void, CleanupFn>(cleanup.as_ptr()),
                chip_name: std::mem::transmute::<*mut c_void, ChipNameFn>(chip_name.as_ptr()),
                detected_chips: std::mem::transmute::<*mut c_void, DetectedChipsFn>(
                    detected_chips.as_ptr(),
                ),
                features: std::mem::transmute::<*mut c_void, FeaturesFn>(features.as_ptr()),
                subfeatures: std::mem::transmute::<*mut c_void, SubfeaturesFn>(
                    subfeatures.as_ptr(),
                ),
                label: std::mem::transmute::<*mut c_void, LabelFn>(label.as_ptr()),
                value: std::mem::transmute::<*mut c_void, ValueFn>(value.as_ptr()),
            })
        }
    }
}

/// A library opened with dlopen, closed on drop.
struct Library(NonNull<c_void>);

impl Library {
    fn open(names: &[&CStr]) -> Result<Self, Error> {
        let mut error = String::from("no library name");
        for name in names {
            // SAFETY: `name` is NUL-terminated. Loading libsensors runs only
            // its own and libc's initialisers.
            let handle = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if let Some(handle) = NonNull::new(handle) {
                return Ok(Self(handle));
            }
            error = dlerror();
        }
        Err(Error::Library(error))
    }

    fn symbol(&self, name: &'static CStr) -> Result<NonNull<c_void>, Error> {
        // SAFETY: the handle stays open until drop; `name` is NUL-terminated.
        let symbol = unsafe { libc::dlsym(self.0.as_ptr(), name.as_ptr()) };
        NonNull::new(symbol).ok_or(Error::Symbol(name))
    }

    /// The `libsensors_version` string, if the library has one.
    fn version(&self) -> String {
        let Ok(symbol) = self.symbol(c"libsensors_version") else {
            return String::new();
        };
        // SAFETY: `libsensors_version` is a `const char *` variable; dlsym
        // gives its address.
        let version = unsafe { *symbol.as_ptr().cast::<*const c_char>() };
        // SAFETY: the variable points to a static C string, or is null.
        unsafe { text(version) }.unwrap_or_default()
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: opened by dlopen and closed once; no function or data
        // pointer into the library is used after this.
        unsafe { libc::dlclose(self.0.as_ptr()) };
    }
}

/// A configuration file opened with fopen, closed on drop.
struct ConfigFile(NonNull<libc::FILE>);

impl ConfigFile {
    fn open(path: &Path) -> Result<Self, Error> {
        let name =
            CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::Config(path.into()))?;
        // SAFETY: both strings are NUL-terminated.
        let file = unsafe { libc::fopen(name.as_ptr(), c"r".as_ptr()) };
        NonNull::new(file)
            .map(Self)
            .ok_or_else(|| Error::Config(path.into()))
    }
}

impl Drop for ConfigFile {
    fn drop(&mut self) {
        // SAFETY: opened by fopen and closed once.
        unsafe { libc::fclose(self.0.as_ptr()) };
    }
}

/// Only one [`Libsensors`] may exist: libsensors has global state.
static IN_USE: AtomicBool = AtomicBool::new(false);

struct Claim;

impl Claim {
    fn take() -> Option<Self> {
        (!IN_USE.swap(true, Ordering::AcqRel)).then_some(Self)
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        IN_USE.store(false, Ordering::Release);
    }
}

fn dlerror() -> String {
    // SAFETY: dlerror returns null or a message that stays valid until the
    // next dl* call on this thread; `text` copies it at once.
    unsafe { text(libc::dlerror()) }.unwrap_or_else(|| "unknown error".to_owned())
}

/// A copy of a C string; `None` for a null pointer.
///
/// # Safety
///
/// `ptr` must be null or point to a NUL-terminated string that stays valid
/// during the call.
unsafe fn text(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: not null, and NUL-terminated by the caller's guarantee.
    Some(
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(test)]
mod tests {
    //! These tests read the real /sys through libsensors (it cannot be
    //! pointed elsewhere) with a configuration of their own, never the
    //! system one. Where libsensors is missing they are skipped.

    use std::collections::HashSet;
    use std::sync::Mutex;

    use baseboard_core::sensors::{self, Chip, FeatureKey, Kind, Round, SensorSource, SysfsSource};

    use super::*;

    /// libsensors has global state: one test at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        SERIAL.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A configuration file for one test, in the system temporary folder.
    fn config(test: &str, text: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "baseboard-libsensors-{}-{test}.conf",
            std::process::id()
        ));
        std::fs::write(&path, text).unwrap();
        path
    }

    /// libsensors with only `path` as configuration; `None` where the
    /// library is missing or does not work on this system.
    fn load(path: &Path) -> Option<Libsensors> {
        match Libsensors::load(Some(path)) {
            Ok(lib) => Some(lib),
            Err(e @ (Error::Library(_) | Error::Symbol(_) | Error::Init(_))) => {
                eprintln!("libsensors not usable here, test skipped: {e}");
                None
            }
            Err(e) => panic!("{e}"),
        }
    }

    fn sysfs() -> Vec<Chip> {
        SysfsSource::new(Path::new("/sys")).chips()
    }

    fn kind(name: &str) -> Option<(Kind, u8)> {
        let split = name.find(|c: char| c.is_ascii_digit())?;
        let kind = match &name[..split] {
            "in" => Kind::Voltage,
            "fan" => Kind::Fan,
            "temp" => Kind::Temp,
            "power" => Kind::Power,
            _ => return None,
        };
        Some((kind, name[split..].parse().ok()?))
    }

    /// The sysfs reading of feature `name` of the chip libsensors reports in
    /// folder `lm_path`.
    fn reading<'a>(chips: &'a [Chip], lm_path: &Path, name: &str) -> Option<&'a sensors::Reading> {
        let path = std::fs::canonicalize(lm_path).ok()?;
        let (kind, index) = kind(name)?;
        chips.iter().find(|c| c.path == path)?.reading(kind, index)
    }

    #[test]
    fn missing_library() {
        let _serial = serial();
        let result = Libsensors::load_from(&[c"libbaseboard-test-missing.so"], None);
        assert!(matches!(result, Err(Error::Library(_))));
        assert!(!IN_USE.load(Ordering::Acquire), "claim released");
    }

    #[test]
    fn missing_configuration_file() {
        let _serial = serial();
        let path = Path::new("/nonexistent/baseboard.conf");
        match Libsensors::load(Some(path)) {
            Err(Error::Config(p)) => assert_eq!(p, path),
            Err(Error::Library(_)) => {}
            other => panic!("{:?}", other.err()),
        }
        assert!(!IN_USE.load(Ordering::Acquire), "claim released");
    }

    /// A syntax error does not stop libsensors: it reports the line on
    /// stderr and keeps the other statements (its grammar recovers at the
    /// end of the line). So a broken line in /etc/sensors.d is not a reason
    /// to fall back to raw sysfs.
    #[test]
    fn syntax_error_keeps_the_other_statements() {
        let _serial = serial();
        let empty = config("syntax-empty", "");
        let Some(lib) = load(&empty) else {
            return;
        };
        let Some(chip) = lib.chips().into_iter().find(|c| !c.features.is_empty()) else {
            eprintln!("no hwmon chip with features on this system, test skipped");
            return;
        };
        drop(lib);
        let ignored = &chip.features[0].name;
        let bad = config(
            "syntax",
            &format!(
                "chip \"{}\"\n    compute {ignored} @*, @\n    ignore {ignored}\n",
                chip.name
            ),
        );
        let lib = load(&bad).unwrap();
        let reloaded = lib
            .chips()
            .into_iter()
            .find(|c| c.name == chip.name)
            .unwrap();
        assert!(reloaded.features.iter().all(|f| &f.name != ignored));
        drop(lib);
        std::fs::remove_file(empty).unwrap();
        std::fs::remove_file(bad).unwrap();
    }

    #[test]
    fn only_one_at_a_time() {
        let _serial = serial();
        let empty = config("one", "");
        let Some(first) = load(&empty) else {
            return;
        };
        assert!(matches!(Libsensors::load(Some(&empty)), Err(Error::Busy)));
        drop(first);
        assert!(load(&empty).is_some());
        std::fs::remove_file(empty).unwrap();
    }

    /// Three rounds of sysfs, libsensors and sysfs again, as the app does
    /// at start.
    fn rounds(lib: &Libsensors) -> Vec<Round> {
        (0..3)
            .map(|_| Round {
                before: sysfs(),
                lm: lib.chips(),
                after: sysfs(),
            })
            .collect()
    }

    /// Whether the sysfs reading of feature `name` was the same in every
    /// read of every round, before and after libsensors.
    fn held_still(rounds: &[Round], lm_path: &Path, name: &str) -> bool {
        let mut values = rounds.iter().flat_map(|round| {
            [
                reading(&round.before, lm_path, name),
                reading(&round.after, lm_path, name),
            ]
        });
        match values.next().flatten() {
            Some(first) => values.all(|v| v.is_some_and(|v| v.value == first.value)),
            None => false,
        }
    }

    /// The features of the chips whose readings held still in all the
    /// rounds: drivers that keep a reading for a while, like nct6683 for the
    /// board's voltage rails. Other chips (a Wi-Fi card, a graphics card)
    /// change between any two reads, and a value that changes and comes
    /// back could look converted by chance.
    fn steady(rounds: &[Round]) -> HashSet<FeatureKey> {
        let mut keys = HashSet::new();
        let Some(first) = rounds.first() else {
            return keys;
        };
        for chip in &first.lm {
            let Ok(path) = std::fs::canonicalize(&chip.path) else {
                continue;
            };
            let names: Vec<&str> = chip
                .features
                .iter()
                .filter(|f| f.value.is_some())
                .filter(|f| reading(&first.before, &chip.path, &f.name).is_some())
                .map(|f| f.name.as_str())
                .collect();
            if !names.is_empty()
                && names
                    .iter()
                    .all(|name| held_still(rounds, &chip.path, name))
            {
                keys.extend(names.iter().map(|name| (path.clone(), name.to_string())));
            }
        }
        keys
    }

    /// Without a configuration, libsensors reports the sysfs values (no
    /// steady feature looks converted) and leaves nothing out.
    #[test]
    fn same_as_sysfs_without_configuration() {
        let _serial = serial();
        let empty = config("same", "");
        let Some(lib) = load(&empty) else {
            return;
        };
        let rounds = rounds(&lib);
        if rounds[0].lm.is_empty() {
            eprintln!("no hwmon chips on this system, only the empty case checked");
        }
        let candidates = steady(&rounds);
        assert_eq!(sensors::computed(&rounds, &candidates), HashSet::new());
        let merged = sensors::merge(rounds[0].before.clone(), &rounds[0].lm, &HashSet::new());
        for (a, b) in rounds[0].before.iter().zip(&merged) {
            assert_eq!(a.readings.len(), b.readings.len(), "{}", a.name);
        }
        eprintln!(
            "{} steady values of {} chips compared with sysfs",
            candidates.len(),
            candidates
                .iter()
                .map(|(path, _)| path)
                .collect::<HashSet<_>>()
                .len()
        );
        std::fs::remove_file(empty).unwrap();
    }

    /// `compute`, `ignore` and `label` of a configuration, on the steady
    /// chip with the most voltage and temperature features.
    #[test]
    fn compute_ignore_and_label() {
        let _serial = serial();
        let empty = config("statements-empty", "");
        let Some(lib) = load(&empty) else {
            return;
        };
        let plain_rounds = rounds(&lib);
        let steady = steady(&plain_rounds);
        let Some(chip) = plain_rounds
            .into_iter()
            .next()
            .unwrap()
            .lm
            .into_iter()
            .filter_map(|mut chip| {
                let path = std::fs::canonicalize(&chip.path).ok()?;
                chip.features.retain(|f| {
                    matches!(kind(&f.name), Some((Kind::Voltage | Kind::Temp, _)))
                        && steady.contains(&(path.clone(), f.name.clone()))
                });
                Some(chip)
            })
            .max_by_key(|chip| chip.features.len())
            .filter(|chip| chip.features.len() >= 4)
        else {
            eprintln!(
                "no chip with 4 voltages or temperatures that hold still, \
                 test skipped (needs a driver that keeps its readings, like nct6683)"
            );
            return;
        };
        drop(lib);
        let [computed, ignored, labelled, plain] =
            [0, 1, 2, 3].map(|i| chip.features[i].name.clone());
        let statements = config(
            "statements",
            &format!(
                "chip \"{}\"\n    compute {computed} 0*@+123.25, @\n    ignore {ignored}\n    label {labelled} \"Baseboard test\"\n",
                chip.name
            ),
        );
        let lib = load(&statements).unwrap();
        let rounds = rounds(&lib);
        let lm = &rounds[0].lm;
        let configured = lm.iter().find(|c| c.name == chip.name).unwrap();
        let feature = |name: &str| configured.features.iter().find(|f| f.name == name);
        assert_eq!(feature(&computed).unwrap().value, Some(123.25));
        assert!(feature(&ignored).is_none());
        assert_eq!(
            feature(&labelled).unwrap().label.as_deref(),
            Some("Baseboard test")
        );

        // Of two candidates, only the one with `compute` is converted. The
        // plain one can only be told apart while its reading holds still.
        let at = std::fs::canonicalize(&configured.path).unwrap();
        let converted = (at.clone(), computed.clone());
        let candidates = HashSet::from([converted.clone(), (at.clone(), plain.clone())]);
        let found = sensors::computed(&rounds, &candidates);
        if held_still(&rounds, &configured.path, &plain) {
            assert_eq!(found, HashSet::from([converted.clone()]));
        } else {
            assert!(found.contains(&converted), "{found:?}");
            eprintln!(
                "{plain} of {} changed during the rounds, not compared",
                chip.name
            );
        }

        let before = &rounds[0].before;
        let merged = sensors::merge(before.clone(), lm, &HashSet::from([converted]));
        let computed_reading = reading(&merged, &at, &computed).unwrap();
        assert_eq!(computed_reading.value, 123.25);
        assert!(computed_reading.scaled);
        assert!(!reading(&merged, &at, &plain).unwrap().scaled);
        assert!(reading(&merged, &at, &ignored).is_none());
        assert!(reading(before, &at, &ignored).is_some());
        assert_eq!(
            reading(&merged, &at, &labelled)
                .unwrap()
                .sensors_label
                .as_deref(),
            Some("Baseboard test")
        );
        drop(lib);
        std::fs::remove_file(empty).unwrap();
        std::fs::remove_file(statements).unwrap();
    }
}
