//! Hosted evaluation-model client (TypeSafe's Jev): one `evaluate` call posts
//! a shared `state` plus typed questions to the configured backend and returns
//! calibrated answers — choices, scores, and boolean probabilities rather than
//! generated text. TypeSafe's own API, the Vercel AI Gateway, and Cloudflare
//! Workers AI all answer the same envelope; only the request envelope and
//! credentials differ.
//!
//! Every call also appends to a daemon-owned JSONL decision log. The log is
//! the calibration dataset for every eval-driven feature — routing decisions,
//! their confidences, and what happened next.
//!
//! Everything here blocks on a subprocess and the network; callers must
//! already be off the request thread's latency budget. Credentials travel in
//! a 0600 curl config file rather than argv, so they are not visible to
//! `ps` — the same posture `usage.rs` takes for provider tokens.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use serde::Serialize;
use serde_json::{Value, json};
use waku_protocol::eval::{
    EvalAnswer, EvalBackend, EvalQuestion, EvalSettings, EvalUsage, EvalUsageStats,
    EvalUsageTotals, Evaluation,
};

const TYPESAFE_URL: &str = "https://api.typesafe.ai/v1/systemone";
const VERCEL_EVALUATION_URL: &str = "https://ai-gateway.vercel.sh/v4/ai/evaluation-model";
const CLOUDFLARE_RUN_URL: &str = "https://api.cloudflare.com/client/v4/accounts";
const GATEWAY_MODEL_ID: &str = "typesafe-ai/jev";
const JEV_MODEL_ALIAS: &str = "jev-latest";

/// The eval call's share of a user action's latency budget; callers degrade to
/// their default route when it elapses.
pub const EVAL_TIMEOUT_SECS: u64 = 5;
/// Answer envelopes are a few hundred bytes; far past that the response is
/// not an answer worth parsing.
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// Delimiter `curl -w` appends after the body so the status code rides the
/// same stdout stream without multi-block header parsing.
const STATUS_MARKER: &str = "GODDARD_EVAL_STATUS:";

/// The absolute path keeps a shadowed `curl` on `PATH` out of the credential
/// exchange. Windows 10 build 17063 and later ship the same tool in System32.
#[cfg(not(windows))]
const CURL_PATH: &str = "/usr/bin/curl";
#[cfg(windows)]
const CURL_PATH: &str = r"C:\Windows\System32\curl.exe";

/// One line in the daemon's eval decision log. Eval-call fields and
/// routing-outcome fields are both optional so one record type covers
/// `evaluate`, `route`, and `route-override` events.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalDecisionRecord {
    /// Unix seconds when the call was made.
    pub ts: u64,
    /// Which eval-driven feature made the call.
    pub feature: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<EvalBackend>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    /// The versioned model id the backend reported, when it answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Backend-reported token usage. Absent on failed calls and on records
    /// written before usage was tracked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<EvalUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub questions: Option<BTreeMap<String, EvalQuestion>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answers: Option<BTreeMap<String, EvalAnswer>>,
    /// The failure summary when the call did not produce answers. Backend
    /// error bodies are never recorded — they can echo prompts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The session a routing outcome belongs to, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<uuid::Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_provider: Option<waku_protocol::model::ProviderKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_effort: Option<String>,
    /// The deterministic reason chain that produced the route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
}

impl EvalDecisionRecord {
    /// A record with every optional field unset; callers fill in what their
    /// feature produced.
    pub fn empty(feature: &'static str) -> Self {
        Self {
            ts: crate::model::unix_time(),
            feature: feature.to_owned(),
            backend: None,
            latency_ms: None,
            model: None,
            usage: None,
            state: None,
            questions: None,
            answers: None,
            error: None,
            session_id: None,
            resolved_provider: None,
            resolved_model: None,
            resolved_effort: None,
            reason: None,
            class: None,
        }
    }

    /// Fold a routing decision's outcome into the record.
    pub fn complete(&mut self, decision: &waku_protocol::routing::RouteDecision) {
        self.resolved_provider = Some(decision.target.provider);
        self.resolved_model = decision.target.model.clone();
        self.resolved_effort = decision.target.effort.clone();
        self.reason = Some(decision.reason.clone());
        self.class = decision.class.map(|class| class.id().to_owned());
        if self.latency_ms.is_none() {
            self.latency_ms = decision.eval_latency_ms;
        }
    }
}

/// Where the decision log lives: beside the daemon's `settings.json`.
pub fn default_log_path() -> PathBuf {
    waku_protocol::settings::DaemonSettings::default_path()
        .parent()
        .map(|dir| dir.join("eval-decisions.jsonl"))
        .unwrap_or_else(|| PathBuf::from("eval-decisions.jsonl"))
}

/// Append one record as a JSON line. Logging is best-effort: a write failure
/// must never fail the feature that produced the decision.
pub fn append_decision_log(path: &Path, record: &EvalDecisionRecord) {
    let _ = (|| -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut line = serde_json::to_vec(record)?;
        line.push(b'\n');
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(&line)?;
        Ok(())
    })();
}

/// The fields a usage scan reads out of each log line. Everything else —
/// the state payload, questions, answers — is skipped, so the scan stays
/// proportional to line count rather than record size.
#[derive(serde::Deserialize)]
struct UsageLogLine {
    #[serde(default)]
    feature: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    usage: Option<EvalUsage>,
}

/// Sum token usage across the decision log for the settings pane. A record
/// counts as an eval call when it carries the marks of an attempted call —
/// usage, a model id, or an error — so `route-override` outcome records and
/// fallback routes that never reached a backend stay out of the totals.
/// Malformed lines are skipped: the log is append-only and a torn tail line
/// must not blank the whole readout.
pub fn usage_stats(path: &Path) -> EvalUsageStats {
    let mut stats = EvalUsageStats::default();
    let Ok(contents) = std::fs::read(path) else {
        return stats;
    };
    for line in contents.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<UsageLogLine>(line) else {
            continue;
        };
        if record.usage.is_none() && record.model.is_none() && record.error.is_none() {
            continue;
        }
        let fold = |totals: &mut EvalUsageTotals| {
            totals.calls += 1;
            if let Some(usage) = &record.usage {
                totals.calls_with_usage += 1;
                totals.input_tokens += usage.input_tokens;
                totals.output_tokens += usage.output_tokens;
            }
        };
        fold(&mut stats.totals);
        fold(stats.features.entry(record.feature).or_default());
    }
    stats
}

/// Run one evaluation against the configured backend, on the default
/// latency budget shared by latency-bound callers. Blocking.
pub fn evaluate(
    settings: &EvalSettings,
    state: &Value,
    questions: &BTreeMap<String, EvalQuestion>,
) -> anyhow::Result<Evaluation> {
    evaluate_with_timeout(settings, state, questions, EVAL_TIMEOUT_SECS)
}

/// Run one evaluation with an explicit latency budget. Long-context callers
/// — provider-switch compaction asks one question per transcript item —
/// need more than the routing share. Blocking.
pub fn evaluate_with_timeout(
    settings: &EvalSettings,
    state: &Value,
    questions: &BTreeMap<String, EvalQuestion>,
    timeout_secs: u64,
) -> anyhow::Result<Evaluation> {
    let (url, headers, body) = backend_request(settings, state, questions)?;
    let started = Instant::now();
    let (status, raw) = curl_post_json(&url, &headers, &body, timeout_secs)?;
    let latency_ms = started.elapsed().as_millis() as u64;
    if !(200..300).contains(&status) {
        // Provider error bodies can echo the request — including the prompt —
        // so the failure carries the status and nothing else.
        return Err(anyhow!(EvaluationHttpStatus(status)));
    }
    if raw.len() > MAX_RESPONSE_BYTES {
        bail!("evaluation response exceeded {} bytes", MAX_RESPONSE_BYTES);
    }
    let parsed: Value =
        serde_json::from_slice(&raw).context("evaluation backend returned invalid JSON")?;
    // Cloudflare wraps the model output in `result`; the other backends answer
    // the envelope directly.
    let mut envelope = parsed.get("result").unwrap_or(&parsed).clone();
    if settings.backend == EvalBackend::VercelGateway {
        translate_vercel_envelope(&mut envelope);
    }
    let mut evaluation: Evaluation =
        serde_json::from_value(envelope).context("invalid evaluation answer envelope")?;
    evaluation.latency_ms = latency_ms;
    Ok(evaluation)
}

#[derive(Debug)]
struct EvaluationHttpStatus(u16);

impl std::fmt::Display for EvaluationHttpStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "evaluation backend answered HTTP {}", self.0)
    }
}

impl std::error::Error for EvaluationHttpStatus {}

/// Retry one service-unavailable response without exceeding the caller's
/// total timeout budget. Provider-switch is the only caller that opts in.
pub(crate) fn evaluate_with_timeout_retry_503(
    settings: &EvalSettings,
    state: &Value,
    questions: &BTreeMap<String, EvalQuestion>,
    timeout_secs: u64,
) -> anyhow::Result<Evaluation> {
    const RETRY_BACKOFF: Duration = Duration::from_millis(250);

    let started = Instant::now();
    let error = match evaluate_with_timeout(settings, state, questions, timeout_secs) {
        Ok(evaluation) => return Ok(evaluation),
        Err(error) => error,
    };
    if !error
        .downcast_ref::<EvaluationHttpStatus>()
        .is_some_and(|status| status.0 == 503)
    {
        return Err(error);
    }

    let budget = Duration::from_secs(timeout_secs);
    if budget.saturating_sub(started.elapsed()) <= RETRY_BACKOFF {
        return Err(error);
    }
    std::thread::sleep(RETRY_BACKOFF);
    let retry_timeout_secs = budget.saturating_sub(started.elapsed()).as_secs();
    if retry_timeout_secs == 0 {
        return Err(error);
    }

    evaluate_with_timeout(settings, state, questions, retry_timeout_secs)
}

/// The settings pane's "Test connection" check: one trivial question against
/// a fixed state. Enough to verify credentials, headers, and the answer
/// envelope — a bare HTTP 200 that skipped envelope parsing could pass on a
/// broken contract. Deliberately writes no decision log record.
pub fn probe(settings: &EvalSettings) -> anyhow::Result<Evaluation> {
    let questions = BTreeMap::from([(
        "connection".to_owned(),
        EvalQuestion::Noul {
            instructions: "Is this a connection check?".to_owned(),
            criteria: None,
        },
    )]);
    evaluate(settings, &json!({"task": "connection-check"}), &questions)
}

fn backend_request(
    settings: &EvalSettings,
    state: &Value,
    questions: &BTreeMap<String, EvalQuestion>,
) -> anyhow::Result<(String, Vec<String>, Vec<u8>)> {
    match settings.backend {
        EvalBackend::TypeSafe => {
            let key = required(&settings.typesafe_api_key, "TypeSafe API key")?;
            let body = json!({
                "model": JEV_MODEL_ALIAS,
                "state": state,
                "questions": questions,
            });
            Ok((
                TYPESAFE_URL.to_owned(),
                bearer_headers(key),
                serde_json::to_vec(&body)?,
            ))
        }
        EvalBackend::VercelGateway => {
            let key = required(&settings.vercel_api_key, "Vercel AI Gateway credential")?;
            let mut headers = bearer_headers(key);
            // The evaluation endpoint's wire contract is versioned separately
            // from the rest of the Gateway — the AI SDK sends these exact
            // headers for evaluation models.
            headers.push("ai-gateway-protocol-version: 0.0.1".to_owned());
            headers.push("ai-evaluation-model-specification-version: 4".to_owned());
            headers.push(format!("ai-model-id: {GATEWAY_MODEL_ID}"));
            if let Some(team) = settings
                .vercel_team_id
                .as_deref()
                .filter(|team| !team.is_empty())
            {
                headers.push(format!("x-vercel-ai-gateway-team: {team}"));
            }
            // The spec's question discriminator is `choice | score | boolean`;
            // TypeSafe's `noul` must be renamed on the way out (and back on
            // the answer side — see `translate_vercel_envelope`).
            let mut questions_json = serde_json::to_value(questions)?;
            if let Some(map) = questions_json.as_object_mut() {
                for question in map.values_mut() {
                    if question.get("type").and_then(Value::as_str) == Some("noul") {
                        question["type"] = json!("boolean");
                    }
                }
            }
            let body = json!({
                "state": state,
                "questions": questions_json,
            });
            Ok((
                VERCEL_EVALUATION_URL.to_owned(),
                headers,
                serde_json::to_vec(&body)?,
            ))
        }
        EvalBackend::Cloudflare => {
            let account = required(&settings.cloudflare_account_id, "Cloudflare account id")?;
            let token = required(&settings.cloudflare_api_token, "Cloudflare API token")?;
            let body = json!({
                "model": GATEWAY_MODEL_ID.replace("-ai/", "/"),
                "input": {
                    "state": state,
                    "questions": questions,
                },
            });
            Ok((
                format!("{CLOUDFLARE_RUN_URL}/{account}/ai/run"),
                bearer_headers(token),
                serde_json::to_vec(&body)?,
            ))
        }
    }
}

/// Normalize a Vercel answer envelope into the shared `Evaluation` shape:
/// boolean answers carry `probability` instead of `noul`, and the answering
/// model id lives under `providerMetadata.gateway.routing.canonicalSlug`
/// rather than a top-level `model` field.
fn translate_vercel_envelope(envelope: &mut Value) {
    if let Some(answers) = envelope.get_mut("answers").and_then(Value::as_object_mut) {
        for answer in answers.values_mut() {
            if answer.get("type").and_then(Value::as_str) == Some("boolean") {
                let probability = answer.get("probability").cloned().unwrap_or(Value::Null);
                *answer = json!({ "type": "noul", "noul": probability });
            }
        }
    }
    if envelope.get("model").is_none() {
        let model = envelope
            .pointer("/providerMetadata/gateway/routing/canonicalSlug")
            .and_then(Value::as_str)
            .unwrap_or(GATEWAY_MODEL_ID)
            .to_owned();
        if let Some(map) = envelope.as_object_mut() {
            map.insert("model".to_owned(), json!(model));
        }
    }
}

fn required<'a>(value: &'a Option<String>, what: &str) -> anyhow::Result<&'a str> {
    value
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("{what} is not configured"))
}

fn bearer_headers(credential: &str) -> Vec<String> {
    vec![
        format!("Authorization: Bearer {credential}"),
        "Content-Type: application/json".to_owned(),
        "Accept: application/json".to_owned(),
    ]
}

/// POST a JSON body through the system curl and return the HTTP status and
/// raw body. Headers ride a 0600 config file so credentials never appear in
/// argv; the body travels stdin unchanged (curl config files would interpret
/// JSON's `\t`/`\n` escapes literally).
fn curl_post_json(
    url: &str,
    headers: &[String],
    body: &[u8],
    timeout_secs: u64,
) -> anyhow::Result<(u16, Vec<u8>)> {
    let config = write_curl_config(url, headers)?;
    let mut child = crate::command_env::plain_command(CURL_PATH)
        .args([
            "-sS",
            "--max-time",
            &timeout_secs.to_string(),
            "-K",
            &config.path,
            "--data-binary",
            "@-",
            "-w",
            &format!("\n{STATUS_MARKER}%{{http_code}}"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run curl")?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(body);
    }
    let output = child
        .wait_with_output()
        .context("curl did not finish the evaluation request")?;
    let _ = std::fs::remove_file(&config.path);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error = stderr
            .lines()
            .last()
            .map(str::trim)
            .filter(|error| !error.is_empty())
            .unwrap_or("unknown curl error");
        bail!("evaluation request failed: {error}");
    }
    split_status_and_body(&output.stdout)
}

struct CurlConfig {
    path: String,
}

/// One temp file per request, written 0600 and deleted once curl exits.
fn write_curl_config(url: &str, headers: &[String]) -> anyhow::Result<CurlConfig> {
    let mut contents = String::new();
    for header in headers {
        contents.push_str(&format!("header = \"{}\"\n", escape_config_value(header)));
    }
    contents.push_str(&format!("url = \"{}\"\n", escape_config_value(url)));
    let path = std::env::temp_dir().join(format!("goddard-eval-{}.cfg", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .context("could not create the curl config file")?;
    file.write_all(contents.as_bytes())?;
    drop(file);
    Ok(CurlConfig {
        path: path.to_string_lossy().into_owned(),
    })
}

/// Inside a double-quoted curl config value, `\` and `"` are the only
/// characters that change meaning.
fn escape_config_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// stdout is the response body followed by the `-w` status marker.
fn split_status_and_body(raw: &[u8]) -> anyhow::Result<(u16, Vec<u8>)> {
    let marker = raw
        .windows(STATUS_MARKER.len())
        .rposition(|window| window == STATUS_MARKER.as_bytes())
        .context("evaluation response carried no status marker")?;
    let status = std::str::from_utf8(&raw[marker + STATUS_MARKER.len()..])
        .context("evaluation response carried an invalid status")?
        .trim()
        .parse::<u16>()
        .context("evaluation response carried an invalid status")?;
    // The marker is preceded by the newline the `-w` format prepends.
    Ok((status, raw[..marker.saturating_sub(1)].to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_reads_status_marker_after_body() {
        let (status, body) = split_status_and_body(b"{\"a\":1}\nGODDARD_EVAL_STATUS:200").unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"{\"a\":1}");
    }

    #[test]
    fn split_handles_body_that_ends_without_newline() {
        let (status, _) = split_status_and_body(b"[]\nGODDARD_EVAL_STATUS:429").unwrap();
        assert_eq!(status, 429);
    }

    #[test]
    fn config_escaping_protects_quoted_values() {
        assert_eq!(escape_config_value("a\"b\\c"), "a\\\"b\\\\c");
    }

    #[test]
    fn missing_credential_names_the_field() {
        let settings = EvalSettings {
            backend: EvalBackend::TypeSafe,
            ..Default::default()
        };
        let error = backend_request(&settings, &json!({"task": "x"}), &BTreeMap::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("TypeSafe API key"));
    }

    #[test]
    fn vercel_request_carries_evaluation_spec_headers() {
        let settings = EvalSettings {
            backend: EvalBackend::VercelGateway,
            vercel_api_key: Some("key".into()),
            vercel_team_id: Some("team_1".into()),
            ..Default::default()
        };
        let mut questions = BTreeMap::new();
        questions.insert(
            "planning".to_owned(),
            EvalQuestion::Noul {
                instructions: "needs a plan?".into(),
                criteria: None,
            },
        );
        let (url, headers, body) =
            backend_request(&settings, &json!({"task": "x"}), &questions).unwrap();
        assert_eq!(url, VERCEL_EVALUATION_URL);
        assert!(
            headers
                .iter()
                .any(|h| h == "ai-evaluation-model-specification-version: 4")
        );
        assert!(
            headers
                .iter()
                .any(|h| h == "x-vercel-ai-gateway-team: team_1")
        );
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["questions"]["planning"]["type"], "boolean");
    }

    #[test]
    fn vercel_envelope_maps_boolean_answers_and_model() {
        let mut envelope = json!({
            "answers": {
                "planning": { "type": "boolean", "probability": 0.17 },
                "class": {
                    "type": "choice",
                    "choice": "routine",
                    "probabilities": { "routine": 1.0 }
                }
            },
            "providerMetadata": {
                "gateway": { "routing": { "canonicalSlug": "typesafe-ai/jev" } }
            }
        });
        translate_vercel_envelope(&mut envelope);
        let evaluation: Evaluation = serde_json::from_value(envelope).unwrap();
        assert_eq!(evaluation.model, "typesafe-ai/jev");
        assert_eq!(
            evaluation.answers["planning"],
            EvalAnswer::Noul { noul: 0.17 }
        );
        assert!(matches!(
            evaluation.answers["class"],
            EvalAnswer::Choice { .. }
        ));
    }

    #[test]
    fn vercel_envelope_falls_back_to_gateway_model_id() {
        let mut envelope = json!({ "answers": {} });
        translate_vercel_envelope(&mut envelope);
        assert_eq!(envelope["model"], GATEWAY_MODEL_ID);
    }

    #[test]
    fn usage_parses_camel_and_snake_case() {
        // TypeSafe and Vercel report camelCase; Cloudflare's Workers AI
        // envelope reports snake_case.
        let camel: EvalUsage =
            serde_json::from_str(r#"{"inputTokens": 3, "outputTokens": 1}"#).unwrap();
        let snake: EvalUsage =
            serde_json::from_str(r#"{"input_tokens": 3, "output_tokens": 1}"#).unwrap();
        assert_eq!(camel, snake);
        assert_eq!(camel.input_tokens, 3);
        assert_eq!(snake.output_tokens, 1);
    }

    #[test]
    fn usage_stats_sums_reporting_calls_only() {
        let path =
            std::env::temp_dir().join(format!("goddard-eval-log-{}.jsonl", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            concat!(
                r#"{"feature":"route","model":"jev","usage":{"inputTokens":10,"outputTokens":2}}"#,
                "\n",
                // A call logged before usage tracking: counts, no tokens.
                r#"{"feature":"route","model":"jev"}"#,
                "\n",
                // An outcome record, not an eval call: skipped entirely.
                r#"{"feature":"route-override","resolvedModel":"x"}"#,
                "\n",
                // A torn tail line must not fail the scan.
                "{not json\n",
                // A failed call still counts as a call.
                r#"{"feature":"memory-rank","error":"boom"}"#,
                "\n",
            ),
        )
        .unwrap();
        let stats = usage_stats(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(stats.totals.calls, 3);
        assert_eq!(stats.totals.calls_with_usage, 1);
        assert_eq!(stats.totals.input_tokens, 10);
        assert_eq!(stats.totals.output_tokens, 2);
        assert_eq!(stats.features["route"].calls, 2);
        assert_eq!(stats.features["memory-rank"].calls, 1);
        assert!(!stats.features.contains_key("route-override"));
    }

    #[test]
    fn usage_stats_tolerates_a_missing_log() {
        let stats = usage_stats(Path::new("/nonexistent/eval-decisions.jsonl"));
        assert_eq!(stats.totals.calls, 0);
    }

    #[test]
    fn cloudflare_request_scopes_the_url_and_wraps_input() {
        let settings = EvalSettings {
            backend: EvalBackend::Cloudflare,
            cloudflare_account_id: Some("acct".into()),
            cloudflare_api_token: Some("tok".into()),
            ..Default::default()
        };
        let (url, _, body) =
            backend_request(&settings, &json!({"task": "x"}), &BTreeMap::new()).unwrap();
        assert!(url.ends_with("/accounts/acct/ai/run"));
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["model"], "typesafe/jev");
        assert!(body["input"]["questions"].is_object());
    }
}
