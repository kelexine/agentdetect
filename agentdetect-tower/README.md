# agentdetect-tower

Tower middleware that reads the `x-agentdetect-*` propagation header
(written by a trusted CLI using [`agentdetect::propagation::inject`]) and
emits OpenTelemetry signals on the API side.

## What this is (and isn't)

This middleware does **NOT** sniff `User-Agent` or any other third-party
header.  agentdetect's detection model is env-var-only — a tool like
`curl` running under an agent has no idea it's under an agent, so its wire
traffic carries no agent-identifying signal.

The only way an agent identity reaches your API is if YOUR CLI (built with
agentdetect) detects the agent via env vars, then writes it onto its
outgoing request via [`agentdetect::propagation::inject`].  This middleware
reads that same header on the API side.

## What it does per request

1. Reads the `x-agentdetect-agent` / `x-agentdetect-confidence` /
   `x-agentdetect-version` headers via
   [`agentdetect::propagation::read`].
2. Stores the reconstructed [`agentdetect::Detection`] in request
   extensions so downstream handlers can read it.
3. Records `agentdetect.requests.total{agent_id, agent_family,
   status_class}` and `agentdetect.request.duration{agent_id,
   agent_family}` once the response is produced.
4. Enriches the current OTel span with agent identity attributes
   (`agent.id`, `agent.label`, `agent.family`, …) when the `otel`
   feature is enabled.

## Example

```no_run
use agentdetect_tower::AgentDetectLayer;
use tower::ServiceBuilder;
# use std::convert::Infallible;
# use http::{Request, Response};
# use bytes::Bytes;
# async fn handler(req: Request<Bytes>) -> Result<Response<Bytes>, Infallible> {
#     Ok(Response::new(Bytes::new()))
# }

// Wrap any tower service:
let service = ServiceBuilder::new()
    .layer(AgentDetectLayer::new())
    .service_fn(handler);
```

## Axum usage

Axum's `Router::layer` accepts any tower `Layer`, so the same
`AgentDetectLayer` drops in directly:

```text
use agentdetect_tower::AgentDetectLayer;
use axum::Router;

let app: Router = Router::new()
    .route("/v1/predict", axum::routing::post(|| async { "ok" }))
    .layer(AgentDetectLayer::new());
```

## Reading the detection in handlers

The layer stashes the [`agentdetect::Detection`] in request extensions,
so handlers read it via `request.extensions().get::<Detection>()`.  In
axum, implement `FromRequestParts` in one line:

```text
use agentdetect::Detection;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::response::Response;

#[derive(Clone, Debug)]
pub struct DetectedAgent(pub Option<Detection>);

impl<S: Send + Sync> FromRequestParts<S> for DetectedAgent {
    type Rejection = Response;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(DetectedAgent(parts.extensions.get::<Detection>().cloned()))
    }
}
```

## Feature flags

| Feature | Default? | Purpose |
|---------|----------|---------|
| `otel`  | yes      | OpenTelemetry span enrichment + metric emission |

(`http` is always required — the middleware reads the propagation header.)

## Examples

| Example | What it shows |
|---------|---------------|
| `tower_demo`        | Tower layer reading the propagation header + emitting OTel |

```bash
cargo run --example tower_demo --all-features
```

## License

MIT.

[`agentdetect::propagation::inject`]: https://docs.rs/agentdetect/latest/agentdetect/propagation/fn.inject.html
[`agentdetect::propagation::read`]: https://docs.rs/agentdetect/latest/agentdetect/propagation/fn.read.html
[`agentdetect::Detection`]: https://docs.rs/agentdetect/latest/agentdetect/detection/struct.Detection.html
