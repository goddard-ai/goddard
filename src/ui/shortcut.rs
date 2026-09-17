//! Keyboard-shortcut hints shared by menus, tooltips, and the command
//! palette: one label format, resolved from the live keymap.

use std::rc::Rc;

use gpui::{
    Action, App, FocusHandle, KeyBinding, KeyContext, KeybindingKeystroke, SharedString, Window,
};

/// A shortcut hint shown beside a menu row, tooltip, or palette entry.
#[derive(Clone)]
pub enum ShortcutHint {
    /// An authored string, for chords the keymap cannot see — the terminal's
    /// hand-rolled `on_key_down` handling, or a compound hint like "↑↓".
    Text(SharedString),
    /// Resolved from the registered bindings at render time, so the hint can
    /// never drift from the binding it advertises.
    Action {
        action: Rc<dyn Action>,
        /// Resolve as if this handle were focused. Required when the surface
        /// showing the hint owns focus while the shortcut belongs to another
        /// element's context — a text field's Cut while its menu card is
        /// focused.
        focus: Option<FocusHandle>,
    },
}

impl ShortcutHint {
    pub fn text(label: impl Into<SharedString>) -> Self {
        Self::Text(label.into())
    }

    pub fn action(action: &dyn Action) -> Self {
        Self::Action {
            action: action.boxed_clone().into(),
            focus: None,
        }
    }

    pub fn action_in(action: &dyn Action, focus: &FocusHandle) -> Self {
        Self::Action {
            action: action.boxed_clone().into(),
            focus: Some(focus.clone()),
        }
    }

    /// The display string, or `None` when the action has no binding in the
    /// relevant context.
    pub fn resolve(&self, window: &Window, cx: &App) -> Option<String> {
        match self {
            Self::Text(label) => Some(label.to_string()),
            Self::Action { action, focus } => {
                let binding = match focus {
                    Some(focus) => {
                        window.highest_precedence_binding_for_action_in(action.as_ref(), focus)
                    }
                    // The focused node's real dispatch path, like
                    // `Window::context_stack` — but that accessor asserts the
                    // rendered frame's dispatch tree is non-empty, which does
                    // not hold during a window's first render. The `_in`
                    // lookup returns `None` there instead; fall back to an
                    // empty stack so context-free bindings still resolve.
                    None => window
                        .focused(cx)
                        .and_then(|focus| {
                            window.highest_precedence_binding_for_action_in(
                                action.as_ref(),
                                &focus,
                            )
                        })
                        .or_else(|| highest_precedence_binding(action.as_ref(), &[], cx)),
                }?;
                Some(binding_label(&binding))
            }
        }
    }
}

/// `Window::highest_precedence_binding_for_action` resolves against the
/// dispatch tree's leftover build stack — the context path of whatever painted
/// last — not the focused element's path. A tooltip paints last, grafted at
/// the tree's root with no key contexts, so one frame after it appears the
/// stack is empty and context-scoped bindings drop out. This is the same
/// lookup, run against `Window::context_stack` — the focused node's real
/// dispatch path.
fn highest_precedence_binding(
    action: &dyn Action,
    context_stack: &[KeyContext],
    cx: &App,
) -> Option<KeyBinding> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap
        .bindings_for_action(action)
        .rev()
        .find(|binding| {
            keymap
                .bindings_for_input(binding.keystrokes(), context_stack)
                .0
                .first()
                .is_some_and(|found| found.action().partial_eq(binding.action()))
        })
        .cloned()
}

impl std::fmt::Debug for ShortcutHint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(label) => f.debug_tuple("Text").field(label).finish(),
            Self::Action { action, .. } => f
                .debug_struct("Action")
                .field("action", &action.name())
                .finish(),
        }
    }
}

/// "⌘⇧T" on macOS, "Ctrl+Shift+T" elsewhere — the same format the app's
/// authored shortcut strings use.
pub fn binding_label(binding: &KeyBinding) -> String {
    binding
        .keystrokes()
        .iter()
        .map(keystroke_label)
        .collect::<Vec<_>>()
        .join(" ")
}

fn keystroke_label(keystroke: &KeybindingKeystroke) -> String {
    let modifiers = keystroke.modifiers();
    let mut label = String::new();
    #[cfg(target_os = "macos")]
    {
        // The authored order, matching the existing strings ("⌥⌘B", "⌘⇧T").
        if modifiers.control {
            label.push('⌃');
        }
        if modifiers.alt {
            label.push('⌥');
        }
        if modifiers.platform {
            label.push('⌘');
        }
        if modifiers.shift {
            label.push('⇧');
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        for (pressed, name) in [
            (modifiers.control, "Ctrl"),
            (modifiers.alt, "Alt"),
            (
                modifiers.platform,
                if cfg!(target_os = "windows") {
                    "Win"
                } else {
                    "Super"
                },
            ),
            (modifiers.shift, "Shift"),
        ] {
            if pressed {
                label.push_str(name);
                label.push('+');
            }
        }
    }
    label.push_str(&key_label(keystroke.key()));
    label
}

fn key_label(key: &str) -> String {
    #[cfg(target_os = "macos")]
    if let Some(glyph) = match key {
        "backspace" => Some("⌫"),
        "delete" => Some("⌦"),
        "up" => Some("↑"),
        "down" => Some("↓"),
        "left" => Some("←"),
        "right" => Some("→"),
        "tab" => Some("⇥"),
        "escape" => Some("⎋"),
        "enter" | "return" => Some("↵"),
        _ => None,
    } {
        return glyph.to_owned();
    }
    let mut chars = key.chars();
    match chars.next() {
        // "n" -> "N", "pagedown" -> "Pagedown".
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Context, InteractiveElement, IntoElement, Keystroke, Modifiers, ParentElement, Render,
        Styled, div, px,
    };

    fn keystroke(modifiers: Modifiers, key: &str) -> KeybindingKeystroke {
        KeybindingKeystroke::from_keystroke(Keystroke {
            modifiers,
            key: key.to_owned(),
            key_char: None,
        })
    }

    #[test]
    fn labels_match_the_authored_shortcut_strings() {
        #[cfg(target_os = "macos")]
        {
            let secondary = Modifiers {
                platform: true,
                ..Default::default()
            };
            assert_eq!(keystroke_label(&keystroke(secondary, "n")), "⌘N");
            assert_eq!(
                keystroke_label(&keystroke(
                    Modifiers {
                        shift: true,
                        ..secondary
                    },
                    "t"
                )),
                "⌘⇧T"
            );
            assert_eq!(
                keystroke_label(&keystroke(
                    Modifiers {
                        alt: true,
                        ..secondary
                    },
                    "b"
                )),
                "⌥⌘B"
            );
            assert_eq!(
                keystroke_label(&keystroke(
                    Modifiers {
                        control: true,
                        ..Default::default()
                    },
                    "`"
                )),
                "⌃`"
            );
            assert_eq!(keystroke_label(&keystroke(secondary, "enter")), "⌘↵");
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert_eq!(
                keystroke_label(&keystroke(
                    Modifiers {
                        control: true,
                        shift: true,
                        ..Default::default()
                    },
                    "t"
                )),
                "Ctrl+Shift+T"
            );
        }
    }

    struct Harness {
        field_focus: FocusHandle,
        other_focus: FocusHandle,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .child(
                    div()
                        .key_context("TextInput")
                        .track_focus(&self.field_focus)
                        .size(px(10.0)),
                )
                .child(div().track_focus(&self.other_focus).size(px(10.0)))
        }
    }

    #[gpui::test]
    fn resolve_reads_the_live_keymap(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("secondary-n", crate::NewSession, None),
                KeyBinding::new("secondary-c", crate::input::Copy, Some("TextInput")),
            ]);
        });
        let (view, cx) = cx.add_window_view(|_, cx| Harness {
            field_focus: cx.focus_handle(),
            other_focus: cx.focus_handle(),
        });
        let (field_focus, other_focus) = view.read_with(cx, |harness, _| {
            (harness.field_focus.clone(), harness.other_focus.clone())
        });

        cx.update(|window, cx| window.focus(&other_focus, cx));
        cx.update(|window, cx| {
            assert_eq!(
                ShortcutHint::action(&crate::NewSession)
                    .resolve(window, cx)
                    .as_deref(),
                Some(crate::platform::primary_shortcut("⌘N", "Ctrl+N"))
            );
            // Copy's binding lives on the unfocused TextInput context, so the
            // focused stack cannot see it.
            assert_eq!(
                ShortcutHint::action(&crate::input::Copy).resolve(window, cx),
                None
            );
            assert_eq!(
                ShortcutHint::action_in(&crate::input::Copy, &field_focus)
                    .resolve(window, cx)
                    .as_deref(),
                Some(crate::platform::primary_shortcut("⌘C", "Ctrl+C"))
            );
            assert_eq!(
                ShortcutHint::text("⌘V").resolve(window, cx).as_deref(),
                Some("⌘V")
            );
        });
    }
}
