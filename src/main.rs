mod controller;
mod power;
#[cfg(target_os = "macos")]
mod status_item;
mod ui;
#[cfg(target_os = "macos")]
mod updater;

#[cfg(not(target_os = "macos"))]
compile_error!("Never Sleep requires macOS for native power management.");

use controller::Controller;
use power::PowerSource;
use rune::{App, AppSpec, Key, KeyEvent, Tx};
use std::time::Duration;

pub(crate) struct State {
    controller: Controller,
    next_poll: f64,
    menu_error: Option<String>,
    native_started: bool,
    handle: Option<rune::AppHandle<State>>,
    icon: Option<rune::Image>,
    image_cache: Option<rune::ImageCache>,
    reduce_transparency: bool,
    #[cfg(target_os = "macos")]
    menu: Option<status_item::StatusItem>,
    #[cfg(target_os = "macos")]
    updater: Option<updater::Updater>,
}

impl State {
    fn new() -> Self {
        // All tooling paths (including HTML exports) start without side effects.
        // Only a real native window's hook installs the live controller.
        Self::with_controller(Controller::preview(PowerSource::Ac, false, false))
    }

    fn with_controller(controller: Controller) -> Self {
        Self {
            controller,
            next_poll: 0.0,
            menu_error: None,
            native_started: false,
            handle: None,
            icon: None,
            image_cache: None,
            reduce_transparency: false,
            #[cfg(target_os = "macos")]
            menu: None,
            #[cfg(target_os = "macos")]
            updater: None,
        }
    }

    fn sync_menu(&self) {
        #[cfg(target_os = "macos")]
        if let Some(menu) = &self.menu {
            menu.update(
                self.controller.enabled,
                self.controller.display,
                self.controller.status(),
                self.updater
                    .as_ref()
                    .map_or_default(updater::Updater::status),
            );
        }
    }

    fn toggle_enabled(&mut self) {
        self.controller.toggle_enabled();
        self.sync_menu();
    }

    fn toggle_display(&mut self) {
        self.controller.toggle_display();
        self.sync_menu();
    }

    fn toggle_show_in_dock(&mut self) {
        self.controller.toggle_show_in_dock();
        self.sync_dock();
    }

    fn sync_dock(&self) {
        if let Some(handle) = &self.handle {
            // Keep a way back to settings if the menu bar could not be created.
            handle.set_dock_icon(self.controller.show_in_dock || self.menu.is_none());
        }
    }
}

fn poll(s: &mut State, clock: f64, tx: &mut Tx) -> bool {
    if clock < s.next_poll {
        tx.wake_after(Duration::from_secs_f64(s.next_poll - clock));
        return false;
    }
    s.next_poll = clock + 2.0;
    tx.wake_after(Duration::from_secs(2));
    let mut changed = s.controller.refresh();
    if s.native_started {
        let reduce_transparency = status_item::reduce_transparency();
        changed |= s.reduce_transparency != reduce_transparency;
        s.reduce_transparency = reduce_transparency;
    }
    // Sparkle's canCheckForUpdates changes independently of power state.
    if s.native_started || changed {
        s.sync_menu();
    }
    changed
}

fn on_key(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    match event.key {
        Key::Command('q') => tx.quit(),
        Key::Command('w') => tx.close_window(),
        Key::Command('k') => s.toggle_enabled(),
        _ => return false,
    }
    true
}

fn main() {
    let app = App::new(AppSpec::<State> {
        min_size: Some((420.0, 560.0)),
        families: ("System Font", "ui-monospace"),
        setup: Some(|s, waker| {
            let cache = rune::ImageCache::new(waker);
            let icon = cache.load(&include_bytes!("../assets/app-icon.png")[..]);
            if rune::tooling::headless_run().is_some() {
                icon.wait().expect("embedded icon decodes");
            }
            s.icon = Some(icon);
            s.image_cache = Some(cache);
        }),
        poll: Some(poll),
        on_key: Some(on_key),
        scenes: &[
            ("active", |s| {
                s.controller = Controller::preview(PowerSource::Ac, true, false)
            }),
            ("display", |s| {
                s.controller = Controller::preview(PowerSource::Ac, true, true)
            }),
            ("battery", |s| {
                s.controller = Controller::preview(PowerSource::Battery, true, false)
            }),
            ("off", |s| {
                s.controller = Controller::preview(PowerSource::Ac, false, false)
            }),
            ("unknown", |s| {
                s.controller = Controller::preview(PowerSource::Unknown, true, false)
            }),
        ],
        ..AppSpec::new("Never Sleep", (460.0, 560.0), State::new, ui::view)
    })
    // Start without a Dock flash; apply the saved choice once the menu is ready.
    .accessory()
    .keep_running()
    .release_gpu_when_closed()
    .standard_menus()
    .window_options(rune::WindowOptions {
        transparent: true,
        ..Default::default()
    })
    .srgb_blend(true)
    .chrome(rune::Chrome {
        full_size_content: true,
        traffic_lights: Some((16.0, 15.0)),
        drag_regions: true,
    });

    // Native windows only: headless renders never create a status item.
    #[cfg(target_os = "macos")]
    let app = {
        let handle = app.handle();
        app.on_window(move |_| {
            let actions = handle.clone();
            let window_handle = handle.clone();
            handle.update(move |s, tx| {
                s.handle = Some(window_handle);
                if !s.native_started {
                    // Restoring saved state is initialization, not a user toggle.
                    // Paint its final control positions even before activation.
                    tx.instant();
                    // Replace any requested fixture before native use. A native
                    // --scene must never claim protection from a fake assertion.
                    s.controller = Controller::new();
                    s.native_started = true;
                    s.reduce_transparency = status_item::reduce_transparency();
                    match updater::Updater::new() {
                        Ok(updater) => s.updater = Some(updater),
                        Err(error) => eprintln!("Never Sleep: {error}"),
                    }
                }
                if s.menu.is_some() {
                    return;
                }
                match status_item::StatusItem::new(move |action| {
                    use status_item::Action;
                    match action {
                        Action::Open => actions.open(),
                        Action::Quit => actions.quit(),
                        Action::ToggleEnabled => actions.update(|s, _| s.toggle_enabled()),
                        Action::ToggleDisplay => actions.update(|s, _| s.toggle_display()),
                        Action::CheckForUpdates => actions.update(|s, _| {
                            if let Some(updater) = &s.updater {
                                updater.check();
                            }
                            s.sync_menu();
                        }),
                        Action::ToggleAutomaticChecks => actions.update(|s, _| {
                            if let Some(updater) = &s.updater {
                                updater.toggle_automatic_checks();
                            }
                            s.sync_menu();
                        }),
                        Action::ToggleAutomaticDownloads => actions.update(|s, _| {
                            if let Some(updater) = &s.updater {
                                updater.toggle_automatic_downloads();
                            }
                            s.sync_menu();
                        }),
                    }
                }) {
                    Ok(menu) => {
                        s.menu = Some(menu);
                        s.menu_error = None;
                        s.sync_menu();
                    }
                    Err(error) => s.menu_error = Some(error),
                }
                s.sync_dock();
            });
        })
    };
    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn initialization_is_isolated_until_a_native_window_exists() {
        let mut state = State::new();
        state.toggle_enabled();
        assert!(!state.native_started);
        assert!(state.updater.is_none());
        assert!(state.controller.active()); // simulated AC, no native hook
    }

    #[test]
    fn switches_control_the_power_policy_through_real_ui_handlers() {
        let state = State::with_controller(Controller::preview(PowerSource::Ac, false, false));
        let mut rt = Runtime::new(state, ui::view, (560.0, 700.0), Env::new(false, 2.0));
        rt.frame(0.0);
        for key in ["enable-switch", "display-switch"] {
            let (x, y) = rt.find(key).expect("setting must be reachable");
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(0.3);
        }
        assert!(rt.state.controller.active());
        assert!(rt.state.controller.display);
        let (x, y) = rt.find("enable-switch").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.3);
        assert!(!rt.state.controller.active());
    }

    #[test]
    fn enabling_on_battery_stays_in_standby() {
        let state = State::with_controller(Controller::preview(PowerSource::Battery, false, false));
        let mut rt = Runtime::new(state, ui::view, (540.0, 660.0), Env::new(true, 2.0));
        rt.frame(0.0);
        let (x, y) = rt.find("enable-switch").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.3);
        assert!(rt.state.controller.enabled);
        assert!(!rt.state.controller.active());
        assert_eq!(rt.state.controller.status(), "Ready when you plug in");
    }
}
