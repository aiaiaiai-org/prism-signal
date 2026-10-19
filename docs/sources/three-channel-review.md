# Three additional Telegram sources: initial compatibility review

Reviewed 2026-10-20 (Europe/Kyiv), against master 53b1ff764d538088e9c3dda585969afe16d21f42.

## Evidence and scope

Inspected the public previews of [kpszsu](https://t.me/s/kpszsu),
[war_monitor](https://t.me/s/war_monitor), and [monitorwarr](https://t.me/s/monitorwarr).
This is a qualitative preview sample, not a historical backfill or a labelled precision/recall
evaluation. No percentage coverage or calibrated TTL can be claimed from it.
The preview may be cached; inspection time is not publication time.

Three short excerpts with resolved post URLs are replayed in
`crates/prism-signal-runtime/tests/new_sources.rs`:
[kpszsu/88489](https://t.me/kpszsu/88489),
[war_monitor/47650](https://t.me/war_monitor/47650),
[monitorwarr/42828](https://t.me/monitorwarr/42828).
Replay timestamps are explicitly synthetic. Remaining test cases are synthetic boundary cases.

## Per-source findings

| Source | Observed format | Existing reader and change |
| --- | --- | --- |
| kpszsu | Explicit UAV terms, city directions, near-city directions, regional reports and full-length guided-bomb wording | Explicit UAV plus known target already works. No special parser is needed for that case. Full guided-bomb phrases and regions need further coverage. |
| war_monitor | Ballistic abbreviation, city-first descent reports, reply-only cancellation and general ballistic all-clear | Add exact BR abbreviation for this source. City-first reports remain mentions under the current cue rules; kindless replies and unlocated clears remain unlocated. |
| monitorwarr | Standalone jet-target shorthand, multiline routes, regional directions, later context-only updates and reconnaissance notices | Add exact noun forms of the shorthand for this source only. Interpret as the existing generic drone class, an explicit lexical interpretation; do not claim a new JetDrone subtype, speed, count or trajectory. |

Source rules are keyed by the collector's `telegram.channel:<username>` identifier.
They are not the bot's display-only `telegram:<username>` IDs.
Only reviewed exact words are added; the adjective meaning reactive/jet is not a drone noun.
Both clause reading and comma/all-clear splitting use the same source vocabulary.

## Evidence to signal

The Telegram adapter provides Evidence with source ID, external post ID, text, publication time,
forward information and URL. The default normalizer produces readings; bridge converts only
known target/via places into located SignalObservations. The runtime then assesses observations
into episodes and events. Tests exercise that public runtime path and retain source/post/URL
provenance. They do not fetch live Telegram or test preview HTML extraction.

A known city without a direction cue is still only a mention. Unknown places and regions are
not guessed as city coordinates. Separate posts never inherit a hazard merely because they
are adjacent. Reconnaissance, silence and lost tracking are not all-clears. Forwarded readings
are dropped by bridge rather than counted as independent corroboration.

## Multi-source blocker

**Do not activate the four-source rollout until [#14](https://github.com/aiaiaiai-org/prism-signal/issues/14)
is resolved.** Fusion currently indexes an episode by hazard class and place. Its Clear branch
can retract the entire episode using a clear from any source, despite other live source reports.
The same-post contradiction guard does not prevent this cross-source case.

Copied text without Telegram forward metadata can also count as multiple sources: provenance
is retained, but independent origin is not established. Do not interpret source count as
independently verified truth.

## Remaining work and activation gate

- Capture a dated, bounded history per channel and label positives/negatives, edits, replies,
  summaries, region-only reports and unknown place names. Compare expected outcomes and report
  false positives/misses per source. The original 495-post single-channel study does not apply here.
- Review city-first syntax, guided-bomb phrases, abbreviations, long multiline posts and missing
  gazetteer aliases. The 500-character whole-post cutoff can drop a live multiline update.
- Define reply-linked lifecycle and region geometry separately; neither is implicit here.
- Resolve #14 with per-source expiry/clear semantics and deterministic lifecycle regressions.
- Revisit TTLs: bridge defaults were estimated on one channel and are not calibrated for these three.
- After green CI and policy verification, update Prism Hub's pinned PRISM_SIGNAL_REV to the reviewed
  runtime commit, build/validate its image, and only then activate collection and matching bot display.
  The source-list configuration PRs alone do not ship this parser.

No merge or deployment is performed by this review.

<!-- © 2026 aiaiaiai · aiaiaiai.org -->
