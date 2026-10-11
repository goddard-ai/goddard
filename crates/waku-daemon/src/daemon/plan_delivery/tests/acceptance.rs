//! Deterministic provider contract probes. These prove dispatch progression,
//! not that a real provider disables its native tools during review.
use super::*;

const BRIEF: &str = "TASK_MUTATION_SENTINEL";

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
fn plan_acceptance_every_ack_identity_and_section_is_required() {
    // All eight identity/coverage fields must match, not just the digest.
    for field in 1..=8 {
        let fixture = Fixture::new(&"approved section\n".repeat(2500));
        fixture.gate.submit(BRIEF.into(), Vec::new());
        let ack = fixture.acknowledgement();
        let mut fields: Vec<_> = ack.split_whitespace().collect();
        fields[field] = "wrong-or-stale";
        fixture.finish(&fields.join(" "));
        assert!(!fixture.gate.ready(), "field {field}");
        assert_eq!(mutations(&fixture), 0, "field {field}");
        assert_eq!(fixture.gate.review.lock().pending[0].0, BRIEF);
    }
    for coverage in ["repeated", "reversed", "extra-text", "overflow"] {
        let fixture = Fixture::new(&format!(
            "{}{}",
            "a".repeat(SECTION_BYTES),
            "b".repeat(SECTION_BYTES)
        ));
        fixture.gate.submit(BRIEF.into(), Vec::new());
        let ack = fixture.acknowledgement();
        let (prefix, sections) = ack.rsplit_once(' ').unwrap();
        let section: Vec<_> = sections.split(',').collect();
        let response = match coverage {
            "repeated" => format!("{prefix} {},{}", section[0], section[0]),
            "reversed" => format!("{prefix} {},{}", section[1], section[0]),
            "extra-text" => format!("{ack}\nExtra trailing text."),
            "overflow" => format!("{}\n{ack}", "x".repeat(64 * 1024)),
            _ => unreachable!(),
        };
        fixture.finish(&response);
        assert_eq!(mutations(&fixture), 0, "{coverage}");
        assert!(!fixture.gate.ready(), "{coverage}");
    }
}

#[test]
fn plan_acceptance_revocation_after_durable_ack_before_dispatch() {
    let mut fixture = Fixture::new("approved bytes");
    let authorized = fixture.authorized.clone();
    Arc::get_mut(&mut fixture.gate).unwrap().publish = Box::new(move |receipt, _| {
        if receipt.phase == Phase::Ready {
            authorized.store(false, Ordering::Release);
        }
        Ok(())
    });
    fixture.gate.submit(BRIEF.into(), Vec::new());
    fixture.finish(&fixture.acknowledgement());
    assert_eq!(mutations(&fixture), 0);
    let stored: StoredDelivery =
        serde_json::from_slice(&fs::read(fixture.root.join("delivery.json")).unwrap()).unwrap();
    assert!(stored.receipt.acknowledged);
    assert_eq!(stored.receipt.phase, Phase::Blocked);
    assert_eq!(stored.withheld[0].0, BRIEF);
}

#[test]
fn plan_acceptance_dispatch_and_projection_failures_preserve_work() {
    for fault in ["review-dispatch", "implementation-dispatch", "projection"] {
        let mut fixture = Fixture::new("approved bytes");
        if fault == "projection" {
            Arc::get_mut(&mut fixture.gate).unwrap().publish = Box::new(|receipt, _| {
                anyhow::ensure!(
                    receipt.phase != Phase::Ready,
                    "supervisor transport unavailable"
                );
                Ok(())
            });
        } else {
            Arc::get_mut(&mut fixture.gate).unwrap().dispatch =
                Box::new(move |driver, prompt, files| {
                    let implementation = prompt.contains(BRIEF);
                    anyhow::ensure!(
                        !(fault == "review-dispatch" || implementation),
                        "plan runtime unavailable: injected dispatch failure"
                    );
                    driver.prompt_with_attachments(prompt, files);
                    Ok(())
                });
        }
        fixture.gate.submit(BRIEF.into(), Vec::new());
        fixture.finish(&fixture.acknowledgement());
        assert_eq!(mutations(&fixture), 0, "{fault}");
        assert!(!fixture.gate.ready(), "{fault}");
        let stored: StoredDelivery =
            serde_json::from_slice(&fs::read(fixture.root.join("delivery.json")).unwrap()).unwrap();
        assert_eq!(stored.receipt.phase, Phase::Blocked);
        assert_eq!(stored.withheld[0].0, BRIEF);
    }
}

#[test]
fn plan_acceptance_size_boundary_and_exact_reread_after_compaction() {
    for bytes in [INLINE_BYTES, INLINE_BYTES + 1, 256 * 1024, 256 * 1024 + 2] {
        let text = "x".repeat(bytes);
        let fixture = Fixture::new(&text);
        fixture.gate.submit(BRIEF.into(), Vec::new());
        if bytes > 256 * 1024 + 1 {
            assert!(fixture.capture.0.lock().is_empty());
            assert_eq!(mutations(&fixture), 0);
            continue;
        }
        fixture.finish(&fixture.acknowledgement());
        assert!(fixture.gate.ready());
        assert_eq!(mutations(&fixture), 1);
        // Review response is discarded; the immutable reread surface survives.
        assert!(fixture.gate.review.lock().response.is_empty());
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
    fixture.finish(&fixture.acknowledgement());
    assert_eq!(mutations(&fixture), 0);
    assert!(!fixture.gate.ready());
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
fn plan_acceptance_concurrent_retirement_fences_old_acknowledgement() {
    let fixture = Fixture::new("approved bytes");
    fixture.gate.submit(BRIEF.into(), Vec::new());
    let gate = fixture.gate.clone();
    // Join makes the race boundary deterministic: retirement wins before ack.
    std::thread::spawn(move || gate.lifecycle.lock().set(None))
        .join()
        .unwrap();
    fixture.finish(&fixture.acknowledgement());
    assert_eq!(mutations(&fixture), 0);
    assert!(!fixture.gate.ready());
}

#[test]
fn plan_acceptance_withheld_attachments_only_reach_implementation() {
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
    fixture.gate.submit(BRIEF.into(), vec![attachment.clone()]);
    assert!(files.0.lock()[0].is_empty());
    let stored: StoredDelivery =
        serde_json::from_slice(&fs::read(fixture.root.join("delivery.json")).unwrap()).unwrap();
    assert_eq!(stored.withheld[0].1[0].path, attachment.path);
    fixture.finish(&fixture.acknowledgement());
    assert_eq!(files.0.lock().len(), 2);
    assert_eq!(files.0.lock()[1][0].path, attachment.path);
}
