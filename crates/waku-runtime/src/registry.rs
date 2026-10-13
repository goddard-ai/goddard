//! The adapter registry the runtime executable composes.
//!
//! Adapters own provider specifics; the host knows only names. An
//! admission naming an unregistered adapter is a recorded spawn failure —
//! never a silent substitution for another provider.

use std::collections::HashMap;
use std::sync::Arc;

use crate::adapter::{
    AdapterFactory, AdapterHandle, RecoveredOwner, SessionContext, SpawnError, Termination,
};

/// Maps the admission's `adapter` name to its factory. Provider stages
/// register their factories here; nothing unregistered can spawn.
#[derive(Default)]
pub struct AdapterRegistry {
    factories: HashMap<String, Arc<dyn AdapterFactory>>,
    versions: std::collections::BTreeMap<String, String>,
}

impl AdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, name: impl Into<String>, factory: Arc<dyn AdapterFactory>) {
        self.factories.insert(name.into(), factory);
    }

    pub fn register_versioned(
        &mut self,
        name: impl Into<String>,
        version: impl Into<String>,
        factory: Arc<dyn AdapterFactory>,
    ) {
        let name = name.into();
        self.versions.insert(name.clone(), version.into());
        self.register(name, factory);
    }

    pub fn registered(&self, name: &str) -> bool {
        self.factories.contains_key(name)
    }
}

impl AdapterFactory for AdapterRegistry {
    fn adapter_versions(&self) -> std::collections::BTreeMap<String, String> {
        self.versions.clone()
    }
    fn fence_recovered(&self, owner: &RecoveredOwner) -> Termination {
        self.factories.get(&owner.adapter).map_or(
            Termination::Unconfirmed {
                detail: "adapter recovery unavailable".into(),
            },
            |factory| factory.fence_recovered(owner),
        )
    }
    fn spawn(&self, ctx: SessionContext) -> Result<Box<dyn AdapterHandle>, SpawnError> {
        match self.factories.get(&ctx.adapter) {
            Some(factory) => factory.spawn(ctx),
            None => Err(SpawnError(format!(
                "no adapter registered for '{}'",
                ctx.adapter
            ))),
        }
    }
}
