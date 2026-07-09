# agentdetect

A pure Rust library that answers one question: **is this process running
under an AI agent harness, and if so, which one?**

When an agent harness (Claude Code, Cursor, Codex, etc.) spawns a shell to
run a command, it sets an env var identifying itself — `CLAUDE_CODE=1`,
`CURSOR_TRACE_ID=…`, `CODEX_SANDBOX=1`, etc. agentdetect reads those
variables and tells you which harness is active.

## The core use case: bit-flip output switching

The primary purpose is the **bit-flip pattern** — switch output format
based on whether the caller is a human or an agent:

```
┌─────────────────────────────┐
│  Is an agent harness active? │
└──────────────┬──────────────┘
         ┌─────┴─────┐
        NO           YES
         │             │
         ▼             ▼
  ┌────────────┐  ┌────────────────────┐
  │  Human in  │  │  Agent harness     │
  │  terminal  │  │  detected          │
  │            │  │                    │
  │  → Pretty  │  │  → Machine-readable│
  │    output  │  │    output (TSV)    │
  └────────────┘  └────────────────────┘
```

This is exactly how [loc-rs](https://github.com/kelexine/loc-rs) uses it:
when a human runs `loc`, they get a coloured summary table; when Claude
Code runs `loc`, it gets TSV with `# Agent-Detected: claude-code` so the
agent can parse the output efficiently.

```rust
if agentdetect::is_agent() {
    // Agent harness is active — emit machine-readable output.
    let d = agentdetect::detect().unwrap();
    println!("# Agent-Detected: {}", d.agent.id);
    println!("metric\tvalue");
    // ... TSV data ...
} else {
    // Human terminal — emit pretty output.
    // ... coloured table ...
}
```

## Env vars are the ONLY detection surface

agentdetect reads the process environment.  A harness spawning a
shell sets an env var on the process tree it spawns — that is the *entire*
signal. I case of Claude Code that will be `CLAUDE_CODE,=1`

## Optional: propagation (CLI → API)

When you're building BOTH a CLI AND the API it talks to, and you want the
API to know which agent is calling, enable the `http` feature and use the
[`propagation`] module:

1. **CLI side**: detect the agent via env vars, then write the identity
   onto your outgoing request via `agentdetect::propagation::inject` —
   which sets the `x-agentdetect-agent` / `x-agentdetect-confidence`
   headers **defined in the lib**.
2. **API side**: your middleware (e.g. [`agentdetect-tower`]'s
   `AgentDetectLayer`) reads those same headers via
   `agentdetect::propagation::read` and reconstructs a `Detection`.

## Optional: OpenTelemetry (feature-gated)

The `otel` feature adds span enrichment and metric emission for the
**secondary use case**: you're building a CLI that speaks to a public API,
and you want to track which agents are calling, how often, success rate,
% of traffic, top-N.  This is a feature on top of the core detection —
the core library has zero non-std dependencies.

> Built on the detection patterns from
> [`loc-rs`](https://github.com/kelexine/loc-rs) — same compile-time
> `const` registry, same priority-ordered scan, same `AI_AGENT` / `AGENT`
> fallback — but as a reusable library with the bit-flip primitive,
> optional propagation, and optional OpenTelemetry emission.

## Detection model

A [`Detection`] carries not just *which* agent was found but:

- **`Confidence`** (`High` / `Medium` / `Low`) — dedicated harness marker
  beats standard `AI_AGENT` / `AGENT` channel beats unrecognised value.
- **`SourceKind`** (`EnvVar` / `Propagated`) — direct detection vs
  reconstructed from a propagated header on the API side.
- **Evidence trail** (`raw_signals`) — every matching signal is retained,
  so you can audit "why was this classified as Claude Code?".

## Supported harnesses (23)

`antigravity`, `augment-cli`, `cline`, `cowork`, `claude-code`, `codex`,
`crush`, `gemini-cli`, `github-copilot`, `goose`, `hermes-agent`,
`kilo-code`, `kiro`, `openclaw`, `opencode`, `pi`, `replit`, `trae`,
`warp`, `zed`, `cursor-cli`, `cursor`, `devin`.

Each carries:

- Static identity (`id`, `pretty_label`, `family`, `repo_url`, `docs_url`,
  `description`).
- One or more env-var checks (presence / exact / prefix patterns).
- A `HarnessFamily` for vendor-level grouping (Anthropic, OpenAI, Google,
  GitHub, ByteDance, Cognition, Charm, Cursor, Block, Replit, AWS, Nous
  Research, Community, Other).

To list every harness at runtime:

```rust
for &key in agentdetect::AgentHarnessKey::ALL {
    let info = key.info();
    println!("{:<15} | {:<20} | {}", key.id(), info.pretty_label, info.family);
}
```

## Feature flags

| Feature | Default? | Pulls in | Purpose |
|---------|----------|----------|---------|
| `http`  | no       | `http` v1 | Propagation helpers (`inject` / `read` for the `x-agentdetect-*` headers) |
| `otel`  | no       | `opentelemetry` v0.27 | OTel attribute / span / metric emission |

Both are optional — the pure detection core has zero non-std dependencies.

## Examples

| Example | What it shows |
|---------|---------------|
| `bit_flip`          | **the canonical use case** — switch between pretty and TSV output based on agent detection |
| `basic`             | env-var detection from the current process (detailed output) |
| `cli_to_api`        | full round-trip: CLI detects → injects header → API reads → OTel |
| `otel_demo`         | span attrs + metric labels + emission for an env-var detection |
| `axum_middleware`   | API-side middleware reading the propagation header |

```bash
# The canonical bit-flip demo:
cargo run --example bit_flip                # human → pretty
CLAUDE_CODE=1 cargo run --example bit_flip  # agent → TSV
ANTIGRAVITY_AGENT=1 cargo run --example bit_flip

# Full CLI → API round-trip:
cargo run --example cli_to_api --features "http otel"
CLAUDE_CODE=1 cargo run --example cli_to_api --features "http otel"

cargo run --example basic
cargo run --example otel_demo --features otel
cargo run --example axum_middleware --features "http otel"
```

## Project layout

```
agentdetect/
├── src/
│   ├── lib.rs          # Public API + re-exports
│   ├── pattern.rs      # EnvPattern (const fn matcher)
│   ├── registry.rs     # Static registry of 23 harnesses (env vars only)
│   ├── detection.rs    # Detection, AgentInfo, Confidence, SourceKind, RawSignal
│   ├── detect.rs       # is_agent / detect_from_env (the only detection surface)
│   ├── propagation.rs  # inject / read for the x-agentdetect-* headers (feature-gated)
│   └── otel.rs         # OpenTelemetry attributes, span enrichment, metrics
├── examples/           # 5 examples
└── tests/              # Integration tests
```

## Why no behavioral detection?

agentdetect intentionally does NOT classify "this looks like an agent"
based on behavioral patterns (request rate, payload shape, model string,
etc.).  Every detection is grounded in an explicit signal set by the
harness itself (env var).  This makes detection explainable and impossible
to silently drift — if you can't see the signal, you don't classify the
request.

## License

MIT.

[`propagation`]: https://docs.rs/agentdetect/latest/agentdetect/propagation/index.html
[`agentdetect-tower`]: https://crates.io/crates/agentdetect-tower
[`Detection`]: https://docs.rs/agentdetect/latest/agentdetect/detection/struct.Detection.html
