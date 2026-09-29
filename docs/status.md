# Implementation status

## Implemented now

| Surface | Current behavior |
| --- | --- |
| `prism-signal-core` | `Evidence`, `SourceId`, `ExternalId`, UTC `Timestamp`, media references, provenance; `SignalObservation` with `Geometry`, closed `HazardKind` vocabulary, `Stance`, `TtlSeconds` |
| `prism-signal-geo` | `cover(geometry, resolution, max_cells)` on H3: sorted, deduplicated, bounded with `cover_too_large`; cell lists recast between resolutions; `0x1` 7/8 profile named, not hard-coded |
| `prism-signal-observe` | Rule-based normalization of Russian/Ukrainian alert-channel text into located observations through a bundled GeoNames gazetteer of Ukraine; see [`observation.md`](observation.md) |
| `prism-signal-source` | Stateless `EvidenceSource` port with `Latest` / `Before` / `After` paging and typed failures |
| `prism-signal-source-telegram` | Public channel preview (`t.me/s/…`) parser with an injected fetcher and a reqwest implementation; checked against pages captured from a live channel |
| `prism-signal-collect` | CLI that streams evidence as NDJSON: `backfill` walks history, `follow` polls for new posts |
| `prism-signal-normalize` | Reads a post into hazard readings: kind, threat or all-clear, and places with a target, via, origin, or mention role. A curated gazetteer of 117 Ukrainian places built from GeoNames, Russian and Ukrainian vocabulary, kind carry-over between sentences. CLI reads `Evidence` NDJSON and prints readings with a coverage report. See [`normalization.md`](normalization.md) |

Not implemented yet: `SignalObservation` built from a reading (`prism-signal-observe` builds them from text with its own rules, and the two readers are not reconciled), regions (a report about an oblast reaches nobody), fusion, `prism-signal.v1`, hub integration.

## Open questions

| Question | Owner |
| --- | --- |
| Which sources are in scope first and what each one's validity model is | product decision; the first is `t.me/vanek_nikolaev`, see [`air-alerts.md`](air-alerts.md) |
| Fusion policy contents: weights, decay, corroboration thresholds | this repository, with evidence |
| Calibration: which evidence classes may report a numeric probability | this repository |
| Broadcast class for hazard alerts and its gating | `nilx-one/0x1` |
| Broadcast reach: every attested client in a cell or bounded local edges | `nilx-one/0x1` |
| Privacy-preserving aggregation for client reports | `nilx-one/0x1` |
| Issuer policy: who may authorize an alert | `prism-hub` |
| Licensing and repository policy | repository owner |
