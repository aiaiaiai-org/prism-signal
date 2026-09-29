#!/usr/bin/env python3
# © 2026 aiaiaiai · aiaiaiai.org
# SPDX-License-Identifier: Apache-2.0
"""Builds crates/prism-signal-normalize/data/gazetteer.v1.json.

The human-curated input is data/gazetteer.spec.json: which GeoNames places to include and how
each one is spelled in running text (stems, forms, exclusions). This script adds what must not
be typed from memory: coordinates and a population-derived reach radius, both read from the
GeoNames Ukraine dump (https://download.geonames.org/export/dump/UA.zip, CC BY 4.0).

    scripts/build-gazetteer.py [path/to/UA.txt]

Without an argument the dump is downloaded into a temporary directory. Standard library only.
The result is written deterministically, so a rebuild with unchanged inputs is a no-op diff.
"""

import io
import json
import sys
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent / "crates" / "prism-signal-normalize" / "data"
SPEC = ROOT / "gazetteer.spec.json"
OUT = ROOT / "gazetteer.v1.json"
DUMP_URL = "https://download.geonames.org/export/dump/UA.zip"

# GeoNames columns: 0 id, 1 name, 2 asciiname, 4 lat, 5 lon, 6 feature class, 7 feature code,
# 8 country, 10 admin1, 14 population.


def reach_km(population: int) -> int:
    """Radius in km around the place centre within which someone counts as being 'there'.

    A coarse population step, not a measurement: it stands in for the built-up footprint until
    real boundaries are available. A curated `reach_km` in the spec overrides it, because
    GeoNames populations are stale for some frontline cities.
    """
    if population >= 200_000:
        return 20
    if population >= 50_000:
        return 12
    if population >= 10_000:
        return 8
    return 5


def load_dump(arg):
    if arg:
        text = Path(arg).read_text(encoding="utf-8")
    else:
        with urllib.request.urlopen(DUMP_URL, timeout=120) as response:
            archive = zipfile.ZipFile(io.BytesIO(response.read()))
        text = archive.read("UA.txt").decode("utf-8")
    rows = {}
    for line in text.splitlines():
        cols = line.split("\t")
        rows[int(cols[0])] = cols
    return rows


def main():
    rows = load_dump(sys.argv[1] if len(sys.argv) > 1 else None)
    spec = json.loads(SPEC.read_text(encoding="utf-8"))
    places = []
    seen = set()
    for entry in spec["places"]:
        gid = entry["geonames_id"]
        if gid in seen:
            sys.exit(f"duplicate geonames_id {gid}")
        seen.add(gid)
        row = rows.get(gid)
        if row is None or row[6] != "P" or row[8] != "UA":
            sys.exit(f"geonames_id {gid} ({entry['name']}) is not a Ukrainian populated place")
        population = int(row[14] or 0)
        place = {
            "id": f"geonames:{gid}",
            "name": entry["name"],
            "lat": round(float(row[4]), 4),
            "lon": round(float(row[5]), 4),
            "reach_km": entry.get("reach_km", reach_km(population)),
        }
        match = {k: entry[k] for k in ("stems", "forms", "not", "max_suffix", "capitalized") if k in entry}
        if not match.get("stems") and not match.get("forms"):
            sys.exit(f"{entry['name']} has neither stems nor forms")
        place["match"] = match
        places.append(place)
        print(
            f"{gid:>9} {entry['name']:<24} {row[2]:<24} pop={population:>8} "
            f"adm1={row[10]} {place['lat']:.4f},{place['lon']:.4f} reach={place['reach_km']}",
            file=sys.stderr,
        )
    document = {
        "version": "gazetteer.v1",
        "attribution": (
            "Coordinates and identifiers from GeoNames (https://www.geonames.org), "
            "licensed CC BY 4.0. Spellings and reach radii are curated by aiaiaiai."
        ),
        "places": places,
    }
    OUT.write_text(json.dumps(document, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({len(places)} places)", file=sys.stderr)


if __name__ == "__main__":
    main()
