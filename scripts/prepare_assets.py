#!/usr/bin/env python3
"""Convert sips-generated P3 intermediates into deterministic 16x16 P6 assets."""

from pathlib import Path
import math
import sys


PLAINS_TEMPERATURE = 0.8
PLAINS_RAINFALL = 0.4
PLAINS_X = 50
PLAINS_Y = 173

# Derived cobblestone normal map. See `derive_normal_map` for the full convention.
NORMAL_MAP_STRENGTH = 2.0
LUMINANCE_WEIGHTS = (0.2126, 0.7152, 0.0722)


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


def height_field(pixels: list[tuple[int, int, int]]) -> list[float]:
    """Rec. 709 luminance of each texel in [0, 1]; brighter texels are higher."""
    wr, wg, wb = LUMINANCE_WEIGHTS
    return [(wr * r + wg * g + wb * b) / 255.0 for r, g, b in pixels]


def derive_normal_map(
    width: int, height: int, pixels: list[tuple[int, int, int]], strength: float
) -> list[tuple[int, int, int]]:
    """Derive a tangent-space normal map from a texture's luminance.

    Tangent space matches the UV convention: +X is increasing `u` (image right) and +Y is
    increasing `v` (image down), so no axis is flipped. With height `h` the perturbed normal of
    the surface `P + h * N` is `(-dh/du, -dh/dv, 1)`, which holds for either tangent handedness.

    Finite differences are central over two texels and wrap at the borders, because Minecraft
    block textures tile seamlessly:

        dx = h(x + 1, y) - h(x - 1, y)
        dy = h(x, y + 1) - h(x, y - 1)
        n  = normalize(-strength * dx, -strength * dy, 1)

    Each component is encoded as `round((n * 0.5 + 0.5) * 255)`, so a flat surface is
    (128, 128, 255). Only `math.sqrt` and IEEE arithmetic are involved, so the result is
    reproducible.
    """
    heights = height_field(pixels)

    def h(x: int, y: int) -> float:
        return heights[(y % height) * width + (x % width)]

    encoded = []
    for y in range(height):
        for x in range(width):
            nx = -strength * (h(x + 1, y) - h(x - 1, y))
            ny = -strength * (h(x, y + 1) - h(x, y - 1))
            inverse_length = 1.0 / math.sqrt(nx * nx + ny * ny + 1.0)
            encoded.append(
                tuple(
                    math.floor((component * inverse_length * 0.5 + 0.5) * 255.0 + 0.5)
                    for component in (nx, ny, 1.0)
                )
            )
    return encoded


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
        if stem == "cobblestone":
            if (width, height) != (16, 16):
                raise ValueError("cobblestone must be 16x16 to derive its normal map")
            normals = derive_normal_map(width, height, pixels, NORMAL_MAP_STRENGTH)
            write_p6(output_dir / "cobblestone_normal.ppm", width, height, normals)

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
