//! Deterministic provider contract probes. These prove dispatch progression,
//! not that a real provider disables its native tools during execution.
use super::*;

const BRIEF: &str = "TASK_MUTATION_SENTINEL";

/// Recorded provider inputs carrying the brief — a physical submission, which
/// stays observable separately from the daemon's delivery authorization.
fn mutations(fixture: &Fixture) -> usize {
    fixture
        .capture
        .0
        .lock()
        .iter()
        .filter(|text| text.contains(BRIEF))
        .count()
}

#[test]
fn plan_acceptance_execution_input_carries_exact_bytes_identity_and_brief() {
    let fixture = Fixture::new("# Approved\n\nsection α\n");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    let capture = fixture.capture.0.lock();
    assert_eq!(capture.len(), 1);
    let input = &capture[0];
    // The exact approved bytes — not a summary or locator — are in the input.
    assert!(input.contains("# Approved\n\nsection α\n"));
    let review = fixture.gate.review.lock();
    for field in [
        review.receipt.assignment.to_string(),
        review.receipt.attempt.to_string(),
        review.receipt.runtime_generation.to_string(),
        review.receipt.revision.review.owner_id.to_string(),
        review.receipt.revision.review.plan_id.to_string(),
        review.receipt.revision.id.to_string(),
        review.receipt.revision.review.content_digest.clone(),
        review.receipt.artifact.display().to_string(),
        review.receipt.execution_workspace.display().to_string(),
    ] {
        assert!(input.contains(&field), "input missing {field}");
    }
    assert!(input.contains(BRIEF));
    drop(review);
    drop(capture);
    // Briefs queued while acceptance is in flight withhold — the input is
    // never duplicated to chase a pending acknowledgment.
    fixture.gate.submit("FOLLOWUP_SENTINEL".into(), Vec::new());
    assert_eq!(fixture.capture.0.lock().len(), 1);
    fixture.accept();
    assert!(fixture.gate.delivered());
    let capture = fixture.capture.0.lock();
    assert_eq!(capture.len(), 2);
    assert!(capture[1].contains("FOLLOWUP_SENTINEL"));
    assert!(!capture[1].contains(BRIEF));
}

#[test]
fn plan_acceptance_revocation_between_submission_and_acceptance_fences_release() {
    // Authority revoked after the input physically submitted but before the
    // adapter confirmed acceptance: delivery is held, never marked delivered,
    // and nothing redispatches against the stale attempt.
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    fixture.authorized.store(false, Ordering::Release);
    fixture.accept();
    assert!(!fixture.gate.delivered());
    assert_eq!(mutations(&fixture), 1);
    let stored = fixture.stored();
    assert_eq!(stored.receipt.phase, Phase::Blocked);
    assert!(!stored.receipt.delivered);
    fixture.gate.submit("RETRY_SENTINEL".into(), Vec::new());
    assert_eq!(mutations(&fixture), 1);
}

#[test]
fn plan_acceptance_dispatch_and_projection_failures_preserve_work() {
    for fault in ["reject-before-send", "accept-then-uncertain", "projection"] {
        let mut fixture = Fixture::new("approved bytes");
        if fault == "projection" {
            Arc::get_mut(&mut fixture.gate).unwrap().publish = Box::new(|receipt, _| {
                anyhow::ensure!(
                    receipt.phase != Phase::Delivered,
                    "supervisor transport unavailable"
                );
                Ok(())
            });
        } else {
            let accepted_first = fault == "accept-then-uncertain";
            let sent = AtomicBool::new(false);
            Arc::get_mut(&mut fixture.gate).unwrap().dispatch =
                Box::new(move |driver, prompt, files| {
                    // Fail the first complete-input submission only; the
                    // uncertain leg hands the bytes to the provider first so
                    // physical effects record separately from authorization.
                    if !sent.swap(true, Ordering::AcqRel) {
                        if accepted_first {
                            driver.prompt_with_attachments(prompt, files);
                        }
                        anyhow::bail!("plan delivery unconfirmed: injected runtime submission failure");
                    }
                    driver.prompt_with_attachments(prompt, files);
                    Ok(())
                });
        }
        fixture.gate.submit(BRIEF.into(), Vec::new());
        if fault == "reject-before-send" || fault == "projection" {
            fixture.accept();
        }
        assert!(!fixture.gate.delivered(), "{fault}");
        // Only a pre-send rejection guarantees zero physical submission; the
        // uncertain legs record the input the provider may already hold.
        assert_eq!(
            mutations(&fixture),
            usize::from(fault != "reject-before-send"),
            "{fault}"
        );
        assert_eq!(
            fixture.gate.review.lock().receipt.phase,
            Phase::Blocked,
            "{fault}"
        );
        // The withheld brief is retained exactly once — no follow-up or
        // recovery path may release it repeatedly.
        fixture.gate.submit("RETRY_SENTINEL".into(), Vec::new());
        assert_eq!(
            mutations(&fixture),
            usize::from(fault != "reject-before-send"),
            "{fault}"
        );
    }
}

#[test]
fn plan_acceptance_size_boundary_and_exact_reread_after_compaction() {
    // The qualified inline mode delivers the complete bundle; over the bound
    // blocks predictably as unsupported — never truncated or summarized.
    for (bytes, supported) in [(INLINE_BYTES, true), (INLINE_BYTES + 1, false)] {
        let text = "x".repeat(bytes);
        let fixture = Fixture::new(&text);
        fixture.gate.submit(BRIEF.into(), Vec::new());
        if !supported {
            assert!(fixture.capture.0.lock().is_empty());
            assert!(!fixture.gate.delivered());
            assert_eq!(
                fixture.stored().receipt.readiness().failure.unwrap().kind,
                PlanFailureKind::DeliveryUnsupported
            );
            continue;
        }
        assert_eq!(mutations(&fixture), 1);
        let input = &fixture.capture.0.lock()[0];
        assert!(input.contains(&text));
        fixture.accept();
        assert!(fixture.gate.delivered());
        // The immutable reread surface survives compaction of any response.
        fixture.gate.review.lock().response.clear();
        let artifact = fixture.gate.review.lock().receipt.artifact.clone();
        let reread = fs::read(artifact).unwrap();
        assert_eq!(reread, text.as_bytes());
        assert_eq!(
            digest(&reread),
            fixture
                .gate
                .review
                .lock()
                .receipt
                .revision
                .review
                .content_digest
        );
    }
}

#[cfg(unix)]
#[test]
fn plan_acceptance_symlink_substitution_never_releases_private_bytes() {
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    let private = fixture.root.join("private-credential");
    fs::write(&private, "PRIVATE_SENTINEL").unwrap();
    let artifact = fixture.root.join("approved-plan.md");
    fs::remove_file(&artifact).unwrap();
    std::os::unix::fs::symlink(&private, artifact).unwrap();
    fixture.accept();
    assert!(!fixture.gate.delivered());
    assert_eq!(
        fixture.gate.review.lock().receipt.phase,
        Phase::Blocked
    );
    assert!(
        !fixture
            .capture
            .0
            .lock()
            .iter()
            .any(|text| text.contains("PRIVATE_SENTINEL"))
    );
    assert_eq!(fs::read_to_string(private).unwrap(), "PRIVATE_SENTINEL");
}

#[test]
fn plan_acceptance_concurrent_retirement_fences_stale_attempt() {
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    let gate = fixture.gate.clone();
    // Join makes the race boundary deterministic: retirement wins before the
    // adapter's acceptance signal lands.
    std::thread::spawn(move || gate.lifecycle.lock().set(None))
        .join()
        .unwrap();
    fixture.accept();
    assert!(!fixture.gate.delivered());
    assert_eq!(
        fixture.gate.review.lock().receipt.phase,
        Phase::Blocked
    );
    fixture.gate.submit("RETRY_SENTINEL".into(), Vec::new());
    assert_eq!(mutations(&fixture), 1);
}

#[test]
fn plan_acceptance_attachments_reach_the_execution_input_exactly_once() {
    #[derive(Default)]
    struct Attachments(Mutex<Vec<Vec<crate::model::MessageAttachment>>>);
    impl driver::DriverControl for Attachments {
        fn prompt(&self, _: String) {}
        fn prompt_with_attachments(&self, _: String, files: Vec<crate::model::MessageAttachment>) {
            self.0.lock().push(files);
        }
        fn cancel(&self) {}
        fn respond(&self, _: String, _: String) {}
        fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
            Ok(None)
        }
    }
    let mut fixture = Fixture::new("approved bytes");
    let files = Arc::new(Attachments::default());
    Arc::get_mut(&mut fixture.gate).unwrap().driver = DriverHandle::from_control(files.clone());
    let attachment = crate::model::MessageAttachment {
        path: fixture.root.join("task-input"),
        mention: "@task-input".into(),
        name: "task-input".into(),
        is_dir: false,
        is_image: false,
        blob_reference: None,
        pasted_text_preview: None,
        session_id: None,
    };
    fixture
        .gate
        .submit(BRIEF.into(), vec![attachment.clone()]);
    assert_eq!(files.0.lock().len(), 1);
    assert_eq!(files.0.lock()[0][0].path, attachment.path);
    fixture.accept();
    assert!(fixture.gate.delivered());
    // Withheld state cleared — a recovered attempt cannot replay the files.
    assert!(fixture.stored().withheld.is_empty());
    let followup = crate::model::MessageAttachment {
        name: "task-input-2".into(),
        path: fixture.root.join("task-input-2"),
        ..attachment
    };
    fixture.gate.submit("followup".into(), vec![followup.clone()]);
    assert_eq!(files.0.lock().len(), 2);
    assert_eq!(files.0.lock()[1][0].path, followup.path);
}

#[test]
fn plan_acceptance_intentional_interrupt_mid_submission_holds_uncertain_input() {
    // An intentional stop while the submitted input's acceptance is unknown
    // holds the attempt rather than risking an overlapping redispatch; the
    // withheld brief stays durable for a fresh attempt's verified redelivery.
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    fixture.gate.interrupt_delivery();
    assert!(!fixture.gate.delivered());
    assert_eq!(
        fixture.gate.review.lock().receipt.phase,
        Phase::Blocked
    );
    let stored = fixture.stored();
    assert_eq!(stored.receipt.phase, Phase::Blocked);
    assert_eq!(stored.withheld[0].0, BRIEF);
    fixture.gate.submit("RETRY_SENTINEL".into(), Vec::new());
    assert_eq!(mutations(&fixture), 1);
    // Interrupting after verified acceptance is ordinary cancellation — the
    // delivered marker and cleared withheld state are unaffected.
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    fixture.accept();
    fixture.gate.interrupt_delivery();
    assert!(fixture.gate.delivered());
    assert!(fixture.stored().withheld.is_empty());
}

#[test]
fn plan_acceptance_settled_input_turn_also_counts_as_acceptance() {
    // An adapter that settles the submitted turn without a start signal still
    // delivered the input; a failed settle is uncertainty and holds.
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    fixture.gate.observe(&DriverEvent::TurnFinished {
        success: true,
        summary: None,
        summary_i18n: None,
    });
    assert!(fixture.gate.delivered());
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    fixture.gate.observe(&DriverEvent::TurnFinished {
        success: false,
        summary: None,
        summary_i18n: None,
    });
    assert!(!fixture.gate.delivered());
    assert_eq!(fixture.stored().receipt.phase, Phase::Blocked);
    assert_eq!(fixture.stored().withheld[0].0, BRIEF);
}
