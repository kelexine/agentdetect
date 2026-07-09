//! # agentdetect-tower
//!
//! Tower middleware that reads the `x-agentdetect-*` propagation header
//! (written by a trusted CLI using [`agentdetect::propagation::inject`]) and
//! emits OpenTelemetry signals on the API side.
//!
//! ## What this is (and isn't)
//!
//! This middleware does **NOT** sniff `User-Agent` or any other third-party
//! header.  agentdetect's detection model is env-var-only — a tool like
//! `curl` running under an agent has no idea it's under an agent, so its
//! wire traffic carries no agent-identifying signal.
//!
//! The only way an agent identity reaches your API is if YOUR CLI (built
//! with agentdetect) detects the agent via env vars, then writes it onto
//! its outgoing request via [`agentdetect::propagation::inject`].  This
//! middleware reads that same header on the API side.
//!
//! ## What it does per request
//!
//! 1. Reads the `x-agentdetect-agent` / `x-agentdetect-confidence` /
//!    `x-agentdetect-version` headers via [`agentdetect::propagation::read`].
//! 2. Stores the reconstructed [`agentdetect::Detection`] in request
//!    extensions so downstream handlers can read it.
//! 3. Records `agentdetect.requests.total{agent_id, agent_family,
//!    status_class}` and `agentdetect.request.duration{agent_id,
//!    agent_family}` once the response is produced.
//! 4. Enriches the current OTel span with agent identity attributes
//!    (`agent.id`, `agent.label`, `agent.family`, …) when the `otel`
//!    feature is enabled.
//!
//! # Example
//!
//! ```no_run
//! use agentdetect_tower::AgentDetectLayer;
//! use tower::ServiceBuilder;
//! # use std::convert::Infallible;
//! # use http::{Request, Response};
//! # use bytes::Bytes;
//! # async fn handler(req: Request<Bytes>) -> Result<Response<Bytes>, Infallible> {
//! #     Ok(Response::new(Bytes::new()))
//! # }
//!
//! // Wrap any tower service:
//! let service = ServiceBuilder::new()
//!     .layer(AgentDetectLayer::new())
//!     .service_fn(handler);
//! ```
//!
//! ## Axum usage
//!
//! Axum's `Router::layer` accepts any tower `Layer`, so the same
//! [`AgentDetectLayer`] drops in directly:
//!
//! ```text
//! use agentdetect_tower::AgentDetectLayer;
//! use axum::Router;
//!
//! let app: Router = Router::new()
//!     .route("/v1/predict", axum::routing::post(|| async { "ok" }))
//!     .layer(AgentDetectLayer::new());
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;

use agentdetect::Detection;
use agentdetect::propagation;
use futures_util::future::BoxFuture;
use http::Request;
use tower_layer::Layer as _Layer;
use tower_service::Service;

#[cfg(feature = "otel")]
use agentdetect::otel;

// ─── Public re-exports ────────────────────────────────────────────────────────

pub use agentdetect;

// ─── Extractor for downstream handlers ────────────────────────────────────────

/// Axum / tower extractor that yields the [`Detection`] stored in the
/// request extensions by [`AgentDetectLayer`].
///
/// We don't implement axum's `FromRequestParts` directly to keep this
/// crate axum-agnostic; downstream axum users implement the trait in one
/// line.  See the crate-level docs for an example.
pub mod extractor {
    use super::Detection;

    /// Wrapper extracted from request extensions.
    #[derive(Clone, Debug)]
    pub struct DetectedAgent(pub Option<Detection>);
}

// ─── Configuration ────────────────────────────────────────────────────────────

/// Knobs for [`AgentDetectLayer`].
///
/// Built via [`AgentDetectConfig::builder()`].  All fields have sensible
/// defaults; most users should just call [`AgentDetectLayer::new`].
#[derive(Clone)]
pub struct AgentDetectConfig {
    /// OTel instruments used to record per-request metrics.
    ///
    /// When `None` (and the `otel` feature is enabled), the global
    /// [`agentdetect::otel::Instruments`] is used.  When the `otel`
    /// feature is disabled, this field is ignored.
    #[cfg(feature = "otel")]
    pub instruments: Option<otel::Instruments>,

    /// If `true`, the layer enriches the active OTel span with agent
    /// identity attributes.  Defaults to `true`.
    pub enrich_span: bool,

    /// If `true`, the layer records per-request OTel metrics.  Defaults
    /// to `true`.
    pub record_metrics: bool,
}

#[cfg(feature = "otel")]
impl Default for AgentDetectConfig {
    fn default() -> Self {
        Self {
            instruments: None,
            enrich_span: true,
            record_metrics: true,
        }
    }
}

#[cfg(not(feature = "otel"))]
impl Default for AgentDetectConfig {
    fn default() -> Self {
        Self {
            enrich_span: false,
            record_metrics: false,
        }
    }
}

impl AgentDetectConfig {
    /// Start building a custom config.
    pub fn builder() -> AgentDetectConfigBuilder {
        AgentDetectConfigBuilder::default()
    }
}

/// Builder for [`AgentDetectConfig`].
#[derive(Default)]
pub struct AgentDetectConfigBuilder {
    #[cfg(feature = "otel")]
    instruments: Option<otel::Instruments>,
    #[cfg(feature = "otel")]
    enrich_span: Option<bool>,
    #[cfg(feature = "otel")]
    record_metrics: Option<bool>,
    #[cfg(not(feature = "otel"))]
    enrich_span: Option<bool>,
    #[cfg(not(feature = "otel"))]
    record_metrics: Option<bool>,
}

impl AgentDetectConfigBuilder {
    /// Use a specific [`agentdetect::otel::Instruments`] instance instead of
    /// the global one.
    #[cfg(feature = "otel")]
    pub fn instruments(mut self, i: otel::Instruments) -> Self {
        self.instruments = Some(i);
        self
    }

    /// Toggle OTel span enrichment (default: on).
    pub fn enrich_span(mut self, on: bool) -> Self {
        self.enrich_span = Some(on);
        self
    }

    /// Toggle OTel metric recording (default: on).
    pub fn record_metrics(mut self, on: bool) -> Self {
        self.record_metrics = Some(on);
        self
    }

    /// Finalize.
    #[cfg(feature = "otel")]
    pub fn build(self) -> AgentDetectConfig {
        AgentDetectConfig {
            instruments: self.instruments,
            enrich_span: self.enrich_span.unwrap_or(true),
            record_metrics: self.record_metrics.unwrap_or(true),
        }
    }

    /// Finalize (no-otel build).
    #[cfg(not(feature = "otel"))]
    pub fn build(self) -> AgentDetectConfig {
        AgentDetectConfig {
            enrich_span: self.enrich_span.unwrap_or(false),
            record_metrics: self.record_metrics.unwrap_or(false),
        }
    }
}

// ─── Layer ────────────────────────────────────────────────────────────────────

/// Tower [`Layer`](tower_layer::Layer) that wraps a service with
/// agentdetect propagation reading + OTel emission.
///
/// Cheap to clone — internally `Arc`-backed.  Drop it into
/// `tower::ServiceBuilder::new().layer(...)` or `axum::Router::layer(...)`.
#[derive(Clone)]
pub struct AgentDetectLayer {
    config: Arc<AgentDetectConfig>,
}

impl AgentDetectLayer {
    /// Build a layer with default configuration.
    pub fn new() -> Self {
        Self::with_config(AgentDetectConfig::default())
    }

    /// Build a layer with custom configuration.
    pub fn with_config(config: AgentDetectConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl Default for AgentDetectLayer {
    fn default() -> Self {
        Self::new()
    }
}

impl<S> _Layer<S> for AgentDetectLayer {
    type Service = AgentDetectMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AgentDetectMiddleware {
            inner,
            config: self.config.clone(),
        }
    }
}

// ─── Middleware service ───────────────────────────────────────────────────────

/// The [`Service`] produced by [`AgentDetectLayer`].
///
/// Generic over the inner service `S` and the request body type `B`.
/// Implements `Service<http::Request<B>>` for any `B` that the inner
/// service accepts.
#[derive(Clone)]
pub struct AgentDetectMiddleware<S> {
    inner: S,
    config: Arc<AgentDetectConfig>,
}

impl<S, ReqBody> Service<Request<ReqBody>> for AgentDetectMiddleware<S>
where
    S: Service<Request<ReqBody>> + Clone + Send + 'static,
    S::Response: IntoResponse,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = MiddlewareFuture<S::Response, S::Error>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // tower's `Service::call` takes `&mut self` and returns a `Future`
        // that may outlive the borrow.  The canonical pattern is to clone
        // the inner service and `std::mem::replace` it into place so we
        // own one clone for the future.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let config = self.config.clone();
        let started = Instant::now();

        // ── Phase 1: read propagated detection ───────────────────────────
        // This is NOT `User-Agent` sniffing — `propagation::read` only
        // looks at the `x-agentdetect-*` headers that a trusted CLI wrote
        // via `propagation::inject`.
        let detection: Option<Detection> = {
            let headers: &http::HeaderMap = req.headers();
            propagation::read(headers)
        };

        // Stash the detection in extensions so handlers can read it via
        // `request.extensions().get::<Detection>()`.
        if let Some(ref d) = detection {
            req.extensions_mut().insert(d.clone());
        }

        // ── Phase 2: enrich span (if enabled) ────────────────────────────
        #[cfg(feature = "otel")]
        if config.enrich_span {
            if let Some(ref d) = detection {
                otel::enrich_span(d);
            }
        }

        // ── Phase 3: call inner service ──────────────────────────────────
        let fut = inner.call(req);

        // ── Phase 4: wrap the inner future to record metrics on completion
        MiddlewareFuture::new(Box::pin(fut), detection, started, config)
    }
}

// ─── Response trait bound + future ────────────────────────────────────────────

/// Anything we can extract an HTTP status code from, for metric labels.
///
/// Implemented for `http::Response<T>` directly; axum's `Response` is just
/// a re-export of that type.  For other response types, implement this
/// trait (one method, one line) to opt into metric recording.
pub trait IntoResponse {
    /// Return the HTTP status code, or `None` if the response doesn't have
    /// one (in which case metrics record `status_class = "other"`).
    fn status_code(&self) -> Option<u16>;
}

impl<B> IntoResponse for http::Response<B> {
    fn status_code(&self) -> Option<u16> {
        Some(self.status().as_u16())
    }
}

// Future returned by `AgentDetectMiddleware::call`.
//
// Pins the inner future and records metrics once the inner future resolves.
// Uses `pin_project_lite` for safe pin projection — no hand-rolled `unsafe`.
pin_project_lite::pin_project! {
    #[allow(missing_docs)]
    pub struct MiddlewareFuture<R, E> {
        #[pin]
        inner: BoxFuture<'static, Result<R, E>>,
        detection: Option<Detection>,
        started: Instant,
        config: Arc<AgentDetectConfig>,
        _phantom: std::marker::PhantomData<R>,
    }
}

impl<R, E> MiddlewareFuture<R, E> {
    fn new(
        inner: BoxFuture<'static, Result<R, E>>,
        detection: Option<Detection>,
        started: Instant,
        config: Arc<AgentDetectConfig>,
    ) -> Self {
        Self {
            inner,
            detection,
            started,
            config,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<R, E> Future for MiddlewareFuture<R, E>
where
    R: IntoResponse,
{
    type Output = Result<R, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.inner.poll(cx) {
            Poll::Ready(result) => {
                #[cfg(feature = "otel")]
                {
                    if this.config.record_metrics {
                        if let Some(d) = this.detection.as_ref() {
                            let status = match &result {
                                Ok(r) => r.status_code().unwrap_or(0),
                                Err(_) => 500,
                            };
                            let elapsed = this.started.elapsed();
                            match this.config.instruments.as_ref() {
                                Some(i) => i.record_request(d, status, elapsed),
                                None => otel::record_request(d, status, elapsed),
                            }
                        }
                    }
                }
                Poll::Ready(result)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{Request, Response, StatusCode};
    use tower::service_fn;
    use tower::util::ServiceExt;

    async fn echo(_req: Request<Bytes>) -> Result<Response<Bytes>, std::convert::Infallible> {
        Ok(Response::builder()
            .status(StatusCode::OK)
            .body(Bytes::from_static(b"ok"))
            .unwrap())
    }

    #[tokio::test]
    async fn middleware_reads_propagated_claude_code_header() {
        let service = AgentDetectLayer::new().layer(service_fn(echo));

        let req = Request::builder()
            .header(propagation::HEADER_AGENT, "claude-code")
            .header(propagation::HEADER_CONFIDENCE, "high")
            .body(Bytes::new())
            .unwrap();

        let resp = service.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn middleware_stores_detection_in_extensions() {
        let captured: std::sync::Arc<std::sync::Mutex<Option<Detection>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));

        let captured_clone = captured.clone();
        let inner = service_fn(move |req: Request<Bytes>| {
            let captured = captured_clone.clone();
            async move {
                let d = req.extensions().get::<Detection>().cloned();
                *captured.lock().unwrap() = d;
                Ok::<_, std::convert::Infallible>(
                    Response::builder().status(200).body(Bytes::new()).unwrap(),
                )
            }
        });

        let service = AgentDetectLayer::new().layer(inner);

        let req = Request::builder()
            .header(propagation::HEADER_AGENT, "claude-code")
            .header(propagation::HEADER_CONFIDENCE, "high")
            .body(Bytes::new())
            .unwrap();

        service.oneshot(req).await.unwrap();
        let captured = captured.lock().unwrap().clone();
        assert!(
            captured.is_some(),
            "detection should be stored in extensions"
        );
        assert_eq!(captured.unwrap().agent.id, "claude-code");
    }

    #[tokio::test]
    async fn middleware_passes_through_when_no_propagation_header() {
        // A plain request with no `x-agentdetect-*` header — should pass
        // through undetected (no User-Agent sniffing).
        let service = AgentDetectLayer::new().layer(service_fn(echo));

        let req = Request::builder()
            .header("User-Agent", "Mozilla/5.0 (X11; Linux x86_64)")
            .body(Bytes::new())
            .unwrap();

        let resp = service.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
