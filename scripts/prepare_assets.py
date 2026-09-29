#!/usr/bin/env python3
"""Convert sips-generated P3 intermediates into deterministic 16x16 P6 assets."""

from pathlib import Path
import sys


PLAINS_TEMPERATURE = 0.8
PLAINS_RAINFALL = 0.4
PLAINS_X = 50
PLAINS_Y = 173


def read_p3(path: Path) -> tuple[int, int, list[tuple[int, int, int]]]:
    without_comments = "\n".join(
        line.split("#", 1)[0] for line in path.read_text(encoding="ascii").splitlines()
    )
    tokens = without_comments.split()
    if len(tokens) < 4 or tokens[0] != "P3":
        raise ValueError(f"{path}: expected a sips-generated P3 image")

    width, height, maximum = map(int, tokens[1:4])
    if width <= 0 or height <= 0 or maximum != 255:
        raise ValueError(f"{path}: invalid P3 dimensions or maximum channel value")

    channels = list(map(int, tokens[4:]))
    expected_channels = width * height * 3
    if len(channels) != expected_channels or any(value < 0 or value > 255 for value in channels):
        raise ValueError(f"{path}: invalid P3 pixel payload")

    pixels = [tuple(channels[index : index + 3]) for index in range(0, len(channels), 3)]
    return width, height, pixels


def write_p6(
    path: Path, width: int, height: int, pixels: list[tuple[int, int, int]]
) -> None:
    if len(pixels) != width * height:
        raise ValueError(f"{path}: output pixel count does not match dimensions")

    payload = bytes(channel for pixel in pixels for channel in pixel)
    path.write_bytes(f"P6\n{width} {height}\n255\n".encode("ascii") + payload)


def multiply_color(
    pixel: tuple[int, int, int], tint: tuple[int, int, int]
) -> tuple[int, int, int]:
    return tuple((source * factor + 127) // 255 for source, factor in zip(pixel, tint))


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: prepare_assets.py INTERMEDIATE_DIR OUTPUT_DIR")

    intermediate_dir = Path(sys.argv[1])
    output_dir = Path(sys.argv[2])

    grass_width, grass_height, grass_colormap = read_p3(intermediate_dir / "grass.ppm")
    if (grass_width, grass_height) != (256, 256):
        raise ValueError("grass colormap must be 256x256")
    plains_tint = grass_colormap[PLAINS_Y * grass_width + PLAINS_X]

    # Minecraft's lookup uses humidity *= temperature, then indexes from (1-temperature,
    # 1-humidity). Plains uses temperature 0.8 and rainfall 0.4, selecting (50, 173).
    adjusted_humidity = PLAINS_RAINFALL * PLAINS_TEMPERATURE
    expected_x = int((1.0 - PLAINS_TEMPERATURE) * 255.0)
    expected_y = int((1.0 - adjusted_humidity) * 255.0)
    if (expected_x, expected_y) != (PLAINS_X, PLAINS_Y):
        raise ValueError("internal Plains colormap lookup no longer matches documented coordinates")

    width, height, grass_top = read_p3(intermediate_dir / "grass_top.ppm")
    tinted_grass = [multiply_color(pixel, plains_tint) for pixel in grass_top]
    write_p6(output_dir / "grass_top.ppm", width, height, tinted_grass)

    direct_assets = (
        "grass_side",
        "dirt",
        "cobblestone",
        "obsidian",
        "glass",
        "coal_ore",
        "iron_ore",
        "gold_ore",
        "diamond_ore",
    )
    for stem in direct_assets:
        width, height, pixels = read_p3(intermediate_dir / f"{stem}.ppm")
        write_p6(output_dir / f"{stem}.ppm", width, height, pixels)

    lava_width, lava_height, lava_pixels = read_p3(intermediate_dir / "lava_still.ppm")
    if (lava_width, lava_height) != (16, 320):
        raise ValueError("lava source must contain twenty vertically stacked 16x16 frames")
    write_p6(output_dir / "lava.ppm", 16, 16, lava_pixels[: 16 * 16])

    print(
        "Grass tint: "
        f"RGB{plains_tint} / #{plains_tint[0]:02X}{plains_tint[1]:02X}{plains_tint[2]:02X} "
        f"from colormap ({PLAINS_X}, {PLAINS_Y})"
    )


if __name__ == "__main__":
    main()
