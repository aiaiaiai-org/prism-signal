# `prism-signal.v1`

Draft. The contract lets the hub and other runtimes invoke Prism Signal without reimplementing fusion or cell logic. It follows the shape of `prism-execution.v1`.

## Envelope

Every request contains:

- `protocol_version` — exactly `prism-signal.v1`;
- `request_id` — caller-owned correlation reference;
- `operation` — `capabilities`, `normalize`, `cover`, or `assess`;
- `payload` — operation-specific data.

Every response repeats `request_id` and returns either a `result` or a typed `failure`. JSON is canonical with no numeric tokens for identifiers; 64-bit values are strings. stdout is protocol-only; diagnostics use stderr.

## Operations

| Operation | Input | Output |
| --- | --- | --- |
| `capabilities` | none | supported kinds, policy versions, grid profile, limits |
| `normalize` | raw source payload, `source_id` | `SignalObservation` list or typed failure |
| `cover` | geometry, resolution, `max_cells` | sorted, deduplicated `CellSet` |
| `assess` | observation window, `policy_version`, `evaluation_time` | ordered `SignalAssessment` list and `AssessmentEvent` list |

`assess` never reads a clock. `evaluation_time` is required.

## Example sketch

```json
{
  "protocol_version": "prism-signal.v1",
  "request_id": "req-1",
  "operation": "assess",
  "payload": {
    "policy_version": "fusion.v1",
    "evaluation_time": "2026-09-29T12:00:00Z",
    "observations": []
  }
}
```

## Compatibility

Wire versions are independent from binary versions. Adding an optional field or a new `kind` is compatible. Changing a field meaning, a policy result, or a required field is a new protocol version. Unknown closed-vocabulary values fail explicitly.
