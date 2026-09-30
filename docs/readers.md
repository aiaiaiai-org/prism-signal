# Two readers

Two crates turn channel text into observations. They were written independently and are not reconciled. Fusion does not care which produced its input, and the runtime lets a caller choose (`normalize` accepts `reader`).

| | `prism-signal-observe` | `prism-signal-normalize` + `prism-signal-bridge` |
| --- | --- | --- |
| Gazetteer | about 3,300 places from GeoNames | 117 curated places, with spellings |
| Place roles | none: `к Киеву со стороны Гостомеля` gives Kyiv and Hostomel alike | `target`, `via`, `origin`, `mention`; only target and via become observations |
| Counts (`2 шахеда`) | yes | no |
| Jet drones | yes | no |
| Kind carry-over between lines | no | yes, on explicit signals |
| All-clear scope | its own clause; a kindless clear reaches one clause back | its own comma segment; needs kind and place to become an observation |
| Regions | not places | not places |

## Measured on the same 495 posts

| | `observe` | `normalize` |
| --- | --- | --- |
| places under threat | 454 | 626 |
| posts with a located threat that only this reader found | 15 | 66 |
| posts both found | 249 | 249 |
| places the reader alerts on that the text names as where a threat *came from* | **9** | 0 |
| all-clear observations | 13 | 13 (through the bridge; the reader itself found 81 all-clear readings, and the bridge drops the ones that cannot be located) |

The nine are real: Hostomel, Boryspil, Bashtanka, Halytsynove, Radsad, Obukhiv, Slavutych. Each would alert people beside a launch site. Most launch sites are near their target, which hides the error in a big city and shows it at a village. `observe.md` lists it as a known limit.

Neither reader is right everywhere. `observe` has the larger vocabulary and counts; `normalize` has roles and a stricter all-clear. The default is `normalize` because a launch site is not a danger and a false alert costs trust, but that is a judgement on one channel.

## What converging would take

The clean end state is one reader. Two moves would get there, and both are for the repository owner to choose:

1. Give `observe` roles and the stricter all-clear rules (negation, no interceptions, comma scope), keeping its gazetteer and counts.
2. Give `normalize` the `observe` gazetteer and counts.

Until then the bridge keeps the two behind one contract, and the safety rules that matter live where both feed them: in fusion.
