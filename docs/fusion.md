# Fusion

`prism-signal-fusion` turns a window of located observations into **assessments** with a lifecycle. `assess(observations, policy, evaluation_time)` is a pure function: no clock, no state, no knowledge of who is subscribed or where anyone is. The same window always gives byte-identical output, whatever the order of its observations.

## What an assessment is

One assessment is one *episode* of one *class* of hazard at one *place*: "drones at Kyiv, since 22:19". It carries:

| Field | Meaning |
| --- | --- |
| `assessment_id` | stable: policy, class, place, and the first evidence. `fusion.v1/drone/geonames:703448/vanek_nikolaev/43219` |
| `class` | `drone`, `bomb`, or `missile`. Cruise, ballistic, and unspecified missiles are one class, and so are attack and jet drones: they mean the same to someone deciding whether to shelter |
| `kinds` | the specific kinds the evidence named |
| `place` | id and display name, when the reader gave them |
| `cells` | the H3 cells (resolution 6, about 3 km) a person must be standing in to be concerned |
| `valid_from`, `valid_until` | the first report, and when it lapses unless renewed |
| `status` | `active`, `expired`, or `retracted`, as of the evaluation time |
| `revision` | grows with every renewal |
| `likelihood`, `corroboration_count` | a band, never a percentage: `moderate` for one source, `high` for two or more |
| `evidence` | the reports behind it, oldest first, each with its public URL |

It is a proposal. Whether to tell anyone is the consumer's decision.

## Cells

Each observation's geometry is covered at the policy's resolution and widened by one ring of neighbours. A bare point is first drawn as a 10 km disc.

The ring is what makes the cover safe. A person is placed in the cell that contains them, and a cover holds the cells whose centres lie inside the zone, so someone just inside the edge can stand in a cell whose centre is just outside. One ring closes that gap in the safe direction: it may alert someone slightly outside the zone, and never miss someone inside. A test walks people round a zone at 0.99 of its radius to prove it.

The reader decides how big a place is. `prism-signal-bridge` draws each place as a disc of its own reach (5, 8, 12, or 20 km by size), so fusion needs no idea of what a village and a city are.

## Lifecycle

| Event | When |
| --- | --- |
| `issued` | the first threat observation of an episode |
| `superseded` | a later post reports the same hazard at the same place while the assessment is valid; the window is extended |
| `expired` | the window ran out with no newer report. Effective when it ran out, not when it was noticed |
| `retracted` | the source calls that hazard off at that place |

`(assessment_id, seq)` is unique and never reused, so a consumer applies events idempotently. Events carry the evidence that caused them and an `effective_at` derived from the evidence or the window, never from a clock. After a retraction or an expiry the next threat starts a new episode with a new id.

An `expired` event is only produced once the evaluation time has passed `valid_until`. Evaluating an hour earlier gives an active assessment and no `expired` event; the window is what a consumer supplies, and the events are what that window implies.

## Not saying "all clear" when it is not

A false all-clear is the worst thing this layer can produce. A retraction is deliberately narrow:

1. It needs an explicit `clear` observation, which the readers only produce from explicit all-clear words.
2. It applies to the **same class at the same place**. Overlapping cells are not enough: an all-clear for Brovary must not end the alert for Kyiv, whose people may still be in danger. A test pins it.
3. It applies only to an episode still valid when the all-clear was reported, and never to a later threat.
4. It is ignored when its own post also reports that class at that place as a threat.
5. An all-clear that names no place, or no kind, is not evidence this layer can use. The threat it meant to end lapses on its window instead.

Point 5 has a price. On 495 live posts the reader found 81 all-clear readings; 13 could be located to a kind and a place and 10 of those ended an assessment. The rest, mostly `минус по всем этим мопедам`, end nothing, and the alert lapses on its window (30 minutes for drones, 15 for bombs, 20 for missiles). A missed all-clear leaves a threat to lapse; a misapplied one hides a live one, and the second is the worse failure.

**A consumer must not send "all clear" on a retraction alone.** If another active assessment of the same class covers the person's cell, they are still in danger and the retraction does not concern them.

## Measured on the channel

`normalize` then `assess` over 495 posts from `vanek_nikolaev` spanning 26 days:

| | |
| --- | --- |
| observations | 639: 626 threats, 13 all-clears |
| assessments | 452: 442 expired, 10 retracted |
| events | 452 issued, 157 superseded, 442 expired, 10 retracted |
| left out | 69 all-clears that could not be located, 2 forwarded posts |

Every one of the 10 retractions was read against its source post. Nine were correct. The tenth was a false all-clear, `по первым 2 КАБам минус, еще 2 КАБа подлетают к Черноморску`, in which the reader had joined two statements across a comma. It was fixed in the reader and is pinned by a policy test.

The validity windows are rounded up from that data: 90th percentile of the time from a kind's last report to the channel's own all-clear was 22 minutes for drones, 9 for guided bombs, 18 for missiles. One channel and a small sample: they are working values, and a deployment should set its own.

## Not built

- Regions: a report about an oblast reaches nobody. Regions need boundaries, not points.
- A second policy. `fusion.v1` is the only one, and the parameters are a struct, not a file.
- Severity. The contract has none for hazards yet.
- Corroboration across sources is implemented and tested, but only one source exists.
