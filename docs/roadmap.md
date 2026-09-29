# Roadmap

Order follows evidence, not calendar. Each step ships behind a contract and tests before the next starts.

1. **Contracts** — `prism-signal-core` values and the `prism-signal.v1` wire types are done; a JSON Schema for the envelope, canonical fixtures, and repository policy remain.
2. **Geo** — done: `cover`, `disc`, `expand`, bounded and deterministic.
3. **Fusion v1** — done for one source kind: three hazard classes, lifecycle, narrow retraction, band-only likelihood. See [`fusion.md`](fusion.md). Corroboration across a second source kind waits for a second source.
4. **Runtime** — done: `prism-signal-runtime`, see [`protocol.md`](protocol.md). Testkit conformance helpers remain.
5. **First real source adapter** — one official feed, injected transport, no live credentials in CI.
6. **Hub integration** — hub invokes runtime; issuer policy gate; porter `signal.alert` renderer.
7. **`0x1` bridge** — only after the alert class is specified in `0x1`; adapter reports aggregate receipts.
8. **Feedback** — corroboration from k-thresholded client aggregates.
