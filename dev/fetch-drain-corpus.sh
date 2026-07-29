#!/usr/bin/env bash
# Fetch the loghub_2k ground-truth corpus used by `just drain-accuracy`.
#
# The corpus is NOT vendored: it is third-party data with its own terms, it is a
# few MB, and it is only needed when someone is measuring template mining. This
# script downloads it on demand into a gitignored directory, like the fuzzing
# corpus it sits alongside — both are manual, neither runs in CI.
#
# Only the `*_2k.log_structured.csv` files are fetched. Each row already carries
# the message text (`Content`) and the ground-truth cluster it belongs to
# (`EventId`, with `EventTemplate` for reporting), so the raw logs are redundant:
# mining `Content` is exactly the comparison the published benchmark makes.
#
# Output: one `<Dataset>.records` per dataset, three US-separated (0x1f) fields
# per line — EventId, EventTemplate, Content. US is used because log messages
# contain commas, quotes and tabs but never control characters; a record that
# does contain one (or an embedded newline) is reported and skipped rather than
# silently corrupting a line's field boundaries.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

dest="${1:-target/drain-corpus}"
base_url="https://raw.githubusercontent.com/logpai/loghub/master"

# The 16 datasets of the loghub_2k benchmark suite, 2000 annotated lines each.
datasets=(
    Android Apache BGL Hadoop HDFS HealthApp HPC Linux
    Mac OpenSSH OpenStack Proxifier Spark Thunderbird Windows Zookeeper
)

if ! command -v curl >/dev/null 2>&1; then
    echo "error: curl is required to fetch the drain corpus." >&2
    exit 1
fi
if ! command -v python3 >/dev/null 2>&1; then
    echo "error: python3 is required to extract the drain corpus." >&2
    exit 1
fi

mkdir -p "$dest/csv"

missing=0
for d in "${datasets[@]}"; do
    if [[ -s "$dest/$d.records" ]]; then
        continue
    fi
    missing=1
    if [[ ! -s "$dest/csv/$d.csv" ]]; then
        echo "==> fetching $d"
        curl -fsSL --retry 3 --retry-delay 2 -m 120 \
            -o "$dest/csv/$d.csv" \
            "$base_url/$d/${d}_2k.log_structured.csv"
    fi
done

if [[ "$missing" -eq 0 ]]; then
    echo "drain corpus already present in $dest (delete it to re-fetch)"
    exit 0
fi

echo "==> extracting records"
python3 - "$dest" "${datasets[@]}" <<'PY'
import csv, sys, os

dest = sys.argv[1]
US = "\x1f"

for name in sys.argv[2:]:
    out_path = os.path.join(dest, name + ".records")
    if os.path.exists(out_path) and os.path.getsize(out_path) > 0:
        continue
    src = os.path.join(dest, "csv", name + ".csv")
    with open(src, newline="", encoding="utf-8", errors="replace") as fh:
        rows = list(csv.DictReader(fh))
    kept, skipped = [], 0
    for row in rows:
        fields = (row["EventId"], row["EventTemplate"], row["Content"])
        # A control character would break the record framing; such a row is
        # dropped loudly rather than corrupting the ones around it.
        if any(any(c in f for c in "\x1f\n\r") for f in fields):
            skipped += 1
            continue
        kept.append(US.join(fields))
    with open(out_path, "w", encoding="utf-8") as fh:
        fh.write("\n".join(kept) + "\n")
    note = f" ({skipped} row(s) skipped: control characters)" if skipped else ""
    print(f"    {name}: {len(kept)} records, "
          f"{len({r.split(US)[0] for r in kept})} ground-truth clusters{note}")
PY

echo "drain corpus ready in $dest"
