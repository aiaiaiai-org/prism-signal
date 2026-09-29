# Geo cells

## Purpose

Prism Signal addresses places by grid cell so that delivery can fan out by area without any component storing who is where.

## Grid profile

The grid is H3. The resolution profile is owned by `0x1`, not by Prism Signal. As of the current `0x1` specification the constants are:

| Use | H3 resolution | Owner |
| --- | --- | --- |
| Broadcast grid | 7 | [`0x1` Proximity, Relay, and Broadcast](https://github.com/nilx-one/0x1/blob/master/documents/11-proximity-relay-and-broadcast.md) |
| Proximity and map aggregation | 8 | same |

Prism Signal reads the profile through configuration bound to a `0x1` contract version. It never hard-codes a resolution in core logic. Changing a resolution never alters observation evidence.

## Covering a zone

A zone is a polygon. `cover(polygon, resolution)` returns the cells whose centers fall inside it, using the H3 polygon fill, with a declared bound:

- `max_cells` is part of the request; a larger cover fails with `cover_too_large` rather than silently truncating;
- the result is sorted and deduplicated, so equal input gives equal output;
- an optional compaction step may emit parents for transport, but the canonical form is the uniform resolution set.

Cell identifiers are the 64-bit H3 index rendered as a lowercase hexadecimal string, following the binding-safe identifier rule of the `0x1` Core contract.

## Fan-out without a subscriber registry

```text
zone polygon -> cover(res 7) -> [cell_h3_res7, ...] -> one message per cell
client: coordinate -> own cell + bounded neighbors -> listens on those cell topics
```

No component maps people to cells. The mapping from coordinates to a cell is a pure function that a client evaluates locally; the transport only needs the cell key. This matches the `0x1` broadcast body, which already targets `cell_h3_res7`, and its `1 <-> 9` neighbor tolerance for boundary placement.

What this does and does not remove:

| Concern | Outcome |
| --- | --- |
| Backend table of user locations | Not required |
| Broker or push-service topic state | Still exists; it is transport state, not location history |
| Which cell a client listens on | Visible to the transport as coarse location |
| Reaching a closed app | Needs the platform push service; topic limits apply |
| Reaching a bot user (Telegram) | Needs a chat identifier; cell fan-out alone cannot address them |

To reduce disclosure, a client may subscribe to a coarser parent cell and filter finely on the device. Coarse regional delivery still reveals which bundle was requested; it must not be described as zero-knowledge, consistent with the `0x1` map contract.

## Routing choice is not decided here

`0x1` lists as open whether a broadcast reaches every attested client in a cell or only clients within `N` edges of the sender's local relationship projection. Prism Signal does not resolve that question. It emits a `CellSet`; the delivery layer applies whichever routing `0x1` settles on.

## Invariants

1. A cell is derived from geometry and never stored as evidence.
2. Covers are bounded, sorted, and deduplicated.
3. Resolution comes from a `0x1`-bound profile, not from core code.
4. No Prism Signal output identifies or locates a person.
