#!/usr/bin/env bash
set -euo pipefail

source_dir="${1:-../docs/src/api}"
target_dir="$(dirname "$0")/../resources/api"

if [[ ! -f "$source_dir/APIFiles.json" ]]; then
    echo "error: '$source_dir' does not look like the nanos world api repository (APIFiles.json missing)" >&2
    exit 1
fi

rm -rf "$target_dir/latest" "$target_dir/stable"
mkdir -p "$target_dir/latest" "$target_dir/stable"

for category in Classes StaticClasses StandardLibraries Structs UtilityClasses; do
    cp -r "$source_dir/$category" "$target_dir/latest/"
    cp -r "$source_dir/Stable/$category" "$target_dir/stable/"
done
cp "$source_dir/APIFiles.json" "$target_dir/latest/"
cp "$source_dir/APIFiles.json" "$target_dir/stable/"
cp "$source_dir/Enums.json" "$target_dir/latest/"
cp "$source_dir/Stable/Enums.json" "$target_dir/stable/"

revision="$(git -C "$source_dir" rev-parse --short HEAD 2>/dev/null || echo unknown)"
echo "$revision" > "$target_dir/REVISION"
echo "synced nanos world api ($revision) into $target_dir"
