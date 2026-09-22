//! The sandbox sign-in flow: a sandboxed session whose provider holds no
//! credentials in its shared guest home gets a right-panel terminal tab
//! running the provider's interactive login inside a throwaway VM. When
//! that tab exits, the daemon checks the provider home for credentials and
//! any parked submission resubmits.

use std::collections::HashMap;

use super::*;

impl Waku {
    /// Open the provider's in-sandbox sign-in as a right-panel terminal tab
    /// on the gated session. The daemon answers with the `shuru run` argv;
    /// the tab runs it on this host under a real PTY, which is what gives
    /// the guest its tty. One tab per provider — a second gated session
    /// reuses the open sign-in.
    pub(super) fn open_sandbox_sign_in(&mut self, session_id: Uuid, cx: &mut Context<Self>) {
        let Some(provider) = self
            .state
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .map(|session| session.provider)
        else {
            return;
        };
        if self
            .sandbox_sign_in_tabs
            .values()
            .any(|kind| *kind == provider)
        {
            return;
        }
        // The sign-in tab runs a host argv in a desktop PTY — there is no
        // terminal on a remote daemon, so a remote session only gets the
        // NeedsAuth label.
        if self.is_remote_session(session_id) {
            return;
        }
        let Some(client) = self
            .daemon_for_session(session_id)
            .map(|daemon| daemon.client())
        else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let response = cx
                .background_executor()
                .spawn(async move {
                    client.request(
                        session_id,
                        Uuid::nil(),
                        waku_client::Command::SandboxSignIn { provider },
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match response {
                    Ok(waku_client::ResponsePayload::SandboxSignIn { program, args, cwd }) => {
                        if let Some(terminal_id) =
                            this.create_program_terminal(cwd, Some(session_id), program, args, cx)
                        {
                            this.sandbox_sign_in_tabs.insert(terminal_id, provider);
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        this.show_toast(tr!("sandbox.sign_in_failed", error = error.to_string()))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A sandbox sign-in tab's process exited — drop the tracking entry,
    /// then probe the provider's shared home for credentials. Signed in:
    /// every parked submission on that provider resubmits. Not signed in:
    /// the submission returns to the composer so nothing is silently lost.
    pub(super) fn sandbox_sign_in_terminal_exited(
        &mut self,
        terminal_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(provider) = self.sandbox_sign_in_tabs.remove(&terminal_id) else {
            return;
        };
        let pending: Vec<(Uuid, ComposerSubmission)> = self
            .sandbox_pending_submissions
            .iter()
            .filter(|(session_id, _)| {
                self.state
                    .sessions
                    .iter()
                    .any(|session| session.id == **session_id && session.provider == provider)
            })
            .map(|(session_id, submission)| (*session_id, submission.clone()))
            .collect();
        if pending.is_empty() {
            return;
        }
        // Provider homes are per-daemon: group the parked sessions by the
        // daemon that owns each so a remote session probes its own host.
        let mut by_daemon: HashMap<waku_client::DaemonKey, Vec<(Uuid, ComposerSubmission)>> =
            HashMap::new();
        for (session_id, submission) in pending {
            by_daemon
                .entry(self.daemons.session_owner(session_id))
                .or_default()
                .push((session_id, submission));
        }
        for (daemon_key, submissions) in by_daemon {
            let Some(client) = self
                .daemons
                .supervisor(daemon_key)
                .map(|daemon| daemon.client())
            else {
                continue;
            };
            cx.spawn(async move |this, cx| {
                let response = cx
                    .background_executor()
                    .spawn(async move {
                        client.request(
                            Uuid::nil(),
                            Uuid::nil(),
                            waku_client::Command::SandboxAuthStatus { provider },
                        )
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let signed_in = matches!(
                        response,
                        Ok(waku_client::ResponsePayload::SandboxAuthStatus { signed_in: true })
                    );
                    for (session_id, submission) in submissions {
                        this.sandbox_pending_submissions.remove(&session_id);
                        if signed_in {
                            this.submit_submission_for_session(session_id, submission, cx);
                        } else {
                            this.restore_composer_submission(submission, cx);
                            this.show_toast(tr!(
                                "sandbox.sign_in_incomplete",
                                provider = provider.display_name()
                            ));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
    }
}
