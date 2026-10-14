//! Tauri commands bridging the Svelte front-end to the shared device layer.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use device::{DeviceManager, DeviceStatus, DeviceSummary, Os, Probe, Setting};
use serde::Serialize;
use tauri::State;

/// Everything the UI needs for one render, fetched in a single call.
#[derive(Serialize)]
pub struct Snapshot {
    /// Host operating system.
    pub os: Os,
    /// Latest bus scan (drives the setup-help banner).
    pub probe: Probe,
    /// All tracked devices for the sidebar.
    pub devices: Vec<DeviceSummary>,
    /// Full status of the selected device, if any.
    pub status: Option<DeviceStatus>,
    /// Setting descriptors for the selected device's model.
    pub settings: Vec<Setting>,
    /// The selected receiver's side-button binding and state (macOS only).
    pub side_button: Option<SideButtonDevice>,
}

/// The udev-rules helper contents shown on Linux when access is blocked.
#[derive(Serialize)]
pub struct UdevHelp {
    pub file: String,
    pub rule: String,
    pub steps: Vec<String>,
}

/// Read the current state for `device` (or the sole device when omitted).
#[tauri::command]
pub fn snapshot(
    device: Option<String>,
    mgr: State<'_, Arc<DeviceManager>>,
    side_button: State<'_, Arc<SideButtonStore>>,
) -> Snapshot {
    let devices = mgr.list();
    let probe = mgr.probe();
    let selected = device.or_else(|| mgr.only_device());

    let (status, settings) = match &selected {
        Some(id) => (
            mgr.status(id).ok(),
            mgr.settings(id).map(|s| s.to_vec()).unwrap_or_default(),
        ),
        None => (None, Vec::new()),
    };
    let side_button = selected
        .as_deref()
        .and_then(|id| side_button.device_state(id, &mgr));

    Snapshot {
        os: device::current_os(),
        probe,
        devices,
        status,
        settings,
        side_button,
    }
}

/// Change one setting on a device.
#[tauri::command]
pub fn set_setting(
    device: String,
    setting: String,
    value: String,
    mgr: State<'_, Arc<DeviceManager>>,
) -> Result<(), String> {
    mgr.set(&device, &setting, &value).map_err(|e| e.to_string())
}

/// Change one setting on a specific transmitter slot (0-based) of a device —
/// for a setting that targets one TX individually rather than mirroring
/// across both (currently just Voice Tone).
#[tauri::command]
pub fn set_tx_setting(
    device: String,
    tx: usize,
    setting: String,
    value: String,
    mgr: State<'_, Arc<DeviceManager>>,
) -> Result<(), String> {
    mgr.set_tx(&device, tx, &setting, &value).map_err(|e| e.to_string())
}

/// The udev rule and instructions for Linux setup.
#[tauri::command]
pub fn udev_help() -> UdevHelp {
    UdevHelp {
        file: device::UDEV_RULE_FILE.to_string(),
        rule: device::udev_rule(),
        steps: device::udev_instructions(),
    }
}

/// The side-button service, the per-receiver bindings, and where they're saved.
pub struct SideButtonStore {
    pub service: remap::SideButton,
    config: Mutex<remap::Config>,
    /// `side-button.json` in the app config dir, once known.
    path: OnceLock<PathBuf>,
}

impl Default for SideButtonStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SideButtonStore {
    /// Start the service with no bindings; see [`load`](Self::load).
    pub fn new() -> Self {
        Self {
            service: remap::SideButton::new(),
            config: Mutex::new(remap::Config::default()),
            path: OnceLock::new(),
        }
    }

    /// Load the saved bindings from `path`, which later changes are saved to.
    pub fn load(&self, path: PathBuf) {
        *self.config.lock().unwrap() = remap::Config::load(&path);
        let _ = self.path.set(path);
    }

    /// Point the service at every tracked receiver whose binding is enabled.
    /// Cheap and idempotent: call it whenever devices or bindings change.
    pub fn sync(&self, mgr: &DeviceManager) {
        if !remap::SideButton::supported() {
            return;
        }
        let config = self.config.lock().unwrap();
        let targets = mgr
            .list()
            .into_iter()
            .filter_map(|d| {
                let binding = config.devices.get(&d.id).filter(|b| b.enabled)?;
                Some(remap::Target {
                    location_id: mgr.location_id(&d.id)?,
                    shortcut: binding.shortcut.clone(),
                })
            })
            .collect();
        self.service.set_targets(targets);
    }

    /// The side-button state of `device` for the UI, or `None` where
    /// remapping isn't supported.
    fn device_state(&self, device: &str, mgr: &DeviceManager) -> Option<SideButtonDevice> {
        if !remap::SideButton::supported() {
            return None;
        }
        let binding = self.config.lock().unwrap().binding(device);
        let location = mgr.location_id(device);
        let status = self.service.status();
        let receiver = location
            .and_then(|l| status.receivers.get(&l).cloned())
            .unwrap_or_default();
        Some(SideButtonDevice {
            available: location.is_some(),
            input_monitoring: status.input_monitoring,
            accessibility: status.accessibility,
            attached: receiver.attached,
            presses: receiver.presses,
            sent: receiver.sent,
            // Only surface service-wide problems for a receiver that's on.
            error: receiver
                .error
                .or(status.error.filter(|_| binding.enabled)),
            binding,
        })
    }
}

/// One receiver's side-button binding and live state.
#[derive(Serialize)]
pub struct SideButtonDevice {
    pub binding: remap::Binding,
    /// The receiver can be addressed (its location is known).
    pub available: bool,
    pub input_monitoring: bool,
    pub accessibility: bool,
    /// Its button is currently seized.
    pub attached: bool,
    pub presses: u64,
    pub sent: u64,
    pub error: Option<String>,
}

/// Keys a side-button shortcut may use, in display order.
#[tauri::command]
pub fn side_button_keys() -> &'static [remap::Key] {
    remap::KEYS
}

/// Apply and persist `device`'s side-button binding.
#[tauri::command]
pub fn set_side_button(
    device: String,
    binding: remap::Binding,
    store: State<'_, Arc<SideButtonStore>>,
    mgr: State<'_, Arc<DeviceManager>>,
) -> Result<(), String> {
    binding.shortcut.validate()?;
    let saved = {
        let mut config = store.config.lock().unwrap();
        config.devices.insert(device, binding);
        config.clone()
    };
    store.sync(&mgr);
    if let Some(path) = store.path.get() {
        saved
            .save(path)
            .map_err(|e| format!("couldn't save side-button settings: {e}"))?;
    }
    Ok(())
}

/// Prompt for the macOS permissions the side button needs. Async so the
/// system prompts can't stall the main thread.
#[tauri::command]
pub async fn side_button_request_permissions(
    store: State<'_, Arc<SideButtonStore>>,
) -> Result<(), String> {
    store.service.request_permissions();
    Ok(())
}

/// Open a Privacy & Security pane in System Settings.
#[tauri::command]
pub fn side_button_open_settings(pane: remap::PrivacyPane, store: State<'_, Arc<SideButtonStore>>) {
    store.service.open_privacy_pane(pane);
}
