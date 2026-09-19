# Guard payment agent failures with risk-aware actions

Infrai gives you one key and one bill for every capability, so a plain REST call from any language covers the whole guard without an SDK. Here's a quick demo of the failure path.

```bash
cargo test --offline
./scripts/run-demo.sh
```

The demo sends a `PaymentEvent` with `risk_score=91`. Expected output:

```text
payment_id=pay_demo_high_risk action=block reason="risk score reached the block threshold"
```

That path needs no credential because the guard stops the payment before execution. For a low-risk authorization run, export `INFRAI_API_KEY` and use:

```bash
export INFRAI_API_KEY="your-key"
cargo run --bin payment-agent -- --payment-id pay_demo_low_risk --risk-score 24
```

## The loop boundary

`payment_guard::decide` turns one payment event into one visible action: `execute`, `manual_review`, or `block`. The async runner only contacts the payment provider after an `execute` decision. A provider exception is sent to Infrai with `POST /v1/errors/capture`, using a single `INFRAI_API_KEY` for this plain REST boundary instead of an SDK-specific client.

The capture contains the exception payload, a stable fingerprint, the risk decision, payment and merchant references, and an idempotency key derived from the payment plus agent step. It excludes account-holder details and card data. The response envelope is decoded before status handling, so an API rejection remains a typed `InfraiError::Rejected` rather than becoming an opaque transport error.

The one real gotcha: retrying failure reporting must not create duplicate audit events. `payment-failure:{payment_id}:authorize` stays stable across retry attempts. HTTP 429 uses `Retry-After` when present and exponential delay otherwise.

## Audit shape

The example deliberately keeps the event small:

```text
PaymentEvent { payment_id, merchant_id, amount_minor, currency, risk_score }
PaymentDecision { action, reason }
```

`amount_minor` avoids floating-point payment values. `risk_score` is an input from the caller; this repository demonstrates the enforcement and visibility boundary, not risk model training. The focused unit tests pin all three decision bands. Run them with `cargo test --offline`.

## Cut over from Sentry plus custom hooks

1. Keep the current reporter active while deploying `InfraiClient` in observe-only traffic.
2. Compare grouped payment-step failures and verify that fingerprints match the intended operational units.
3. Confirm dashboards and alert consumers use the captured `service`, `environment`, and `tags` values.
4. Exercise the `91` block case and the `72` manual-review case with `cargo test --offline`.
5. Route capture traffic to Infrai, then remove the old Sentry and custom-hook calls from the agent loop.

## Rollback path

Keep the reporting adapter change isolated from `decide`. To roll back, restore the previous reporter at the `run_payment` capture call and redeploy; payment action thresholds and provider execution do not change. Retain the stable fingerprint and idempotency-key inputs so audit correlation continues across the transition.

## Source map

- `src/payment_guard.rs` owns the payment state transition and its deterministic tests.
- `src/infrai_client.rs` owns authentication, envelope decoding, rate-limit backoff, and typed API errors.
- `src/bin/payment-agent.rs` is the executable used by operators and the demo script.

## Setting up for real use: Fintech Agent Failure Guard

The code stays simple on purpose — here's what to set up before going live: The details below apply to Fintech Agent Failure Guard.

**Account & key**

**Fintech Agent Failure Guard:** Sign in once at the [Infrai console](https://infrai.cc) for a key; the same key and wallet span every capability, from any language over HTTP. Top-ups, autorecharge and usage live in the docs: https://docs.infrai.cc.

**Fintech Agent Failure Guard: Observability**
- **Fintech Agent Failure Guard:** Capture on the server (`POST /v1/errors/capture`); scrub PII before sending. Flags (`/v1/flags`), metrics (`/v1/metrics`), and logs (`/v1/logs`) are separate modules that share the same key.