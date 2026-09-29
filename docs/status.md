# Implementation status

## Implemented now

| Surface | Current behavior |
| --- | --- |
| `prism-signal-core` | `Evidence`, `SourceId`, `ExternalId`, UTC `Timestamp`, media references, provenance; `SignalObservation` with `Geometry`, closed `HazardKind` vocabulary, `Stance`, `TtlSeconds` |
| `prism-signal-geo` | `cover(geometry, resolution, max_cells)` on H3: sorted, deduplicated, bounded with `cover_too_large`; cell lists recast between resolutions; `0x1` 7/8 profile named, not hard-coded |
| `prism-signal-normalize` | Rule-based normalization of Russian/Ukrainian alert-channel text into located observations through a bundled GeoNames gazetteer of Ukraine; see [`normalization.md`](normalization.md) |
| `prism-signal-source` | Stateless `EvidenceSource` port with `Latest` / `Before` / `After` paging and typed failures |
| `prism-signal-source-telegram` | Public channel preview (`t.me/s/…`) parser with an injected fetcher and a reqwest implementation; checked against pages captured from a live channel |
| `prism-signal-collect` | CLI that streams evidence as NDJSON: `backfill` walks history, `follow` polls for new posts |

Not implemented yet: fusion, `prism-signal.v1`, hub integration.

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
