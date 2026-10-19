//! Fail-closed budget validation for Boss adapter integration.
//!
//! Numeric validation is separate from provider qualification. Synthetic finite
//! bounds can test arithmetic, but cannot qualify a running provider. No current
//! provider has an effective-runtime contract; startup therefore remains blocked.

use uuid::Uuid;
use waku_protocol::model::ProviderKind;

use crate::rotation_continuity::qualify_provider;
use crate::rotation_oracle::ContextBounds;

/// Authoritative expected configuration, obtained from the actual launched
/// runtime, never inferred from PATH, settings on disk, or a saved receipt.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct StartupBudgetBinding {
    pub provider: ProviderKind,
    pub runtime_version: String,
    pub model: String,
    pub configuration_revision: String,
    pub instruction_revision: String,
    pub runtime_id: Uuid,
    pub generation: u64,
    pub package_digest: String,
    /// Fresh identity for this first-request admission attempt. Never reuse a
    /// successful validation after context growth, replay, resume or changes.
    pub operation_id: Uuid,
}

/// Adapter-supplied conservative bounds, in one coherent effective-token unit.
/// None means unknown or unbounded, never zero. U includes the complete startup
/// package, instructions, attachments and provider overhead. G includes every
/// output, tool result and autonomous loop before the next enforceable guard.
#[derive(Clone, Debug, Default)]
pub struct BudgetBounds {
    pub compaction_floor: Option<u64>,
    pub current_context: Option<u64>,
    pub max_growth: Option<u64>,
    pub safety_margin: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct StartupBudgetEvidence {
    pub binding: StartupBudgetBinding,
    pub bounds: BudgetBounds,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetBlocker {
    MissingEvidence,
    IncompleteBinding,
    BindingChanged,
    UnknownCompactionFloor,
    UnknownCurrentContext,
    UnboundedGrowth,
    UnknownMargin,
    ZeroMargin,
    ArithmeticOverflow,
    BoundaryReached,
    UnqualifiedRuntime,
}

/// Check only arithmetic on bounds already independently established by a
/// qualified adapter. Positive headroom is not provider qualification.
pub fn validate_context_budget(bounds: &BudgetBounds) -> Result<u64, BudgetBlocker> {
    let bounds = ContextBounds {
        compaction_floor: bounds
            .compaction_floor
            .ok_or(BudgetBlocker::UnknownCompactionFloor)?,
        current_context: bounds
            .current_context
            .ok_or(BudgetBlocker::UnknownCurrentContext)?,
        max_growth: bounds.max_growth.ok_or(BudgetBlocker::UnboundedGrowth)?,
        safety_margin: bounds.safety_margin.ok_or(BudgetBlocker::UnknownMargin)?,
    };
    if bounds.safety_margin == 0 {
        return Err(BudgetBlocker::ZeroMargin);
    }
    let projected = bounds
        .projected_context()
        .ok_or(BudgetBlocker::ArithmeticOverflow)?;
    if projected >= bounds.compaction_floor {
        return Err(BudgetBlocker::BoundaryReached);
    }
    Ok(bounds.compaction_floor - projected)
}

/// Item 2 must call this before startup submission/activation and keep blocked
/// packages durable. Receipt acceptance and pointer ownership cannot substitute
/// for it. A success would only establish budget eligibility for this binding,
/// not identity, grants, delivery acceptance or ownership. Continuation guards
/// must independently repeat validation at every context-growing boundary.
///
/// Evidence must come from trusted runtime/adapter observations, not model text.
/// All current routes return an error: no effective runtime is yet qualified.
pub fn validate_startup_budget(
    expected: &StartupBudgetBinding,
    evidence: Option<&StartupBudgetEvidence>,
) -> Result<u64, BudgetBlocker> {
    if [
        &expected.runtime_version,
        &expected.model,
        &expected.configuration_revision,
        &expected.instruction_revision,
        &expected.package_digest,
    ]
    .iter()
    .any(|value| value.is_empty())
        || expected.runtime_id.is_nil()
        || expected.operation_id.is_nil()
    {
        return Err(BudgetBlocker::IncompleteBinding);
    }
    let evidence = evidence.ok_or(BudgetBlocker::MissingEvidence)?;
    if evidence.binding != *expected {
        return Err(BudgetBlocker::BindingChanged);
    }
    let headroom = validate_context_budget(&evidence.bounds)?;
    if !qualify_provider(expected.provider, Some(&expected.runtime_version)).qualified() {
        return Err(BudgetBlocker::UnqualifiedRuntime);
    }
    Ok(headroom)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(current: u64) -> BudgetBounds {
        BudgetBounds {
            compaction_floor: Some(100),
            current_context: Some(current),
            max_growth: Some(10),
            safety_margin: Some(1),
        }
    }

    fn binding() -> StartupBudgetBinding {
        StartupBudgetBinding {
            provider: ProviderKind::Codex,
            runtime_version: "0.162.1".into(),
            model: "fixture-model".into(),
            configuration_revision: "fixture-config".into(),
            instruction_revision: "fixture-instructions".into(),
            runtime_id: Uuid::new_v4(),
            generation: 1,
            package_digest: "fixture-package".into(),
            operation_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn strict_boundary_unknowns_and_overflow_fail_closed() {
        assert_eq!(validate_context_budget(&bounds(88)), Ok(1));
        for current in [89, 90, u64::MAX - 11] {
            assert_eq!(
                validate_context_budget(&bounds(current)),
                Err(BudgetBlocker::BoundaryReached)
            );
        }
        assert_eq!(
            validate_context_budget(&bounds(u64::MAX - 10)),
            Err(BudgetBlocker::ArithmeticOverflow)
        );
        assert_eq!(
            validate_context_budget(&bounds(u64::MAX)),
            Err(BudgetBlocker::ArithmeticOverflow)
        );
        for (field, error) in [
            (0, BudgetBlocker::UnknownCompactionFloor),
            (1, BudgetBlocker::UnknownCurrentContext),
            (2, BudgetBlocker::UnboundedGrowth),
            (3, BudgetBlocker::UnknownMargin),
        ] {
            let mut b = bounds(0);
            match field {
                0 => b.compaction_floor = None,
                1 => b.current_context = None,
                2 => b.max_growth = None,
                _ => b.safety_margin = None,
            }
            assert_eq!(validate_context_budget(&b), Err(error));
        }
        let mut b = bounds(0);
        b.safety_margin = Some(0);
        assert_eq!(validate_context_budget(&b), Err(BudgetBlocker::ZeroMargin));
        b = bounds(0);
        b.compaction_floor = Some(0);
        assert_eq!(
            validate_context_budget(&b),
            Err(BudgetBlocker::BoundaryReached)
        );
    }

    #[test]
    fn startup_binds_every_revision_and_operation() {
        let expected = binding();
        assert_eq!(
            validate_startup_budget(&expected, None),
            Err(BudgetBlocker::MissingEvidence)
        );
        for field in 0..9 {
            let mut changed = expected.clone();
            match field {
                0 => changed.provider = ProviderKind::Pi,
                1 => changed.runtime_version.push('x'),
                2 => changed.model.push('x'),
                3 => changed.configuration_revision.push('x'),
                4 => changed.instruction_revision.push('x'),
                5 => changed.runtime_id = Uuid::new_v4(),
                6 => changed.generation += 1,
                7 => changed.package_digest.push('x'),
                _ => changed.operation_id = Uuid::new_v4(),
            }
            let evidence = StartupBudgetEvidence {
                binding: changed,
                bounds: bounds(0),
            };
            assert_eq!(
                validate_startup_budget(&expected, Some(&evidence)),
                Err(BudgetBlocker::BindingChanged)
            );
        }
        let mut incomplete = expected;
        incomplete.configuration_revision.clear();
        assert_eq!(
            validate_startup_budget(&incomplete, None),
            Err(BudgetBlocker::IncompleteBinding)
        );
    }

    #[test]
    fn every_current_route_blocks_even_with_synthetic_positive_headroom() {
        for provider in ProviderKind::ALL {
            let mut binding = binding();
            binding.provider = provider;
            let evidence = StartupBudgetEvidence {
                binding: binding.clone(),
                bounds: bounds(0),
            };
            assert_eq!(
                validate_startup_budget(&binding, Some(&evidence)),
                Err(BudgetBlocker::UnqualifiedRuntime)
            );
        }
    }
}
