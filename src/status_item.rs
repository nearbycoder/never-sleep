//! AppKit status item for Rune's background lifetime. Create, update and drop on
//! the main thread; the retained target outlives AppKit's non-retaining references.

use objc2::{ClassType, DeclaredClass, declare_class, msg_send_id, mutability, rc::Retained, sel};
use objc2_app_kit::{
    NSApplication, NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength, NSWorkspace,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSSize, NSString};
use std::cell::Cell;

pub fn reduce_transparency() -> bool {
    // Read only on Rune's UI thread; AppKit supplies the user's accessibility choice.
    unsafe { NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceTransparency() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    ToggleEnabled,
    ToggleDisplay,
    CheckForUpdates,
    ToggleAutomaticChecks,
    ToggleAutomaticDownloads,
    Quit,
}

struct TargetState {
    callback: Box<dyn Fn(Action)>,
    can_check_updates: Cell<bool>,
}

declare_class!(
    struct NeverSleepMenuTarget;

    unsafe impl ClassType for NeverSleepMenuTarget {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "NeverSleepMenuTarget";
    }

    impl DeclaredClass for NeverSleepMenuTarget {
        type Ivars = TargetState;
    }

    unsafe impl NeverSleepMenuTarget {
        #[method(validateMenuItem:)]
        fn validate(&self, sender: &NSMenuItem) -> bool {
            unsafe { sender.tag() != 5 || self.ivars().can_check_updates.get() }
        }

        #[method(neverSleepAction:)]
        fn perform(&self, sender: &NSMenuItem) {
            // SAFETY: AppKit invokes target/action on its main thread, with the
            // NSMenuItem attached to this selector. Tags are assigned below.
            let action = match unsafe { sender.tag() } {
                1 => Action::Open,
                2 => Action::ToggleEnabled,
                3 => Action::ToggleDisplay,
                4 => Action::Quit,
                5 => Action::CheckForUpdates,
                6 => Action::ToggleAutomaticChecks,
                7 => Action::ToggleAutomaticDownloads,
                _ => return,
            };
            // Never allow a Rust unwind through Objective-C's event dispatch.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (self.ivars().callback)(action);
            })).is_err() {
                eprintln!("Never Sleep: menu action callback panicked");
            }
        }
    }
);

pub struct StatusItem {
    main_thread: MainThreadMarker,
    bar: Retained<NSStatusBar>,
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    status: Retained<NSMenuItem>,
    enabled: Retained<NSMenuItem>,
    display: Retained<NSMenuItem>,
    check_updates: Retained<NSMenuItem>,
    automatic_checks: Retained<NSMenuItem>,
    automatic_downloads: Retained<NSMenuItem>,
    application_check: Option<(Retained<NSMenu>, Retained<NSMenuItem>)>,
    // NSMenuItem does not retain its target. Keep it until Drop clears every
    // target, even if AppKit is still retaining a tracking menu.
    _target: Retained<NeverSleepMenuTarget>,
}

impl StatusItem {
    pub fn new(on_action: impl Fn(Action) + 'static) -> Result<Self, String> {
        let main_thread = MainThreadMarker::new()
            .ok_or_else(|| "The menu bar must be created on the main thread".to_owned())?;

        // SAFETY: all AppKit objects are allocated and used on the main thread.
        // Retained ownership covers the menu, items and target for their entire
        // installation. NSObject initialization follows setting the Rust ivars.
        unsafe {
            let application = NSApplication::sharedApplication(main_thread);
            let target: Retained<NeverSleepMenuTarget> = msg_send_id![
                super(main_thread.alloc().set_ivars(TargetState {
                    callback: Box::new(on_action),
                    can_check_updates: Cell::new(false),
                })),
                init
            ];
            let menu = NSMenu::new(main_thread);
            menu.setAutoenablesItems(false);
            let make_item = |title: &str, tag: isize, key: &str| {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    main_thread.alloc(),
                    &NSString::from_str(title),
                    Some(sel!(neverSleepAction:)),
                    &NSString::from_str(key),
                );
                item.setTag(tag);
                item.setTarget(Some(&target));
                item.setEnabled(true);
                item
            };

            let status = NSMenuItem::initWithTitle_action_keyEquivalent(
                main_thread.alloc(),
                &NSString::from_str("Never Sleep"),
                None,
                &NSString::from_str(""),
            );
            status.setEnabled(false);
            menu.addItem(&status);
            menu.addItem(&NSMenuItem::separatorItem(main_thread));
            menu.addItem(&make_item("Show Never Sleep…", 1, ","));
            let enabled = make_item("Keep awake on external power", 2, "");
            let display = make_item("Keep display awake", 3, "");
            menu.addItem(&enabled);
            menu.addItem(&display);
            menu.addItem(&NSMenuItem::separatorItem(main_thread));
            let check_updates = make_item("Check for Updates…", 5, "");
            let automatic_checks = make_item("Automatically Check for Updates", 6, "");
            let automatic_downloads = make_item("Install Updates Automatically", 7, "");
            for item in [&check_updates, &automatic_checks, &automatic_downloads] {
                item.setEnabled(false);
                menu.addItem(item);
            }
            menu.addItem(&NSMenuItem::separatorItem(main_thread));
            menu.addItem(&make_item("Quit Never Sleep", 4, "q"));

            let bar = NSStatusBar::systemStatusBar();
            let item = bar.statusItemWithLength(NSVariableStatusItemLength);
            let Some(button) = item.button(main_thread) else {
                bar.removeStatusItem(&item);
                return Err("macOS could not create the menu bar button".to_owned());
            };
            if let Some(icon) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
                &NSString::from_str("sun.max"),
                Some(&NSString::from_str("Never Sleep")),
            ) {
                icon.setTemplate(true);
                icon.setSize(NSSize::new(19.0, 19.0));
                button.setImage(Some(&icon));
            } else {
                // A readable fallback if the installed macOS lacks the symbol.
                button.setTitle(&NSString::from_str("NS"));
            }
            button.setToolTip(Some(&NSString::from_str("Never Sleep")));
            item.setMenu(Some(&menu));
            let application_check = application
                .mainMenu()
                .and_then(|main_menu| main_menu.itemAtIndex(0))
                .and_then(|item| item.submenu())
                .map(|application_menu| {
                    let item = make_item("Check for Updates…", 5, "");
                    item.setEnabled(false);
                    application_menu
                        .insertItem_atIndex(&item, 2.min(application_menu.numberOfItems()));
                    (application_menu, item)
                });

            Ok(Self {
                main_thread,
                bar,
                item,
                menu,
                status,
                enabled,
                display,
                check_updates,
                automatic_checks,
                automatic_downloads,
                application_check,
                _target: target,
            })
        }
    }

    pub fn update(
        &self,
        enabled: bool,
        display: bool,
        status: &str,
        updates: crate::updater::UpdateStatus,
    ) {
        // SAFETY: the stored MainThreadMarker prevents moving this owner to
        // another thread. These retained items remain installed in our menu.
        unsafe {
            self.enabled.setState(isize::from(enabled));
            self.display.setState(isize::from(display));
            self.check_updates.setEnabled(updates.can_check);
            self._target
                .ivars()
                .can_check_updates
                .set(updates.can_check);
            if let Some((_, item)) = &self.application_check {
                item.setEnabled(updates.can_check);
            }
            self.automatic_checks.setEnabled(updates.available);
            self.automatic_checks
                .setState(isize::from(updates.automatic_checks));
            self.automatic_downloads
                .setEnabled(updates.available && updates.automatic_checks);
            self.automatic_downloads
                .setState(isize::from(updates.automatic_downloads));
            self.status.setTitle(&NSString::from_str(status));
            if let Some(button) = self.item.button(self.main_thread) {
                button.setToolTip(Some(&NSString::from_str(&format!(
                    "Never Sleep — {status}"
                ))));
            }
        }
    }
}

impl Drop for StatusItem {
    fn drop(&mut self) {
        // SAFETY: clear AppKit's weak target/action links before releasing the
        // Rust callback; removing the status item ends its installation.
        unsafe {
            if let Some((menu, item)) = &self.application_check {
                item.setTarget(None);
                item.setAction(None);
                menu.removeItem(item);
            }
            for index in 0..self.menu.numberOfItems() {
                if let Some(item) = self.menu.itemAtIndex(index) {
                    item.setTarget(None);
                    item.setAction(None);
                }
            }
            self.item.setMenu(None);
            self.bar.removeStatusItem(&self.item);
        }
    }
}
