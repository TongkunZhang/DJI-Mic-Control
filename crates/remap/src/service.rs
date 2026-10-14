//! The long-running side-button service front-ends talk to.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::shortcut::Shortcut;

/// The receiver whose side button is remapped (DJI Wireless Mic Rx).
#[cfg(target_os = "macos")]
const RECEIVER: (u16, u16) = (0x2ca3, 0x4011);

/// One receiver's side-button setting.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Binding {
    /// Seize this receiver's side button and send [`shortcut`](Self::shortcut).
    /// When false the receiver is left alone and the button acts as Volume Up.
    pub enabled: bool,
    pub shortcut: Shortcut,
}

/// Per-receiver bindings, persisted by the front-end.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Keyed by the front-end's stable device id (e.g. the USB serial).
    pub devices: BTreeMap<String, Binding>,
}

impl Config {
    /// Read a saved config, falling back to defaults if missing or
    /// unreadable and dropping any binding with an unusable shortcut.
    pub fn load(path: &Path) -> Self {
        let mut config: Config = std::fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice(&data).ok())
            .unwrap_or_default();
        config.devices.retain(|_, b| b.shortcut.validate().is_ok());
        config
    }

    /// Write this config as JSON, creating the parent directory.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, data)
    }

    /// `device`'s binding, or the default (off, ⌃F18) if it has none.
    pub fn binding(&self, device: &str) -> Binding {
        self.devices.get(device).cloned().unwrap_or_default()
    }
}

/// A receiver to remap right now, identified by its IOKit location ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub location_id: u32,
    pub shortcut: Shortcut,
}

/// Live state of one targeted receiver.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ReceiverStatus {
    /// Its button interface is currently seized.
    pub attached: bool,
    /// Button presses seen since it was targeted.
    pub presses: u64,
    /// Shortcuts successfully posted since it was targeted.
    pub sent: u64,
    /// Last problem sending its shortcut, if any.
    pub error: Option<String>,
}

/// Live state for the UI.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    /// Whether remapping exists on this platform at all.
    pub supported: bool,
    /// Input Monitoring granted (needed to read the button).
    pub input_monitoring: bool,
    /// Accessibility granted (needed to post the shortcut).
    pub accessibility: bool,
    /// A problem affecting every receiver (permissions, seizing), if any.
    pub error: Option<String>,
    /// Targeted receivers by location ID.
    pub receivers: BTreeMap<u32, ReceiverStatus>,
}

/// Which macOS Privacy & Security pane to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivacyPane {
    InputMonitoring,
    Accessibility,
}

struct Shared {
    /// Location ID → shortcut for every receiver to seize.
    targets: Mutex<BTreeMap<u32, Shortcut>>,
    status: Mutex<Status>,
}

/// Owns the side-button worker. Cheap to share behind an `Arc`.
pub struct SideButton {
    shared: Arc<Shared>,
}

impl Default for SideButton {
    fn default() -> Self {
        Self::new()
    }
}

impl SideButton {
    /// Start the service with no targets. On macOS this spawns the worker
    /// thread that owns the HID run loop; elsewhere it does nothing.
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            targets: Mutex::new(BTreeMap::new()),
            status: Mutex::new(Status {
                supported: Self::supported(),
                ..Status::default()
            }),
        });
        #[cfg(target_os = "macos")]
        {
            let worker = shared.clone();
            std::thread::Builder::new()
                .name("side-button".into())
                .spawn(move || worker::run(worker))
                .expect("spawn side-button thread");
        }
        Self { shared }
    }

    /// Whether this platform supports remapping.
    pub const fn supported() -> bool {
        cfg!(target_os = "macos")
    }

    /// Replace the set of receivers to remap. Receivers not listed are
    /// released (their button acts as Volume Up again). Changing only a
    /// shortcut takes effect on the next press without reopening anything;
    /// the worker reacts to a changed receiver set within a fraction of a
    /// second.
    pub fn set_targets(&self, targets: Vec<Target>) {
        let map: BTreeMap<u32, Shortcut> = targets
            .into_iter()
            .filter(|t| t.shortcut.validate().is_ok())
            .map(|t| (t.location_id, t.shortcut))
            .collect();
        let wanted: Vec<u32> = map.keys().copied().collect();
        {
            let mut current = self.shared.targets.lock().unwrap();
            if *current == map {
                return;
            }
            *current = map;
        }

        // Keep counters for receivers still targeted; start fresh for new ones.
        let mut status = self.shared.status.lock().unwrap();
        status.receivers = retarget(std::mem::take(&mut status.receivers), &wanted);
    }

    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }

    /// Ask macOS for Input Monitoring and Accessibility. No-op elsewhere.
    pub fn request_permissions(&self) {
        #[cfg(target_os = "macos")]
        crate::macos::request_permissions();
    }

    /// Open a Privacy & Security pane in System Settings. No-op elsewhere.
    pub fn open_privacy_pane(&self, pane: PrivacyPane) {
        #[cfg(target_os = "macos")]
        {
            let anchor = match pane {
                PrivacyPane::InputMonitoring => "Privacy_ListenEvent",
                PrivacyPane::Accessibility => "Privacy_Accessibility",
            };
            let _ = std::process::Command::new("open")
                .arg(format!(
                    "x-apple.systempreferences:com.apple.preference.security?{anchor}"
                ))
                .spawn();
        }
        #[cfg(not(target_os = "macos"))]
        let _ = pane;
    }
}

/// Per-receiver status for exactly the `wanted` locations, carrying over
/// existing entries.
fn retarget(
    mut previous: BTreeMap<u32, ReceiverStatus>,
    wanted: &[u32],
) -> BTreeMap<u32, ReceiverStatus> {
    wanted
        .iter()
        .map(|&loc| (loc, previous.remove(&loc).unwrap_or_default()))
        .collect()
}

#[cfg(target_os = "macos")]
mod worker {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::{Shared, RECEIVER};
    use crate::macos::{self, Ref};
    use crate::shortcut::Buttons;

    /// Callback context. Boxed by [`run`] and outlives every manager.
    struct Context {
        shared: Arc<Shared>,
        buttons: Buttons,
        /// Attached IOHIDDevice pointer → its location ID.
        devices: HashMap<usize, u32>,
    }

    impl Context {
        fn reset(&mut self) {
            self.devices.clear();
            self.buttons = Buttons::default();
            let mut status = self.shared.status.lock().unwrap();
            for r in status.receivers.values_mut() {
                r.attached = false;
            }
        }
    }

    // Every callback runs on the worker's run-loop thread. The Box<Context>
    // outlives the HID manager; no references are retained across callbacks.
    unsafe extern "C" fn input(context: *mut c_void, result: i32, _: *mut c_void, value: Ref) {
        if result != 0 || value.is_null() {
            return;
        }
        let context = &mut *context.cast::<Context>();
        let Some((device, down)) = macos::button_value(value) else {
            return;
        };
        if !context.buttons.update(device, down) {
            return;
        }
        let Some(&location) = context.devices.get(&device) else {
            return;
        };
        let Some(shortcut) = context
            .shared
            .targets
            .lock()
            .unwrap()
            .get(&location)
            .cloned()
        else {
            return;
        };
        let sent = macos::send_shortcut(&shortcut);
        let mut status = context.shared.status.lock().unwrap();
        if let Some(r) = status.receivers.get_mut(&location) {
            r.presses += 1;
            match sent {
                Ok(()) => {
                    r.sent += 1;
                    r.error = None;
                }
                Err(e) => r.error = Some(e),
            }
        }
    }

    unsafe extern "C" fn attached(context: *mut c_void, result: i32, _: *mut c_void, device: Ref) {
        let context = &mut *context.cast::<Context>();
        if result != 0 {
            context.shared.status.lock().unwrap().error =
                Some(format!("receiver attachment failed: {result:#x}"));
            return;
        }
        let Some(location) = macos::device_location(device) else {
            return;
        };
        context.devices.insert(device as usize, location);
        let mut status = context.shared.status.lock().unwrap();
        if let Some(r) = status.receivers.get_mut(&location) {
            r.attached = true;
        }
    }

    unsafe extern "C" fn removed(context: *mut c_void, _: i32, _: *mut c_void, device: Ref) {
        let context = &mut *context.cast::<Context>();
        context.buttons.remove(device as usize);
        let Some(location) = context.devices.remove(&(device as usize)) else {
            return;
        };
        let still_attached = context.devices.values().any(|&l| l == location);
        let mut status = context.shared.status.lock().unwrap();
        if let Some(r) = status.receivers.get_mut(&location) {
            r.attached = still_attached;
        }
    }

    const PERMISSIONS_MESSAGE: &str =
        "Grant DJI Mic Control Input Monitoring and Accessibility in System Settings.";

    pub fn run(shared: Arc<Shared>) {
        let mut context = Box::new(Context {
            shared: shared.clone(),
            buttons: Buttons::default(),
            devices: HashMap::new(),
        });
        let mut manager: Option<macos::Manager> = None;
        // The receiver set the current (or next) manager is for.
        let mut wanted: Vec<u32> = Vec::new();
        let mut next_check = Instant::now();
        loop {
            let latest: Vec<u32> = shared.targets.lock().unwrap().keys().copied().collect();
            if latest != wanted {
                // Release everything and reopen for the new set right away.
                // Dropping the manager un-seizes the interfaces, so receivers
                // no longer targeted act as Volume Up again.
                wanted = latest;
                manager = None;
                context.reset();
                next_check = Instant::now();
            }

            if Instant::now() >= next_check {
                next_check = Instant::now() + Duration::from_secs(3);
                let (input_ok, ax_ok) = macos::permissions();
                {
                    let mut status = shared.status.lock().unwrap();
                    status.input_monitoring = input_ok;
                    status.accessibility = ax_ok;
                }
                if wanted.is_empty() {
                    shared.status.lock().unwrap().error = None;
                } else if !(input_ok && ax_ok) {
                    manager = None;
                    context.reset();
                    shared.status.lock().unwrap().error = Some(PERMISSIONS_MESSAGE.into());
                } else if manager.is_none() {
                    // SAFETY: context stays allocated until after the manager
                    // drops; callbacks and teardown run only on this thread.
                    let opened = unsafe {
                        macos::Manager::open(
                            RECEIVER.0,
                            RECEIVER.1,
                            &wanted,
                            (&mut *context as *mut Context).cast(),
                            input,
                            attached,
                            removed,
                        )
                    };
                    match opened {
                        Ok(m) => {
                            manager = Some(m);
                            shared.status.lock().unwrap().error = None;
                        }
                        Err(e) => {
                            context.reset();
                            shared.status.lock().unwrap().error = Some(e);
                        }
                    }
                }
            }

            if manager.is_some() {
                macos::pump();
            } else {
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcut(key: &str) -> Shortcut {
        Shortcut {
            key: key.into(),
            ..Shortcut::default()
        }
    }

    #[test]
    fn config_round_trips_and_drops_invalid_bindings() {
        let dir = std::env::temp_dir().join(format!("remap-test-{}", std::process::id()));
        let path = dir.join("side-button.json");
        assert_eq!(Config::load(&path), Config::default());

        let mut config = Config::default();
        config.devices.insert(
            "RX-A".into(),
            Binding {
                enabled: true,
                shortcut: shortcut("KeyJ"),
            },
        );
        config.devices.insert("RX-B".into(), Binding::default());
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path), config);

        std::fs::write(
            &path,
            br#"{"devices": {"RX-A": {"enabled": true, "shortcut": {"key": "Nope"}},
                             "RX-B": {"enabled": true, "shortcut": {"key": "F13"}}}}"#,
        )
        .unwrap();
        let loaded = Config::load(&path);
        assert_eq!(loaded.devices.len(), 1);
        assert_eq!(loaded.binding("RX-B").shortcut.key, "F13");
        assert_eq!(loaded.binding("RX-A"), Binding::default());

        // A file from the earlier global format yields an empty config.
        std::fs::write(&path, br#"{"enabled": true, "shortcut": {"key": "F18"}}"#).unwrap();
        assert_eq!(Config::load(&path), Config::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn retarget_keeps_counters_for_remaining_receivers() {
        let mut previous = BTreeMap::new();
        previous.insert(
            1,
            ReceiverStatus {
                presses: 5,
                ..ReceiverStatus::default()
            },
        );
        previous.insert(2, ReceiverStatus::default());
        let next = retarget(previous, &[1, 3]);
        assert_eq!(next.keys().copied().collect::<Vec<_>>(), vec![1, 3]);
        assert_eq!(next[&1].presses, 5);
        assert_eq!(next[&3].presses, 0);
    }

    #[test]
    fn set_targets_tracks_receivers_and_skips_invalid() {
        let svc = SideButton::new();
        svc.set_targets(vec![
            Target {
                location_id: 7,
                shortcut: shortcut("F18"),
            },
            Target {
                location_id: 9,
                shortcut: shortcut("Nope"),
            },
        ]);
        let status = svc.status();
        assert_eq!(
            status.receivers.keys().copied().collect::<Vec<_>>(),
            vec![7]
        );
        svc.set_targets(Vec::new());
        assert!(svc.status().receivers.is_empty());
    }
}
