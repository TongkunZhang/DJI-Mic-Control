//! DJI Mic Mini model definition and heartbeat decoder.
//!
//! The v2 firmware update speaks a different dialect than v1 firmware: a
//! target-addressed 22-byte command shape (see [`packet::Dialect`]), a
//! different CRC-16 seed, and a restructured heartbeat.
//! [`decode`](MicMini::decode) recognises either version from the frame
//! itself; [`DeviceModel::build_command`]'s default implementation needs to
//! be told which one to speak since a command can't carry that information
//! implicitly the way a heartbeat can.

use super::DeviceModel;
use crate::packet;
use crate::settings::{Setting, SettingKind, SettingOption};
use crate::state::{DeviceStatus, Firmware, RxInfo, TxInfo};

/// The DJI Mic Mini (USB-C receiver + up to two transmitters).
pub struct MicMini;

const MODEL_ID: &str = "dji-mic-mini";

// v2 target addresses (see `PROTOCOL.md`).
const TARGET_RX: u16 = 0x0000;
const TARGET_TX: u16 = 0xffff;

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
static NC_POWER_OPTS: [SettingOption; 2] = off_on();
static NC_BUTTON_OPTS: [SettingOption; 2] = off_on();
static LOW_CUT_OPTS: [SettingOption; 2] = off_on();
// Segmented like Noise Cancel rather than a switch, since "off/on" reads
// oddly for a channel-mode choice. Wire values unchanged (stereo is 0x02,
// not 0x01).
static AUDIO_CHANNEL_OPTS: [SettingOption; 2] = [
    SettingOption { value: "mono", label: "Mono", wire: 0x00 },
    SettingOption { value: "stereo", label: "Stereo", wire: 0x02 },
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

static SETTINGS: [Setting; 12] = [
    Setting {
        id: "noise-cancel",
        label: "Noise Cancel",
        group: G_AUDIO,
        kind: SettingKind::Enum,
        v1_command: Some(0x031d),
        v2_command: 0x0037,
        v2_target: TARGET_TX,
        options: &NC_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "noise-cancel-power",
        label: "Noise Cancelling",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        // New in v2 firmware: previously this could only be toggled by the
        // physical button on the TX (see `noise-cancel-button` below).
        v1_command: None,
        v2_command: 0x0038,
        v2_target: TARGET_TX,
        options: &NC_POWER_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "noise-cancel-button",
        label: "Toggle via TX power button",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        // New in v2 firmware. Controls whether a quick single press of the
        // transmitter's power button toggles noise cancellation — it does
        // not itself enable/disable NC (see `noise-cancel-power`).
        v1_command: None,
        v2_command: 0x000f,
        v2_target: TARGET_TX,
        options: &NC_BUTTON_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "low-cut",
        label: "Low Cut",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        v1_command: Some(0x0303),
        v2_command: 0x0003,
        v2_target: TARGET_TX,
        options: &LOW_CUT_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "stereo",
        label: "Audio Channels",
        group: G_AUDIO,
        kind: SettingKind::Enum,
        v1_command: Some(0x0008),
        v2_command: 0x0008,
        v2_target: TARGET_RX,
        options: &AUDIO_CHANNEL_OPTS,
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
        v1_command: Some(0x0021),
        v2_command: 0x0021,
        v2_target: TARGET_RX,
        options: &SAFETY_OPTS,
        exclusive_with: &["stereo"],
        note: None,
    },
    Setting {
        id: "clip-limiter",
        label: "Clipping Control",
        group: G_AUDIO,
        kind: SettingKind::Toggle,
        v1_command: Some(0x001e),
        v2_command: 0x001e,
        v2_target: TARGET_RX,
        options: &CLIP_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "auto-off-15m",
        label: "Auto Off RX (15 min)",
        group: G_POWER,
        kind: SettingKind::Toggle,
        v1_command: Some(0x0010),
        v2_command: 0x0010,
        v2_target: TARGET_RX,
        options: &AUTO_OFF_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "tx-auto-off-15m",
        label: "Auto Off TX (15 min)",
        group: G_POWER,
        kind: SettingKind::Toggle,
        // New in v2 firmware: the receiver and transmitters' auto-off timers
        // are now configured independently, sharing one command id
        // distinguished only by target address.
        v1_command: None,
        v2_command: 0x0010,
        v2_target: TARGET_TX,
        options: &AUTO_OFF_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "camera-power",
        label: "RX On/Off follows Camera",
        group: G_POWER,
        kind: SettingKind::Toggle,
        v1_command: Some(0x0020),
        v2_command: 0x0020,
        v2_target: TARGET_RX,
        options: &CAMERA_OPTS,
        exclusive_with: &[],
        note: None,
    },
    Setting {
        id: "plug-free",
        label: "Plug-Free Ext. Speaker",
        group: G_DEVICE,
        kind: SettingKind::Toggle,
        v1_command: Some(0x0023),
        v2_command: 0x0023,
        v2_target: TARGET_RX,
        options: &PLUG_FREE_OPTS,
        exclusive_with: &[],
        note: Some("Changing this reboots the receiver."),
    },
    Setting {
        id: "mic-leds",
        label: "Mic LEDs",
        group: G_DEVICE,
        kind: SettingKind::Toggle,
        v1_command: Some(0x030a),
        // v2 firmware issued this setting a new command id even though
        // nothing else about it changed.
        v2_command: 0x000a,
        v2_target: TARGET_TX,
        options: &LED_OPTS,
        exclusive_with: &[],
        note: None,
    },
];

const HEADER_LEN: usize = 14;
const PRESENT_TX_LEN: usize = 23;
const ABSENT_TX_LEN: usize = 9;
const RX_LEN: usize = 22;

/// A decoded v1 TX slot: its info if present, plus fields valid even in a stub.
struct V1Slot {
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

fn parse_v1_slot(f: &[u8], off: usize) -> Option<V1Slot> {
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
        let serial = read_serial(&f[off + 9..off + 23]);
        Some(V1Slot {
            tx: Some(TxInfo { level: Some(level), serial: Some(serial), firmware: Some(firmware) }),
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
        Some(V1Slot { tx: None, state: f[off], flags, led_off: None, len: ABSENT_TX_LEN })
    }
}

/// Decode a v1 heartbeat.
fn decode_v1(f: &[u8]) -> Option<DeviceStatus> {
    if f.len() < HEADER_LEN + RX_LEN {
        return None;
    }

    let slot_a = parse_v1_slot(f, HEADER_LEN)?;
    let slot_b = parse_v1_slot(f, HEADER_LEN + slot_a.len)?;
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
        firmware: Some(read_firmware(&f[rx_off + 2..rx_off + 6])),
        serial: Some(read_serial(&f[rx_off + 7..rx_off + 21])),
    };
    let plug_free_on = f[rx_off + 21] == 0x01;

    let mut status = DeviceStatus::disconnected(MODEL_ID);
    status.connected = true;
    status.nc_enabled = flags & 0x01 != 0;
    status.tx = [slot_a.tx, slot_b.tx];
    status.rx = Some(rx);

    let s = &mut status.settings;
    s.insert("noise-cancel".into(), bit(state & 0x20 != 0, "strong", "basic"));
    // V1 firmware can't set this independently of the mode above (only the
    // TX's physical button toggles it), but it does report it — reflect the
    // real state so the GUI can show it read-only rather than blank.
    s.insert("noise-cancel-power".into(), on_off(status.nc_enabled));
    s.insert("low-cut".into(), on_off(state & 0x04 != 0));
    s.insert("stereo".into(), bit(rx_flags0 & 0x08 != 0, "stereo", "mono"));
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

// v2 status push: a fixed 52-byte flags header, then one 32-byte slot per
// connected transmitter (none for an absent one — unlike v1 there is no
// stub), then a 2-byte CRC. Identity (serial) and live audio level arrive in
// separate periodic pushes on this dialect (see [`decode_v2_records`]) that
// this frame carries forward from `previous` rather than knowing itself.
const V2_HEADER_LEN: usize = 52;
const V2_TX_SLOT_LEN: usize = 32;

/// Decode a v2 status push.
///
/// Returns `None` for anything of this dialect that isn't a status push (the
/// identity and audio-level pushes share the same heartbeat marker but don't
/// fit this frame's fixed header-plus-32-byte-slots shape).
fn decode_v2_status(previous: &DeviceStatus, f: &[u8]) -> Option<DeviceStatus> {
    if f.len() < V2_HEADER_LEN + 2 {
        return None;
    }
    let remainder = f.len() - (V2_HEADER_LEN + 2);
    if remainder % V2_TX_SLOT_LEN != 0 {
        return None;
    }
    let tx_count = (remainder / V2_TX_SLOT_LEN).min(2);

    let flags1 = f[20]; // strong-NC 0x08, NC power 0x20, camera-power 0x80
    let flags2 = f[21]; // auto-off (RX) 0x01, stereo 0x04
    let flags3 = f[48]; // plug-free 0x02, clip limiter 0x10, safety track 0x40

    // Low Cut, Mic LEDs, TX Auto Off, and the NC-button permission are purely
    // per-transmitter with no global fallback, so they're only knowable when
    // at least one TX is connected; all mirror identically across TX slots,
    // so slot 0 stands for all of them.
    let tx_flags = (tx_count > 0).then(|| f[V2_HEADER_LEN + 6]);
    let low_cut_flags = (tx_count > 0).then(|| f[V2_HEADER_LEN + 9]);

    let mut status = DeviceStatus::disconnected(MODEL_ID);
    status.connected = true;
    status.nc_enabled = flags1 & 0x20 != 0;
    // A slot's identity/level is only ever known from the separate identity
    // and audio-level pushes; carry forward whatever we already learned for
    // transmitters that are still connected, and drop it for ones that aren't.
    status.tx = std::array::from_fn(|i| {
        (i < tx_count).then(|| previous.tx[i].clone().unwrap_or(TxInfo {
            level: None,
            serial: None,
            firmware: None,
        }))
    });
    status.rx =
        Some(previous.rx.clone().unwrap_or(RxInfo { serial: None, firmware: None }));

    let s = &mut status.settings;
    s.insert("noise-cancel".into(), bit(flags1 & 0x08 != 0, "strong", "basic"));
    s.insert("noise-cancel-power".into(), on_off(flags1 & 0x20 != 0));
    s.insert("camera-power".into(), on_off(flags1 & 0x80 != 0));
    s.insert("auto-off-15m".into(), on_off(flags2 & 0x01 != 0));
    s.insert("stereo".into(), bit(flags2 & 0x04 != 0, "stereo", "mono"));
    s.insert("plug-free".into(), on_off(flags3 & 0x02 != 0));
    s.insert("clip-limiter".into(), on_off(flags3 & 0x10 != 0));
    s.insert("safety-track".into(), on_off(flags3 & 0x40 != 0));
    if let Some(tf) = tx_flags {
        s.insert("mic-leds".into(), bit(tf & 0x02 != 0, "off", "on"));
        s.insert("tx-auto-off-15m".into(), on_off(tf & 0x10 != 0));
        s.insert("noise-cancel-button".into(), on_off(tf & 0x80 != 0));
    }
    if let Some(lc) = low_cut_flags {
        s.insert("low-cut".into(), on_off(lc & 0x20 != 0));
    }

    Some(status)
}

// v2 identity and audio-level pushes share one record format starting at
// offset 14 (offsets 12–13 are a header this decoder doesn't need to
// interpret): repeated `[tag:1][unit index:1][reserved:3][length:1][data:
// length]` records running to the frame's trailing CRC.
//
// Known tags:
// - `0x01` (identity push): data is a 4-byte firmware version — most
//   significant component last, i.e. reversed relative to how it reads —
//   followed by a 14-byte ASCII serial. Unit index `0` = receiver, `1`/`2` =
//   transmitter slot 0/1.
// - `0x05` (audio-level push): one-byte data is the live input level. Unit
//   index is `1`/`2` for transmitter slot 0/1 (1-based, unlike the identity
//   push's 0-based index).
// - `0x04` (a 6-character hex-like id) and `0x06` (product name, e.g. `DJI Mic
//   Mini`) also appear in the identity push but aren't modelled by
//   `DeviceStatus` and are skipped.
const RECORDS_START: usize = 14;
const RECORD_HEADER_LEN: usize = 6;

fn decode_v2_records(previous: &DeviceStatus, f: &[u8]) -> Option<DeviceStatus> {
    if f.len() < RECORDS_START + 2 {
        return None;
    }
    let end = f.len() - 2; // exclude CRC
    let mut status = previous.clone();
    let mut matched = false;
    let mut i = RECORDS_START;

    while i + RECORD_HEADER_LEN <= end {
        let tag = f[i];
        let index = f[i + 1];
        let length = f[i + 5] as usize;
        let data_start = i + RECORD_HEADER_LEN;
        let data_end = data_start + length;
        if data_end > end {
            break; // not a frame built from these records after all
        }
        let data = &f[data_start..data_end];

        match tag {
            0x01 if data.len() > 4 => {
                // The first 4 bytes are the firmware version, most-significant
                // component last (i.e. reversed relative to how it reads);
                // the serial is whatever follows.
                let firmware = Firmware([data[3], data[2], data[1], data[0]]);
                let serial = read_serial(&data[4..]);
                match index {
                    0 => {
                        let rx = status.rx.get_or_insert(RxInfo { serial: None, firmware: None });
                        rx.serial = Some(serial);
                        rx.firmware = Some(firmware);
                    }
                    1 | 2 => {
                        if let Some(slot) = status.tx.get_mut((index - 1) as usize) {
                            let tx = slot.get_or_insert_with(|| TxInfo {
                                level: None,
                                serial: None,
                                firmware: None,
                            });
                            tx.serial = Some(serial);
                            tx.firmware = Some(firmware);
                        }
                    }
                    _ => {}
                }
                matched = true;
            }
            0x05 if data.len() == 1 && (index == 1 || index == 2) => {
                if let Some(slot) = status.tx.get_mut((index - 1) as usize) {
                    slot.get_or_insert_with(|| TxInfo {
                        level: None,
                        serial: None,
                        firmware: None,
                    })
                    .level = Some(data[0]);
                }
                matched = true;
            }
            _ => {}
        }

        i = data_end;
    }

    matched.then_some(status)
}

fn decode_v2(previous: &DeviceStatus, f: &[u8]) -> Option<DeviceStatus> {
    decode_v2_status(previous, f).or_else(|| decode_v2_records(previous, f))
}

impl DeviceModel for MicMini {
    fn id(&self) -> &'static str {
        MODEL_ID
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

    fn decode(&self, previous: &DeviceStatus, f: &[u8]) -> Option<DeviceStatus> {
        let dialect = packet::heartbeat_dialect(f)?;
        let mut status = match dialect {
            packet::Dialect::V1 => decode_v1(f),
            packet::Dialect::V2 => decode_v2(previous, f),
        }?;
        status.protocol_version = Some(match dialect {
            packet::Dialect::V1 => 1,
            packet::Dialect::V2 => 2,
        });
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
    use crate::packet::Dialect;

    fn fresh_status() -> DeviceStatus {
        DeviceStatus::disconnected(MODEL_ID)
    }

    /// A synthetic 84-byte 2-TX v1 heartbeat matching the protocol spec's
    /// documented layout, with placeholder serials and firmware.
    fn sample_2tx_v1() -> Vec<u8> {
        let body: Vec<u8> = vec![
            0x55, 0x54, 0x04, 0xf6, 0x5a, 0x02, 0x0e, 0x79, 0x00, 0x5b, 0x03,
            0x00, 0x47, 0x00, // header
            // TX1, serial "8SNTX100000000"
            0xb0, 0x25, 0x38, 0x00, 0x01, 0x01, 0x00, 0x38, 0x0e, 0x38, 0x53,
            0x4e, 0x54, 0x58, 0x31, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
            0x30, // TX2, serial "8SNTX200000000"
            0xb0, 0x25, 0x38, 0x00, 0x01, 0x01, 0x00, 0x38, 0x0e, 0x38, 0x53,
            0x4e, 0x54, 0x58, 0x32, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
            0x30, // RX, serial "SNRX0000000000"
            0x32, 0x20, 0x01, 0x01, 0x00, 0x38, 0x0e, 0x53, 0x4e, 0x52, 0x58,
            0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x00,
        ];
        crate::crc::append(body, crate::crc::V1_INIT, crate::crc::V1_RESIDUE)
    }

    #[test]
    fn decodes_two_tx_v1_heartbeat() {
        let hb = sample_2tx_v1();
        let st = MicMini.decode(&fresh_status(), &hb).expect("decode");
        assert!(st.connected);
        assert_eq!(st.protocol_version, Some(1));
        assert_eq!(st.tx_count(), 2);
        let tx1 = st.tx[0].as_ref().unwrap();
        assert_eq!(tx1.serial.as_deref(), Some("8SNTX100000000"));
        assert_eq!(tx1.firmware.unwrap().to_string(), "01.01.00.56");
        assert_eq!(st.tx[1].as_ref().unwrap().serial.as_deref(), Some("8SNTX200000000"));
        assert_eq!(st.rx.as_ref().unwrap().serial.as_deref(), Some("SNRX0000000000"));
        // state 0xb0 -> Strong NC (0x20 set), low cut clear.
        assert_eq!(st.settings["noise-cancel"], "strong");
        assert_eq!(st.settings["low-cut"], "off");
        // flags 0x25 -> NC enabled; v1 firmware still reports this even
        // though only the TX's button can change it.
        assert!(st.nc_enabled);
        assert_eq!(st.settings["noise-cancel-power"], "on");
        // rx flags 0x32/0x20 -> auto-off on, clip on, others off.
        assert_eq!(st.settings["auto-off-15m"], "on");
        assert_eq!(st.settings["stereo"], "mono");
        assert_eq!(st.settings["clip-limiter"], "on");
        assert_eq!(st.settings["safety-track"], "off");
        assert_eq!(st.settings["plug-free"], "off");
    }

    /// A synthetic 118-byte 2-TX v2 status push, CRC stripped (the test
    /// re-appends it) so this is exactly the 116-byte body.
    fn sample_2tx_v2() -> Vec<u8> {
        let hex = concat!(
            "557604a65a020000005b0303660003000000002028310c00000000000000",
            "0000000000000000000000000000030000001c00000002010000001ab825",
            "04010078000000000000000000000000000000000000000002020000001a",
            "b825040100780000000000000000000000000000000000000000",
        );
        let body: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        crate::crc::append(body, crate::crc::V2_INIT, crate::crc::V2_RESIDUE)
    }

    #[test]
    fn decodes_two_tx_v2_heartbeat() {
        let hb = sample_2tx_v2();
        let st = MicMini.decode(&fresh_status(), &hb).expect("decode");
        assert!(st.connected);
        assert_eq!(st.protocol_version, Some(2));
        assert_eq!(st.tx_count(), 2);
        // Identity isn't carried by this push on v2 firmware.
        assert!(st.tx[0].as_ref().unwrap().serial.is_none());
        assert!(st.rx.as_ref().unwrap().serial.is_none());
        // offset 20 = 0x28 -> strong NC (0x08) + NC power on (0x20).
        assert_eq!(st.settings["noise-cancel"], "strong");
        assert_eq!(st.settings["noise-cancel-power"], "on");
        assert!(st.nc_enabled);
        assert_eq!(st.settings["camera-power"], "off");
        // offset 21 = 0x31 -> auto-off on (0x01), stereo off (0x04 clear).
        assert_eq!(st.settings["auto-off-15m"], "on");
        assert_eq!(st.settings["stereo"], "mono");
        // offset 48 = 0x1c -> plug-free off, clip on (0x10), safety off.
        assert_eq!(st.settings["plug-free"], "off");
        assert_eq!(st.settings["clip-limiter"], "on");
        assert_eq!(st.settings["safety-track"], "off");
        // TX slot flags byte (offset 58) = 0xb8 -> LED on (0x02 clear),
        // TX auto-off on (0x10 set), NC-button permission on (0x80 set).
        assert_eq!(st.settings["mic-leds"], "on");
        assert_eq!(st.settings["tx-auto-off-15m"], "on");
        assert_eq!(st.settings["noise-cancel-button"], "on");
        // Low-cut byte (offset 61) = 0x01 -> off (0x20 clear).
        assert_eq!(st.settings["low-cut"], "off");
    }

    fn hex_body(hex: &str) -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect()
    }

    /// A synthetic receiver-only identity push (70 bytes), CRC stripped.
    fn sample_identity_rx_only() -> Vec<u8> {
        let hex = concat!(
            "5546048b5a020000005b030336000100000000120011030238534e5258313030",
            "30303030303004000000000630303030303006000000000c444a49204d696320",
            "4d696e69",
        );
        crate::crc::append(hex_body(hex), crate::crc::V2_INIT, crate::crc::V2_RESIDUE)
    }

    /// A synthetic receiver + one transmitter identity push (124 bytes, as
    /// sent after a reboot caused by toggling the plug-free speaker), CRC
    /// stripped.
    fn sample_identity_rx_and_tx() -> Vec<u8> {
        let hex = concat!(
            "557c04415a020000005b03036c000100000000120011030238534e5258313030",
            "30303030303004000000000630303030303006000000000c444a49204d696320",
            "4d696e690101000000120011030238534e545833303030303030303004010000",
            "000630303030303006010000000c444a49204d6963204d696e69",
        );
        crate::crc::append(hex_body(hex), crate::crc::V2_INIT, crate::crc::V2_RESIDUE)
    }

    /// A synthetic receiver + both transmitters identity push (178 bytes),
    /// CRC stripped. This is the largest v2 frame the protocol produces —
    /// `packet::take_frame` used to silently drop it as an "implausible
    /// length" until `MAX_FRAME` was raised to account for it, which is why
    /// this fixture exists.
    fn sample_identity_rx_and_two_tx() -> Vec<u8> {
        let hex = concat!(
            "55b204295a020000005b0303a2000100000000120011030238534e5258313030",
            "30303030303004000000000630303030303006000000000c444a49204d696320",
            "4d696e690101000000120011030238534e545833303030303030303004010000",
            "000630303030303006010000000c444a49204d6963204d696e69010200000012",
            "0011030238534e54583430303030303030300402000000063030303030300602",
            "0000000c444a49204d6963204d696e69",
        );
        crate::crc::append(hex_body(hex), crate::crc::V2_INIT, crate::crc::V2_RESIDUE)
    }

    /// A synthetic 2-TX audio-level push (30 bytes), CRC stripped.
    fn sample_level_2tx() -> Vec<u8> {
        let hex = "551e048a5a020000005b03030e000501000000013005020000000130";
        crate::crc::append(hex_body(hex), crate::crc::V2_INIT, crate::crc::V2_RESIDUE)
    }

    #[test]
    fn decodes_v2_identity_push_for_receiver_only() {
        let st = MicMini.decode(&fresh_status(), &sample_identity_rx_only()).expect("decode");
        let rx = st.rx.as_ref().unwrap();
        assert_eq!(rx.serial.as_deref(), Some("8SNRX100000000"));
        assert_eq!(rx.firmware.unwrap().to_string(), "02.03.17.00");
    }

    #[test]
    fn decodes_v2_identity_push_for_receiver_and_transmitter() {
        let st = MicMini.decode(&fresh_status(), &sample_identity_rx_and_tx()).expect("decode");
        assert_eq!(st.rx.as_ref().unwrap().serial.as_deref(), Some("8SNRX100000000"));
        let tx1 = st.tx[0].as_ref().unwrap();
        assert_eq!(tx1.serial.as_deref(), Some("8SNTX300000000"));
        assert_eq!(tx1.firmware.unwrap().to_string(), "02.03.17.00");
    }

    #[test]
    fn decodes_v2_identity_push_for_receiver_and_both_transmitters() {
        let st =
            MicMini.decode(&fresh_status(), &sample_identity_rx_and_two_tx()).expect("decode");
        assert_eq!(st.rx.as_ref().unwrap().serial.as_deref(), Some("8SNRX100000000"));
        assert_eq!(st.tx[0].as_ref().unwrap().serial.as_deref(), Some("8SNTX300000000"));
        assert_eq!(st.tx[1].as_ref().unwrap().serial.as_deref(), Some("8SNTX400000000"));
        // Firmware is identical across every unit in this fixture.
        for tx in st.tx.iter().flatten() {
            assert_eq!(tx.firmware.unwrap().to_string(), "02.03.17.00");
        }
    }

    #[test]
    fn take_frame_does_not_drop_the_178_byte_identity_push() {
        // Regression test for the bug this fixture exists to catch:
        // `MAX_FRAME` used to reject this frame outright as an implausible
        // length, silently discarding every identity push with two TX
        // connected before it ever reached `decode`.
        let mut buf = sample_identity_rx_and_two_tx();
        let frame = packet::take_frame(&mut buf).expect("frame");
        assert_eq!(frame.len(), 178);
        assert!(buf.is_empty());
    }

    #[test]
    fn decodes_v2_level_push() {
        // The level push alone doesn't know a TX is connected, so seed
        // `previous` as the status push already would have.
        let mut previous = fresh_status();
        previous.tx = [
            Some(TxInfo { level: None, serial: None, firmware: None }),
            Some(TxInfo { level: None, serial: None, firmware: None }),
        ];
        let st = MicMini.decode(&previous, &sample_level_2tx()).expect("decode");
        assert_eq!(st.tx[0].as_ref().unwrap().level, Some(0x30));
        assert_eq!(st.tx[1].as_ref().unwrap().level, Some(0x30));
    }

    #[test]
    fn v2_status_push_carries_identity_and_level_forward() {
        // Identity/level arrive on separate pushes from the flags; a later
        // status push for the same connected TX must not erase them.
        let after_identity =
            MicMini.decode(&fresh_status(), &sample_identity_rx_and_tx()).unwrap();
        let with_level = MicMini.decode(&after_identity, &sample_level_2tx()).unwrap();
        let status_push = MicMini.decode(&with_level, &sample_2tx_v2()).unwrap();

        assert_eq!(status_push.rx.as_ref().unwrap().serial.as_deref(), Some("8SNRX100000000"));
        assert_eq!(status_push.rx.as_ref().unwrap().firmware.unwrap().to_string(), "02.03.17.00");
        assert_eq!(status_push.tx[0].as_ref().unwrap().serial.as_deref(), Some("8SNTX300000000"));
        assert_eq!(status_push.tx[0].as_ref().unwrap().level, Some(0x30));
        assert_eq!(status_push.tx[1].as_ref().unwrap().level, Some(0x30));
        // The flags themselves still decode normally alongside the carried-
        // forward identity/level.
        assert_eq!(status_push.settings["noise-cancel"], "strong");
    }

    #[test]
    fn builds_v1_command_from_setting_slug() {
        let pkt = MicMini
            .build_command(1, Dialect::V1, "noise-cancel", "strong")
            .unwrap();
        assert_eq!(pkt.len(), packet::V1_COMMAND_LEN);
        assert_eq!(pkt[16], 0x01); // wire value
        assert!(MicMini.build_command(1, Dialect::V1, "bogus", "x").is_none());
        assert!(MicMini.build_command(1, Dialect::V1, "stereo", "maybe").is_none());
    }

    #[test]
    fn builds_v2_command_from_setting_slug() {
        let pkt = MicMini
            .build_command(1, Dialect::V2, "noise-cancel", "strong")
            .unwrap();
        assert_eq!(pkt.len(), packet::V2_COMMAND_LEN);
        assert_eq!(pkt[19], 0x01); // wire value
    }

    #[test]
    fn v2_only_setting_has_no_v1_command() {
        // "noise-cancel-power" was introduced by v2 firmware, so building it
        // for v1 devices must fail cleanly rather than send a command that
        // doesn't exist there.
        assert!(MicMini
            .build_command(1, Dialect::V1, "noise-cancel-power", "on")
            .is_none());
        assert!(MicMini
            .build_command(1, Dialect::V2, "noise-cancel-power", "on")
            .is_some());
    }
}
