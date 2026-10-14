//! Side-button remapping for the DJI receiver.
//!
//! The receiver's side button reports itself as a Consumer Control "Volume
//! Up" key on a separate HID interface from the vendor interface the
//! `device` crate drives. On macOS, [`SideButton`] seizes that HID interface
//! on each targeted receiver (so the system no longer sees Volume Up there)
//! and posts that receiver's keyboard [`Shortcut`] for each press instead.
//! Receivers that aren't targeted are left alone; releasing one — disabling
//! its binding or quitting the app — restores the original behavior.
//!
//! Other platforms compile a no-op [`SideButton`] that reports itself as
//! unsupported, so front-ends can hide the feature.
//!
//! Adapted from <https://github.com/TongkunZhang/dji-mic>.

#[cfg(target_os = "macos")]
mod macos;
mod service;
mod shortcut;

pub use service::{Binding, Config, PrivacyPane, ReceiverStatus, SideButton, Status, Target};
pub use shortcut::{keycode, Buttons, Key, Modifiers, Shortcut, KEYS};
