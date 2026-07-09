//! Tower layer demo.
//!
//! Shows the API side of the propagation flow: the middleware reads the
//! `x-agentdetect-*` header (written by a trusted CLI) and emits OTel.
//!
//! Run:
//!     cargo run --example basic

use std::convert::Infallible;
use std::time::Duration;

use agentdetect::Detection;
use agentdetect_tower::AgentDetectLayer;
use bytes::Bytes;
use http::{HeaderMap, Request, Response, StatusCode};
use tower::ServiceBuilder;
use tower::util::ServiceExt;

/// A handler that reads the `Detection` from request extensions (where
/// `AgentDetectLayer` stashes it) and propagates the agent ID back in a
/// response header so the demo can show what was detected.
async fn handler(mut req: Request<Bytes>) -> Result<Response<Bytes>, Infallible> {
    // Simulate a small amount of work so the duration histogram has
    // something interesting to record.
    tokio::time::sleep(Duration::from_millis(5)).await;

    let detected = req
        .extensions_mut()
        .remove::<Detection>()
        .map(|d| d.agent.id.to_string());

    let mut builder = Response::builder().status(StatusCode::OK);
    if let Some(ref id) = detected {
        builder = builder.header("x-detected-agent", id.clone());
    }
    Ok(builder.body(Bytes::new()).unwrap())
}

#[tokio::main]
async fn main() {
    println!("agentdetect-tower — tower layer demo\n");

    let service = ServiceBuilder::new()
        .layer(AgentDetectLayer::new())
        .service_fn(handler);

    // Each request simulates what the API would see: the `x-agentdetect-*`
    // header is present only when a trusted CLI injected it.
    type ReqSpec = (Option<&'static str>, Option<&'static str>, u16);

    let requests: &[ReqSpec] = &[
        (Some("claude-code"), Some("high"), 200),
        (Some("cursor"), Some("high"), 200),
        (Some("codex"), Some("high"), 500), // simulate a failure
        (None, None, 200),                  // no header — undetected
        (Some("claude-code"), Some("medium"), 200),
    ];

    for (i, (agent, confidence, want_status)) in requests.iter().enumerate() {
        let mut builder = Request::builder()
            .method("POST")
            .uri(format!("/v1/call/{i}"));
        if let Some(a) = agent {
            builder = builder.header("x-agentdetect-agent", *a);
        }
        if let Some(c) = confidence {
            builder = builder.header("x-agentdetect-confidence", *c);
        }
        let req = builder.body(Bytes::new()).unwrap();

        let resp = service.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let detected = resp
            .headers()
            .get("x-detected-agent")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        println!(
            "  request #{i}: status={status} detected={detected:?} (wanted status={want_status})",
        );
    }

    println!("\nEach request that carried the `x-agentdetect-agent` header emitted:");
    println!("  • span attrs (agent.id, agent.label, agent.family, …)");
    println!("  • agentdetect.requests.total{{agent_id, agent_family, status_class}} +1");
    println!("  • agentdetect.request.duration{{agent_id, agent_family}} = <elapsed>");
    println!();
    println!("Requests WITHOUT the header (plain browser / curl traffic) are");
    println!("undetected — agentdetect does NOT sniff User-Agent.  Only CLIs");
    println!("using `agentdetect::propagation::inject` can set the header.");

    // Reference the imports that aren't otherwise used by name.
    let _ = HeaderMap::new();
    let _: Option<Detection> = None;
}
