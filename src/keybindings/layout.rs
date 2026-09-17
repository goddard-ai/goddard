//! Keyboard layout model for the keybinding manager's fixed stage.
//!
//! A key has two identities: `code` is the stable physical position (Web
//! `KeyboardEvent.code` spelling — `KeyQ`, `Digit1`, `IntlBackslash`), and
//! `unshifted`/`shifted` are the logical characters the position produces in
//! the layout. Goddard stores and dispatches *logical* bindings in v1; the
//! layout maps a logical key to the position that produces it, which is why
//! legends come from layout data, never from the code name.

/// Stable layout identifier, persisted in `keybindings.json` and exports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutId {
    UsAnsi,
    UkIso,
    DeIso,
    DvorakAnsi,
}

impl LayoutId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UsAnsi => "us-ansi",
            Self::UkIso => "uk-iso",
            Self::DeIso => "de-iso",
            Self::DvorakAnsi => "dvorak-ansi",
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "us-ansi" => Some(Self::UsAnsi),
            "uk-iso" => Some(Self::UkIso),
            "de-iso" => Some(Self::DeIso),
            "dvorak-ansi" => Some(Self::DvorakAnsi),
            _ => None,
        }
    }
}

/// Where the on-screen layout comes from — surfaced honestly in the
/// selector (`Auto` vs `Manual` vs a warning).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutSource {
    /// Detected from the active OS input source.
    Auto,
    /// Explicit user choice, or OS switch is locked out.
    Manual,
    /// Detection ran but the input source matched nothing bundled; legends
    /// fall back to US and the UI warns.
    Unrecognized,
}

/// One physical position.
#[derive(Clone, Copy, Debug)]
pub struct KeyCap {
    /// Physical position, e.g. `"KeyQ"`, `"Digit1"`, `"IntlBackslash"`.
    pub code: &'static str,
    /// Logical character produced unshifted in this layout.
    pub unshifted: &'static str,
    /// Logical character produced shifted, when known.
    pub shifted: Option<&'static str>,
    /// Width in key units (1.0 = one alpha key).
    pub width: f32,
}

/// A keyboard layout: display metadata plus rows of key caps.
pub struct KeyboardLayout {
    pub id: LayoutId,
    pub name: &'static str,
    /// Rows top to bottom; each row left to right.
    pub rows: &'static [&'static [KeyCap]],
}

impl KeyboardLayout {
    /// Physical position producing logical `key` unshifted, preferring the
    /// unshifted legend, then shifted. `None` → the binding can't be drawn
    /// on this layout and the manager reports "layout mapping uncertain".
    pub fn physical_for_logical(&self, key: &str) -> Option<&'static str> {
        let key = key.to_lowercase();
        let mut shifted_hit = None;
        for row in self.rows {
            for cap in *row {
                if cap.unshifted.eq_ignore_ascii_case(&key) {
                    return Some(cap.code);
                }
                if cap
                    .shifted
                    .is_some_and(|shifted| shifted.eq_ignore_ascii_case(&key))
                {
                    shifted_hit = Some(cap.code);
                }
            }
        }
        // Named keys whose legend is the name itself (enter, tab, …) fall
        // through here when no cap legend matched.
        shifted_hit
    }
}

const fn cap(
    code: &'static str,
    unshifted: &'static str,
    shifted: &'static str,
    width: f32,
) -> KeyCap {
    KeyCap {
        code,
        unshifted,
        shifted: if shifted.is_empty() {
            None
        } else {
            Some(shifted)
        },
        width,
    }
}

const fn key(code: &'static str, legend: &'static str, width: f32) -> KeyCap {
    cap(code, legend, "", width)
}

// --- Shared geometry ---------------------------------------------------------
// Rows are ordered [function] [number] [qwer] [asdf] [zxcv] [space]. Modifier
// and named keys use their logical name as `unshifted` so a binding like
// `secondary-j` highlights `KeyJ` plus the `⌘` cap.

const ANSI_NUMBER_ROW: &[KeyCap] = &[
    key("Backquote", "`", 1.0),
    cap("Digit1", "1", "!", 1.0),
    cap("Digit2", "2", "@", 1.0),
    cap("Digit3", "3", "#", 1.0),
    cap("Digit4", "4", "$", 1.0),
    cap("Digit5", "5", "%", 1.0),
    cap("Digit6", "6", "^", 1.0),
    cap("Digit7", "7", "&", 1.0),
    cap("Digit8", "8", "*", 1.0),
    cap("Digit9", "9", "(", 1.0),
    cap("Digit0", "0", ")", 1.0),
    cap("Minus", "-", "_", 1.0),
    cap("Equal", "=", "+", 1.0),
    key("Backspace", "backspace", 2.0),
];

const UK_NUMBER_ROW: &[KeyCap] = &[
    cap("Backquote", "`", "¬", 1.0),
    cap("Digit1", "1", "!", 1.0),
    cap("Digit2", "2", "\"", 1.0),
    cap("Digit3", "3", "£", 1.0),
    cap("Digit4", "4", "$", 1.0),
    cap("Digit5", "5", "%", 1.0),
    cap("Digit6", "6", "^", 1.0),
    cap("Digit7", "7", "&", 1.0),
    cap("Digit8", "8", "*", 1.0),
    cap("Digit9", "9", "(", 1.0),
    cap("Digit0", "0", ")", 1.0),
    cap("Minus", "-", "_", 1.0),
    cap("Equal", "=", "+", 1.0),
    key("Backspace", "backspace", 2.0),
];

const DE_NUMBER_ROW: &[KeyCap] = &[
    cap("Backquote", "^", "°", 1.0),
    cap("Digit1", "1", "!", 1.0),
    cap("Digit2", "2", "\"", 1.0),
    cap("Digit3", "3", "§", 1.0),
    cap("Digit4", "4", "$", 1.0),
    cap("Digit5", "5", "%", 1.0),
    cap("Digit6", "6", "&", 1.0),
    cap("Digit7", "7", "/", 1.0),
    cap("Digit8", "8", "(", 1.0),
    cap("Digit9", "9", ")", 1.0),
    cap("Digit0", "0", "=", 1.0),
    cap("Minus", "ß", "?", 1.0),
    cap("Equal", "´", "`", 1.0),
    key("Backspace", "backspace", 2.0),
];

const ANSI_QWER: &[KeyCap] = &[
    key("Tab", "tab", 1.5),
    cap("KeyQ", "q", "Q", 1.0),
    cap("KeyW", "w", "W", 1.0),
    cap("KeyE", "e", "E", 1.0),
    cap("KeyR", "r", "R", 1.0),
    cap("KeyT", "t", "T", 1.0),
    cap("KeyY", "y", "Y", 1.0),
    cap("KeyU", "u", "U", 1.0),
    cap("KeyI", "i", "I", 1.0),
    cap("KeyO", "o", "O", 1.0),
    cap("KeyP", "p", "P", 1.0),
    cap("BracketLeft", "[", "{", 1.0),
    cap("BracketRight", "]", "}", 1.0),
    cap("Backslash", "\\", "|", 1.5),
];

const UK_QWER: &[KeyCap] = &[
    key("Tab", "tab", 1.5),
    cap("KeyQ", "q", "Q", 1.0),
    cap("KeyW", "w", "W", 1.0),
    cap("KeyE", "e", "E", 1.0),
    cap("KeyR", "r", "R", 1.0),
    cap("KeyT", "t", "T", 1.0),
    cap("KeyY", "y", "Y", 1.0),
    cap("KeyU", "u", "U", 1.0),
    cap("KeyI", "i", "I", 1.0),
    cap("KeyO", "o", "O", 1.0),
    cap("KeyP", "p", "P", 1.0),
    cap("BracketLeft", "[", "{", 1.0),
    cap("BracketRight", "]", "}", 1.0),
    // ISO Enter is tall; rendered as the 2-row cap on the home row below.
    key("Enter", "enter", 1.25),
];

const DE_QWER: &[KeyCap] = &[
    key("Tab", "tab", 1.5),
    cap("KeyQ", "q", "Q", 1.0),
    cap("KeyW", "w", "W", 1.0),
    cap("KeyE", "e", "E", 1.0),
    cap("KeyR", "r", "R", 1.0),
    cap("KeyT", "t", "T", 1.0),
    // QWERTZ: the Y position produces z.
    cap("KeyY", "z", "Z", 1.0),
    cap("KeyU", "u", "U", 1.0),
    cap("KeyI", "i", "I", 1.0),
    cap("KeyO", "o", "O", 1.0),
    cap("KeyP", "p", "P", 1.0),
    cap("BracketLeft", "ü", "Ü", 1.0),
    cap("BracketRight", "+", "*", 1.0),
    key("Enter", "enter", 1.25),
];

const DVORAK_QWER: &[KeyCap] = &[
    key("Tab", "tab", 1.5),
    cap("KeyQ", "'", "\"", 1.0),
    cap("KeyW", ",", "<", 1.0),
    cap("KeyE", ".", ">", 1.0),
    cap("KeyR", "p", "P", 1.0),
    cap("KeyT", "y", "Y", 1.0),
    cap("KeyY", "f", "F", 1.0),
    cap("KeyU", "g", "G", 1.0),
    cap("KeyI", "c", "C", 1.0),
    cap("KeyO", "r", "R", 1.0),
    cap("KeyP", "l", "L", 1.0),
    cap("BracketLeft", "/", "?", 1.0),
    cap("BracketRight", "=", "+", 1.0),
    cap("Backslash", "\\", "|", 1.5),
];

const ANSI_ASDF: &[KeyCap] = &[
    key("CapsLock", "capslock", 1.75),
    cap("KeyA", "a", "A", 1.0),
    cap("KeyS", "s", "S", 1.0),
    cap("KeyD", "d", "D", 1.0),
    cap("KeyF", "f", "F", 1.0),
    cap("KeyG", "g", "G", 1.0),
    cap("KeyH", "h", "H", 1.0),
    cap("KeyJ", "j", "J", 1.0),
    cap("KeyK", "k", "K", 1.0),
    cap("KeyL", "l", "L", 1.0),
    cap("Semicolon", ";", ":", 1.0),
    cap("Quote", "'", "\"", 1.0),
    key("Enter", "enter", 2.25),
];

const UK_ASDF: &[KeyCap] = &[
    key("CapsLock", "capslock", 1.75),
    cap("KeyA", "a", "A", 1.0),
    cap("KeyS", "s", "S", 1.0),
    cap("KeyD", "d", "D", 1.0),
    cap("KeyF", "f", "F", 1.0),
    cap("KeyG", "g", "G", 1.0),
    cap("KeyH", "h", "H", 1.0),
    cap("KeyJ", "j", "J", 1.0),
    cap("KeyK", "k", "K", 1.0),
    cap("KeyL", "l", "L", 1.0),
    cap("Semicolon", ";", ":", 1.0),
    cap("Quote", "'", "@", 1.0),
    cap("Backslash", "#", "~", 1.0),
];

const DE_ASDF: &[KeyCap] = &[
    key("CapsLock", "capslock", 1.75),
    cap("KeyA", "a", "A", 1.0),
    cap("KeyS", "s", "S", 1.0),
    cap("KeyD", "d", "D", 1.0),
    cap("KeyF", "f", "F", 1.0),
    cap("KeyG", "g", "G", 1.0),
    cap("KeyH", "h", "H", 1.0),
    cap("KeyJ", "j", "J", 1.0),
    cap("KeyK", "k", "K", 1.0),
    cap("KeyL", "l", "L", 1.0),
    cap("Semicolon", "ö", "Ö", 1.0),
    cap("Quote", "ä", "Ä", 1.0),
    cap("Backslash", "#", "'", 1.0),
];

const DVORAK_ASDF: &[KeyCap] = &[
    key("CapsLock", "capslock", 1.75),
    cap("KeyA", "a", "A", 1.0),
    cap("KeyS", "o", "O", 1.0),
    cap("KeyD", "e", "E", 1.0),
    cap("KeyF", "u", "U", 1.0),
    cap("KeyG", "i", "I", 1.0),
    cap("KeyH", "d", "D", 1.0),
    cap("KeyJ", "h", "H", 1.0),
    cap("KeyK", "t", "T", 1.0),
    cap("KeyL", "n", "N", 1.0),
    cap("Semicolon", "s", "S", 1.0),
    cap("Quote", "-", "_", 1.0),
    key("Enter", "enter", 2.25),
];

const ANSI_ZXCV: &[KeyCap] = &[
    key("ShiftLeft", "shift", 2.25),
    cap("KeyZ", "z", "Z", 1.0),
    cap("KeyX", "x", "X", 1.0),
    cap("KeyC", "c", "C", 1.0),
    cap("KeyV", "v", "V", 1.0),
    cap("KeyB", "b", "B", 1.0),
    cap("KeyN", "n", "N", 1.0),
    cap("KeyM", "m", "M", 1.0),
    cap("Comma", ",", "<", 1.0),
    cap("Period", ".", ">", 1.0),
    cap("Slash", "/", "?", 1.0),
    key("ShiftRight", "shift", 2.75),
];

// ISO bottom row carries the extra `IntlBackslash` position between left
// shift and Z.
const UK_ZXCV: &[KeyCap] = &[
    key("ShiftLeft", "shift", 1.25),
    cap("IntlBackslash", "\\", "|", 1.0),
    cap("KeyZ", "z", "Z", 1.0),
    cap("KeyX", "x", "X", 1.0),
    cap("KeyC", "c", "C", 1.0),
    cap("KeyV", "v", "V", 1.0),
    cap("KeyB", "b", "B", 1.0),
    cap("KeyN", "n", "N", 1.0),
    cap("KeyM", "m", "M", 1.0),
    cap("Comma", ",", ";", 1.0),
    cap("Period", ".", ":", 1.0),
    cap("Slash", "/", "?", 1.0),
    key("ShiftRight", "shift", 2.75),
];

const DE_ZXCV: &[KeyCap] = &[
    key("ShiftLeft", "shift", 1.25),
    cap("IntlBackslash", "<", ">", 1.0),
    // QWERTZ: the Z position produces y.
    cap("KeyZ", "y", "Y", 1.0),
    cap("KeyX", "x", "X", 1.0),
    cap("KeyC", "c", "C", 1.0),
    cap("KeyV", "v", "V", 1.0),
    cap("KeyB", "b", "B", 1.0),
    cap("KeyN", "n", "N", 1.0),
    cap("KeyM", "m", "M", 1.0),
    cap("Comma", ",", ";", 1.0),
    cap("Period", ".", ":", 1.0),
    cap("Slash", "-", "_", 1.0),
    key("ShiftRight", "shift", 2.75),
];

const DVORAK_ZXCV: &[KeyCap] = &[
    key("ShiftLeft", "shift", 2.25),
    cap("KeyZ", ";", ":", 1.0),
    cap("KeyX", "q", "Q", 1.0),
    cap("KeyC", "j", "J", 1.0),
    cap("KeyV", "k", "K", 1.0),
    cap("KeyB", "x", "X", 1.0),
    cap("KeyN", "b", "B", 1.0),
    cap("KeyM", "m", "M", 1.0),
    cap("Comma", "w", "W", 1.0),
    cap("Period", "v", "V", 1.0),
    cap("Slash", "z", "Z", 1.0),
    key("ShiftRight", "shift", 2.75),
];

const BOTTOM_ROW: &[KeyCap] = &[
    key("ControlLeft", "ctrl", 1.25),
    key("AltLeft", "alt", 1.25),
    key("MetaLeft", "cmd", 1.25),
    key("Space", "space", 6.25),
    key("MetaRight", "cmd", 1.25),
    key("AltRight", "alt", 1.25),
    key("ControlRight", "ctrl", 1.25),
];

static US_ANSI_ROWS: &[&[KeyCap]] = &[
    ANSI_NUMBER_ROW,
    ANSI_QWER,
    ANSI_ASDF,
    ANSI_ZXCV,
    BOTTOM_ROW,
];
static UK_ISO_ROWS: &[&[KeyCap]] = &[UK_NUMBER_ROW, UK_QWER, UK_ASDF, UK_ZXCV, BOTTOM_ROW];
static DE_ISO_ROWS: &[&[KeyCap]] = &[DE_NUMBER_ROW, DE_QWER, DE_ASDF, DE_ZXCV, BOTTOM_ROW];
static DVORAK_ROWS: &[&[KeyCap]] = &[
    ANSI_NUMBER_ROW,
    DVORAK_QWER,
    DVORAK_ASDF,
    DVORAK_ZXCV,
    BOTTOM_ROW,
];

static BUNDLED: &[KeyboardLayout] = &[
        KeyboardLayout {
            id: LayoutId::UsAnsi,
            name: "US ANSI",
            rows: US_ANSI_ROWS,
        },
        KeyboardLayout {
            id: LayoutId::UkIso,
            name: "UK ISO",
            rows: UK_ISO_ROWS,
        },
        KeyboardLayout {
            id: LayoutId::DeIso,
            name: "German ISO",
            rows: DE_ISO_ROWS,
        },
        KeyboardLayout {
            id: LayoutId::DvorakAnsi,
            name: "Dvorak ANSI",
            rows: DVORAK_ROWS,
        },
];

/// The layouts bundled in v1.
pub fn bundled_layouts() -> &'static [KeyboardLayout] {
    BUNDLED
}

pub fn layout_by_id(id: LayoutId) -> &'static KeyboardLayout {
    bundled_layouts()
        .iter()
        .find(|layout| layout.id == id)
        .unwrap_or(&bundled_layouts()[0])
}

/// Map a macOS input-source id (`TISCopyCurrentKeyboardLayoutInputSource` →
/// `kTISPropertyInputSourceID`, e.g. `com.apple.keylayout.US`) to a bundled
/// layout. Unknown ids return `Unrecognized` with the US fallback — the
/// caller surfaces the warning, detection never guesses silently.
pub fn detect_layout(source_id: &str) -> (LayoutId, LayoutSource) {
    let matched = match source_id {
        id if id.contains("US") && !id.contains("International") => Some(LayoutId::UsAnsi),
        id if id.contains("British") => Some(LayoutId::UkIso),
        id if id.contains("German") => Some(LayoutId::DeIso),
        id if id.contains("Dvorak") => Some(LayoutId::DvorakAnsi),
        _ => None,
    };
    match matched {
        Some(id) => (id, LayoutSource::Auto),
        None => (LayoutId::UsAnsi, LayoutSource::Unrecognized),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_to_physical_is_layout_aware() {
        let us = layout_by_id(LayoutId::UsAnsi);
        let de = layout_by_id(LayoutId::DeIso);
        let dvorak = layout_by_id(LayoutId::DvorakAnsi);
        assert_eq!(us.physical_for_logical("z"), Some("KeyZ"));
        // QWERTZ: the Z glyph lives on the Y position.
        assert_eq!(de.physical_for_logical("z"), Some("KeyY"));
        // Dvorak: the Q position produces '.
        assert_eq!(dvorak.physical_for_logical("'"), Some("KeyQ"));
        assert_eq!(us.physical_for_logical("j"), Some("KeyJ"));
        assert_eq!(us.physical_for_logical("="), Some("Equal"));
    }

    #[test]
    fn ids_round_trip() {
        for layout in bundled_layouts() {
            assert_eq!(LayoutId::parse(layout.id.as_str()), Some(layout.id));
        }
    }
}
