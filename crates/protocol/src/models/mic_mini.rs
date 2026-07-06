//! DJI Mic Mini model definition and heartbeat decoder.

use super::DeviceModel;
use crate::settings::{Setting, SettingKind, SettingOption};
use crate::state::{DeviceStatus, Firmware, RxInfo, TxInfo};

/// The DJI Mic Mini (USB-C receiver + up to two transmitters).
pub struct MicMini;

// Command ids.
const CMD_NOISE_CANCEL: u16 = 0x031d;
const CMD_LOW_CUT: u16 = 0x0303;
const CMD_STEREO: u16 = 0x0008;
const CMD_SAFETY: u16 = 0x0021;
const CMD_CLIP: u16 = 0x001e;
const CMD_AUTO_OFF: u16 = 0x0010;
const CMD_CAMERA: u16 = 0x0020;
const CMD_PLUG_FREE: u16 = 0x0023;
const CMD_LEDS: u16 = 0x030a;

// Standard Off=0x00 / On=0x01 pair shared by most toggles.
const fn off_on() -> [SettingOption; 2] {
    [
        SettingOption { value: "off", label: "Off", wire: 0x00 },
        SettingOption { value: "on", label: "On", wire: 0x01 },
    ]
}

static NC_OPTS: [SettingOption; 2] = [
    SettingOption { value: "basic", label: "Basic", wire: 0x00 },
    SettingOption { value: "strong", label: "Strong", wire: 0x01 },
];
static LOW_CUT_OPTS: [SettingOption; 2] = off_on();
static STEREO_OPTS: [SettingOption; 2] = [
    SettingOption { value: "off", label: "Off", wire: 0x00 },
    SettingOption { value: "on", label: "On", wire: 0x02 }, // ON is 0x02, not 0x01
];
static SAFETY_OPTS: [SettingOption; 2] = off_on();
static CLIP_OPTS: [SettingOption; 2] = off_on();
static AUTO_OFF_OPTS: [SettingOption; 2] = off_on();
static CAMERA_OPTS: [SettingOption; 2] = off_on();
static PLUG_FREE_OPTS: [SettingOption; 2] = off_on();
// Off first / On second like every other toggle, so the UI switch reads
// naturally; the wire values remain inverted (On=0x00, Off=0x02).
static LED_OPTS: [SettingOption; 2] = [
    SettingOption { value: "off", label: "Off", wire: 0x02 },
    SettingOption { value: "on", label: "On", wire: 0x00 },
];

const G_AUDIO: &str = "Audio";
const G_POWER: &str = "Power & Startup";
const G_DEVICE: &str = "Device";

static SETTINGS: [Setting; 9] = [
    Setting {
        id: "noise-cancel",
        label: "Noise Cancel",
        group: G_AUDIO,
        kind: SettingKind::Enum,
        command: CMD_NOISE_CANCEL,
        options: &NC_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "low-cut",
        label: "Low Cut",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        command: CMD_LOW_CUT,
        options: &LOW_CUT_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "stereo",
        label: "Stereo Tracking",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        command: CMD_STEREO,
        options: &STEREO_OPTS,
        // Stereo and Safety Track share the second channel; enabling one
        // disables the other on the device.
        exclusive_with: &["safety-track"],
        note: None,
    },
    Setting {
        id: "safety-track",
        label: "Safety Track",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        command: CMD_SAFETY,
        options: &SAFETY_OPTS,
        exclusive_with: &["stereo"],
        note: None,
    },
    Setting {
        id: "clip-limiter",
        label: "Clip Limiter",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        command: CMD_CLIP,
        options: &CLIP_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "auto-off-15m",
        label: "Auto Off (15 min)",
        group: G_POWER,
        kind: SettingKind::Toggle,
        command: CMD_AUTO_OFF,
        options: &AUTO_OFF_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "camera-power",
        label: "Turn On With Camera",
        group: G_POWER,
        kind: SettingKind::Toggle,
        command: CMD_CAMERA,
        options: &CAMERA_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "plug-free",
        label: "Plug-Free Ext. Speaker",
        group: G_DEVICE,
        kind: SettingKind::Toggle,
        command: CMD_PLUG_FREE,
        options: &PLUG_FREE_OPTS,
        exclusive_with: &[],
        note: Some("Changing this reboots the receiver."),
    },
    Setting {
        id: "mic-leds",
        label: "Mic LEDs",
        group: G_DEVICE,
        kind: SettingKind::Toggle,
        command: CMD_LEDS,
        options: &LED_OPTS,
        exclusive_with: &[],
        note: None,
    },
];

const HEADER_LEN: usize = 14;
const PRESENT_TX_LEN: usize = 23;
const ABSENT_TX_LEN: usize = 9;
const RX_LEN: usize = 22;

/// A decoded TX slot: its info if present, plus fields valid even in a stub.
struct Slot {
    tx: Option<TxInfo>,
    state: u8,
    flags: u8,
    led_off: Option<bool>,
    len: usize,
}

fn read_firmware(b: &[u8]) -> Firmware {
    Firmware([b[0], b[1], b[2], b[3]])
}

fn read_serial(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).trim().to_string()
}

fn parse_slot(f: &[u8], off: usize) -> Option<Slot> {
    let flags = *f.get(off + 1)?;
    if flags & 0x20 != 0 {
        // Present entry.
        let end = off + PRESENT_TX_LEN;
        if f.len() < end {
            return None;
        }
        let state = f[off];
        let level = f[off + 2];
        let led_off = Some(f[off + 3] & 0x80 != 0);
        let firmware = read_firmware(&f[off + 4..off + 8]);
        let serial = read_serial(&f[off + 10..off + 23]);
        Some(Slot {
            tx: Some(TxInfo { level, serial, firmware }),
            state,
            flags,
            led_off,
            len: PRESENT_TX_LEN,
        })
    } else {
        // Absent stub.
        let end = off + ABSENT_TX_LEN;
        if f.len() < end {
            return None;
        }
        Some(Slot {
            tx: None,
            state: f[off],
            flags,
            led_off: None,
            len: ABSENT_TX_LEN,
        })
    }
}

impl DeviceModel for MicMini {
    fn id(&self) -> &'static str {
        "dji-mic-mini"
    }
    fn name(&self) -> &'static str {
        "DJI Mic Mini"
    }
    fn pictogram_key(&self) -> &'static str {
        "mic-mini"
    }
    fn usb_ids(&self) -> &'static [(u16, u16)] {
        &[(0x2ca3, 0x4011)]
    }
    fn interface(&self) -> u8 {
        6
    }
    fn detach_interfaces(&self) -> &'static [u8] {
        // Only the HID interface (0) blocks bulk I/O. Interfaces 1–3 are USB
        // Audio and must stay bound to snd-usb-audio so the mic still works as a
        // recording device.
        &[0]
    }
    fn bulk_in(&self) -> u8 {
        0x86
    }
    fn bulk_out(&self) -> u8 {
        0x06
    }
    fn settings(&self) -> &'static [Setting] {
        &SETTINGS
    }

    fn decode(&self, f: &[u8]) -> Option<DeviceStatus> {
        // Must be a heartbeat: SOF, no-ack flag, heartbeat marker.
        if f.len() < HEADER_LEN + RX_LEN || f[0] != 0x55 || f[8] != 0x00 {
            return None;
        }
        if f[9..12] != [0x5b, 0x03, 0x00] {
            return None;
        }

        let slot_a = parse_slot(f, HEADER_LEN)?;
        let slot_b = parse_slot(f, HEADER_LEN + slot_a.len)?;
        let rx_off = HEADER_LEN + slot_a.len + slot_b.len;
        if f.len() < rx_off + RX_LEN {
            return None;
        }

        // State/flags are identical across slots and valid even in stubs; prefer
        // a present slot so we read live data rather than a stale stub.
        let (state, flags) = if slot_a.tx.is_some() {
            (slot_a.state, slot_a.flags)
        } else {
            (slot_b.state, slot_b.flags)
        };
        let led_off = slot_a.led_off.or(slot_b.led_off);

        let rx_flags0 = f[rx_off];
        let rx_flags1 = f[rx_off + 1];
        let rx = RxInfo {
            firmware: read_firmware(&f[rx_off + 2..rx_off + 6]),
            serial: read_serial(&f[rx_off + 7..rx_off + 21]),
        };
        let plug_free_on = f[rx_off + 21] == 0x01;

        let mut status = DeviceStatus::disconnected(self.id());
        status.connected = true;
        status.nc_enabled = flags & 0x01 != 0;
        status.tx = [slot_a.tx, slot_b.tx];
        status.rx = Some(rx);

        let s = &mut status.settings;
        s.insert("noise-cancel".into(), bit(state & 0x20 != 0, "strong", "basic"));
        s.insert("low-cut".into(), on_off(state & 0x04 != 0));
        s.insert("stereo".into(), on_off(rx_flags0 & 0x08 != 0));
        s.insert("camera-power".into(), on_off(rx_flags0 & 0x01 != 0));
        s.insert("auto-off-15m".into(), on_off(rx_flags0 & 0x02 != 0));
        s.insert("safety-track".into(), on_off(rx_flags1 & 0x80 != 0));
        s.insert("clip-limiter".into(), on_off(rx_flags1 & 0x20 != 0));
        s.insert("plug-free".into(), on_off(plug_free_on));
        if let Some(off) = led_off {
            // LED state is only meaningful from a present TX entry.
            s.insert("mic-leds".into(), bit(off, "off", "on"));
        }

        Some(status)
    }
}

fn on_off(b: bool) -> String {
    bit(b, "on", "off")
}

fn bit(b: bool, yes: &str, no: &str) -> String {
    if b { yes } else { no }.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 84-byte 2-TX heartbeat from the protocol spec, CRC re-appended.
    fn sample_2tx() -> Vec<u8> {
        let body: Vec<u8> = vec![
            0x55, 0x54, 0x04, 0xf6, 0x5a, 0x02, 0x0e, 0x79, 0x00, 0x5b, 0x03,
            0x00, 0x47, 0x00, // header
            // TX1
            0xb0, 0x25, 0x38, 0x00, 0x01, 0x01, 0x00, 0x38, 0x0e, 0x38, 0x4e,
            0x57, 0x4b, 0x4a, 0x35, 0x4d, 0x30, 0x39, 0x34, 0x37, 0x42, 0x58,
            0x33, // TX2
            0xb0, 0x25, 0x38, 0x00, 0x01, 0x01, 0x00, 0x38, 0x0e, 0x38, 0x4e,
            0x57, 0x4b, 0x4a, 0x35, 0x50, 0x31, 0x31, 0x38, 0x33, 0x56, 0x48,
            0x47, // RX
            0x32, 0x20, 0x01, 0x01, 0x00, 0x38, 0x0e, 0x34, 0x4c, 0x42, 0x58,
            0x52, 0x36, 0x54, 0x30, 0x32, 0x38, 0x4d, 0x59, 0x44, 0x37, 0x00,
        ];
        crate::crc::append(body)
    }

    #[test]
    fn decodes_two_tx_heartbeat() {
        let hb = sample_2tx();
        let st = MicMini.decode(&hb).expect("decode");
        assert!(st.connected);
        assert_eq!(st.tx_count(), 2);
        let tx1 = st.tx[0].as_ref().unwrap();
        assert_eq!(tx1.serial, "NWKJ5M0947BX3");
        assert_eq!(tx1.firmware.to_string(), "01.01.00.56");
        assert_eq!(st.tx[1].as_ref().unwrap().serial, "NWKJ5P1183VHG");
        assert_eq!(st.rx.as_ref().unwrap().serial, "4LBXR6T028MYD7");
        // state 0xb0 -> Strong NC (0x20 set), low cut clear.
        assert_eq!(st.settings["noise-cancel"], "strong");
        assert_eq!(st.settings["low-cut"], "off");
        // flags 0x25 -> NC enabled.
        assert!(st.nc_enabled);
        // rx flags 0x32/0x20 -> auto-off on, clip on, others off.
        assert_eq!(st.settings["auto-off-15m"], "on");
        assert_eq!(st.settings["stereo"], "off");
        assert_eq!(st.settings["clip-limiter"], "on");
        assert_eq!(st.settings["safety-track"], "off");
        assert_eq!(st.settings["plug-free"], "off");
    }

    #[test]
    fn builds_command_from_setting_slug() {
        let pkt = MicMini.build_command(1, "noise-cancel", "strong").unwrap();
        assert_eq!(pkt[16], 0x01); // wire value
        assert!(MicMini.build_command(1, "bogus", "x").is_none());
        assert!(MicMini.build_command(1, "stereo", "maybe").is_none());
    }
}
