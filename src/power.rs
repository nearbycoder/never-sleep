//! Temporary macOS sleep prevention. No `pmset` preferences change.
//!
//! Process-owned assertions prevent automatic idle sleep. macOS ignores them
//! when the lid closes, so an optional lid override asks the kernel's power
//! root domain to skip clamshell sleep (the mechanism closed-display utilities
//! use). Unlike assertions, that override is global and is *not* released when
//! this process exits: the guard clears it on drop, a watchdog child clears it
//! if the app quits or crashes, and every launch clears stale state. Neither
//! blocks an explicit Sleep command, shutdown, or emergency thermal sleep. The
//! app must keep checking the power source and drop its guard on battery.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    Ac,
    Battery,
    /// Includes UPS power and sources macOS cannot identify. Fail closed.
    Unknown,
}

/// Never keep a machine awake on battery or on an unrecognized power source.
pub fn should_inhibit(enabled: bool, source: PowerSource) -> bool {
    enabled && source == PowerSource::Ac
}

/// Read the source actually supplying the computer, including desktop Macs.
/// Charging state is deliberately irrelevant: a full battery on AC is still AC.
pub fn power_source() -> Result<PowerSource, String> {
    platform::power_source()
}

pub use platform::{SleepGuard, clear_lid_override, run_lid_watchdog};

/// Hidden argument that starts the lid-override watchdog instead of the app.
pub const LID_WATCHDOG_ARG: &str = "--lid-override-watchdog";

#[cfg(target_os = "macos")]
mod platform {
    use super::{LID_WATCHDOG_ARG, PowerSource};
    use std::ffi::{CStr, c_char, c_void};
    use std::io::Read;
    use std::process::{ChildStdin, Command, Stdio};
    use std::ptr::{self, NonNull};
    use std::sync::Mutex;

    type CfRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    const ASSERTION_ON: u32 = 255;

    // Signatures and constants are from the installed Apple SDK's IOPMLib.h,
    // IOPowerSources.h and CFString.h. CF's Boolean is an unsigned byte.
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: CfRef);
        fn CFStringCreateWithCString(allocator: CfRef, text: *const c_char, encoding: u32)
        -> CfRef;
        fn CFStringGetCString(
            value: CfRef,
            buffer: *mut c_char,
            buffer_size: isize,
            encoding: u32,
        ) -> u8;
    }

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CfRef;
        fn IOPSGetProvidingPowerSourceType(snapshot: CfRef) -> CfRef;
        fn IOPMAssertionCreateWithName(
            assertion_type: CfRef,
            level: u32,
            name: CfRef,
            assertion_id: *mut u32,
        ) -> i32;
        fn IOPMAssertionRelease(assertion_id: u32) -> i32;
        fn IOServiceMatching(name: *const c_char) -> CfRef;
        fn IOServiceGetMatchingService(main_port: u32, matching: CfRef) -> u32;
        fn IOServiceOpen(service: u32, owning_task: u32, kind: u32, connect: *mut u32) -> i32;
        fn IOServiceClose(connect: u32) -> i32;
        fn IOObjectRelease(object: u32) -> i32;
        fn IOConnectCallScalarMethod(
            connection: u32,
            selector: u32,
            input: *const u64,
            input_count: u32,
            output: *mut u64,
            output_count: *mut u32,
        ) -> i32;
        #[cfg(test)]
        fn IOPMAssertionCopyProperties(assertion_id: u32) -> CfRef;
    }

    /// Owns one +1 Core Foundation reference; Get-rule references never go here.
    struct OwnedCf(NonNull<c_void>);

    impl OwnedCf {
        fn from_created(value: CfRef) -> Option<Self> {
            NonNull::new(value.cast_mut()).map(Self)
        }

        fn as_ptr(&self) -> CfRef {
            self.0.as_ptr().cast_const()
        }

        fn string(text: &CStr) -> Result<Self, String> {
            // SAFETY: text is NUL-terminated and CF copies it; a null allocator
            // selects the default. The Create-rule return belongs to this owner.
            Self::from_created(unsafe {
                CFStringCreateWithCString(ptr::null(), text.as_ptr(), UTF8)
            })
            .ok_or_else(|| "macOS could not allocate a power-management string.".into())
        }
    }

    impl Drop for OwnedCf {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns exactly one valid, nonnull CF reference.
            unsafe { CFRelease(self.as_ptr()) };
        }
    }

    pub(super) fn power_source() -> Result<PowerSource, String> {
        // SAFETY: IOKit takes no arguments and returns a Create-rule reference.
        let snapshot = OwnedCf::from_created(unsafe { IOPSCopyPowerSourcesInfo() })
            .ok_or_else(|| "macOS could not read the current power source.".to_string())?;
        // SAFETY: snapshot remains alive until after the borrowed string is read.
        let source = unsafe { IOPSGetProvidingPowerSourceType(snapshot.as_ptr()) };
        if source.is_null() {
            return Ok(PowerSource::Unknown);
        }
        let mut buffer = [0 as c_char; 64];
        // SAFETY: source is a CFString and the output buffer has the stated size.
        let copied =
            unsafe { CFStringGetCString(source, buffer.as_mut_ptr(), buffer.len() as isize, UTF8) };
        if copied == 0 {
            return Ok(PowerSource::Unknown);
        }
        // SAFETY: successful CFStringGetCString always NUL-terminates its output.
        let source = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        Ok(match source.to_bytes() {
            b"AC Power" => PowerSource::Ac,
            b"Battery Power" => PowerSource::Battery,
            _ => PowerSource::Unknown,
        })
    }

    struct Assertion(u32);

    impl Assertion {
        fn create(kind: &CStr, reason: &CStr) -> Result<Self, String> {
            let kind_string = OwnedCf::string(kind)?;
            let reason_string = OwnedCf::string(reason)?;
            let mut id = 0;
            // SAFETY: both CFStrings outlive this call; id points to initialized,
            // writable storage. On success this process owns the returned ID.
            let result = unsafe {
                IOPMAssertionCreateWithName(
                    kind_string.as_ptr(),
                    ASSERTION_ON,
                    reason_string.as_ptr(),
                    &mut id,
                )
            };
            if result != 0 {
                return Err(format!(
                    "macOS could not activate {} (IOKit error 0x{:08x}).",
                    kind.to_string_lossy(),
                    result as u32
                ));
            }
            Ok(Self(id))
        }
    }

    impl Drop for Assertion {
        fn drop(&mut self) {
            // SAFETY: this guard uniquely owns a successful assertion creation.
            // macOS also releases all of a process's assertions when it exits.
            let result = unsafe { IOPMAssertionRelease(self.0) };
            if result != 0 {
                eprintln!(
                    "Never Sleep: could not release assertion {} (IOKit error 0x{:08x}).",
                    self.0, result as u32
                );
            }
        }
    }

    unsafe extern "C" {
        static mach_task_self_: u32;
    }

    /// IOPMLibDefs.h: RootDomainUserClient selector that disables lid sleep.
    const SET_CLAMSHELL_SLEEP_STATE: u32 = 12;

    /// Set whether closing the lid is ignored. This is system-wide state that
    /// survives this process, so every enable must be paired with a clear.
    fn set_lid_sleep_disabled(disabled: bool) -> Result<(), String> {
        // SAFETY: IOServiceMatching returns a +1 dictionary that
        // IOServiceGetMatchingService always consumes. A zero service is
        // checked; the returned service and connection are released below.
        unsafe {
            let matching = IOServiceMatching(c"IOPMrootDomain".as_ptr());
            if matching.is_null() {
                return Err("macOS could not locate power management.".into());
            }
            let service = IOServiceGetMatchingService(0, matching);
            if service == 0 {
                return Err("macOS could not locate power management.".into());
            }
            let mut connection = 0;
            let opened = IOServiceOpen(service, mach_task_self_, 0, &mut connection);
            IOObjectRelease(service);
            if opened != 0 {
                return Err(format!(
                    "macOS refused lid control (IOKit error 0x{:08x}).",
                    opened as u32
                ));
            }
            let value = u64::from(disabled);
            let result = IOConnectCallScalarMethod(
                connection,
                SET_CLAMSHELL_SLEEP_STATE,
                &value,
                1,
                ptr::null_mut(),
                ptr::null_mut(),
            );
            IOServiceClose(connection);
            if result != 0 {
                return Err(format!(
                    "macOS could not change lid sleep (IOKit error 0x{:08x}).",
                    result as u32
                ));
            }
        }
        Ok(())
    }

    /// Restore normal lid-close sleep, e.g. after a previous run was killed.
    pub fn clear_lid_override() {
        if let Err(error) = set_lid_sleep_disabled(false) {
            eprintln!("Never Sleep: {error}");
        }
    }

    /// The write end of the watchdog's stdin. It closes only when this process
    /// exits (normally, by crash, or by signal), which wakes the watchdog.
    static WATCHDOG: Mutex<Option<ChildStdin>> = Mutex::new(None);

    fn ensure_watchdog() -> Result<(), String> {
        let mut watchdog = WATCHDOG.lock().unwrap_or_else(|poison| poison.into_inner());
        if watchdog.is_some() {
            return Ok(());
        }
        let executable = std::env::current_exe()
            .map_err(|error| format!("could not start the lid safety monitor: {error}"))?;
        let mut child = Command::new(executable)
            .arg(LID_WATCHDOG_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("could not start the lid safety monitor: {error}"))?;
        *watchdog = child.stdin.take();
        // The child is intentionally never waited on: it outlives this process
        // and is reparented to launchd, which reaps it.
        Ok(())
    }

    /// Body of the watchdog process: wait for the app to go away, then make
    /// sure closing the lid sleeps the Mac again.
    pub fn run_lid_watchdog() -> ! {
        let mut stdin = std::io::stdin().lock();
        let mut buffer = [0_u8; 64];
        // The app never writes; any return (EOF or error) means it is gone.
        while matches!(stdin.read(&mut buffer), Ok(n) if n > 0) {}
        clear_lid_override();
        std::process::exit(0);
    }

    /// Live `LidOverride`s. The kernel flag is a single switch, so a new
    /// guard acquired before the old one is released must not be undone by
    /// the old guard's drop.
    static LID_HOLDERS: Mutex<usize> = Mutex::new(0);

    /// Keeps the Mac awake with its lid closed until the last one is dropped.
    struct LidOverride;

    impl LidOverride {
        fn create() -> Result<Self, String> {
            // Never enable a global override without a way to undo it on exit.
            ensure_watchdog()?;
            let mut holders = LID_HOLDERS
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            set_lid_sleep_disabled(true)?;
            *holders += 1;
            Ok(Self)
        }
    }

    impl Drop for LidOverride {
        fn drop(&mut self) {
            let mut holders = LID_HOLDERS
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            *holders = holders.saturating_sub(1);
            if *holders == 0 {
                clear_lid_override();
            }
        }
    }

    /// Holds the assertions until dropped. Construct only while policy permits.
    ///
    /// An optional display assertion keeps an already-awake display awake; it
    /// does not wake a display that has already turned off or bypass screen lock.
    pub struct SleepGuard {
        _system: Assertion,
        display: Option<Assertion>,
        lid: Option<LidOverride>,
    }

    impl SleepGuard {
        pub fn new(keep_display_awake: bool, keep_awake_lid_closed: bool) -> Result<Self, String> {
            // Do not use PreventSystemSleep: Apple's IOPMLib.h explicitly marks
            // that type unsupported. The supported idle assertion is sufficient.
            let system = Assertion::create(
                c"PreventUserIdleSystemSleep",
                c"Never Sleep: keep this Mac awake while connected to power",
            )?;
            let display = if keep_display_awake {
                Some(Assertion::create(
                    c"PreventUserIdleDisplaySleep",
                    c"Never Sleep: keep the display awake while connected to power",
                )?)
            } else {
                None
            };
            let lid = if keep_awake_lid_closed {
                Some(LidOverride::create()?)
            } else {
                None
            };
            // If a later step fails, the guards above are dropped automatically.
            // There is no partially enabled session.
            Ok(Self {
                _system: system,
                display,
                lid,
            })
        }

        pub fn keep_display_awake(&self) -> bool {
            self.display.is_some()
        }

        pub fn keep_awake_lid_closed(&self) -> bool {
            self.lid.is_some()
        }

        /// Re-apply the lid override, which is global state another app (or a
        /// previous instance's watchdog during a relaunch) may have cleared.
        pub fn maintain(&self) -> Result<(), String> {
            if self.lid.is_some() {
                set_lid_sleep_disabled(true)?;
            }
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn assertion_exists(id: u32) -> bool {
            // SAFETY: IOKit accepts an ID and returns null if it does not exist.
            OwnedCf::from_created(unsafe { IOPMAssertionCopyProperties(id) }).is_some()
        }

        #[test]
        #[ignore = "reads live macOS hardware; run explicitly on a Mac"]
        fn reads_real_power_source() {
            let source = power_source().expect("IOKit power-source query failed");
            eprintln!("Current power source: {source:?}");
        }

        #[test]
        #[ignore = "briefly acquires real macOS assertions; run explicitly on a Mac"]
        fn native_assertions_are_released_on_drop() {
            for display_enabled in [false, true] {
                let guard = SleepGuard::new(display_enabled, false).expect("create assertions");
                assert_eq!(guard.keep_display_awake(), display_enabled);
                let system_id = guard._system.0;
                let display_id = guard.display.as_ref().map(|value| value.0);
                assert!(assertion_exists(system_id));
                if let Some(id) = display_id {
                    assert!(assertion_exists(id));
                }
                drop(guard);
                assert!(!assertion_exists(system_id), "system assertion leaked");
                if let Some(id) = display_id {
                    assert!(!assertion_exists(id), "display assertion leaked");
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::PowerSource;

    pub(super) fn power_source() -> Result<PowerSource, String> {
        Err("Power management is available only on macOS.".into())
    }

    pub struct SleepGuard;

    pub fn clear_lid_override() {}

    pub fn run_lid_watchdog() -> ! {
        std::process::exit(0)
    }

    impl SleepGuard {
        pub fn new(_: bool, _: bool) -> Result<Self, String> {
            Err("Sleep prevention is available only on macOS.".into())
        }

        pub fn keep_display_awake(&self) -> bool {
            false
        }

        pub fn keep_awake_lid_closed(&self) -> bool {
            false
        }

        pub fn maintain(&self) -> Result<(), String> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_enabled_ac_power_allows_sleep_prevention() {
        for source in [PowerSource::Ac, PowerSource::Battery, PowerSource::Unknown] {
            assert!(!should_inhibit(false, source));
        }
        assert!(should_inhibit(true, PowerSource::Ac));
        assert!(!should_inhibit(true, PowerSource::Battery));
        assert!(!should_inhibit(true, PowerSource::Unknown));
    }
}
