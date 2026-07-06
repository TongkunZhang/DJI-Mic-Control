//! Per-product model definitions.
//!
//! A [`DeviceModel`] describes everything front-ends and the transport need to
//! know about one product: how to recognise it on USB, which settings it has,
//! how to build a command, and how to decode its status. Supporting a new
//! microphone — even one speaking a different protocol — means implementing this
//! trait and adding it to [`MODELS`]; nothing else changes.

mod mic_mini;

pub use mic_mini::MicMini;

use crate::packet;
use crate::settings::Setting;
use crate::state::DeviceStatus;

/// A supported microphone product.
pub trait DeviceModel: Send + Sync {
    /// Stable identifier, e.g. `"dji-mic-mini"`.
    fn id(&self) -> &'static str;
    /// Human name, e.g. `"DJI Mic Mini"`.
    fn name(&self) -> &'static str;
    /// Asset key the GUI maps to a pictogram.
    fn pictogram_key(&self) -> &'static str;

    /// `(vendor, product)` id pairs this model matches.
    fn usb_ids(&self) -> &'static [(u16, u16)];
    /// Vendor interface number to claim.
    fn interface(&self) -> u8;
    /// Interfaces (besides the claimed vendor one) whose kernel driver must be
    /// detached for bulk I/O to work. Keep this minimal so unrelated interfaces
    /// — notably USB Audio — stay bound to their drivers and the device remains
    /// usable as a normal sound device.
    fn detach_interfaces(&self) -> &'static [u8] {
        &[]
    }
    /// Bulk IN endpoint (device → host).
    fn bulk_in(&self) -> u8;
    /// Bulk OUT endpoint (host → device).
    fn bulk_out(&self) -> u8;

    /// The settings this model exposes.
    fn settings(&self) -> &'static [Setting];

    /// Build a command packet setting `setting_id` to the option `value`.
    ///
    /// The default implementation frames a standard DUML command from the
    /// setting's command id and the option's wire byte. Returns `None` if the
    /// setting or value is unknown.
    fn build_command(&self, seq: u16, setting_id: &str, value: &str) -> Option<Vec<u8>> {
        let setting = self.settings().iter().find(|s| s.id == setting_id)?;
        let option = setting.option(value)?;
        Some(packet::build_command(seq, setting.command, option.wire))
    }

    /// Decode a device→host frame into a status snapshot, or `None` if the frame
    /// is not a heartbeat this model understands.
    fn decode(&self, frame: &[u8]) -> Option<DeviceStatus>;
}

/// Every supported model.
pub const MODELS: &[&dyn DeviceModel] = &[&MicMini];

/// Find the model matching a USB `(vendor, product)` pair.
pub fn model_for_usb(vendor: u16, product: u16) -> Option<&'static dyn DeviceModel> {
    MODELS
        .iter()
        .copied()
        .find(|m| m.usb_ids().contains(&(vendor, product)))
}

/// Find a model by its identifier.
pub fn model_by_id(id: &str) -> Option<&'static dyn DeviceModel> {
    MODELS.iter().copied().find(|m| m.id() == id)
}
