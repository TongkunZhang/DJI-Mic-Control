//! The long-running side-button service front-ends talk to.

use std::path::Path;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::shortcut::Shortcut;

/// The receiver whose side button is remapped (DJI Wireless Mic Rx).
#[cfg(target_os = "macos")]
const RECEIVER: (u16, u16) = (0x2ca3, 0x4011);

/// User configuration, persisted by the front-end.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Seize the side button and send [`shortcut`](Self::shortcut). When
    /// false the receiver is left alone and the button acts as Volume Up.
    pub enabled: bool,
    pub shortcut: Shortcut,
}

impl Config {
    /// Read a saved config, falling back to defaults if missing or invalid.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice::<Config>(&data).ok())
            .filter(|c| c.shortcut.validate().is_ok())
            .unwrap_or_default()
    }

    /// Write this config as JSON, creating the parent directory.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let data = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, data)
    }
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
    /// The button interface is currently seized.
    pub active: bool,
    /// Receivers currently attached to the seized interface.
    pub receivers: usize,
    /// Button presses seen since launch.
    pub presses: u64,
    /// Shortcuts successfully posted since launch.
    pub sent: u64,
    /// Last problem, if any.
    pub error: Option<String>,
}

/// Which macOS Privacy & Security pane to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrivacyPane {
    InputMonitoring,
    Accessibility,
}

struct Shared {
    config: Mutex<Config>,
    status: Mutex<Status>,
}

/// Owns the side-button worker. Cheap to share behind an `Arc`.
pub struct SideButton {
    shared: Arc<Shared>,
}

impl SideButton {
    /// Start the service with `config`. On macOS this spawns the worker
    /// thread that owns the HID run loop; elsewhere it only stores config.
    pub fn new(config: Config) -> Self {
        let shared = Arc::new(Shared {
            config: Mutex::new(config),
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

    pub fn config(&self) -> Config {
        self.shared.config.lock().unwrap().clone()
    }

    /// Apply a new config; the worker picks it up within a fraction of a second.
    pub fn set_config(&self, config: Config) -> Result<(), String> {
        config.shortcut.validate()?;
        *self.shared.config.lock().unwrap() = config;
        Ok(())
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

#[cfg(target_os = "macos")]
mod worker {
    use std::collections::HashSet;
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
        devices: HashSet<usize>,
    }

    impl Context {
        fn reset(&mut self) {
            self.devices.clear();
            self.buttons = Buttons::default();
            let mut status = self.shared.status.lock().unwrap();
            status.active = false;
            status.receivers = 0;
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
        let shortcut = context.shared.config.lock().unwrap().shortcut.clone();
        let sent = macos::send_shortcut(&shortcut);
        let mut status = context.shared.status.lock().unwrap();
        status.presses += 1;
        match sent {
            Ok(()) => {
                status.sent += 1;
                status.error = None;
            }
            Err(e) => status.error = Some(e),
        }
    }

    unsafe extern "C" fn attached(context: *mut c_void, result: i32, _: *mut c_void, device: Ref) {
        let context = &mut *context.cast::<Context>();
        let mut status = context.shared.status.lock().unwrap();
        if result == 0 {
            context.devices.insert(device as usize);
            status.receivers = context.devices.len();
        } else {
            status.error = Some(format!("receiver attachment failed: {result:#x}"));
        }
    }

    unsafe extern "C" fn removed(context: *mut c_void, _: i32, _: *mut c_void, device: Ref) {
        let context = &mut *context.cast::<Context>();
        context.buttons.remove(device as usize);
        context.devices.remove(&(device as usize));
        context.shared.status.lock().unwrap().receivers = context.devices.len();
    }

    const PERMISSIONS_MESSAGE: &str =
        "Grant DJI Mic Control Input Monitoring and Accessibility in System Settings.";

    pub fn run(shared: Arc<Shared>) {
        let mut context = Box::new(Context {
            shared: shared.clone(),
            buttons: Buttons::default(),
            devices: HashSet::new(),
        });
        let mut manager: Option<macos::Manager> = None;
        let mut next_check = Instant::now();
        let mut was_enabled = false;
        loop {
            let enabled = shared.config.lock().unwrap().enabled;
            if enabled != was_enabled {
                // React to the switch now rather than at the next poll.
                was_enabled = enabled;
                next_check = Instant::now();
            }
            if !enabled {
                if manager.take().is_some() {
                    // Dropping the manager releases the seized interface, so
                    // the button acts as Volume Up again.
                    context.reset();
                }
                shared.status.lock().unwrap().error = None;
            }

            if Instant::now() >= next_check {
                next_check = Instant::now() + Duration::from_secs(3);
                let (input_ok, ax_ok) = macos::permissions();
                {
                    let mut status = shared.status.lock().unwrap();
                    status.input_monitoring = input_ok;
                    status.accessibility = ax_ok;
                }
                if enabled && !(input_ok && ax_ok) {
                    manager = None;
                    context.reset();
                    shared.status.lock().unwrap().error = Some(PERMISSIONS_MESSAGE.into());
                } else if enabled && manager.is_none() {
                    // SAFETY: context stays allocated until after the manager
                    // drops; callbacks and teardown run only on this thread.
                    let opened = unsafe {
                        macos::Manager::open(
                            RECEIVER.0,
                            RECEIVER.1,
                            (&mut *context as *mut Context).cast(),
                            input,
                            attached,
                            removed,
                        )
                    };
                    match opened {
                        Ok(m) => {
                            manager = Some(m);
                            let mut status = shared.status.lock().unwrap();
                            status.active = true;
                            status.error = None;
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

    #[test]
    fn config_round_trips_and_falls_back() {
        let dir = std::env::temp_dir().join(format!("remap-test-{}", std::process::id()));
        let path = dir.join("side-button.json");
        assert_eq!(Config::load(&path), Config::default());

        let config = Config {
            enabled: true,
            shortcut: Shortcut {
                key: "KeyJ".into(),
                ..Shortcut::default()
            },
        };
        config.save(&path).unwrap();
        assert_eq!(Config::load(&path), config);

        std::fs::write(&path, br#"{"enabled": true, "shortcut": {"key": "Nope"}}"#).unwrap();
        assert_eq!(Config::load(&path), Config::default());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_invalid_shortcut() {
        let svc = SideButton::new(Config::default());
        let bad = Config {
            enabled: true,
            shortcut: Shortcut {
                key: "Nope".into(),
                ..Shortcut::default()
            },
        };
        assert!(svc.set_config(bad).is_err());
        assert_eq!(svc.config(), Config::default());
    }
}
