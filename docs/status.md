# Implementation status

## Implemented now

| Surface | Current behavior |
| --- | --- |
| `prism-signal-core` | `Evidence`, `SourceId`, `ExternalId`, UTC `Timestamp`, media references, provenance; `SignalObservation` with `Geometry`, closed `HazardKind` vocabulary, `Stance`, `TtlSeconds` |
| `prism-signal-geo` | `cover(geometry, resolution, max_cells)` on H3: sorted, deduplicated, bounded with `cover_too_large`; cell lists recast between resolutions; `disc` (a circle as a polygon) and `expand` (rings of neighbours); `0x1` 7/8 profile named, not hard-coded |
| `prism-signal-observe` | Rule-based normalization of Russian/Ukrainian alert-channel text into located observations through a bundled GeoNames gazetteer of Ukraine; see [`observation.md`](observation.md) |
| `prism-signal-source` | Stateless `EvidenceSource` port with `Latest` / `Before` / `After` paging and typed failures |
| `prism-signal-source-telegram` | Public channel preview (`t.me/s/…`) parser with an injected fetcher and a reqwest implementation; checked against pages captured from a live channel |
| `prism-signal-collect` | CLI that streams evidence as NDJSON: `backfill` walks history, `follow` polls for new posts, `poll` reads once from a cursor and exits (the caller keeps the cursor) |
| `prism-signal-bridge` | Turns `prism-signal-normalize` readings into `SignalObservation`s: only target and via places, a disc footprint of the place's own reach, and only all-clears that name a kind and a place. See [`readers.md`](readers.md) |
| `prism-signal-fusion` | `assess(observations, policy, evaluation_time)`: episodes of one hazard class at one place with `issued`, `superseded`, `expired`, and `retracted` events, cells for fan-out, a narrow retraction rule. See [`fusion.md`](fusion.md) |
| `prism-signal-protocol`, `prism-signal-runtime` | `prism-signal.v1`: `capabilities`, `normalize`, `cover`, `assess` over JSON or NDJSON. See [`protocol.md`](protocol.md) |
| `prism-signal-normalize` | Reads a post into hazard readings: kind, threat or all-clear, and places with a target, via, origin, or mention role. A curated gazetteer of 117 Ukrainian places built from GeoNames, Russian and Ukrainian vocabulary, kind carry-over between sentences. CLI reads `Evidence` NDJSON and prints readings with a coverage report. See [`normalization.md`](normalization.md) |

Not implemented yet: reconciling the two readers (see [`readers.md`](readers.md)), regions (a report about an oblast reaches nobody), a JSON Schema for `prism-signal.v1`, hub integration.

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
