# Normalization

`prism-signal-normalize` turns the text of an `Evidence` item into located `SignalObservation`s. It is a deterministic reading of informal text, not an authority. Every observation it produces is uncalibrated evidence and carries no confidence value.

## From a line to observations

The text is read line by line. Each line is split into clauses at `. , ; : ! ?`, an em or en dash, or a free-standing hyphen.

A hazard mention yields one observation for each place in its clause span:

- a place belongs to the nearest clause at or before it that names a hazard;
- places ahead of the first such clause (`Киев: 2 баллистики`) belong to that first clause.

A mention with no place in its span yields nothing and is reported as unlocated. `2 баллистики по вектору Конотоп/Нежин/Киев` yields three ballistic-missile observations with `count = 2`, one for each city.

| Field | Source |
| --- | --- |
| `kind` | lexicon match (see below) |
| `stance` | `clear` when a clear word covers the mention's clause, otherwise `threat` (see below) |
| `count` | a number up to two words before the kind word in the same clause, if any |
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

The scope of a clear word is its own clause:

| Line | Reading |
| --- | --- |
| `минус по мопеду над Одессой, 2 баллистики на Киев` | the drone at Odesa is clear; the ballistic missiles at Kyiv are a threat |
| `мопеды над Николаевом - минус` | the clear clause names no kind, so it clears the nearest earlier clause that does |
| `минус, 2 шахеда на Одессу` | a clear word never reaches forward, so this is a threat |

`реактивн-` makes an attack drone a jet drone only within its own clause.

When a line names a ballistic missile, a generic `ракета` with the same stance in that line is not counted separately.

## Gazetteer

The gazetteer is tab-separated data: `id`, `name`, `lat`, `lon`, GeoNames feature code, `population`, and comma-separated aliases.

`Gazetteer::ukraine()` loads the bundled file `crates/prism-signal-normalize/data/gazetteer-ua.tsv`. `scripts/build_gazetteer.py` generates it from the GeoNames `UA.zip` dump. It keeps:

- populated places of at least 1,000 inhabitants;
- every national, regional, and district seat, whatever its population;
- Cyrillic aliases, with stress marks removed.

City sections (`PPLX`) and historical places are left out, because a Kyiv district such as Vynohradar would otherwise outweigh the Odesa-oblast village of the same name. The generator is deterministic. GeoNames data is licensed CC BY 4.0, and the file header carries that attribution.

To refresh the file:

```bash
curl -sSO https://download.geonames.org/export/dump/UA.zip
python3 scripts/build_gazetteer.py UA.zip > crates/prism-signal-normalize/data/gazetteer-ua.tsv
```

Some tests use a small synthetic gazetteer with placeholder coordinates. They check which names are read, not positions.

Lookup rules:

- A name must start with a capitalized word. This keeps common nouns that are also village names out.
- One case ending is removed before names are compared, so `Киеву`, `Одессы`, and `Гостомеля` match `Киев`, `Одесса`, and `Гостомель`. `-ов`/`-ів` are kept, because they belong to names like `Фастов`.
- In a multi-word name, the words after the first also compare without vowels, so `Белой Церкви` matches `Белая Церковь`.
- A name followed by `район`, `область`, `обл.`, or `громада` is the administrative area, not the town, and is skipped.
- Names that several places share resolve as follows:
  - a higher administrative rank wins;
  - within the same rank, a place wins only with at least ten times the population of the next one;
  - otherwise the name is ambiguous.

## Known limits

- One-word names shorter than four letters do not match an inflected form.
- Ukrainian locatives with a vowel change (`Київ` / `Києві`) do not match.
- Oblasts, districts, "from the sea", and directions such as "south of" are not places in this model. The gazetteer holds only point positions, and a point would misstate an area.
- An oblast named next to a place (`Виноградара Одесской области`) is not used to pick among namesakes.
- A clause's places are not given roles. In `к Киеву со стороны Гостомеля`, both Kyiv and Hostomel are observations for the same hazard.
- Clauses follow punctuation, not grammar. A clear and a threat written in one clause without punctuation read as clear.
- A reply's quoted text is not part of the evidence, so a clear line such as `минус по обоим` that names no place is only reported as unlocated.
- The default TTLs are working values, not measured ones.
