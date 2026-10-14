//! Tauri commands bridging the Svelte front-end to the shared device layer.

use std::sync::Arc;

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
pub fn snapshot(device: Option<String>, mgr: State<'_, Arc<DeviceManager>>) -> Snapshot {
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

    Snapshot {
        os: device::current_os(),
        probe,
        devices,
        status,
        settings,
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

/// The side-button service plus where its config is saved.
pub struct SideButtonStore {
    pub service: remap::SideButton,
    /// `side-button.json` in the app config dir; `None` if it can't be resolved.
    pub path: Option<std::path::PathBuf>,
}

impl SideButtonStore {
    /// Load the saved config from `path` (or defaults) and start the service.
    pub fn load(path: Option<std::path::PathBuf>) -> Self {
        let config = path.as_deref().map(remap::Config::load).unwrap_or_default();
        Self {
            service: remap::SideButton::new(config),
            path,
        }
    }
}

/// Everything the side-button settings need for one render.
#[derive(Serialize)]
pub struct SideButtonState {
    pub config: remap::Config,
    pub status: remap::Status,
    /// Keys a shortcut may use, in display order.
    pub keys: &'static [remap::Key],
}

/// Read the side-button config and live status.
#[tauri::command]
pub fn side_button_state(store: State<'_, SideButtonStore>) -> SideButtonState {
    SideButtonState {
        config: store.service.config(),
        status: store.service.status(),
        keys: remap::KEYS,
    }
}

/// Apply and persist a new side-button config.
#[tauri::command]
pub fn set_side_button(
    config: remap::Config,
    store: State<'_, SideButtonStore>,
) -> Result<(), String> {
    store.service.set_config(config.clone())?;
    if let Some(path) = &store.path {
        config
            .save(path)
            .map_err(|e| format!("couldn't save side-button settings: {e}"))?;
    }
    Ok(())
}

/// Prompt for the macOS permissions the side button needs. Async so the
/// system prompts can't stall the main thread.
#[tauri::command]
pub async fn side_button_request_permissions(
    store: State<'_, SideButtonStore>,
) -> Result<(), String> {
    store.service.request_permissions();
    Ok(())
}

/// Open a Privacy & Security pane in System Settings.
#[tauri::command]
pub fn side_button_open_settings(pane: remap::PrivacyPane, store: State<'_, SideButtonStore>) {
    store.service.open_privacy_pane(pane);
}
