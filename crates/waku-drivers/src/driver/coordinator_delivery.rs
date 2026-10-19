use super::DriverControl;

/// Durable exact-package transport boundary. Unsupported adapters never use a
/// generic prompt fallback. Reopened submitted attempts only reconcile; unknown
/// acknowledgement never creates a new provider request.
pub fn deliver_coordinator_package(
    adapter: &dyn DriverControl,
    ledger_path: &std::path::Path,
    delivery_id: &str,
    package: &waku_boss::coordinator_contract::CoordinatorPackage,
    current: &waku_boss::coordinator_contract::CoordinatorBinding,
    expected_budget: &waku_boss::provider_budget::StartupBudgetBinding,
) -> anyhow::Result<waku_boss::rotation_continuity::ContinuityDeliveryStatus> {
    use waku_boss::rotation_continuity::{ContinuityDeliveryStatus, ContinuityLedger};
    package.validate_current(current)?;
    let existing = ContinuityLedger::load(ledger_path)?;
    anyhow::ensure!(
        !existing
            .deliveries
            .iter()
            .any(|d| d.session_id == current.session
                && d.delivery_id != delivery_id
                && d.status == ContinuityDeliveryStatus::Submitted),
        "another coordinator attempt requires reconciliation"
    );
    let now = crate::model::unix_time();
    ContinuityLedger::update(ledger_path, |ledger| {
        ledger.prepare_coordinator(delivery_id, package, now)
    })??;
    let ledger = ContinuityLedger::load(ledger_path)?;
    let delivery = ledger
        .deliveries
        .iter()
        .find(|d| d.delivery_id == delivery_id)
        .ok_or_else(|| anyhow::anyhow!("missing prepared coordinator package"))?;
    if delivery.status == ContinuityDeliveryStatus::Submitted {
        let attempt = delivery
            .attempt
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("missing coordinator attempt"))?;
        let outcome = adapter.reconcile_coordinator_package(attempt);
        let settled = ContinuityLedger::update(ledger_path, |ledger| {
            ledger.record_coordinator_outcome(attempt, outcome, now)
        })?;
        anyhow::ensure!(
            settled,
            "coordinator acknowledgement unresolved; replay denied"
        );
        let reconciled = ContinuityLedger::load(ledger_path)?;
        return Ok(reconciled
            .deliveries
            .iter()
            .find(|d| d.delivery_id == delivery_id)
            .ok_or_else(|| anyhow::anyhow!("missing reconciled delivery"))?
            .status);
    }
    anyhow::ensure!(
        delivery.status != ContinuityDeliveryStatus::Accepted,
        "accepted package requires independent readiness validation"
    );
    anyhow::ensure!(
        expected_budget.runtime_id != uuid::Uuid::nil()
            && expected_budget.generation == current.generation
            && expected_budget.instruction_revision == current.identity_revision
            && expected_budget.provider == current.provider
            && expected_budget.model == current.model
            && expected_budget.package_digest == package.exact_content()?.digest()?,
        "startup binding changed"
    );
    let observed = adapter.coordinator_startup_observation();
    waku_boss::provider_budget::validate_startup_budget(expected_budget, observed.as_ref())
        .map_err(|blocker| anyhow::anyhow!("coordinator startup blocked: {blocker:?}"))?;
    anyhow::ensure!(
        adapter.coordinator_policy_enforced(),
        "coordinator capability policy unsupported"
    );
    if let Some(frozen) = &ledger.frozen_coordinator {
        anyhow::ensure!(
            frozen.successor == current.session,
            "frozen predecessor cannot submit coordinator work"
        );
        waku_boss::guarded_lifecycle::validate_replacement_capacity(
            frozen.predecessor,
            frozen.successor,
            expected_budget,
            adapter.coordinator_replacement_capacity().as_ref(),
        )?;
    }
    let attempt = ContinuityLedger::update(ledger_path, |ledger| {
        anyhow::ensure!(
            !ledger
                .deliveries
                .iter()
                .any(|d| d.session_id == current.session
                    && d.delivery_id != delivery_id
                    && d.status == ContinuityDeliveryStatus::Submitted),
            "another coordinator attempt requires reconciliation"
        );
        ledger.submit_package(delivery_id, expected_budget.runtime_id, now)
    })??;
    let outcome = adapter.submit_coordinator_package(package, &attempt);
    let settled = ContinuityLedger::update(ledger_path, |ledger| {
        ledger.record_coordinator_outcome(&attempt, outcome, now)
    })?;
    anyhow::ensure!(settled, "coordinator submission unresolved; replay denied");
    let reconciled = ContinuityLedger::load(ledger_path)?;
    Ok(reconciled
        .deliveries
        .iter()
        .find(|d| d.delivery_id == delivery_id)
        .ok_or_else(|| anyhow::anyhow!("missing reconciled delivery"))?
        .status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ProviderKind, ProviderResumeCursor};
    use parking_lot::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use uuid::Uuid;
    use waku_boss::{coordinator_contract::*, provider_budget::*, rotation_continuity::*};
    #[derive(Default)]
    struct Adapter {
        prompts: AtomicUsize,
        submissions: AtomicUsize,
        queried: Mutex<Vec<ContinuityAttempt>>,
        answer: Mutex<Option<PackageOutcome>>,
        observation: Mutex<Option<StartupBudgetEvidence>>,
    }
    impl DriverControl for Adapter {
        fn coordinator_policy_enforced(&self) -> bool {
            true
        }
        fn coordinator_startup_observation(&self) -> Option<StartupBudgetEvidence> {
            self.observation.lock().clone()
        }
        fn prompt(&self, _: String) {
            self.prompts.fetch_add(1, Ordering::SeqCst);
        }
        fn cancel(&self) {}
        fn respond(&self, _: String, _: String) {}
        fn rollback(&self, _: usize) -> anyhow::Result<Option<ProviderResumeCursor>> {
            Ok(None)
        }
        fn submit_coordinator_package(
            &self,
            _: &CoordinatorPackage,
            _: &ContinuityAttempt,
        ) -> PackageOutcome {
            self.submissions.fetch_add(1, Ordering::SeqCst);
            panic!("unqualified adapter submitted")
        }
        fn reconcile_coordinator_package(&self, attempt: &ContinuityAttempt) -> PackageOutcome {
            self.queried.lock().push(attempt.clone());
            self.answer
                .lock()
                .clone()
                .unwrap_or(PackageOutcome::Unknown)
        }
    }
    fn package() -> CoordinatorPackage {
        CoordinatorPackage {
            binding: CoordinatorBinding {
                provider: ProviderKind::Codex,
                session: Uuid::new_v4(),
                generation: 2,
                identity_revision: "canonical".into(),
                grants_revision: "grants".into(),
                model: "model".into(),
                scope: ContinuityScope::Coordinator,
            },
            content: ContinuityPackage {
                identity_revision: "canonical".into(),
                scope: ContinuityScope::Coordinator,
                instructions: "exact authoritative instructions".into(),
                context: "pending user input".into(),
            },
            event_ids: vec![Uuid::new_v4()],
            effect_ids: vec![],
            work_ids: vec![],
        }
    }
    fn budget(package: &CoordinatorPackage) -> StartupBudgetBinding {
        StartupBudgetBinding {
            provider: ProviderKind::Codex,
            runtime_version: "effective-unknown".into(),
            model: package.binding.model.clone(),
            configuration_revision: "config".into(),
            instruction_revision: package.binding.identity_revision.clone(),
            runtime_id: Uuid::new_v4(),
            generation: package.binding.generation,
            package_digest: package.exact_content().unwrap().digest().unwrap(),
            operation_id: Uuid::new_v4(),
        }
    }
    #[test]
    fn coordinator_delivery_never_submits_current_adapters_and_preserves_employee_prompt() {
        let path = std::env::temp_dir().join(format!("coordinator-driver-{}.json", Uuid::new_v4()));
        let mut package = package();
        let adapter = Adapter::default();
        let mut expected = budget(&package);
        for provider in ProviderKind::ALL {
            package.binding.provider = provider;
            expected.provider = provider;
            expected.package_digest = package.exact_content().unwrap().digest().unwrap();
            *adapter.observation.lock() = Some(StartupBudgetEvidence {
                binding: expected.clone(),
                bounds: BudgetBounds {
                    compaction_floor: Some(1000),
                    current_context: Some(1),
                    max_growth: Some(1),
                    safety_margin: Some(1),
                },
            });
            assert!(
                deliver_coordinator_package(
                    &adapter,
                    &path,
                    &format!("delivery-{provider:?}"),
                    &package,
                    &package.binding,
                    &expected
                )
                .is_err()
            );
        }
        let held = ContinuityLedger::load(&path).unwrap();
        assert_eq!(held.deliveries.len(), ProviderKind::ALL.len());
        assert_eq!(
            held.deliveries.last().unwrap().package,
            Some(package.exact_content().unwrap())
        );
        assert_eq!(
            held.deliveries.last().unwrap().status,
            ContinuityDeliveryStatus::Prepared
        );
        assert_eq!(adapter.submissions.load(Ordering::SeqCst), 0);
        adapter.prompt("employee prompt unaffected".into());
        assert_eq!(adapter.prompts.load(Ordering::SeqCst), 1);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn coordinator_delivery_reconciles_original_attempt_after_restart_without_resending() {
        let path =
            std::env::temp_dir().join(format!("coordinator-reconcile-{}.json", Uuid::new_v4()));
        let package = package();
        let expected = budget(&package);
        let adapter = Adapter::default();
        let attempt = ContinuityLedger::update(&path, |ledger| -> anyhow::Result<_> {
            ledger.prepare_coordinator("delivery", &package, 1)?;
            ledger.submit_package("delivery", expected.runtime_id, 2)
        })
        .unwrap()
        .unwrap();
        for _ in 0..2 {
            assert!(
                deliver_coordinator_package(
                    &adapter,
                    &path,
                    "delivery",
                    &package,
                    &package.binding,
                    &expected
                )
                .is_err()
            );
        }
        assert_eq!(
            *adapter.queried.lock(),
            vec![attempt.clone(), attempt.clone()]
        );
        *adapter.answer.lock() = Some(PackageOutcome::Confirmed(ProviderPackageReceipt {
            attempt: attempt.clone(),
            native_session: "native-session".into(),
            native_request: "native-request".into(),
        }));
        deliver_coordinator_package(
            &adapter,
            &path,
            "delivery",
            &package,
            &package.binding,
            &expected,
        )
        .unwrap();
        let accepted = ContinuityLedger::load(&path).unwrap();
        assert_eq!(accepted.provider_receipts[0].attempt, attempt);
        assert_eq!(
            accepted.deliveries[0].status,
            ContinuityDeliveryStatus::Accepted
        );
        assert_eq!(adapter.submissions.load(Ordering::SeqCst), 0);
        std::fs::remove_file(path).unwrap();
    }
}
