//! The bundled Sparkle framework owns update scheduling, validation, and UI.
//! Loading it at runtime keeps cargo/headless tooling independent of the bundle.

use objc2::{
    msg_send, msg_send_id,
    rc::{Allocated, Retained},
    runtime::AnyObject,
};
use objc2_foundation::{MainThreadMarker, NSBundle, NSError, NSString};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UpdateStatus {
    pub available: bool,
    pub can_check: bool,
    pub automatic_checks: bool,
    pub automatic_downloads: bool,
}

pub struct Updater {
    // Both objects and the framework live for the complete native app lifetime.
    controller: Retained<AnyObject>,
    updater: Retained<AnyObject>,
    _framework: Retained<NSBundle>,
    _main_thread: MainThreadMarker,
}

impl Updater {
    pub fn new() -> Result<Self, String> {
        let main_thread = MainThreadMarker::new()
            .ok_or_else(|| "Updates must start on the main thread".to_owned())?;
        // SAFETY: only the main thread loads and calls Sparkle. Selectors and
        // signatures match SPUStandardUpdaterController/SPUUpdater in Sparkle 2.
        unsafe {
            let bundle = NSBundle::mainBundle();
            for key in ["SUFeedURL", "SUPublicEDKey"] {
                if bundle
                    .objectForInfoDictionaryKey(&NSString::from_str(key))
                    .is_none()
                {
                    return Err(format!("This build has no {key}; updates are unavailable"));
                }
            }
            let directory = bundle
                .privateFrameworksPath()
                .ok_or_else(|| "This build has no bundled updater".to_owned())?;
            let framework_path = format!("{directory}/Sparkle.framework");
            let framework = NSBundle::bundleWithPath(&NSString::from_str(&framework_path))
                .ok_or_else(|| "Sparkle is missing from this app bundle".to_owned())?;
            framework.loadAndReturnError().map_err(|error| {
                format!(
                    "Could not load the updater: {}",
                    error.localizedDescription()
                )
            })?;
            let class = framework
                .classNamed(&NSString::from_str("SPUStandardUpdaterController"))
                .ok_or_else(|| "The bundled updater has no standard controller".to_owned())?;
            let controller: Allocated<AnyObject> = msg_send_id![class, alloc];
            let controller: Retained<AnyObject> = msg_send_id![controller,
                initWithStartingUpdater: false,
                updaterDelegate: None::<&AnyObject>,
                userDriverDelegate: None::<&AnyObject>
            ];
            let updater: Retained<AnyObject> = msg_send_id![&controller, updater];
            let mut error: Option<Retained<NSError>> = None;
            let started: bool = msg_send![&updater, startUpdater: &mut error];
            if !started {
                return Err(error.map_or_else(
                    || "The updater could not start".to_owned(),
                    |error| error.localizedDescription().to_string(),
                ));
            }
            Ok(Self {
                controller,
                updater,
                _framework: framework,
                _main_thread: main_thread,
            })
        }
    }

    pub fn status(&self) -> UpdateStatus {
        // SAFETY: the marker prevents moving this owner off the main thread.
        unsafe {
            UpdateStatus {
                available: true,
                can_check: msg_send![&self.updater, canCheckForUpdates],
                automatic_checks: msg_send![&self.updater, automaticallyChecksForUpdates],
                automatic_downloads: msg_send![&self.updater, automaticallyDownloadsUpdates],
            }
        }
    }

    pub fn check(&self) {
        if self.status().can_check {
            // SAFETY: nil sender is supported by the standard user interface.
            unsafe {
                let _: () = msg_send![&self.controller, checkForUpdates: None::<&AnyObject>];
            }
        }
    }

    pub fn toggle_automatic_checks(&self) {
        let enabled = !self.status().automatic_checks;
        // These setters persist through Sparkle's NSUserDefaults. Only invoke
        // them for actual user actions, never overwrite settings on startup.
        unsafe {
            let _: () = msg_send![&self.updater, setAutomaticallyChecksForUpdates: enabled];
        }
    }

    pub fn toggle_automatic_downloads(&self) {
        let enabled = !self.status().automatic_downloads;
        unsafe {
            let _: () = msg_send![&self.updater, setAutomaticallyDownloadsUpdates: enabled];
        }
    }
}
