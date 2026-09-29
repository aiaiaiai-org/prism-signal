# Normalization

`prism-signal-normalize` turns the text of an `Evidence` item into located `SignalObservation`s. It is a deterministic reading of informal text, not an authority. Every observation it produces is uncalibrated evidence and carries no confidence value.

## From a line to observations

The text is read line by line. A line becomes observations only when it names both:

1. at least one hazard kind from the lexicon, and
2. at least one place the gazetteer resolves.

It then yields one observation per kind and per place. `2 баллистики по вектору Конотоп/Нежин/Киев` yields three ballistic-missile observations with `count = 2`, one for each city.

| Field | Source |
| --- | --- |
| `kind` | lexicon match (see below) |
| `stance` | `clear` when the line contains a clear word, otherwise `threat` |
| `count` | a number up to two words before the kind word, if any |
| `observed_at` | `Evidence.published_at` |
| `geometry` | the gazetteer position of the place, as a point |
| `ttl` | `NormalizeRules`, per kind for threats and one value for clear |
| `confidence` | always absent: the channel declares none |
| `provenance` | evidence id and URL, normalizer version, matched words, gazetteer place id |

Nothing is guessed. These cases yield no observation and are reported in `Normalized::skipped` instead:

| Case | Reason |
| --- | --- |
| A forwarded item | `forwarded`: it is not the source's own report |
| A hazard but no resolvable place | `unlocated` |
| A name several places share with no clear winner | `ambiguous_place`, with the candidate ids |

## Lexicon

| Kind | Words (prefix match on the lowercase form) |
| --- | --- |
| `air.ballistic_missile` | баллист-, балліст-, іскандер-/искандер-, кинжал-/кинджал- |
| `air.missile` | ракет-, крылат-/крилат-, калибр-/калібр-, циркон-, оникс-/онікс-, х-101/х-22/х-59/х-69 |
| `air.attack_drone` | шахед-/шахід-, мопед-, бандерол-, бпла, дрон-, герань/герані |
| `air.jet_drone` | an attack-drone word in a line that also contains реактивн- |
| `air.guided_bomb` | whole words каб, каба, кабы, кабов, каби, кабів, фаб…, and умпб-/умпк |

Clear words: минус/мінус, отбой/відбій, сбит-/збит-, уничтож-/знищ-.

When a line names a ballistic missile, a generic `ракета` in the same line is not counted separately.

## Gazetteer

The gazetteer is tab-separated data: `id`, `name`, `lat`, `lon`, GeoNames feature code, `population`, and comma-separated aliases.

The production file is generated from GeoNames with `scripts/build_gazetteer.py`. It keeps populated places of at least 1,000 inhabitants plus every national, regional, and district seat, and it keeps the Cyrillic aliases. GeoNames data is CC BY 4.0, and the generated file carries that attribution. The environment that builds this repository cannot reach GeoNames yet, so tests use a small synthetic gazetteer whose coordinates are placeholders.

Lookup rules:

- A name must start with a capitalized word. This keeps common nouns that are also village names out.
- One case ending is removed before names are compared, so `Киеву`, `Одессы`, and `Гостомеля` match `Киев`, `Одесса`, and `Гостомель`. `-ов`/`-ів` are kept, because they belong to names like `Фастов`.
- In a multi-word name, the words after the first also compare without vowels, so `Белой Церкви` matches `Белая Церковь`.
- Names that several places share resolve as follows:
  - a higher administrative rank wins;
  - within the same rank, a place wins only with at least ten times the population of the next one;
  - otherwise the name is ambiguous.

## Known limits

- One-word names shorter than four letters do not match an inflected form.
- Ukrainian locatives with a vowel change (`Київ` / `Києві`) do not match.
- Oblasts, districts, "from the sea", and directions such as "south of" are not places in this model. The gazetteer holds only point positions, and a point would misstate an area.
- A line's places are not given roles. In `к Киеву со стороны Гостомеля`, both Kyiv and Hostomel are observations for the same hazard.
- A reply's quoted text is not part of the evidence, so a clear line such as `минус по обоим` that names no place is only reported as unlocated.
- The default TTLs are working values, not measured ones.
