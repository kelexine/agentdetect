//! OpenTelemetry emission for agent detections.
//!
//! This module is gated behind the `otel` feature.  It does NOT pin you to
//! a specific OTel SDK — it talks to the `opentelemetry` API crate, so any
//! SDK (OTLP, Jaeger, Prometheus, stdout, …) works as a downstream exporter.
//!
//! # What gets emitted
//!
//! ## Span attributes ([`enrich_span`])
//!
//! Attach a uniform set of attributes to the current span so backend UIs
//! (Tempo, Jaeger, Honeycomb, Datadog APM, …) can filter and group traces
//! by agent identity:
//!
//! | Attribute | Value | Example |
//! |-----------|-------|---------|
//! | `agent.id`        | canonical harness ID       | `"claude-code"` |
//! | `agent.label`     | human-readable label       | `"Claude Code"` |
//! | `agent.family`    | vendor family              | `"anthropic"` |
//! | `agent.version`   | version string (if known)  | `"1.0.7"` |
//! | `agentdetect.confidence`   | `high` / `medium` / `low` | `"high"` |
//! | `agentdetect.source.kind`  | `env-var` / `http-header` | `"env-var"` |
//! | `agentdetect.source.name`  | primary signal name       | `"CLAUDE_CODE"` |
//!
//! ## Metrics ([`record_request`] / [`record_detection`])
//!
//! Three instruments, all owned by a single meter named `"agentdetect"`:
//!
//! | Instrument | Kind | Labels | Answers |
//! |------------|------|--------|---------|
//! | `agentdetect.requests.total`     | Counter   | `agent_id`, `agent_family`, `status_class` | "how many requests per minute from agent X?" |
//! | `agentdetect.request.duration`   | Histogram | `agent_id`, `agent_family`                 | "p99 latency per agent?" |
//! | `agentdetect.detections.total`   | Counter   | `agent_id`, `confidence`, `source_kind`    | "% of traffic detected as agent X?" |
//!
//! `status_class` is one of `"2xx"`, `"4xx"`, `"5xx"` (computed from the HTTP
//! status code) so dashboards can compute success rate per agent without
//! exploding cardinality.
//!
//! # Usage
//!
//! ```
//! # #[cfg(feature = "otel")] {
//! # use agentdetect::{detect_from_env, otel};
//! # fn handle() {
//! # if let Some(d) = detect_from_env() {
//! otel::enrich_span(&d);
//! otel::record_detection(&d);
//! # }
//! # }
//! # }
//! ```

use std::time::Duration;

use opentelemetry::{
    KeyValue,
    global::meter_with_scope,
    metrics::{Counter, Histogram, Meter},
};

use crate::detection::Detection;

// ─── Attribute key constants ──────────────────────────────────────────────────

/// Public attribute key constants.  Exported so consumers can build their
/// own queries / dashboards against the same names without copy-paste drift.
pub mod attrs {
    /// Canonical harness ID (e.g. `"claude-code"`).
    pub const AGENT_ID: &str = "agent.id";
    /// Human-readable label (e.g. `"Claude Code"`).
    pub const AGENT_LABEL: &str = "agent.label";
    /// Vendor family (e.g. `"anthropic"`).
    pub const AGENT_FAMILY: &str = "agent.family";
    /// Version string, when known.
    pub const AGENT_VERSION: &str = "agent.version";
    /// Detection confidence: `"high"` / `"medium"` / `"low"`.
    pub const CONFIDENCE: &str = "agentdetect.confidence";
    /// Source surface: `"env-var"` / `"http-header"`.
    pub const SOURCE_KIND: &str = "agentdetect.source.kind";
    /// Name of the primary signal (env var name or header name).
    pub const SOURCE_NAME: &str = "agentdetect.source.name";
}

/// Metric name constants.
pub mod metrics {
    /// Counter: total requests detected as agent X with HTTP status class Y.
    pub const REQUESTS_TOTAL: &str = "agentdetect.requests.total";
    /// Histogram: request duration, broken down by agent.
    pub const REQUEST_DURATION: &str = "agentdetect.request.duration";
    /// Counter: total detections of agent X via source Y at confidence Z.
    pub const DETECTIONS_TOTAL: &str = "agentdetect.detections.total";
}

// ─── Attribute builders ───────────────────────────────────────────────────────

/// Build the standard set of agent-detection attributes for a [`Detection`].
///
/// Use this when you want to attach the attributes yourself (e.g. to a span
/// from a non-OpenTelemetry framework, or to a log record).
pub fn attributes_for(d: &Detection) -> Vec<KeyValue> {
    let mut out = Vec::with_capacity(7);
    out.push(KeyValue::new(attrs::AGENT_ID, d.agent.id.to_string()));
    out.push(KeyValue::new(
        attrs::AGENT_LABEL,
        d.agent.pretty_label.to_string(),
    ));
    out.push(KeyValue::new(
        attrs::AGENT_FAMILY,
        d.family_id().to_string(),
    ));
    if let Some(v) = &d.agent.version {
        out.push(KeyValue::new(attrs::AGENT_VERSION, v.clone()));
    }
    out.push(KeyValue::new(
        attrs::CONFIDENCE,
        d.confidence.id().to_string(),
    ));
    out.push(KeyValue::new(
        attrs::SOURCE_KIND,
        d.source_kind().id().to_string(),
    ));
    out.push(KeyValue::new(
        attrs::SOURCE_NAME,
        d.primary_signal.signal_name().to_string(),
    ));
    out
}

/// Convenience: just the dimension attributes used for metric labels.
///
/// Returns `agent_id`, `agent_family`, `confidence`, `source_kind` — the
/// minimum set needed to slice traffic in dashboards.  Span-only attributes
/// like `agent_version` are omitted to keep metric cardinality bounded.
pub fn metric_labels_for(d: &Detection) -> Vec<KeyValue> {
    vec![
        KeyValue::new("agent_id", d.agent.id.to_string()),
        KeyValue::new("agent_family", d.family_id().to_string()),
        KeyValue::new("confidence", d.confidence.id().to_string()),
        KeyValue::new("source_kind", d.source_kind().id().to_string()),
    ]
}

// ─── Status class helper ──────────────────────────────────────────────────────

/// Collapse an HTTP status code into a low-cardinality class string.
///
/// Returns `"2xx"`, `"4xx"`, `"5xx"`, etc.  Unknown / non-HTTP status codes
/// return `"other"`.
pub fn status_class(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "other",
    }
}

// ─── Span enrichment ──────────────────────────────────────────────────────────

/// Attach the standard agent-detection attributes to the current active span.
///
/// No-op when there is no active span (the call is cheap and safe to use in
/// hot paths).
pub fn enrich_span(d: &Detection) {
    use opentelemetry::Context;
    use opentelemetry::trace::TraceContextExt;

    let cx = Context::current();
    let span = cx.span();
    if !span.is_recording() {
        return;
    }
    for kv in attributes_for(d) {
        span.set_attribute(kv);
    }
}

// ─── Metric instruments ───────────────────────────────────────────────────────

/// Owned metric instruments.
///
/// Cheap to clone (each instrument is `Arc`-backed internally).  Cloning is
/// the recommended pattern: keep one [`Instruments`] per long-lived owner
/// (e.g. axum `State`), clone into per-request contexts as needed.
#[derive(Clone)]
pub struct Instruments {
    requests_total: Counter<u64>,
    request_duration: Histogram<f64>,
    detections_total: Counter<u64>,
}

impl Instruments {
    /// Build a fresh set of instruments from a custom [`Meter`].
    ///
    /// Use [`Instruments::global`] if you just want the global meter.
    pub fn from_meter(meter: &Meter) -> Self {
        Self {
            requests_total: meter
                .u64_counter(metrics::REQUESTS_TOTAL)
                .with_description(
                    "Total incoming requests detected as coming from a specific AI agent harness, \
                     broken down by HTTP status class. Unit: {request}.",
                )
                .build(),
            request_duration: meter
                .f64_histogram(metrics::REQUEST_DURATION)
                .with_description(
                    "Latency (seconds) of incoming requests detected as coming from a specific AI agent harness.",
                )
                .build(),
            detections_total: meter
                .u64_counter(metrics::DETECTIONS_TOTAL)
                .with_description(
                    "Total agent detections by harness ID, confidence, and source surface. \
                     Use this to compute the % of traffic attributed to each agent. Unit: {detection}.",
                )
                .build(),
        }
    }

    /// Build instruments from the global OTel meter, scoped to a fixed
    /// instrumentation scope (`agentdetect` v`CARGO_PKG_VERSION`).
    pub fn global() -> Self {
        let scope = opentelemetry::InstrumentationScope::builder("agentdetect")
            .with_version(env!("CARGO_PKG_VERSION"))
            .build();
        let meter = meter_with_scope(scope);
        Self::from_meter(&meter)
    }

    /// Record a single completed request: bumps [`metrics::REQUESTS_TOTAL`]
    /// with `agent_id`/`agent_family`/`status_class` labels, and records the
    /// duration on [`metrics::REQUEST_DURATION`].
    pub fn record_request(&self, d: &Detection, http_status: u16, duration: Duration) {
        let labels = vec![
            KeyValue::new("agent_id", d.agent.id.to_string()),
            KeyValue::new("agent_family", d.family_id().to_string()),
            KeyValue::new("status_class", status_class(http_status).to_string()),
        ];
        self.requests_total.add(1, &labels);
        self.request_duration
            .record(duration.as_secs_f64(), &labels);
    }

    /// Record a detection event: bumps [`metrics::DETECTIONS_TOTAL`] with the
    /// full label set (agent_id, agent_family, confidence, source_kind).
    ///
    /// Useful when you're not in a request context (e.g. a one-shot CLI run
    /// that wants to log which agent was detected) but still want the
    /// detection counted.
    pub fn record_detection(&self, d: &Detection) {
        self.detections_total.add(1, &metric_labels_for(d));
    }
}

// Lazy-init globals so callers who don't want to thread `Instruments` through
// their state can still call [`record_request`] / [`record_detection`].
//
// We use `once_cell`-free init via `std::sync::OnceLock` (stable since 1.70).
static GLOBAL_INSTRUMENTS: std::sync::OnceLock<Instruments> = std::sync::OnceLock::new();

fn global_instruments() -> &'static Instruments {
    GLOBAL_INSTRUMENTS.get_or_init(Instruments::global)
}

/// Convenience: record a request using the global [`Instruments`].
pub fn record_request(d: &Detection, http_status: u16, duration: Duration) {
    global_instruments().record_request(d, http_status, duration);
}

/// Convenience: record a detection using the global [`Instruments`].
pub fn record_detection(d: &Detection) {
    global_instruments().record_detection(d);
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::EnvSource;
    use std::collections::HashMap;

    /// FakeEnv duplicated from detect.rs tests so this module is self-contained.
    #[derive(Default)]
    struct FakeEnv(HashMap<&'static str, String>);
    impl FakeEnv {
        fn set(mut self, k: &'static str, v: impl Into<String>) -> Self {
            self.0.insert(k, v.into());
            self
        }
    }
    impl EnvSource for FakeEnv {
        fn get(&self, name: &str) -> Option<String> {
            self.0
                .iter()
                .find_map(|(k, v)| (*k == name).then_some(v.clone()))
        }
    }

    fn sample_detection() -> Detection {
        let env = FakeEnv::default().set("CLAUDE_CODE", "1");
        crate::detect_from_env_with(&env).expect("detection should fire")
    }

    #[test]
    fn attributes_for_includes_all_required_keys() {
        let d = sample_detection();
        let attrs = attributes_for(&d);
        let keys: Vec<&str> = attrs.iter().map(|kv| kv.key.as_str()).collect();
        assert!(keys.contains(&attrs::AGENT_ID));
        assert!(keys.contains(&attrs::AGENT_LABEL));
        assert!(keys.contains(&attrs::AGENT_FAMILY));
        assert!(keys.contains(&attrs::CONFIDENCE));
        assert!(keys.contains(&attrs::SOURCE_KIND));
        assert!(keys.contains(&attrs::SOURCE_NAME));
        // VERSION is omitted when not present.
        assert!(!keys.contains(&attrs::AGENT_VERSION));
    }

    #[test]
    fn metric_labels_are_low_cardinality() {
        let d = sample_detection();
        let labels = metric_labels_for(&d);
        let keys: Vec<&str> = labels.iter().map(|kv| kv.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["agent_id", "agent_family", "confidence", "source_kind"]
        );
    }

    #[test]
    fn status_class_buckets_correctly() {
        assert_eq!(status_class(200), "2xx");
        assert_eq!(status_class(204), "2xx");
        assert_eq!(status_class(301), "3xx");
        assert_eq!(status_class(401), "4xx");
        assert_eq!(status_class(500), "5xx");
        assert_eq!(status_class(0), "other");
    }

    #[test]
    fn enrich_span_does_not_panic_without_active_span() {
        // No global tracer provider is set up — enrich_span must be a no-op.
        let d = sample_detection();
        enrich_span(&d); // should not panic
    }

    #[test]
    fn instruments_global_initialises_lazily() {
        // Initialising the global instrument set without a registered
        // provider should still work — OTel's noop backend accepts it.
        let _i = Instruments::global();
        let d = sample_detection();
        // These calls must not panic even with no exporter configured.
        record_detection(&d);
        record_request(&d, 200, Duration::from_millis(42));
    }

    #[test]
    fn attrs_constants_match_doc_table() {
        assert_eq!(attrs::AGENT_ID, "agent.id");
        assert_eq!(attrs::AGENT_LABEL, "agent.label");
        assert_eq!(attrs::AGENT_FAMILY, "agent.family");
        assert_eq!(attrs::AGENT_VERSION, "agent.version");
        assert_eq!(attrs::CONFIDENCE, "agentdetect.confidence");
        assert_eq!(attrs::SOURCE_KIND, "agentdetect.source.kind");
        assert_eq!(attrs::SOURCE_NAME, "agentdetect.source.name");
    }

    #[test]
    fn metrics_constants_match_doc_table() {
        assert_eq!(metrics::REQUESTS_TOTAL, "agentdetect.requests.total");
        assert_eq!(metrics::REQUEST_DURATION, "agentdetect.request.duration");
        assert_eq!(metrics::DETECTIONS_TOTAL, "agentdetect.detections.total");
    }
}
