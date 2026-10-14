//! Platform-independent shortcut model: key table, event sequencing, and
//! press debouncing. Kept free of OS calls so it is unit-tested everywhere.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

/// One key the side button can be mapped to.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Key {
    /// Stable id, matching the browser's `KeyboardEvent.code` (e.g. `"F18"`,
    /// `"KeyA"`) so the GUI can record a shortcut straight from a key press.
    pub code: &'static str,
    /// Human label, e.g. `"F18"`, `"A"`.
    pub label: &'static str,
    /// macOS virtual keycode (`kVK_*`).
    pub keycode: u16,
}

const fn key(code: &'static str, label: &'static str, keycode: u16) -> Key {
    Key {
        code,
        label,
        keycode,
    }
}

/// Every mappable key, in display order. F13–F20 come first: they exist on
/// no laptop keyboard, so they never collide with a real shortcut.
pub const KEYS: &[Key] = &[
    key("F13", "F13", 105),
    key("F14", "F14", 107),
    key("F15", "F15", 113),
    key("F16", "F16", 106),
    key("F17", "F17", 64),
    key("F18", "F18", 79),
    key("F19", "F19", 80),
    key("F20", "F20", 90),
    key("F1", "F1", 122),
    key("F2", "F2", 120),
    key("F3", "F3", 99),
    key("F4", "F4", 118),
    key("F5", "F5", 96),
    key("F6", "F6", 97),
    key("F7", "F7", 98),
    key("F8", "F8", 100),
    key("F9", "F9", 101),
    key("F10", "F10", 109),
    key("F11", "F11", 103),
    key("F12", "F12", 111),
    key("KeyA", "A", 0),
    key("KeyB", "B", 11),
    key("KeyC", "C", 8),
    key("KeyD", "D", 2),
    key("KeyE", "E", 14),
    key("KeyF", "F", 3),
    key("KeyG", "G", 5),
    key("KeyH", "H", 4),
    key("KeyI", "I", 34),
    key("KeyJ", "J", 38),
    key("KeyK", "K", 40),
    key("KeyL", "L", 37),
    key("KeyM", "M", 46),
    key("KeyN", "N", 45),
    key("KeyO", "O", 31),
    key("KeyP", "P", 35),
    key("KeyQ", "Q", 12),
    key("KeyR", "R", 15),
    key("KeyS", "S", 1),
    key("KeyT", "T", 17),
    key("KeyU", "U", 32),
    key("KeyV", "V", 9),
    key("KeyW", "W", 13),
    key("KeyX", "X", 7),
    key("KeyY", "Y", 16),
    key("KeyZ", "Z", 6),
    key("Digit0", "0", 29),
    key("Digit1", "1", 18),
    key("Digit2", "2", 19),
    key("Digit3", "3", 20),
    key("Digit4", "4", 21),
    key("Digit5", "5", 23),
    key("Digit6", "6", 22),
    key("Digit7", "7", 26),
    key("Digit8", "8", 28),
    key("Digit9", "9", 25),
    key("Space", "Space", 49),
    key("Enter", "Return", 36),
    key("Tab", "Tab", 48),
    key("Escape", "Esc", 53),
    key("Backspace", "Delete", 51),
    key("Delete", "Forward Delete", 117),
    key("ArrowUp", "↑", 126),
    key("ArrowDown", "↓", 125),
    key("ArrowLeft", "←", 123),
    key("ArrowRight", "→", 124),
    key("Home", "Home", 115),
    key("End", "End", 119),
    key("PageUp", "Page Up", 116),
    key("PageDown", "Page Down", 121),
    key("Minus", "-", 27),
    key("Equal", "=", 24),
    key("BracketLeft", "[", 33),
    key("BracketRight", "]", 30),
    key("Backslash", "\\", 42),
    key("Semicolon", ";", 41),
    key("Quote", "'", 39),
    key("Comma", ",", 43),
    key("Period", ".", 47),
    key("Slash", "/", 44),
    key("Backquote", "`", 50),
];

/// The macOS virtual keycode for a `KeyboardEvent.code`-style key id.
pub fn keycode(code: &str) -> Option<u16> {
    KEYS.iter().find(|k| k.code == code).map(|k| k.keycode)
}

/// A set of keyboard modifiers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub option: bool,
    pub shift: bool,
    pub command: bool,
}

/// `CGEventFlags` mask bits and left-hand virtual keycodes for each modifier,
/// in the order they are pressed.
pub(crate) const CONTROL: (u64, u16) = (1 << 18, 59);
pub(crate) const OPTION: (u64, u16) = (1 << 19, 58);
pub(crate) const SHIFT: (u64, u16) = (1 << 17, 56);
pub(crate) const COMMAND: (u64, u16) = (1 << 20, 55);

impl Modifiers {
    /// `(flag, keycode)` for each modifier set here, in press order.
    fn active(self) -> Vec<(u64, u16)> {
        [
            (self.ctrl, CONTROL),
            (self.option, OPTION),
            (self.shift, SHIFT),
            (self.command, COMMAND),
        ]
        .into_iter()
        .filter_map(|(on, m)| on.then_some(m))
        .collect()
    }
}

/// A keyboard shortcut: modifiers plus one key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcut {
    #[serde(flatten)]
    pub modifiers: Modifiers,
    /// A [`Key::code`] from [`KEYS`].
    pub key: String,
}

impl Default for Shortcut {
    /// Control+F18 — the hands-free dictation shortcut the original `dji-mic`
    /// bridge sends.
    fn default() -> Self {
        Self {
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
            key: "F18".into(),
        }
    }
}

impl fmt::Display for Shortcut {
    /// macOS notation, e.g. `⌃⌥F18`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let m = self.modifiers;
        for (on, sym) in [
            (m.ctrl, "⌃"),
            (m.option, "⌥"),
            (m.shift, "⇧"),
            (m.command, "⌘"),
        ] {
            if on {
                f.write_str(sym)?;
            }
        }
        let label = KEYS
            .iter()
            .find(|k| k.code == self.key)
            .map_or(self.key.as_str(), |k| k.label);
        f.write_str(label)
    }
}

impl Shortcut {
    /// Check the key is one we can post.
    pub fn validate(&self) -> Result<(), String> {
        match keycode(&self.key) {
            Some(_) => Ok(()),
            None => Err(format!("unsupported key: {:?}", self.key)),
        }
    }

    /// The `(keycode, key_down, flags)` events that type this shortcut.
    ///
    /// Modifiers the shortcut needs are pressed (in a fixed order) unless the
    /// user is already physically holding them, then the key is tapped, then
    /// the synthesized modifiers are released in reverse. Physical modifier
    /// flags are preserved throughout, and a physically held modifier is never
    /// released — that would desynchronize it from the real keyboard.
    pub fn sequence(&self, held: Modifiers, physical_flags: u64) -> Vec<(u16, bool, u64)> {
        let Some(key) = keycode(&self.key) else {
            return Vec::new();
        };
        let wanted = self.modifiers.active();
        let all: u64 = wanted.iter().fold(0, |acc, (flag, _)| acc | flag);
        let synthesized: Vec<(u64, u16)> = wanted
            .into_iter()
            .filter(|(flag, _)| !held.active().iter().any(|(h, _)| h == flag))
            .collect();

        let mut events = Vec::with_capacity(synthesized.len() * 2 + 2);
        let mut flags = physical_flags;
        for &(flag, code) in &synthesized {
            flags |= flag;
            events.push((code, true, flags));
        }
        events.push((key, true, physical_flags | all));
        events.push((key, false, physical_flags | all));
        for &(flag, code) in synthesized.iter().rev() {
            flags &= !flag;
            events.push((code, false, flags | physical_flags));
        }
        events
    }
}

/// Edge detection for button presses across several receivers: a press
/// triggers once, holding it does not repeat, and unplugging one receiver
/// resets only its state.
#[derive(Debug, Default)]
pub struct Buttons {
    down: HashSet<usize>,
}

impl Buttons {
    /// Record a button state change; true if this is a new press.
    pub fn update(&mut self, device: usize, down: bool) -> bool {
        if down {
            self.down.insert(device)
        } else {
            self.down.remove(&device);
            false
        }
    }

    /// Forget a removed receiver.
    pub fn remove(&mut self, device: usize) {
        self.down.remove(&device);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CTRL: u64 = CONTROL.0;

    #[test]
    fn balanced_shortcut() {
        assert_eq!(
            Shortcut::default().sequence(Modifiers::default(), 0),
            vec![
                (59, true, CTRL),
                (79, true, CTRL),
                (79, false, CTRL),
                (59, false, 0)
            ]
        );
    }

    #[test]
    fn never_releases_physical_control() {
        let held = Modifiers {
            ctrl: true,
            ..Modifiers::default()
        };
        let events = Shortcut::default().sequence(held, CTRL);
        assert_eq!(events, vec![(79, true, CTRL), (79, false, CTRL)]);
    }

    #[test]
    fn preserves_other_modifiers() {
        let shift = SHIFT.0;
        assert!(Shortcut::default()
            .sequence(Modifiers::default(), shift)
            .iter()
            .all(|e| e.2 & shift != 0));
    }

    #[test]
    fn multiple_modifiers_press_in_order_and_release_in_reverse() {
        let shortcut = Shortcut {
            modifiers: Modifiers {
                ctrl: true,
                option: true,
                command: true,
                shift: false,
            },
            key: "KeyD".into(),
        };
        let all = CONTROL.0 | OPTION.0 | COMMAND.0;
        assert_eq!(
            shortcut.sequence(Modifiers::default(), 0),
            vec![
                (59, true, CONTROL.0),
                (58, true, CONTROL.0 | OPTION.0),
                (55, true, all),
                (2, true, all),
                (2, false, all),
                (55, false, CONTROL.0 | OPTION.0),
                (58, false, CONTROL.0),
                (59, false, 0),
            ]
        );
    }

    #[test]
    fn bare_key_has_no_modifier_events() {
        let shortcut = Shortcut {
            modifiers: Modifiers::default(),
            key: "F13".into(),
        };
        assert_eq!(
            shortcut.sequence(Modifiers::default(), 0),
            vec![(105, true, 0), (105, false, 0)]
        );
    }

    #[test]
    fn unknown_key_is_rejected_and_sends_nothing() {
        let shortcut = Shortcut {
            modifiers: Modifiers::default(),
            key: "NotAKey".into(),
        };
        assert!(shortcut.validate().is_err());
        assert!(shortcut.sequence(Modifiers::default(), 0).is_empty());
        assert!(Shortcut::default().validate().is_ok());
    }

    #[test]
    fn key_table_has_unique_codes_and_keycodes() {
        let codes: HashSet<_> = KEYS.iter().map(|k| k.code).collect();
        let keycodes: HashSet<_> = KEYS.iter().map(|k| k.keycode).collect();
        assert_eq!(codes.len(), KEYS.len());
        assert_eq!(keycodes.len(), KEYS.len());
        assert_eq!(keycode("F18"), Some(79));
    }

    #[test]
    fn display_uses_mac_symbols() {
        assert_eq!(Shortcut::default().to_string(), "⌃F18");
        let s = Shortcut {
            modifiers: Modifiers {
                ctrl: true,
                option: true,
                shift: true,
                command: true,
            },
            key: "KeyK".into(),
        };
        assert_eq!(s.to_string(), "⌃⌥⇧⌘K");
    }

    #[test]
    fn serializes_flat() {
        let json = serde_json::to_value(Shortcut::default()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"ctrl": true, "option": false, "shift": false, "command": false, "key": "F18"})
        );
        let back: Shortcut = serde_json::from_value(json).unwrap();
        assert_eq!(back, Shortcut::default());
    }

    #[test]
    fn triggers_once_until_release() {
        let mut buttons = Buttons::default();
        assert!(buttons.update(1, true));
        assert!(!buttons.update(1, true));
        assert!(!buttons.update(1, false));
        assert!(buttons.update(1, true));
    }

    #[test]
    fn unplug_resets_only_that_receiver() {
        let mut buttons = Buttons::default();
        assert!(buttons.update(1, true));
        assert!(buttons.update(2, true));
        buttons.remove(1);
        assert!(buttons.update(1, true));
        assert!(!buttons.update(2, true));
    }
}
