//! Power-source policy, assertion lifetime, and persisted user preferences.
//!
//! The UI and menu bar share this controller. Preview instances never read power
//! sources, acquire system assertions, or write preferences to disk.

use std::collections::BTreeMap;

use rune::prefs::Prefs;

use crate::power::{self, PowerSource, SleepGuard};

const APP_ID: &str = "com.nearby.never-sleep";

/// What an active guard is currently enforcing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Options {
    display: bool,
    lid: bool,
}

trait Assertion {
    fn options(&self) -> Options;
    /// Re-apply global state that something outside this process may reset.
    fn maintain(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Assertion for SleepGuard {
    fn options(&self) -> Options {
        Options {
            display: self.keep_display_awake(),
            lid: self.keep_awake_lid_closed(),
        }
    }

    fn maintain(&self) -> Result<(), String> {
        SleepGuard::maintain(self)
    }
}

trait PowerBackend {
    fn source(&mut self) -> Result<PowerSource, String>;
    fn acquire(&mut self, options: Options) -> Result<Box<dyn Assertion>, String>;
}

struct NativePower;

impl PowerBackend for NativePower {
    fn source(&mut self) -> Result<PowerSource, String> {
        power::power_source()
    }

    fn acquire(&mut self, options: Options) -> Result<Box<dyn Assertion>, String> {
        SleepGuard::new(options.display, options.lid)
            .map(|guard| Box::new(guard) as Box<dyn Assertion>)
    }
}

struct PreviewPower(PowerSource);
struct PreviewAssertion(Options);

impl Assertion for PreviewAssertion {
    fn options(&self) -> Options {
        self.0
    }
}

impl PowerBackend for PreviewPower {
    fn source(&mut self) -> Result<PowerSource, String> {
        Ok(self.0)
    }

    fn acquire(&mut self, options: Options) -> Result<Box<dyn Assertion>, String> {
        Ok(Box::new(PreviewAssertion(options)))
    }
}

pub struct Controller {
    pub enabled: bool,
    pub display: bool,
    /// Also ignore closing the lid while protection is active.
    pub lid: bool,
    pub show_in_dock: bool,
    pub source: PowerSource,
    /// The current power-source or assertion failure, cleared after recovery.
    pub error: Option<String>,
    /// Saving preferences failed; the selected settings still apply this run.
    pub save_error: Option<String>,
    guard: Option<Box<dyn Assertion>>,
    backend: Box<dyn PowerBackend>,
    prefs: Prefs,
}

impl Controller {
    pub fn new() -> Self {
        // A previous run that was force-quit alongside its watchdog could have
        // left lid sleep disabled. Start from macOS's normal behavior.
        power::clear_lid_override();
        Self::with_backend(Prefs::open(APP_ID), Box::new(NativePower))
    }

    /// Isolated state for headless screenshots and scripted UI checks.
    pub fn preview(source: PowerSource, enabled: bool, display: bool) -> Self {
        let mut controller = Self::with_backend(Prefs::memory(), Box::new(PreviewPower(source)));
        controller.enabled = enabled;
        controller.display = display;
        controller.refresh();
        controller
    }

    fn with_backend(prefs: Prefs, backend: Box<dyn PowerBackend>) -> Self {
        let settings = prefs
            .get::<BTreeMap<String, bool>>("settings")
            .unwrap_or_default();
        let mut controller = Self {
            enabled: settings.get("enabled").copied().unwrap_or(false),
            display: settings.get("display").copied().unwrap_or(false),
            // On by default: "never sleep on power" includes closing the lid.
            lid: settings.get("lid").copied().unwrap_or(true),
            show_in_dock: settings.get("show_in_dock").copied().unwrap_or(true),
            source: PowerSource::Unknown,
            error: None,
            save_error: None,
            guard: None,
            backend,
            prefs,
        };
        controller.refresh();
        controller
    }

    /// Refresh even while the settings window is closed. Returns whether the
    /// visible source, status, or error changed; no assertion is recreated when
    /// its settings remain the same.
    pub fn refresh(&mut self) -> bool {
        let before = (self.source, self.active(), self.error.clone());
        match self.backend.source() {
            Ok(source) => {
                self.source = source;
                self.error = None;
                self.reconcile();
            }
            Err(error) => {
                self.source = PowerSource::Unknown;
                self.error = Some(format!("Could not read the power source: {error}"));
                // Losing reliable AC information must never strand protection
                // on battery, even if reading the source repeatedly fails.
                self.guard = None;
            }
        }
        before != (self.source, self.active(), self.error.clone())
    }

    pub fn toggle_enabled(&mut self) {
        self.enabled = !self.enabled;
        self.save();
        self.refresh();
    }

    pub fn toggle_display(&mut self) {
        self.display = !self.display;
        self.save();
        self.refresh();
    }

    pub fn toggle_lid(&mut self) {
        self.lid = !self.lid;
        self.save();
        self.refresh();
    }

    pub fn toggle_show_in_dock(&mut self) {
        self.show_in_dock = !self.show_in_dock;
        self.save();
    }

    pub fn active(&self) -> bool {
        self.guard.is_some()
    }

    pub fn status(&self) -> &'static str {
        if !self.enabled {
            "Sleep is allowed"
        } else if self.error.is_some() {
            "Unable to prevent sleep"
        } else if self.active() {
            "Keeping your Mac awake"
        } else if self.source == PowerSource::Battery {
            "Ready when you plug in"
        } else {
            "Check power connection"
        }
    }

    fn reconcile(&mut self) {
        if !power::should_inhibit(self.enabled, self.source) {
            self.guard = None;
            return;
        }
        let options = Options {
            display: self.display,
            lid: self.lid,
        };
        if let Some(guard) = self
            .guard
            .as_ref()
            .filter(|guard| guard.options() == options)
        {
            if let Err(error) = guard.maintain() {
                self.error = Some(format!("Could not keep awake with the lid closed: {error}"));
            }
            return;
        }
        // Acquire first so a successful option change doesn't briefly permit
        // sleep. A failure drops the old guard as well: keeping it could leave
        // the display or lid override on after the user turned it off.
        match self.backend.acquire(options) {
            Ok(guard) => self.guard = Some(guard),
            Err(error) => {
                self.guard = None;
                self.error = Some(format!("Could not prevent sleep: {error}"));
            }
        }
    }

    fn save(&mut self) {
        let settings = BTreeMap::from([
            ("enabled", self.enabled),
            ("display", self.display),
            ("lid", self.lid),
            ("show_in_dock", self.show_in_dock),
        ]);
        self.save_error = self
            .prefs
            .set("settings", &settings)
            .err()
            .map(|error| format!("Settings apply now but could not be saved: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;

    struct FakeState {
        source: Result<PowerSource, String>,
        fail_acquire: bool,
        events: Vec<String>,
        maintained: usize,
    }

    struct FakePower(Rc<RefCell<FakeState>>);
    struct FakeAssertion {
        state: Rc<RefCell<FakeState>>,
        options: Options,
    }

    impl Assertion for FakeAssertion {
        fn options(&self) -> Options {
            self.options
        }

        fn maintain(&self) -> Result<(), String> {
            self.state.borrow_mut().maintained += 1;
            Ok(())
        }
    }

    impl Drop for FakeAssertion {
        fn drop(&mut self) {
            let event = format!("release:{}", label(self.options));
            self.state.borrow_mut().events.push(event);
        }
    }

    /// Display flag, plus "-lid" only when the lid override is off, so the
    /// default (lid on) keeps the original event names readable.
    fn label(options: Options) -> String {
        format!(
            "{}{}",
            options.display,
            if options.lid { "" } else { "-lid" }
        )
    }

    impl PowerBackend for FakePower {
        fn source(&mut self) -> Result<PowerSource, String> {
            self.0.borrow().source.clone()
        }

        fn acquire(&mut self, options: Options) -> Result<Box<dyn Assertion>, String> {
            let mut state = self.0.borrow_mut();
            state.events.push(format!("acquire:{}", label(options)));
            if state.fail_acquire {
                return Err("test failure".into());
            }
            Ok(Box::new(FakeAssertion {
                state: self.0.clone(),
                options,
            }))
        }
    }

    fn fixture() -> (Controller, Rc<RefCell<FakeState>>) {
        let state = Rc::new(RefCell::new(FakeState {
            source: Ok(PowerSource::Ac),
            fail_acquire: false,
            events: Vec::new(),
            maintained: 0,
        }));
        let controller =
            Controller::with_backend(Prefs::memory(), Box::new(FakePower(state.clone())));
        (controller, state)
    }

    #[test]
    fn unplug_releases_and_replug_reacquires_without_changing_preference() {
        let (mut controller, state) = fixture();
        assert!(!controller.active());
        controller.toggle_enabled();
        assert!(controller.active());
        assert!(!controller.refresh(), "steady state has no UI changes");
        assert_eq!(state.borrow().events, ["acquire:false"]);

        state.borrow_mut().source = Ok(PowerSource::Battery);
        assert!(controller.refresh());
        assert!(!controller.active());
        assert!(controller.enabled);
        assert_eq!(controller.status(), "Ready when you plug in");

        state.borrow_mut().source = Ok(PowerSource::Ac);
        assert!(controller.refresh());
        assert!(controller.active());
        controller.toggle_enabled();
        assert!(!controller.active());
        assert_eq!(controller.status(), "Sleep is allowed");
        assert_eq!(
            state.borrow().events,
            [
                "acquire:false",
                "release:false",
                "acquire:false",
                "release:false"
            ]
        );
    }

    #[test]
    fn unknown_or_unreadable_source_releases_protection_and_recovers() {
        let (mut controller, state) = fixture();
        controller.toggle_enabled();
        state.borrow_mut().source = Ok(PowerSource::Unknown);
        controller.refresh();
        assert!(!controller.active());
        assert_eq!(controller.status(), "Check power connection");

        state.borrow_mut().source = Ok(PowerSource::Ac);
        controller.refresh();
        assert!(controller.active());
        state.borrow_mut().source = Err("unavailable".into());
        controller.refresh();
        assert!(!controller.active());
        assert!(controller.error.is_some());
        assert_eq!(controller.source, PowerSource::Unknown);

        state.borrow_mut().source = Ok(PowerSource::Ac);
        controller.refresh();
        assert!(controller.active());
        assert!(controller.error.is_none());
    }

    #[test]
    fn display_change_acquires_before_releasing_and_drops_every_guard() {
        let (mut controller, state) = fixture();
        controller.toggle_enabled();
        controller.toggle_display();
        assert!(controller.active());
        assert_eq!(
            state.borrow().events,
            ["acquire:false", "acquire:true", "release:false"]
        );
        drop(controller);
        assert_eq!(state.borrow().events.last().unwrap(), "release:true");
    }

    #[test]
    fn failed_display_change_releases_old_guard_then_retries_on_refresh() {
        let (mut controller, state) = fixture();
        controller.toggle_enabled();
        controller.toggle_display();
        state.borrow_mut().fail_acquire = true;
        controller.toggle_display();
        assert!(!controller.active());
        assert!(!controller.display);
        assert_eq!(controller.status(), "Unable to prevent sleep");
        assert_eq!(state.borrow().events.last().unwrap(), "release:true");
        state.borrow_mut().fail_acquire = false;
        controller.refresh();
        assert!(controller.active());
        assert!(controller.error.is_none());
    }

    #[test]
    fn lid_override_is_on_by_default_toggles_and_is_maintained_while_active() {
        let (mut controller, state) = fixture();
        assert!(controller.lid, "keeping awake includes closing the lid");
        controller.toggle_enabled();
        assert_eq!(state.borrow().maintained, 0);
        controller.refresh();
        controller.refresh();
        assert_eq!(
            state.borrow().maintained,
            2,
            "the global override is re-applied while active"
        );

        controller.toggle_lid();
        assert!(controller.active() && !controller.lid);
        state.borrow_mut().source = Ok(PowerSource::Battery);
        controller.refresh();
        assert!(!controller.active());
        assert_eq!(
            state.borrow().events,
            [
                "acquire:false",
                "acquire:false-lid",
                "release:false",
                "release:false-lid"
            ]
        );
        assert!(
            !controller
                .prefs
                .get::<BTreeMap<String, bool>>("settings")
                .unwrap()["lid"]
        );
    }

    #[test]
    fn changing_display_while_on_battery_does_not_acquire() {
        let (mut controller, state) = fixture();
        state.borrow_mut().source = Ok(PowerSource::Battery);
        controller.toggle_enabled();
        controller.toggle_display();
        assert!(controller.enabled && controller.display);
        assert!(!controller.active());
        assert!(state.borrow().events.is_empty());
    }

    #[test]
    fn saved_settings_restore_and_activate_only_on_ac() {
        let (mut controller, _) = fixture();
        controller.toggle_enabled();
        controller.toggle_display();
        let prefs = std::mem::replace(&mut controller.prefs, Prefs::memory());
        drop(controller);
        let restored =
            Controller::with_backend(prefs, Box::new(PreviewPower(PowerSource::Battery)));
        assert!(restored.enabled && restored.display);
        assert!(!restored.active());
    }

    #[test]
    fn legacy_preferences_keep_dock_visible_and_dock_changes_preserve_assertions() {
        let mut prefs = Prefs::memory();
        prefs
            .set(
                "settings",
                &BTreeMap::from([("enabled", true), ("display", false)]),
            )
            .unwrap();
        let (_, state) = fixture();
        let mut controller = Controller::with_backend(prefs, Box::new(FakePower(state.clone())));
        assert!(
            controller.show_in_dock,
            "existing users retain their Dock icon"
        );
        assert!(controller.lid, "existing users gain lid-closed protection");
        assert!(controller.active());
        assert_eq!(state.borrow().events, ["acquire:false"]);

        controller.toggle_show_in_dock();
        assert!(!controller.show_in_dock);
        assert!(controller.active());
        assert_eq!(state.borrow().events, ["acquire:false"]);
        controller.toggle_enabled();
        assert!(
            !controller
                .prefs
                .get::<BTreeMap<String, bool>>("settings")
                .unwrap()["show_in_dock"]
        );
    }

    #[test]
    fn preferences_round_trip_through_disk_and_write_errors_do_not_disable_protection() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "never-sleep-controller-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("prefs.json");
        let mut controller =
            Controller::with_backend(Prefs::at(&path), Box::new(PreviewPower(PowerSource::Ac)));
        assert!(controller.show_in_dock, "new users start with a Dock icon");
        controller.toggle_enabled();
        controller.toggle_display();
        controller.toggle_show_in_dock();
        let restored =
            Controller::with_backend(Prefs::at(&path), Box::new(PreviewPower(PowerSource::Ac)));
        assert!(restored.enabled && restored.display && restored.active());
        assert!(
            !restored.show_in_dock,
            "menu-bar-only mode survives relaunch"
        );

        // A regular file used as a parent makes writes fail even when tests
        // have permission to write anywhere; no permission mutation is needed.
        controller.prefs = Prefs::at(path.join("cannot-exist.json"));
        controller.toggle_display();
        assert!(controller.save_error.is_some());
        assert!(controller.active());
        assert!(controller.error.is_none());
        controller.prefs = Prefs::at(&path);
        controller.toggle_display();
        assert!(controller.save_error.is_none());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn preview_toggles_use_only_simulated_state() {
        let mut controller = Controller::preview(PowerSource::Ac, true, false);
        assert!(controller.active());
        assert!(!controller.prefs.is_persistent());
        controller.toggle_enabled();
        assert!(!controller.active());
        controller.toggle_display();
        controller.toggle_enabled();
        assert!(controller.active() && controller.display);
    }
}
