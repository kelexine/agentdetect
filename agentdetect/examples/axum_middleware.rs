//! API-side middleware pattern: read the propagated `x-agentdetect-*`
//! header (NOT `User-Agent`) and emit OTel.
//!
//! This is the API side of the propagation flow.  The CLI side is shown in
//! `examples/cli_to_api.rs`.  Together they form the full round-trip:
//!
//!   CLI: detect_from_env() → propagation::inject()
//!   API: propagation::read() → otel::enrich_span() / otel::record_request()
//!
//! Run:
//!     cargo run --example axum_middleware --features "http otel"

use std::time::Instant;

use agentdetect::Detection;
use agentdetect::otel;
use agentdetect::propagation;
use http::HeaderMap;

// ─── Mock server plumbing ─────────────────────────────────────────────────────

#[derive(Debug)]
struct MockRequest {
    path: &'static str,
    method: &'static str,
    headers: HeaderMap,
}

#[derive(Debug)]
struct MockResponse {
    status: u16,
}

struct AppState {
    /// In a real app, you'd hold a typed `otel::Instruments` here so each
    /// request handler can clone the cheap `Arc`-backed instruments.
    _instruments: (),
}

async fn route(req: &MockRequest) -> MockResponse {
    // Pretend this is your real handler dispatch — we just match on path.
    match (req.method, req.path) {
        ("GET", "/healthz") => MockResponse { status: 200 },
        ("POST", "/v1/predict") => MockResponse { status: 200 },
        ("POST", "/v1/chat") => MockResponse { status: 200 },
        ("DELETE", "/v1/delete") => MockResponse { status: 204 },
        _ => MockResponse { status: 404 },
    }
}

// ─── The middleware ───────────────────────────────────────────────────────────

/// Wraps a request handler with agentdetect propagation reading + OTel
/// emission.
///
/// In a real axum app this would be a tower middleware / `from_fn` layer;
/// the body of the function is what matters — the pattern is identical.
async fn middleware(
    state: &AppState,
    req: &MockRequest,
    next: impl std::future::Future<Output = MockResponse>,
) -> MockResponse {
    let started = Instant::now();
    let _ = state; // would be used to access shared instruments / config

    // 1) Read the propagated detection from the `x-agentdetect-*` headers.
    //
    //    This is NOT `User-Agent` sniffing — the header was written by a
    //    trusted CLI using `agentdetect::propagation::inject`.  Only
    //    agentdetect-using code writes this header.
    let detection: Option<Detection> = propagation::read(&req.headers);

    if let Some(ref d) = detection {
        // 2) Enrich the current span with agent identity.
        //    (In axum, you'd extract the span from the request extensions.)
        otel::enrich_span(d);

        // 3) Optional: log the detection so you can correlate with logs.
        eprintln!(
            "[middleware] {} {} → agent={} confidence={} source={}",
            req.method,
            req.path,
            d.agent.id,
            d.confidence.id(),
            d.source_kind().id(),
        );
    } else {
        eprintln!(
            "[middleware] {} {} → no propagation header (undetected request)",
            req.method, req.path,
        );
    }

    // 4) Run the actual handler.
    let resp = next.await;
    let elapsed = started.elapsed();

    // 5) Record metrics.  We emit per-request metrics only when we have a
    //    detection — undetected requests are typically tracked by a separate
    //    generic counter in your framework middleware.
    if let Some(ref d) = detection {
        otel::record_request(d, resp.status, elapsed);
    }

    resp
}

// ─── Demo driver ──────────────────────────────────────────────────────────────

fn make_request(method: &'static str, path: &'static str, agent_id: Option<&str>) -> MockRequest {
    let mut headers = HeaderMap::new();
    if let Some(id) = agent_id {
        // Simulate what the CLI does via `propagation::inject`.
        headers.insert(
            propagation::HEADER_AGENT,
            http::HeaderValue::from_str(id).unwrap(),
        );
        headers.insert(
            propagation::HEADER_CONFIDENCE,
            http::HeaderValue::from_static("high"),
        );
    }
    MockRequest {
        path,
        method,
        headers,
    }
}

#[tokio::main]
async fn main() {
    println!(
        "agentdetect v{} — API middleware demo\n",
        agentdetect::VERSION
    );

    let state = AppState { _instruments: () };

    // Each request simulates what the API would see: the `x-agentdetect-*`
    // header is present only when a trusted CLI injected it.
    let requests: &[MockRequest] = &[
        make_request("GET", "/healthz", Some("claude-code")),
        make_request("POST", "/v1/predict", Some("cursor")),
        make_request("POST", "/v1/chat", Some("codex")),
        make_request("POST", "/v1/chat", None), // no header — undetected
        make_request("DELETE", "/v1/delete", Some("claude-code")),
        make_request("GET", "/unknown", Some("devin")),
    ];

    for req in requests {
        let req_clone = MockRequest {
            path: req.path,
            method: req.method,
            headers: req.headers.clone(),
        };
        let next = route(req);
        let resp = middleware(&state, &req_clone, next).await;
        println!("  → HTTP {} {}\n", resp.status, req.path);
    }

    println!("─── summary ───");
    println!("Each request that carried the `x-agentdetect-agent` header emitted:");
    println!("  • span attrs (agent.id, agent.label, agent.family, …)");
    println!("  • agentdetect.requests.total{{agent_id, agent_family, status_class}} +1");
    println!("  • agentdetect.request.duration{{agent_id, agent_family}} = <elapsed>");
    println!();
    println!("Requests WITHOUT the header (plain browser / curl traffic) are");
    println!("undetected — agentdetect does NOT sniff User-Agent or any other");
    println!("third-party header.  Only CLIs using `agentdetect::propagation::inject`");
    println!("can set the `x-agentdetect-*` headers.");
    println!();
    println!("In your OTel backend, this is enough to answer:");
    println!("  • Which agent called which API?         (filter spans by agent.id)");
    println!("  • How many req/min per agent?            (rate of requests.total by agent_id)");
    println!("  • Success rate per agent?                (2xx / total per agent_id)");
    println!("  • % of traffic per agent?                (per-agent / overall)");
    println!("  • Which agent uses the API most?         (top-N by requests.total)");
    println!("  • p99 latency per agent?                 (histogram_quantile by agent_id)");
}
