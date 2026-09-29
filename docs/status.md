# Implementation status

## Implemented now

| Surface | Current behavior |
| --- | --- |
| `prism-signal-core` | `Evidence`, `SourceId`, `ExternalId`, UTC `Timestamp`, media references, provenance |
| `prism-signal-source` | Stateless `EvidenceSource` port with `Latest` / `Before` / `After` paging and typed failures |
| `prism-signal-source-telegram` | Public channel preview (`t.me/s/…`) parser with an injected fetcher and a reqwest implementation |
| `prism-signal-collect` | CLI that streams evidence as NDJSON: `backfill` walks history, `follow` polls for new posts |

Not implemented yet: normalization into located `SignalObservation`, geo cells, fusion, `prism-signal.v1`, hub integration. The Telegram parser is tested against a synthetic fixture and has not yet been confirmed against a live page.

## Open questions

| Question | Owner |
| --- | --- |
| Which sources are in scope first and what each one's validity model is | product decision |
| Fusion policy contents: weights, decay, corroboration thresholds | this repository, with evidence |
| Calibration: which evidence classes may report a numeric probability | this repository |
| Broadcast class for hazard alerts and its gating | `nilx-one/0x1` |
| Broadcast reach: every attested client in a cell or bounded local edges | `nilx-one/0x1` |
| Privacy-preserving aggregation for client reports | `nilx-one/0x1` |
| Issuer policy: who may authorize an alert | `prism-hub` |
| Licensing and repository policy | repository owner |
