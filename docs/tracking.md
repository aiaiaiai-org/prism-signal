# Threat tracking

Status: **design**. Nothing below is built yet; the order of work is at the end.

## Why

Fusion v1 thinks in places: one assessment is "drones at Kyiv". A person under an attack thinks
in objects: "four Shaheds are heading for Kyiv oblast, one was shot down over Brovary, so three
may still be coming". Today:

- `4 шахеди на Київщину` and `2 шахеди на Бровари` are two unrelated assessments, and the count
  is dropped by the production reader (`normalize`).
- `збито 1 реактивний над Броварами` is ignored by `normalize` (correctly: it is not an
  all-clear), and read as a full clear of every drone at Brovary by `observe` (a false all-clear
  when others are still flying).
- A threat ends only on an explicit clear or when its window lapses, and a lapse is shown to
  nobody. Silence ends an alert that the source never ended.

Tracking adds the missing layer: a **track** is one object, followed from the first report to
an explicit end.

## What a track is

| Field | Meaning |
| --- | --- |
| `track_id` | stable: policy, class, the first evidence, and the object's index in it (`…/43219#3`) |
| `kind` | the most specific kind reported for it (`air.jet_drone`, `air.ballistic_missile`, …) |
| `path` | the places it was reported at or passing, with times, in order |
| `target` | the last place a report aimed it at (`на`, `в сторону`, `над`), if any |
| `cells` | its **corridor**: the cells from its last known place to its target, see below |
| `status` | `active`, `neutralized`, `cleared`, or `quiet` |
| `last_seen_at` | the newest report that concerned it |
| `evidence` | every report that concerned it, with sources |

A report with a count makes that many tracks: `4 шахеди на Київщину` is four tracks with one
target. A report without a count (`шахеди на Київ`) makes one track marked `count_unknown`: it
stands for "at least one" and is never taken to be exactly one when objects are subtracted.

## Following a track

A new threat report is matched against the active tracks of the same class:

1. **Same target.** A report aimed at a track's target, or at a place on its corridor, continues
   it.
2. **Reachable.** Otherwise a report continues a track only if the place is reachable from the
   track's last place in the elapsed time, at the kind's top speed with a margin, and lies
   roughly toward its target when it has one.
3. **Counts.** A report of `n` objects continues at most `n` tracks; if fewer match, the rest are
   new tracks. A report never shrinks the number of active tracks.
4. **Ambiguity** resolves toward more tracks, not fewer: when two groups could explain a report,
   it starts its own track rather than guess. A duplicate on a board is noise; a missing threat
   is harm.

Working top speeds, to be calibrated on the archive like the windows in [`fusion.md`](fusion.md):

| Kind | km/h | Note |
| --- | --- | --- |
| `air.attack_drone` | 200 | Shahed type, 150–185 cruising |
| `air.jet_drone` | 700 | |
| `air.missile` | 1000 | cruise or unspecified |
| `air.ballistic_missile` | — | minutes from launch to target: a report continues a track only at the same target |
| `air.guided_bomb` | — | glide range: same target, or within 70 km of the track's last place |

## Ending a track

| Report | Effect |
| --- | --- |
| `збито / знищено / сбит / уничтожен`, with a count `n` (1 when none) | **neutralizes** `n` tracks of that kind: the ones closest to the named place among those that could be there. The others stay active. A neutralization that matches nothing ends nothing |
| explicit clear naming a kind and a place (`мінус по мопедах над Одесою`) | **clears** every track of that class whose last place or target is that place, from that source, as fusion v1 does now |
| explicit clear naming a place but no kind (`Київ — відбій`) | clears that source's tracks of every class at that place |
| a clear that names no place (`небо чисте`, `мінус по всіх`) | clears **only the tracks that source itself reported** in the last hour, and only when that source's own recent reports name no other active place. Otherwise nothing |
| silence | nothing ends. After the kind's quiet time the track becomes `quiet`: still shown, marked as having had no news |

So `4 шахеди на Київщину`, then `збито 1 над Броварами`, leaves three active tracks, and the
person is told that something may still be on the way. `4 знищено` ends all four. A neutralized
or cleared track is never reopened; a new report starts a new track.

`quiet` is not an end. A consumer shows it apart ("no news for 2 hours"), and it stops being
quiet when a report continues it. Quiet times start from the data in `fusion.md`: one hour for
drones, 30 minutes for missiles and bombs. A track quiet for six hours is closed as `stale`, and
that is shown as "no news since", never as an all-clear.

## Corridors

A track concerns everyone between where it was last seen and where it is going, not only those at
its target: `4 шахеди на Київ` reported over Chernihiv concerns Kozelets and Brovary too.

The corridor is the last place's disc, the target's disc, and the segment between them widened by
the kind's lateral margin (drones 15 km, missiles 25 km), covered with the same safe rule as any
geometry ([`geo-cells.md`](geo-cells.md)): one ring of neighbours, so nobody inside is missed.
A track with no target concerns its last place's disc only.

A target that is a region (`на Київщину`, `з боку Білорусі`) needs region boundaries, which the
gazetteer does not have yet. Until it does, such a target is recorded on the track and the
corridor ends at the last named place.

## What the hub gets

Assessments stay as they are, so nothing that uses them changes. The `assess` answer gains an
optional `tracks` list and track events (`started`, `moved`, `neutralized`, `cleared`, `quiet`,
`stale`) with the same `(id, seq)` idempotency. The hub keeps a person's active tracks until
one of those events ends them and builds the board and status from them; Porter writes the words.

## Not saying "all clear" when it is not

Tracking makes ending threats easier, so the guards matter more:

1. Only explicit words end a track. A neutralization ends exactly the number it names.
2. A track with `count_unknown` is never brought to zero by subtraction. Only a clear ends it.
3. A clear is scoped to its source, its place, and its class; a placeless clear is scoped to
   what that source itself reported.
4. When the person's zone still has any active or quiet track, no message may read as an
   all-clear, whatever ended.
5. Nothing here is an official air-raid alert or its cancellation.

## Order of work

Each step is its own pull request and leaves everything working:

1. **Reader** (`normalize`): counts per reading, and interceptions as their own phase with a
   count (`збито 2`), instead of being dropped. The bridge carries the count and a new
   `neutralized` stance; fusion v1 ignores that stance, so alerts do not change.
2. **Geo**: corridor cover (a widened segment between two discs).
3. **Fusion**: tracks, with policy tests replaying the archive's real posts, including the cases
   above.
4. **Regions**: oblast boundaries, so `на Київщину` reaches people in the oblast.
5. **Hub and Porter**: active tracks per person until ended, one board and one status per person
   through the bot's edit and delete contract, and the `observe` reader's interception reading
   aligned with step 1.
