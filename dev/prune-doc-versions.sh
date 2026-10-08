#!/usr/bin/env bash
set -euo pipefail

dry_run=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)
            dry_run=true
            shift
            ;;
        *)
            echo "error: unknown argument '$1'" >&2
            echo "usage: $0 [--dry-run]" >&2
            exit 1
            ;;
        esac
done

repo_root=$(git rev-parse --show-toplevel 2>/dev/null || true)
if [[ -z "$repo_root" ]]; then
    echo "error: this script must run inside the git repository." >&2
    exit 1
fi
cd "$repo_root"

git fetch origin gh-pages >/dev/null 2>&1 || true

versions_file=$(mktemp)
trap 'rm -f "$versions_file"' EXIT

if ! git show origin/gh-pages:versions.json >"$versions_file" 2>/dev/null; then
    echo "error: could not read origin/gh-pages:versions.json. Ensure gh-pages has been published at least once." >&2
    exit 1
fi
export KELORA_DOCS_VERSIONS_FILE="$versions_file"

plan=$(
python3 - <<'PY'
import json
import os
import re

with open(os.environ["KELORA_DOCS_VERSIONS_FILE"], "r", encoding="utf-8") as fh:
    versions = json.load(fh)

milestones = {
    item.strip()
    for item in os.environ.get("KELORA_DOCS_KEEP_MILESTONES", "v1.0.0").split(",")
    if item.strip()
}

release_re = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
keep = {"dev"} | milestones
release_rows = []

for row in versions:
    version = row["version"]
    aliases = set(row.get("aliases", []))
    if "latest" in aliases:
        keep.add(version)

    match = release_re.match(version)
    if not match:
        continue
    major, minor, patch = map(int, match.groups())
    release_rows.append((major, minor, patch, version))

latest_patch_by_minor = {}
for major, minor, patch, version in release_rows:
    key = (major, minor)
    if key not in latest_patch_by_minor or patch > latest_patch_by_minor[key][0]:
        latest_patch_by_minor[key] = (patch, version)

for patch, version in latest_patch_by_minor.values():
    keep.add(version)

delete = [
    row["version"]
    for row in versions
    if row["version"] not in keep
]

for version in sorted(keep):
    print(f"KEEP\t{version}")
for version in delete:
    print(f"DELETE\t{version}")
PY
)

keep_versions=()
delete_versions=()
while IFS= read -r row; do
    kind=${row%%$'\t'*}
    version=${row#*$'\t'}
    case "$kind" in
        KEEP) keep_versions+=("$version") ;;
        DELETE) delete_versions+=("$version") ;;
    esac
done <<< "$plan"

echo "Keeping documentation versions:"
printf '  %s\n' "${keep_versions[@]}"

if [[ "${#delete_versions[@]}" -eq 0 ]]; then
    echo "No superseded documentation versions to delete."
    exit 0
fi

echo "Deleting documentation versions:"
printf '  %s\n' "${delete_versions[@]}"

if [[ "$dry_run" == true ]]; then
    echo "Dry run only; no changes pushed."
    exit 0
fi

uvx --with 'mkdocs<2' --with mkdocs-material --with mike --with 'markdown-exec[ansi]>=1.12,<2' --with mkdocs-redirects \
    mike delete --push "${delete_versions[@]}"
