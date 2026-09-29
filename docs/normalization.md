# Normalization

`prism-signal-normalize` turns the text of a post into hazard readings. It is the step between `Evidence` (what a source published) and a located observation (what fusion can combine).

```bash
cargo run -p prism-signal-collect -- telegram vanek_nikolaev backfill \
  | cargo run -p prism-signal-normalize -- --actionable
```

stdin is `Evidence` NDJSON as the collector prints it. stdout is one `Reading` per line and nothing else. A summary and the words the gazetteer did not know go to stderr, so every run is also a coverage report.

## What a reading is

A post is split into sentences (a newline, `.`, `!`, `?`, or `;` ends one). Each sentence gives up to one reading per hazard kind it names:

| Field | Meaning |
| --- | --- |
| `kind` | `drone`, `guided_bomb`, `cruise_missile`, `ballistic_missile`, `missile` (generic), or absent for a bare all-clear |
| `phase` | `threat`, or `cleared` when the sentence calls a threat off |
| `places[]` | known places with `role`, coordinates, and `reach_km` |
| `unresolved[]` | capitalised words after a target or via cue that the gazetteer does not know |
| `kind_inherited` | the kind was taken from an earlier sentence of the same post, on an explicit continuation signal |
| `url`, `forwarded` | the post's public link, and whether it was forwarded from elsewhere |

`Reading::actionable()` is true when a reading reports a threat of a known kind at a place with role `target` or `via`. It is a property of the text. Whether anyone is alerted is a hub policy applied to fused assessments, never this crate's decision.

## Roles are the point

Finding place names is easy. Knowing which ones are in danger is the work.

| Role | Cue | Example |
| --- | --- | --- |
| `target` | `на`, `к`, `над`, `по`, `в`, `в сторону`, `курсом на`, `по вектору`, `атакуют` | `2 баллистики на Киев` |
| `via` | `через`, `мимо`, `южнее`, `под`, `возле`, `между`, `пролетает` | `пролетает южнее Каменского` |
| `origin` | `с`, `из`, `от`, `со стороны` | `подлетает к Киеву со стороны Гостомеля` — Hostomel is where it came from |
| `mention` | none | `пролетели Киев дальше в сторону Фастова` — Kyiv is already behind it |

Rules, in the order they apply:

1. A run of cue words directly before a place decides its role. If the run disagrees (`курсом на/через`), the strongest role wins: `target` over `via` over `origin`.
2. Filler words such as `центром` do not break the run (`над центром Николаева`).
3. Failing a cue, a list item takes the role of the item before it (`к Киеву/Ирпеню, Буче и Броварам`), across `/`, `,`, and conjunctions but never across a sentence.
4. Otherwise the place is a `mention`.

`unresolved` only collects words that would have been `target` or `via`. Where a launch came from (`с Курска`) is not a gap in the gazetteer.

## Kinds, all-clears, and carry-over

- Kinds come from `data/lexicon.v1.json`, in both languages, including the channel's slang: `реактивный мопед` is a jet drone. `каб` is exact-form only, because as a prefix it would match `кабинет` and `кабель`.
- A generic `ракета` next to `баллистика` is the same threat named twice, so the generic kind is dropped.
- `минус`, `відбій`, `отбой` call a threat off. `минус по мопедам` is `phase: cleared, kind: drone`. A bare `минус по всему на Маяки` is `cleared` with no kind and its places kept.
- Only those words do. Reports of some interceptions (`сбито`, `сбития`), of a target no longer tracked (`не фиксируется`), and of no further launches (`больше не было`) say nothing about what is still in the air, and the channel itself writes `не фиксируются, но тревога всё ещё активна`. Treating them as all-clears sent a false `відбій` on the sample, as did the news words `знищено` and `сбито`.
- A negator directly before an all-clear word, or `нет`/`немає` within two words after it, voids it: `актуальна до отбоя тревоги` means the threat lasts until the all-clear, and `отбоя пока нет` says it has not come.
- Multi-word all-clear phrases are supported (`cleared.phrases`) but the shipped lexicon has none. A phrase never clears a threat by itself: it needs a stated kind, because phrases such as `больше не` turn up in ordinary speech.
- A sentence with a place but no kind of its own takes the kind of the nearest earlier threat sentence in the same post, but only on an explicit signal that it continues that threat. The reading says so in `kind_inherited`. Two signals count:
  - it refers back: it opens, in its first two words, with `эти`, `остальные`, `ещё`, `также` and the like (`эти летят на Кривой Рог`), or it carries the channel's noise idiom (`может быть громко в Николаеве`);
  - it is an item of a list: the earlier sentence is a header that names a kind but no place (`общая по мопедам:`) and this one opens with a number (`1 под Киевом`).

  A place with a cue is not a signal. `1 мопед на Киев ⏎ ПВО в Киеве работает` gives one reading, not two, and `1 мопед на Киев ⏎ 3 взрыва в Киеве` gives one too, because the earlier sentence already has its own place. A sentence that calls a threat off ends the carry-over. The rule is deliberately narrow: it costs recall (about 2% of actionable readings on the sample, for example `летит пока в сторону Кульбакино`) and buys the guarantee that unrelated prose never becomes a hazard report.
- Posts longer than 500 characters give no readings. Live alerts are one or two short lines; long posts are news and daily summaries of attacks already over. The limit is `Normalizer::max_text_chars`.

## Matching

Words are folded before comparison: lowercase, `ё є` to `е`, `і ї ы` to `и`, `ґ` to `г`, apostrophes dropped. Vocabulary files are written in natural spelling and folded on load, so `Кам'янське` and `Каменское` are both written as a human would and both match.

A place has stems, exact forms, exclusions, and a `max_suffix`:

- a stem accepts at most `max_suffix` further characters, so `киев` matches `Киеву` and `Киевом` but not `Киевщина` (the region);
- an exact form matches the whole word, for names such as `Суми` where a stem would match `сумма`;
- `not` vetoes lookalikes (`Николаевка` for `Николаев`);
- `capitalized` requires an initial capital for names that are also words: `Маяки`, `Ровно`, `Изюм`, `Авангард`;
- a stem with a space is a multi-word name (`крив рог`, `бел церк`).

Regions (`Киевская область`, `Николаевщина`) are deliberately not matched to their capitals. A region is not a city, and collapsing one onto the other would alert people who are nowhere near the threat.

## Place data

`data/gazetteer.v1.json` is generated by `scripts/build-gazetteer.py` from `data/gazetteer.spec.json` and the GeoNames Ukraine dump. Coordinates and identifiers come from GeoNames, licensed [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); the normalizer prints the attribution with every run. Spellings, exclusions, and reach radii are curated here.

`reach_km` is a population step (5, 8, 12, 20 km), a stand-in for a built-up footprint until real boundaries exist. GeoNames populations are stale for some frontline cities, so the spec can override it (Kherson).

To grow the gazetteer, run the normalizer over collected posts, read the `unresolved` report on stderr, find each GeoNames id (a name can match several villages; pick the one the channel means), add an entry to the spec, and rebuild:

```bash
python3 scripts/build-gazetteer.py            # downloads the dump
python3 scripts/build-gazetteer.py UA.txt     # or use a local copy
```

## Measured on the channel

On 495 posts from `vanek_nikolaev` spanning 26 days (collected 2026-09-29), 704 readings, 479 of them actionable, 154 with an inherited kind. The most frequent affected places were Kyiv, Mykolaiv, Odesa, Brovary, and Dnipro.

The other 129 threat readings have a kind but no `target` or `via` place. 68 of them use a region word, 17 look like launch reports, and 22 carry `unresolved` words. In rough order of size:

- **regions**: `к Киевской области`, `на севере Житомирской области`. Needs administrative boundaries, not centroids;
- **launch reports** with no Ukrainian place: `пуски баллистики с курска`. Correctly not actionable;
- **back-references**: `тем же курсом`, `в ту же сторону`;
- **unlisted places**, visible in `unresolved`: `Намыв` (a Mykolaiv district), `Криву Балку`, `Черноморское`.

These numbers describe recall against one channel and one gazetteer, not correctness. Precision was spot-checked by hand on the captured pages in `tests/` and on 40 random actionable readings from the 495 posts, with no wrong place found. Long summary posts were only partly read in that check. It is a smoke test, not a labelled evaluation, and none exists yet.

## Known limits

- A cue further than one word from the place is not seen: `1 реактивный мопед подлетает к пункту пропуска на границе с Польшей "Ягодин"` gives Yahodyn as a `mention`.
- Sentence-level reading cannot tell a live alert from a description of an earlier one that is short: `вчера были мопеды над Киевом` reads as a threat.
- Same-name places are resolved at curation time by the most notable one (`Костянтинівка` is the Donetsk town). A same-named place elsewhere is not disambiguated.
- Russian homonyms of Ukrainian towns (`Первомайск`, `Каменск`) are excluded only where a `not` entry says so.
- A lowercased name that is also a common word (`маяки`) is not read.
- The Telegram preview adapter does not observe edits or deletions, so a corrected post is only re-read if it is collected again.
- The count of drones or missiles in a post is not extracted.

## Invariants

1. Reading is a pure function of the post text and the vocabulary; nothing is remembered between posts.
2. A word the gazetteer does not know is reported, never guessed.
3. A region is never resolved to a city.
4. Only explicit all-clear words end a threat; an interception, a lost track, or a phrase never does.
5. This crate never sees who is subscribed or where they are.
