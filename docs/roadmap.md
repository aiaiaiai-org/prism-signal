# Roadmap

Order follows evidence, not calendar. Each step ships behind a contract and tests before the next starts.

1. **Contracts** — `prism-signal-core` values, `prism-signal.v1` schema, canonical fixtures, repository policy and CI.
2. **Geo** — `cover` with bounds and determinism tests against H3 reference behavior.
3. **Fusion v1** — one hazard kind, two source kinds, replayable tests, band-only likelihood.
4. **Runtime** — stateless JSON/NDJSON runtime and testkit conformance helpers.
5. **First real source adapter** — one official feed, injected transport, no live credentials in CI.
6. **Hub integration** — hub invokes runtime; issuer policy gate; porter `signal.alert` renderer.
7. **`0x1` bridge** — only after the alert class is specified in `0x1`; adapter reports aggregate receipts.
8. **Feedback** — corroboration from k-thresholded client aggregates.
