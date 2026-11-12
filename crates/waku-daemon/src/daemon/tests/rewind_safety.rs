use super::*;
use std::os::unix::fs::PermissionsExt as _;

#[test]
fn codex_message_rewind_preserves_native_source_when_its_result_is_lost() {
    for mode in [
        "confirmed",
        "legacy",
        "rejected",
        "lost",
        "read-rejected",
        "repeated-cursor",
    ] {
        let root = std::env::temp_dir().join(format!("rewind-safety-{}", Uuid::new_v4()));
        let (backend, _session_id) = surface_test_backend(&root);
        let mut source = backend.task_state.lock().sessions[0].clone();
        source.provider = ProviderKind::Codex;
        source.provider_cursor = Some(ProviderResumeCursor::Codex {
            thread_id: "native-source".into(),
        });
        let binary = root.join("codex");
        std::fs::write(&binary, include_str!("fixtures/rewind_peer.py")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(root.join("mode"), mode).unwrap();
        std::fs::write(
            root.join("ledger.json"),
            r#"{"sourceTurns":4,"children":[]}"#,
        )
        .unwrap();

        let (cursor, _, reset) = backend
            .rewind_provider_response(&source, &root, &binary, 0, 4, 0, None)
            .unwrap();
        assert!(cursor.is_none() && reset);
        let result = backend.rewind_provider_response(&source, &root, &binary, 2, 2, 2, None);
        let ledger: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("ledger.json")).unwrap()).unwrap();
        drop(backend);
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(ledger["sourceTurns"], 4, "source history changed in {mode}");
        if matches!(mode, "confirmed" | "legacy") {
            let (cursor, _, reset) = result.unwrap();
            assert_eq!(
                cursor,
                Some(ProviderResumeCursor::Codex {
                    thread_id: "independent-history".into(),
                })
            );
            assert!(!reset);
            assert_eq!(ledger["children"], serde_json::json!([2]));
        } else {
            assert!(
                result.is_err(),
                "{mode} must not select an unconfirmed fork"
            );
            assert_eq!(
                ledger["children"],
                if mode == "lost" {
                    serde_json::json!([2])
                } else {
                    serde_json::json!([])
                }
            );
        }
    }
}
