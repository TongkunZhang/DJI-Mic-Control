# DJI Mic Mini USB protocol

The DJI Mic Mini is controlled over USB Bulk transfers on **interface 6**
(vendor-specific). Two endpoints carry all traffic:

- **Bulk OUT `0x06`** — host → device (commands)
- **Bulk IN `0x86`** — device → host (acknowledgements and heartbeats)

USB identifiers: **vendor `0x2ca3`, product `0x4011`**.

Multi-byte integer fields are little-endian unless stated otherwise.

---

## Packet framing

Every packet uses the DUML framing convention:

- Byte `0` is `0x55`, the start-of-frame marker.
- Byte `1` is the total packet length in bytes, including the header and the
  two-byte CRC at the end.
- The final two bytes are a CRC (see below).

To read a stream, scan for `0x55`, take `length` bytes, and verify the CRC; on a
mismatch, advance one byte and resynchronise.

---

## CRC

Packets end with a 2-byte CRC-16:

- **Polynomial:** `0x8408` (reflected form of `0x1021`)
- **Initial value:** `0x0000`
- **Residue:** `0xbb01` — running the CRC over an entire valid packet, including
  its two CRC bytes, yields `0xbb01`.

The two CRC bytes are chosen so this residue holds. This both validates received
packets and produces the checksum for outgoing ones.

---

## Command packets (host → device)

Sent to change a setting. The device replies with an ACK.

**Length: 19 bytes.** Example (Noise Cancel → Strong):

```
55 13 04 03 02 5a 01 00 40 5b 01 00 03 1d 00 01 01 91 fb
```

| Offset | Len | Value    | Description                                   |
| ------ | --- | -------- | --------------------------------------------- |
| 0      | 1   | `0x55`   | Start of frame                                |
| 1      | 1   | `0x13`   | Packet length (19)                            |
| 2      | 1   | `0x04`   | Protocol version                              |
| 3      | 1   | `0x03`   | Routing (host → device)                       |
| 4      | 1   | `0x02`   | Command set (audio subsystem)                 |
| 5      | 1   | `0x5a`   | Attribute flags                               |
| 6      | 2   | seq LE   | Sequence number, increments per command       |
| 8      | 1   | `0x40`   | ACK requested                                 |
| 9      | 1   | `0x5b`   | Constant                                      |
| 10     | 1   | `0x01`   | Constant                                      |
| 11     | 1   | `0x00`   | Constant                                      |
| 12     | 2   | cmd BE   | Command id, 16-bit **big-endian**             |
| 14     | 1   | `0x00`   | Constant                                      |
| 15     | 1   | `0x01`   | Payload length (1)                            |
| 16     | 1   | value    | Command value                                 |
| 17     | 2   | CRC      | See above                                     |

### Commands

| Command id | Value  | Action                              |
| ---------- | ------ | ----------------------------------- |
| `0x031d`   | `0x00` | Noise cancel: Basic                 |
| `0x031d`   | `0x01` | Noise cancel: Strong                |
| `0x0303`   | `0x00` | Low cut: Off                        |
| `0x0303`   | `0x01` | Low cut: On                         |
| `0x0008`   | `0x00` | Stereo tracking: Off                |
| `0x0008`   | `0x02` | Stereo tracking: On                 |
| `0x0021`   | `0x00` | Safety track: Off                   |
| `0x0021`   | `0x01` | Safety track: On                    |
| `0x001e`   | `0x00` | Clip limiter: Off                   |
| `0x001e`   | `0x01` | Clip limiter: On                    |
| `0x0010`   | `0x00` | Auto off after 15 min: Off          |
| `0x0010`   | `0x01` | Auto off after 15 min: On           |
| `0x0020`   | `0x00` | Turn on with camera: Off            |
| `0x0020`   | `0x01` | Turn on with camera: On             |
| `0x0023`   | `0x00` | Plug-free external speaker: Off      |
| `0x0023`   | `0x01` | Plug-free external speaker: On       |
| `0x030a`   | `0x00` | Mic LEDs: On                        |
| `0x030a`   | `0x02` | Mic LEDs: Off                       |

Notes:

- Stereo tracking uses `0x02` for On (not `0x01`).
- Mic LED values are inverted: `0x00` = On, `0x02` = Off.
- Toggling the plug-free external speaker reboots the receiver; the new state
  appears in heartbeats after the reboot completes.

---

## ACK packets (device → host)

Returned for every command.

**Length: 14 bytes.** Example:

```
55 0e 04 66 5a 02 01 00 80 5b 01 00 c0 e8
```

| Offset | Len | Value  | Description                       |
| ------ | --- | ------ | --------------------------------- |
| 0      | 1   | `0x55` | Start of frame                    |
| 1      | 1   | `0x0e` | Packet length (14)                |
| 2      | 1   | `0x04` | Protocol version                  |
| 3      | 1   | `0x66` | Routing (device → host ACK)       |
| 4      | 1   | `0x5a` | Attribute flags                   |
| 5      | 1   | `0x02` | Command set (audio subsystem)     |
| 6      | 2   | seq LE | Sequence echoed from the command  |
| 8      | 1   | `0x80` | ACK flag                          |
| 9      | 1   | `0x5b` | Constant                          |
| 10     | 1   | `0x01` | Constant                          |
| 11     | 1   | `0x00` | Constant                          |
| 12     | 2   | CRC    | See above                         |

---

## Heartbeat packets (device → host)

The device streams heartbeats continuously (~10 Hz) on the Bulk IN endpoint,
carrying current state and per-transmitter data. Packet size depends on how many
transmitters are powered on:

| Variant | Magic         | Length   | TX slots            |
| ------- | ------------- | -------- | ------------------- |
| 0-TX    | `55 38 04 e1` | 56 bytes | both absent         |
| 1-TX    | `55 46 04 8b` | 70 bytes | one present, one stub |
| 2-TX    | `55 54 04 f6` | 84 bytes | both present        |

All variants share a 14-byte header, two TX slots, a 22-byte RX entry, and a CRC.

### Header (14 bytes)

| Offset | Len | Value       | Description                                   |
| ------ | --- | ----------- | --------------------------------------------- |
| 0      | 1   | `0x55`      | Start of frame                                |
| 1      | 1   | length      | `0x38` / `0x46` / `0x54`                       |
| 2      | 1   | `0x04`      | Protocol version                              |
| 3      | 1   | type        | `0xe1` / `0x8b` / `0xf6` (encodes TX count)   |
| 4      | 1   | `0x5a`      | Attribute flags                               |
| 5      | 1   | `0x02`      | Command set (audio subsystem)                 |
| 6      | 2   | seq LE      | Sequence number                               |
| 8      | 1   | `0x00`      | No ACK required                               |
| 9      | 3   | `5b 03 00`  | Heartbeat marker                              |
| 12     | 1   | —           | Device sub-state                              |
| 13     | 1   | `0x00`      | Constant                                      |

### TX slots

Starting at offset 14, each of the two slots is either a **present entry**
(23 bytes) or an **absent stub** (9 bytes). Byte `+1` of a slot distinguishes
them: `& 0x20` set → present, `& 0x40` set → absent. Slot 0 is TX1, slot 1 is TX2.

**Present TX entry (23 bytes):**

| Slot offset | Len | Description                                                    |
| ----------- | --- | -------------------------------------------------------------- |
| +0          | 1   | State byte (see below)                                         |
| +1          | 1   | TX flags (see below)                                           |
| +2          | 1   | Audio input level                                             |
| +3          | 1   | LED flags — bit `0x80` set = LEDs off                          |
| +4          | 4   | Firmware version (one byte per component, decimal)             |
| +8          | 2   | Constant `0e 38`                                               |
| +10         | 13  | Transmitter serial number (ASCII)                             |

**Absent TX stub (9 bytes):** `+0` state byte, `+1` TX flags, `+2`..`+8` zero.
The state and flags bytes are valid even in a stub.

**State byte (`+0`):**

| Mask   | Meaning                                  |
| ------ | ---------------------------------------- |
| `0x20` | Strong noise-cancel mode (clear = Basic) |
| `0x04` | Low cut active                           |

**TX flags byte (`+1`):**

| Mask   | Meaning                                              |
| ------ | ---------------------------------------------------- |
| `0x20` | Present TX marker                                    |
| `0x40` | Absent stub marker                                   |
| `0x01` | Noise cancel enabled (toggled by the TX's button)    |

### RX entry (22 bytes)

Follows the two TX slots (absolute offset 32 / 46 / 60 for 0 / 1 / 2-TX).

| RX offset | Len | Description                                        |
| --------- | --- | -------------------------------------------------- |
| +0        | 1   | RX flags byte 0 (see below)                        |
| +1        | 1   | RX flags byte 1 (see below)                        |
| +2        | 4   | Firmware version (one byte per component, decimal) |
| +6        | 1   | Constant `0x0e`                                    |
| +7        | 14  | Receiver serial number (ASCII)                     |
| +21       | 1   | Plug-free external speaker: `0x01` on, `0x00` off  |

**RX flags byte 0:**

| Mask   | Meaning                    |
| ------ | -------------------------- |
| `0x01` | Turn on with camera active |
| `0x02` | Auto off after 15 min active |
| `0x08` | Stereo tracking active     |

**RX flags byte 1:**

| Mask   | Meaning              |
| ------ | -------------------- |
| `0x80` | Safety track active  |
| `0x20` | Clip limiter active  |

### Example: 2-TX heartbeat (84 bytes)

```
55 54 04 f6 5a 02 0e 79 00 5b 03 00 47 00        header
b0 25 38 00 01 01 00 38 0e 38 4e 57 4b 4a 35 4d 30 39 34 37 42 58 33   TX1
b0 25 38 00 01 01 00 38 0e 38 4e 57 4b 4a 35 50 31 31 38 33 56 48 47   TX2
32 20 01 01 00 38 0e 34 4c 42 58 52 36 54 30 32 38 4d 59 44 37 00      RX
<crc>
```

Decoded:

- TX1 — state `0xb0` (Strong NC, low cut off), level `0x38`, serial `NWKJ5M0947BX3`,
  firmware `01.01.00.56`.
- TX2 — serial `NWKJ5P1183VHG`.
- RX — flags `0x32`/`0x20` (auto-off on, clip on, others off), serial
  `4LBXR6T028MYD7`, plug-free off.
