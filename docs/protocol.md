# `prism-signal.v1`

The contract that lets the hub, or any other caller, invoke Prism Signal without reimplementing reading, cover, or fusion. It follows the shape of `prism-execution.v1`. Implemented by `prism-signal-protocol` (wire types) and `prism-signal-runtime` (the process).

```bash
prism-signal-runtime            # one JSON request per line on stdin, one response per line
prism-signal-runtime --json     # all of stdin is one request, one response on stdout
```

stdout carries the protocol and nothing else. Diagnostics go to stderr. The runtime is stateless: it holds only immutable vocabulary, reads no clock, and opens no network. The same request always gets the same answer.

## Envelope

```json
{"protocol_version": "prism-signal.v1", "request_id": "r-1", "operation": "assess", "payload": {}}
```

| Field | Meaning |
| --- | --- |
| `protocol_version` | exactly `prism-signal.v1` |
| `request_id` | caller-owned correlation reference, repeated in the response |
| `operation` | `capabilities`, `normalize`, `cover`, or `assess` |
| `payload` | operation-specific; omitted for `capabilities` |

A response repeats `request_id` and carries exactly one of `result` or `failure`. A request that cannot be read still gets a response, with the `request_id` if one could be found.

Unknown fields in a payload fail explicitly. A failure message never echoes the request.

## Operations

### `capabilities`

Returns the operations, the readers, the policy versions, the hazard kinds, the default grid resolution, and the limits of one request.

### `normalize`

Reads evidence into located `SignalObservation`s.

```json
{"reader": "normalize", "evidence": [ /* Evidence, at most 500 */ ]}
```

| Reader | Behaviour |
| --- | --- |
| `normalize` (default) | `prism-signal-normalize` through `prism-signal-bridge`: place roles, so a launch site or a place named in passing is never an observation; disc footprints from the place's own reach |
| `observe` | `prism-signal-observe`: a larger gazetteer, counts, no roles; point geometries |

The result is `{reader, observations, skipped}`. `skipped` names what was left out, with stable codes:

| Code | Meaning |
| --- | --- |
| `forwarded` | not the source's own report |
| `unlocated` | a hazard, but no place the gazetteer resolves (`observe` only) |
| `ambiguous_place` | a name several places share (`observe` only) |
| `unlocated_clear` | an all-clear that names a kind or a place but not both. It cannot be located evidence, so the threat it meant to end lapses on its window (`normalize` only) |
| `invalid_geometry` | a place could not be drawn |

### `cover`

Turns a geometry into grid cells.

```json
{"geometry": {"type": "point", "coordinates": [30.5238, 50.4547]}, "resolution": 6, "rings": 1, "max_cells": 400}
```

`resolution` defaults to the policy's (6). `rings` widens the cover by that many rings of neighbours, and defaults to 0. `max_cells` defaults to and is capped at 400. A larger cover fails with `cover_too_large`; nothing is truncated. The result is sorted and deduplicated.

### `assess`

Fuses a window of observations, as of an instant, into assessments and events. See [`fusion.md`](fusion.md).

```json
{"policy_version": "fusion.v1", "evaluation_time": "2026-09-28T22:30:00Z", "observations": [ /* at most 5000 */ ]}
```

`evaluation_time` is required: the runtime never reads a clock. Observations reported after it are ignored. The result is `{assessments, events, skipped}`.

## Failures

`failure` is `{code, message}`.

| Code | Meaning |
| --- | --- |
| `invalid_request` | not JSON, not the protocol, a wrong `protocol_version`, an unknown field, or a missing required field such as `evaluation_time` |
| `unsupported_operation` | an `operation` this runtime does not know |
| `unknown_policy` | an unknown `policy_version` |
| `invalid_geometry` | a geometry the grid cannot use |
| `cover_too_large` | a cover over the bound |
| `window_too_large` | more evidence or observations than one request may carry |

## Example

```bash
echo '{"protocol_version":"prism-signal.v1","request_id":"r-1","operation":"cover","payload":{"geometry":{"type":"point","coordinates":[30.5238,50.4547]},"rings":1}}' \
  | prism-signal-runtime --json
```

## Compatibility

Wire versions are independent from binary versions. Adding an optional field or a new `kind` is compatible. Changing a field meaning, a policy result, or a required field is a new protocol version. Unknown closed-vocabulary values fail explicitly.

Not yet generated: a JSON Schema for the envelope. The wire types in `prism-signal-protocol` are the definition until it is.
