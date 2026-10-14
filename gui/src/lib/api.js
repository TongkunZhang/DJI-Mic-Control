import { invoke } from "@tauri-apps/api/core";

/** Fetch a full UI snapshot for the given device id (or the sole device). */
export function snapshot(device) {
  return invoke("snapshot", { device: device ?? null });
}

/** Change one setting on a device. Resolves on success, rejects with a message. */
export function setSetting(device, setting, value) {
  return invoke("set_setting", { device, setting, value });
}

/**
 * Change one setting on a specific transmitter slot (0-based) of a device —
 * for a setting that targets one TX individually rather than mirroring
 * across both (currently just Voice Tone).
 */
export function setTxSetting(device, tx, setting, value) {
  return invoke("set_tx_setting", { device, tx, setting, value });
}

/** Fetch the Linux udev-rules helper text. */
export function udevHelp() {
  return invoke("udev_help");
}

/** Side-button remapping config, live status, and the mappable keys. */
export function sideButtonState() {
  return invoke("side_button_state");
}

/** Apply and persist a side-button config (`{ enabled, shortcut }`). */
export function setSideButton(config) {
  return invoke("set_side_button", { config });
}

/** Prompt for the macOS permissions the side button needs. */
export function requestSideButtonPermissions() {
  return invoke("side_button_request_permissions");
}

/** Open a Privacy & Security pane: "input-monitoring" or "accessibility". */
export function openSideButtonSettings(pane) {
  return invoke("side_button_open_settings", { pane });
}
