# Ecosystem

Proposed additions for [`prism/docs/ecosystem.md`](https://github.com/aiaiaiai-org/prism/blob/master/docs/ecosystem.md). They are proposals until that document changes.

## Component row

| Component | Owns | Integrates through |
| --- | --- | --- |
| `prism-signal` | Signal normalization, cell derivation, deterministic fusion, assessment lifecycle | Focused source ports; versioned `prism-signal.v1` and assessment artifacts |

## Dependency direction

- `prism-hub` invokes `prism-signal` and consumes its versioned assessments. The hub owns source scheduling, credentials, window storage, issuer policy, and delivery.
- `prism-signal` reads sources only through adapters; it never calls clients, transports, `prism`, `prism-ai`, or `0x1`.
- `prism-porter` renders assessment artifacts into delivery intents. It receives them from the hub.
- `prism-ai` may help classify unstructured input or explain an assessment through a hub-owned port. It never decides that an alert is issued.
- Clients, including a `0x1` broadcast adapter, depend on the hub.

## Relationship to nearby products

| Neighbor | Relationship |
| --- | --- |
| `prism-mail` | Peer producer of signal-like artifacts; may be a source kind |
| `artificial-intelligence` `aiai-signal` | Unrelated: it validates behavioral signals with a closed schema. It shares a name only and must not be reused for geo evidence |
| `nilx-one/0x1` | Owns the H3 profile, broadcast envelope, and disclosure rules |
| `nilx-one/core` | Implements `0x1` behavior once an owning contract exists |

## Forbidden coupling

- shared mutable databases with the hub or any peer;
- client-domain concepts in `prism-signal-core`;
- provider or transport tokens in the signal layer;
- client identities or subscriptions in any signal input;
- hard-coded grid resolutions outside a `0x1`-bound profile;
- reading a wall clock inside fusion.
