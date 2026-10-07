use super::*;

/// A `goddard://` URL the OS asked the app to open. The scheme is declared
/// in the bundle's Info.plist; today only macOS delivers these events — the
/// other GPUI backends store the open-URLs callback without ever invoking
/// it.
#[derive(Debug, PartialEq)]
pub(super) enum DeepLink {
    /// `goddard://task/<id>` — select the task, the same route a transcript
    /// task link takes inside the app.
    Task,
    /// `goddard://new-task?prompt=<text>` — open the new-task page with the
    /// prompt already in the composer. The text never submits itself: the
    /// link only stages a draft, sending stays the user's gesture.
    NewTask { prompt: Option<String> },
}

/// Classify one URL handed over by the OS. Anything outside the
/// `goddard://` scheme and its known hosts is ignored — LaunchServices can
/// forward links the app never asked for.
pub(super) fn parse_deep_link(url: &str) -> Option<DeepLink> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "goddard" {
        return None;
    }
    match parsed.host_str() {
        // The id rides the path; `open_transcript_link` re-validates it
        // against the session list and toasts on a miss.
        Some("task") => Some(DeepLink::Task),
        Some("new-task") => Some(DeepLink::NewTask {
            prompt: parsed
                .query_pairs()
                .find(|(key, _)| key == "prompt")
                .map(|(_, value)| value.into_owned())
                .filter(|prompt| !prompt.trim().is_empty()),
        }),
        // `goddard://connect` pairs a phone from the QR; the desktop has no
        // page for it.
        _ => None,
    }
}

/// The link's prompt joined into a draft the user may already have been
/// writing: it lands on its own paragraph after the existing text, never
/// replacing it.
fn merged_prompt(existing: &str, prompt: &str) -> String {
    if existing.trim().is_empty() {
        prompt.to_owned()
    } else {
        format!("{}\n\n{prompt}", existing.trim_end())
    }
}

impl Waku {
    /// Route one `goddard://` URL from the OS. Reports whether the link was
    /// one of ours — the caller raises the window only then.
    pub(crate) fn handle_deep_link(
        &mut self,
        url: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match parse_deep_link(url) {
            Some(DeepLink::Task) => self.open_transcript_link(url, cx),
            Some(DeepLink::NewTask { prompt }) => {
                self.pending_deep_link_prompt = prompt;
                self.new_session_action(&NewSession, window, cx);
                // Synchronous landings already consumed the prompt in
                // `finish_session_activation`; Big Picture's untargeted
                // composer is the one branch that activates nothing.
                self.apply_deep_link_prompt(cx);
                true
            }
            None => false,
        }
    }

    /// Move the staged deep-link prompt into the composer when it sits on a
    /// blank-task draft. `finish_session_activation` runs this after every
    /// landing, so the prompt also reaches a "No project" destination whose
    /// workspace is still provisioning; activating a started task leaves it
    /// staged for the next new-task page.
    pub(super) fn apply_deep_link_prompt(&mut self, cx: &mut Context<Self>) {
        if self.pending_deep_link_prompt.is_none() {
            return;
        }
        let Some(key @ crate::persistence::ComposerDraftKey::NewSession(_)) =
            self.composer_draft_key()
        else {
            return;
        };
        let Some(prompt) = self.pending_deep_link_prompt.take() else {
            return;
        };
        let mut draft = self.composer_drafts.get(key).cloned().unwrap_or_default();
        draft.text = merged_prompt(&draft.text, &prompt);
        self.composer_drafts.set(key, draft);
        self.restore_selected_composer_draft(cx);
        self.schedule_composer_draft_save(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_routes_only_the_goddard_scheme() {
        assert_eq!(
            parse_deep_link("https://goddard.ai/new-task?prompt=x"),
            None
        );
        assert_eq!(parse_deep_link("waku://new-task?prompt=x"), None);
        assert_eq!(parse_deep_link("not a url"), None);
        assert_eq!(parse_deep_link("goddard://connect?address=ws://x"), None);
    }

    #[test]
    fn parse_new_task_decodes_the_prompt() {
        assert_eq!(
            parse_deep_link("goddard://new-task?prompt=hello%20world%21"),
            Some(DeepLink::NewTask {
                prompt: Some("hello world!".to_owned())
            })
        );
        // A bare link still opens the page; a blank prompt is no prompt.
        assert_eq!(
            parse_deep_link("goddard://new-task"),
            Some(DeepLink::NewTask { prompt: None })
        );
        assert_eq!(
            parse_deep_link("goddard://new-task?prompt=%20%20"),
            Some(DeepLink::NewTask { prompt: None })
        );
    }

    #[test]
    fn parse_task_leaves_the_id_to_the_link_route() {
        assert_eq!(
            parse_deep_link("goddard://task/00000000-0000-0000-0000-000000000000"),
            Some(DeepLink::Task)
        );
    }

    #[test]
    fn merged_prompt_appends_instead_of_replacing() {
        assert_eq!(merged_prompt("", "do it"), "do it");
        assert_eq!(merged_prompt("   ", "do it"), "do it");
        assert_eq!(merged_prompt("wip note  ", "do it"), "wip note\n\ndo it");
    }
}
