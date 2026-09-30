#!/bin/bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SOURCE_DIR="$REPO_ROOT/assets/source"
OUTPUT_DIR="$REPO_ROOT/assets/textures"
TEMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/minecraft-diorama-assets.XXXXXX")"

cleanup() {
    rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

for command in sips python3; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "error: required asset-preparation command is unavailable: $command" >&2
        exit 1
    fi
done

require_dimensions() {
    local filename="$1"
    local expected_width="$2"
    local expected_height="$3"
    local source_path="$SOURCE_DIR/$filename"

    if [[ ! -f "$source_path" ]]; then
        echo "error: required source asset is missing: $source_path" >&2
        exit 1
    fi

    local width
    local height
    width="$(sips -g pixelWidth "$source_path" | awk '/pixelWidth:/ { print $2 }')"
    height="$(sips -g pixelHeight "$source_path" | awk '/pixelHeight:/ { print $2 }')"

    if [[ "$width" != "$expected_width" || "$height" != "$expected_height" ]]; then
        echo "error: $filename must be ${expected_width}x${expected_height}, got ${width}x${height}" >&2
        exit 1
    fi
}

convert_to_intermediate_ppm() {
    local filename="$1"
    local stem="${filename%.png}"
    sips -s format public.pbm "$SOURCE_DIR/$filename" --out "$TEMP_DIR/$stem.ppm" >/dev/null
}

BLOCK_TEXTURES=(
    grass_top.png
    grass_side.png
    dirt.png
    cobblestone.png
    obsidian.png
    glass.png
    coal_ore.png
    iron_ore.png
    gold_ore.png
    diamond_ore.png
)

for filename in "${BLOCK_TEXTURES[@]}"; do
    require_dimensions "$filename" 16 16
done
require_dimensions lava_still.png 16 320
require_dimensions grass.png 256 256

for filename in "${BLOCK_TEXTURES[@]}" lava_still.png grass.png; do
    convert_to_intermediate_ppm "$filename"
done

mkdir -p "$OUTPUT_DIR"
python3 "$SCRIPT_DIR/prepare_assets.py" "$TEMP_DIR" "$OUTPUT_DIR"

# The derived normal map must stay 16x16 P6: a 13-byte header plus 16*16*3 payload bytes.
NORMAL_MAP="$OUTPUT_DIR/cobblestone_normal.ppm"
if [[ "$(head -c 13 "$NORMAL_MAP" | tr '\n' ' ')" != "P6 16 16 255 " ]] \
    || [[ "$(wc -c < "$NORMAL_MAP" | tr -d ' ')" != "781" ]]; then
    echo "error: cobblestone_normal.ppm must be a 16x16 P6 image" >&2
    exit 1
fi

echo "Prepared 11 runtime P6 textures and the derived cobblestone normal map in $OUTPUT_DIR"
