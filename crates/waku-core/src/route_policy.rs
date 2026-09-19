//! The model-routing policy: a user-editable JSON document at
//! `~/.goddard/route-policy.json` that maps classified task dimensions —
//! family and class — onto concrete provider/model targets.
//!
//! The JSON file is the source of truth. It is validated on load; an invalid
//! document falls back to the shipped default with a warning rather than
//! silently changing how sessions route. The file is re-read whenever its
//! mtime changes, so edits apply on the next routed submission without a
//! daemon restart.
//!
//! Target strings come in four forms:
//!
//! * `session:tier:fast` / `session:tier:default` / `session:tier:heavy` —
//!   resolve through the `tiers` table inside the session's own provider;
//!   degrades to the global tier walk when that provider is not a candidate.
//! * `tier:fast` / `tier:default` / `tier:heavy` — resolve through the
//!   policy's `tiers` table for whichever provider the route lands on. A
//!   provider with no tier entry falls back to its own default model.
//! * `provider:model` — a concrete target.
//! * `provider` — a provider's own default model.
//!
//! `tiers` values are either a bare model id (`"fast": "claude-haiku-4-5"`)
//! or an object carrying an optional effort override
//! (`"heavy": {"model": "claude-opus-5", "effort": "high"}`). Routing uses
//! the model only; effort is consumed by other tier clients such as the
//! subagents experiment.
//!
//! The classifier never emits these strings; policy resolution is ordinary
//! deterministic code.

use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash as _, Hasher as _};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context as _, bail};
use parking_lot::Mutex;
use serde::Deserialize;
use waku_protocol::model::ProviderKind;
use waku_protocol::routing::{TaskClass, TaskFamily};

/// The document version this build understands.
pub const POLICY_VERSION: u32 = 1;

/// Where the user policy lives: beside the daemon's `settings.json`.
pub fn default_policy_path() -> PathBuf {
    waku_protocol::settings::DaemonSettings::default_path()
        .parent()
        .map(|dir| dir.join("route-policy.json"))
        .unwrap_or_else(|| PathBuf::from("route-policy.json"))
}

/// The shipped default, written out for the user on first use.
pub const DEFAULT_POLICY_JSON: &str = r#"{
  "version": 1,
  "default": "last_used",
  "preferredProviders": [
    "claude",
    "codex",
    "cursor",
    "amp",
    "droid",
    "grok",
    "deepseek",
    "devin",
    "fx",
    "kimi",
    "pi",
    "ohmypi",
    "opencode",
    "opencode2"
  ],
  "minFamilyConfidence": 0.55,
  "minClassConfidence": 0.55,
  "planningBoost": true,
  "classes": {
    "routine": "session:tier:fast",
    "general": "session:tier:default",
    "demanding": "session:tier:heavy"
  },
  "familyOverrides": {
    "agentic-tool-use": { "minClass": "general" },
    "planning-ideation": { "minClass": "general" },
    "tutoring": { "minClass": "general" }
  },
  "tiers": {
    "claude": {
      "fast": "claude-haiku-4-5",
      "default": "claude-sonnet-5",
      "heavy": "claude-opus-5"
    },
    "codex": {
      "fast": "gpt-5.6-luna",
      "default": "gpt-5.6-sol",
      "heavy": "gpt-5.6-terra"
    },
    "amp": {
      "fast": "low",
      "default": "medium",
      "heavy": "high"
    },
    "grok": {
      "fast": "grok-4.5",
      "default": "grok-4.6"
    }
  }
}
"#;

/// What the policy's `default` resolves to when classification does not
/// produce a usable route.
#[derive(Clone, Debug, PartialEq)]
pub enum DefaultRoute {
    /// Whatever provider/model the user last picked, carried on the request.
    LastUsed,
    Target(PolicyTarget),
}

/// A `tier:*`, `session:tier:*`, or concrete `provider[:model]` policy target.
#[derive(Clone, Debug, PartialEq)]
pub enum PolicyTarget {
    /// Resolve the tier through the first eligible provider in
    /// `preferredProviders` order.
    Tier(Tier),
    /// Resolve the tier within the session's own provider — what most
    /// people expect Auto to do: upgrade or downgrade the provider they
    /// already picked rather than hop to another one.
    SessionTier(Tier),
    Concrete {
        provider: ProviderKind,
        /// `None` = the provider's own default model.
        model: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Tier {
    Fast,
    Default,
    Heavy,
}

/// One resolved tier entry: the model the tier maps to plus an optional
/// provider-specific effort override.
#[derive(Clone, Debug, PartialEq)]
pub struct TierModel {
    pub model: String,
    pub effort: Option<String>,
}

/// A family's override block: a class floor, per-class target overrides, or
/// both.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FamilyOverride {
    pub min_class: Option<TaskClass>,
    pub classes: BTreeMap<TaskClass, PolicyTarget>,
}

/// A validated policy, ready to resolve.
#[derive(Clone, Debug)]
pub struct RoutePolicy {
    /// Short content hash recorded on decisions so a route can be traced to
    /// the policy document that produced it.
    pub hash: String,
    /// Whether the active policy is the shipped default (the file was absent
    /// or invalid) rather than the user's own document.
    pub is_default: bool,
    pub default: DefaultRoute,
    /// Providers tried in order when a `tier:*` target resolves; unlisted
    /// eligible candidates follow in request order.
    pub preferred_providers: Vec<ProviderKind>,
    pub min_family_confidence: f64,
    pub min_class_confidence: f64,
    /// A `needsPlanning` answer raises the effective class one step.
    pub planning_boost: bool,
    pub classes: BTreeMap<TaskClass, PolicyTarget>,
    /// The raw class target strings as written in the document, for the
    /// settings surface's dropdown values.
    pub classes_raw: BTreeMap<String, String>,
    /// The raw `default` value as written ("last_used" or a target string).
    pub default_raw: String,
    pub family_overrides: BTreeMap<TaskFamily, FamilyOverride>,
    /// provider -> tier -> model/effort entry.
    pub tiers: BTreeMap<ProviderKind, BTreeMap<Tier, TierModel>>,
}

// ---------- raw document ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyDocument {
    version: u32,
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    preferred_providers: Option<Vec<String>>,
    #[serde(default)]
    min_family_confidence: Option<f64>,
    #[serde(default)]
    min_class_confidence: Option<f64>,
    #[serde(default)]
    planning_boost: Option<bool>,
    #[serde(default)]
    classes: BTreeMap<String, String>,
    #[serde(default)]
    family_overrides: BTreeMap<String, FamilyOverrideDocument>,
    #[serde(default)]
    tiers: BTreeMap<String, BTreeMap<String, TierValue>>,
}

/// A `tiers` table value as written in the document: either the bare model
/// id or `{ "model": ..., "effort": ... }`.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum TierValue {
    Model(String),
    Full {
        model: String,
        #[serde(default)]
        effort: Option<String>,
    },
}

impl TierValue {
    fn into_model(self) -> TierModel {
        match self {
            TierValue::Model(model) => TierModel {
                model,
                effort: None,
            },
            TierValue::Full { model, effort } => TierModel { model, effort },
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FamilyOverrideDocument {
    #[serde(default)]
    min_class: Option<String>,
    #[serde(default)]
    classes: Option<BTreeMap<String, String>>,
}

// ---------- parse + validate ----------

pub fn parse_policy(source: &str) -> anyhow::Result<RoutePolicy> {
    let document: PolicyDocument =
        serde_json::from_str(source).context("policy is not valid JSON")?;
    if document.version != POLICY_VERSION {
        bail!(
            "policy version {} is unsupported; expected {POLICY_VERSION}",
            document.version
        );
    }
    let parse_confidence = |name: &str, value: Option<f64>, default: f64| -> anyhow::Result<f64> {
        let value = value.unwrap_or(default);
        if !(0.0..=1.0).contains(&value) {
            bail!("{name} must be between 0 and 1");
        }
        Ok(value)
    };
    let classes = document
        .classes
        .iter()
        .map(|(class, target)| {
            Ok((
                parse_class(class)?,
                parse_target(target).with_context(|| format!("classes.{class}"))?,
            ))
        })
        .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
    for required in [TaskClass::Routine, TaskClass::General, TaskClass::Demanding] {
        if !classes.contains_key(&required) {
            bail!("policy is missing a classes entry for {required:?}");
        }
    }
    let family_overrides = document
        .family_overrides
        .iter()
        .map(|(family, override_doc)| {
            let min_class = override_doc
                .min_class
                .as_deref()
                .map(|class| {
                    parse_class(class).with_context(|| format!("familyOverrides.{family}.minClass"))
                })
                .transpose()?;
            let classes = override_doc
                .classes
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|(class, target)| {
                    Ok((
                        parse_class(&class).with_context(|| format!("familyOverrides.{family}"))?,
                        parse_target(&target)
                            .with_context(|| format!("familyOverrides.{family}.{class}"))?,
                    ))
                })
                .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
            Ok((parse_family(family)?, FamilyOverride { min_class, classes }))
        })
        .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
    let tiers = document
        .tiers
        .iter()
        .map(|(provider, table)| {
            let provider =
                provider_from_id(provider).with_context(|| format!("tiers.{provider}"))?;
            let table = table
                .iter()
                .map(|(tier, value)| Ok((parse_tier(tier)?, value.clone().into_model())))
                .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
            Ok((provider, table))
        })
        .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
    let preferred_providers = document
        .preferred_providers
        .unwrap_or_default()
        .iter()
        .map(|provider| {
            provider_from_id(provider).with_context(|| format!("preferredProviders.{provider}"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let default_raw = document
        .default
        .clone()
        .unwrap_or_else(|| "last_used".into());
    let default = match document.default.as_deref() {
        None | Some("last_used") => DefaultRoute::LastUsed,
        Some(target) => DefaultRoute::Target(parse_target(target).context("default")?),
    };
    Ok(RoutePolicy {
        hash: hash_policy(source),
        is_default: false,
        default,
        preferred_providers,
        min_family_confidence: parse_confidence(
            "minFamilyConfidence",
            document.min_family_confidence,
            0.55,
        )?,
        min_class_confidence: parse_confidence(
            "minClassConfidence",
            document.min_class_confidence,
            0.55,
        )?,
        planning_boost: document.planning_boost.unwrap_or(true),
        classes,
        classes_raw: document.classes,
        default_raw,
        family_overrides,
        tiers,
    })
}

fn hash_policy(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn shipped_default_policy() -> RoutePolicy {
    let mut policy =
        parse_policy(DEFAULT_POLICY_JSON).expect("the shipped default policy must always parse");
    policy.is_default = true;
    policy
}

fn parse_tier(name: &str) -> anyhow::Result<Tier> {
    match name {
        "fast" => Ok(Tier::Fast),
        "default" => Ok(Tier::Default),
        "heavy" => Ok(Tier::Heavy),
        other => bail!("unknown tier {other:?}; expected fast, default, or heavy"),
    }
}

fn parse_class(id: &str) -> anyhow::Result<TaskClass> {
    serde_json::from_value(serde_json::Value::String(id.to_owned()))
        .with_context(|| format!("unknown task class {id:?}"))
}

fn parse_family(id: &str) -> anyhow::Result<TaskFamily> {
    TaskFamily::ALL
        .iter()
        .find(|family| family.id() == id)
        .copied()
        .with_context(|| format!("unknown task family {id:?}"))
}

fn provider_from_id(id: &str) -> anyhow::Result<ProviderKind> {
    ProviderKind::ALL
        .iter()
        .find(|provider| provider.id() == id)
        .copied()
        .with_context(|| format!("unknown provider {id:?}"))
}

/// `session:tier:*` | `tier:fast` | `tier:default` | `tier:heavy` |
/// `provider` | `provider:model`.
pub fn parse_target(raw: &str) -> anyhow::Result<PolicyTarget> {
    let raw = raw.trim();
    if let Some(session_target) = raw.strip_prefix("session:") {
        let Some(tier) = session_target.strip_prefix("tier:") else {
            bail!("unknown session target {raw:?}; expected session:tier:fast, session:tier:default, or session:tier:heavy");
        };
        return Ok(PolicyTarget::SessionTier(parse_tier(tier)?));
    }
    if let Some(tier) = raw.strip_prefix("tier:") {
        return Ok(PolicyTarget::Tier(parse_tier(tier)?));
    }
    let (provider, model) = match raw.split_once(':') {
        Some((provider, model)) => (provider, Some(model.to_owned())),
        None => (raw, None),
    };
    let provider = provider_from_id(provider)?;
    Ok(PolicyTarget::Concrete { provider, model })
}

fn class_id(class: TaskClass) -> &'static str {
    match class {
        TaskClass::Routine => "routine",
        TaskClass::General => "general",
        TaskClass::Demanding => "demanding",
    }
}

// ---------- store ----------

struct CachedPolicy {
    /// Distinguishes "initialized with the shipped default" from "actually
    /// read the path once" — a missing file and a virgin cache share
    /// `mtime: None`, but only the latter should trigger the first-use
    /// materialization write.
    loaded: bool,
    mtime: Option<SystemTime>,
    policy: Arc<RoutePolicy>,
}

/// The daemon's hot-reloading view of the policy file.
pub struct PolicyStore {
    path: PathBuf,
    cached: Mutex<CachedPolicy>,
}

impl PolicyStore {
    pub fn open(path: PathBuf) -> Self {
        Self {
            path,
            cached: Mutex::new(CachedPolicy {
                loaded: false,
                mtime: None,
                policy: Arc::new(shipped_default_policy()),
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The current policy: the user's document when it parses, the shipped
    /// default otherwise. Reloads whenever the file's mtime changes.
    pub fn get(&self) -> Arc<RoutePolicy> {
        let mtime = std::fs::metadata(&self.path)
            .and_then(|metadata| metadata.modified())
            .ok();
        {
            let cached = self.cached.lock();
            if cached.loaded && cached.mtime == mtime {
                return cached.policy.clone();
            }
        }
        let policy = match self.load_document(&mtime) {
            Ok(policy) => policy,
            Err(error) => {
                eprintln!(
                    "Goddard: invalid routing policy at {}: {error:#} — using the shipped default",
                    self.path.display()
                );
                shipped_default_policy()
            }
        };
        let policy = Arc::new(policy);
        *self.cached.lock() = CachedPolicy {
            loaded: true,
            mtime,
            policy: policy.clone(),
        };
        policy
    }

    fn load_document(&self, mtime: &Option<SystemTime>) -> anyhow::Result<RoutePolicy> {
        match std::fs::read_to_string(&self.path) {
            Ok(source) => parse_policy(&source),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                // First use: materialize the shipped default so the user has a
                // real file to edit. A failed write is harmless — the default
                // is already in memory.
                if mtime.is_none() {
                    let _ = write_atomic(&self.path, DEFAULT_POLICY_JSON);
                }
                Ok(shipped_default_policy())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Update one class-level target in the user's document, preserving every
    /// other key. Rejects an invalid target without touching the file.
    pub fn set_class_target(&self, class: TaskClass, target: &str) -> anyhow::Result<()> {
        parse_target(target).with_context(|| format!("invalid target {target:?}"))?;
        let mut document: serde_json::Value = match std::fs::read_to_string(&self.path) {
            Ok(source) => serde_json::from_str(&source)
                .unwrap_or_else(|_| serde_json::from_str(DEFAULT_POLICY_JSON).unwrap_or_default()),
            Err(_) => serde_json::from_str(DEFAULT_POLICY_JSON).unwrap_or_default(),
        };
        document
            .as_object_mut()
            .context("policy document is not an object")?
            .entry("classes")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("policy classes is not an object")?
            .insert(
                class_id(class).to_owned(),
                serde_json::Value::String(target.to_owned()),
            );
        // Reject the write if the edited document would no longer validate —
        // a broken policy file silently degrades to the shipped default, so
        // refuse to create one.
        parse_policy(&serde_json::to_string_pretty(&document)?)?;
        write_atomic(&self.path, &serde_json::to_string_pretty(&document)?)
    }
}

fn write_atomic(path: &Path, contents: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, contents)?;
    std::fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_default_parses() {
        let policy = shipped_default_policy();
        assert!(policy.is_default);
        assert_eq!(policy.default, DefaultRoute::LastUsed);
        assert_eq!(
            policy.classes[&TaskClass::Routine],
            PolicyTarget::SessionTier(Tier::Fast)
        );
        assert_eq!(
            policy.classes[&TaskClass::Demanding],
            PolicyTarget::SessionTier(Tier::Heavy)
        );
        assert_eq!(
            policy.tiers[&ProviderKind::Claude][&Tier::Fast].model,
            "claude-haiku-4-5"
        );
        assert_eq!(
            policy.family_overrides[&TaskFamily::AgenticToolUse].min_class,
            Some(TaskClass::General)
        );
    }

    #[test]
    fn concrete_and_tier_targets_parse() {
        assert_eq!(
            parse_target("claude:claude-opus-5").unwrap(),
            PolicyTarget::Concrete {
                provider: ProviderKind::Claude,
                model: Some("claude-opus-5".into()),
            }
        );
        assert_eq!(
            parse_target("codex").unwrap(),
            PolicyTarget::Concrete {
                provider: ProviderKind::Codex,
                model: None,
            }
        );
        assert_eq!(
            parse_target("tier:heavy").unwrap(),
            PolicyTarget::Tier(Tier::Heavy)
        );
        assert_eq!(
            parse_target("session:tier:fast").unwrap(),
            PolicyTarget::SessionTier(Tier::Fast)
        );
        assert!(parse_target("session:tier:ludicrous").is_err());
        assert!(parse_target("session:claude").is_err());
        assert!(parse_target("tier:ludicrous").is_err());
        assert!(parse_target("notaprovider:x").is_err());
    }

    #[test]
    fn tier_values_accept_an_optional_effort_object() {
        let policy = parse_policy(
            r#"{"version": 1,
                "classes": {"routine": "tier:fast", "general": "tier:default", "demanding": "tier:heavy"},
                "tiers": {"claude": {
                    "fast": "claude-haiku-4-5",
                    "heavy": {"model": "claude-opus-5", "effort": "high"}
                }}}"#,
        )
        .unwrap();
        let table = &policy.tiers[&ProviderKind::Claude];
        assert_eq!(table[&Tier::Fast].model, "claude-haiku-4-5");
        assert_eq!(table[&Tier::Fast].effort, None);
        assert_eq!(table[&Tier::Heavy].model, "claude-opus-5");
        assert_eq!(table[&Tier::Heavy].effort.as_deref(), Some("high"));
    }

    #[test]
    fn invalid_documents_are_rejected() {
        assert!(parse_policy(r#"{"version": 2}"#).is_err());
        assert!(parse_policy(r#"{"version": 1, "classes": {"routine": "tier:fast"}}"#).is_err());
        assert!(parse_policy(
            r#"{"version": 1, "minClassConfidence": 4, "classes": {"routine":"tier:fast","general":"tier:default","demanding":"tier:heavy"}}"#
        )
        .is_err());
        assert!(parse_policy(
            r#"{"version": 1, "familyOverrides": {"nope": {}}, "classes": {"routine":"tier:fast","general":"tier:default","demanding":"tier:heavy"}}"#
        )
        .is_err());
    }

    #[test]
    fn set_class_target_round_trips_and_validates() {
        let directory =
            std::env::temp_dir().join(format!("goddard-policy-{}", uuid::Uuid::new_v4()));
        let path = directory.join("route-policy.json");
        let store = PolicyStore::open(path.clone());
        // First read materializes the shipped default file.
        let _ = store.get();
        assert!(path.exists());

        store
            .set_class_target(TaskClass::Demanding, "codex:gpt-5.6-terra")
            .unwrap();
        let policy = store.get();
        assert!(!policy.is_default);
        assert_eq!(
            policy.classes[&TaskClass::Demanding],
            PolicyTarget::Concrete {
                provider: ProviderKind::Codex,
                model: Some("gpt-5.6-terra".into()),
            }
        );
        // Untouched keys survive the edit.
        assert_eq!(
            policy.classes[&TaskClass::Routine],
            PolicyTarget::SessionTier(Tier::Fast)
        );

        assert!(
            store
                .set_class_target(TaskClass::Routine, "bogus:model")
                .is_err()
        );
        std::fs::remove_dir_all(directory).ok();
    }
}
