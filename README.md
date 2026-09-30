# CPU Raytraced EggWars Diorama

## Overview

This is a Computer Graphics course project written in Rust. It is a CPU raytracer for a
Minecraft-inspired EggWars diorama, drawing on the visual language of the classic Minecraft
1.6–1.8 era. The intended final scene depicts the aftermath of a battle: floating islands,
bridges, damaged defenses, opened chests, exposed resources, and lava beneath a sunset-to-night
sky.

Phase 1 provides the correct, testable 3D foundation and a small debug scene. Phase 2 feature
implementation is complete and has been audited; the larger EggWars world and advanced optical
effects remain planned work.

## Current Status

Phase 1 — Core Raytracer — is complete. Phase 2 — Materials, Textures, and Lighting — feature
implementation is complete. Classic block textures, CPU-side material selection, direct lighting,
hard directional-light shadows, and the five canonical rubric materials are integrated. This
repository has not been advanced to Phase 3.

The current implementation includes:

- custom `Vec3` arithmetic and defensive normalization;
- custom normalized `Ray` values;
- robust slab-based ray/AABB intersection;
- an orbital camera with yaw, pitch, zoom, and perspective ray generation;
- a CPU-owned framebuffer;
- closest-hit traversal over a small scene of AABBs;
- deterministic background shading;
- binary PPM output;
- Raylib presentation of the CPU-generated framebuffer;
- interactive orbital rotation and zoom;
- dirty rendering that avoids raytracing unchanged frames;
- separate CPU render timing and presentation FPS diagnostics;
- automated unit tests for the mathematical and geometric foundation;
- CPU-owned, arbitrary-size textures with deterministic nearest-neighbor sampling;
- project-owned binary P6 PPM loading with explicit validation errors;
- a CPU-owned texture registry addressed by compact, stable `TextureId` values;
- validated material properties and uniform or top/side/bottom texture selection;
- face-aware, local AABB UV coordinates propagated directly from slab intersections;
- CPU ambient, Lambert diffuse, and Blinn-Phong specular lighting from one directional light.
- CPU hard shadows using biased secondary rays and early-exit scene occlusion queries.
- centralized, ID-based definitions for grass, cobblestone, obsidian, glass, and lava.

## Architecture

```text
Source PNG assets
        |
        | offline preparation
        v
Runtime PPM assets -> TextureRegistry -> Material / MaterialId
                                              |
                                              v
                         Scene -> AABB -> SceneHit + CubeFace + UV
                                      |
                                      v
                    OrbitalCamera + Lighting -> CPU Renderer
                                                   |
                                                   v
                                           CPU Framebuffer
                                             /       \
                                            v         v
                                          PPM      Raylib window
```

Raylib does not perform raytracing or render the 3D scene. It is isolated to the application
boundary and currently handles window creation, keyboard/mouse input, frame timing, and display
of the framebuffer produced by the CPU renderer. Core modules do not expose Raylib types.

## Controls

- Arrow keys: orbit horizontally and vertically.
- `W` / `S`: zoom in and out.
- Mouse wheel: zoom in and out.
- `Escape`: exit.

## Running

Use a stable Rust toolchain and Cargo. Run the interactive application in release mode so the
CPU renderer is optimized:

```bash
cargo run --release
```

The development framebuffer is rendered at 320×180 and presented in a 960×540 window using an
exact 3× scale. The application prints the PPM export path and shows CPU render duration,
internal resolution, presentation FPS, and render count in a small overlay.

Run the automated tests with:

```bash
cargo test
```

Other useful validation commands are `cargo fmt --check`, `cargo check`, and `cargo build
--release`.

## Output

The initial CPU render is exported to:

```text
output/phase1.ppm
```

The `output/` directory is ignored by Git so generated renders remain local debugging artifacts.

## Texture Assets

Original PNG files under `assets/source/` are development inputs. The CPU raytracer does not
decode PNG at runtime. Prepare the 16×16 binary P6 PPM assets used by the renderer with:

```bash
scripts/prepare_assets.sh
```

The script validates every source dimension, uses macOS `sips` only for offline PNG decoding,
and writes runtime files under `assets/textures/`. Its Python helper uses only the standard
library and performs deterministic PPM conversion and tint arithmetic.

The grass top uses the supplied Minecraft grass colormap with the classic Plains parameters:
temperature `0.8`, rainfall `0.4`, and Minecraft's humidity-times-temperature lookup. This
selects colormap coordinate `(50, 173)`, RGB `(145, 189, 89)` or `#91BD59`. The source
`grass_side.png` already contains green grass over dirt, so it is preserved without tinting the
dirt pixels. Lava uses only the first 16×16 frame of the 16×320 source strip; animation remains
optional future polish.

PPM cannot preserve alpha. Offline conversion composites transparent glass texels against white,
retaining the cyan edge pattern as RGB; physical transparency remains a material property for a
later raytracing phase. At startup, the project-owned P6 loader validates headers, dimensions,
the `255` maximum channel value, and exact RGB payload size before registering textures. Materials
store compact texture IDs rather than owning or cloning texture data. Raylib is not used to load
or sample surface textures.

## Canonical Materials

The temporary Phase 2 showcase displays all five rubric materials simultaneously. Their optical
parameters are registered once at startup and resolved during rendering through compact
`MaterialId` and `TextureId` values; shading performs no string lookup or per-hit allocation.

| Material | Texture appearance | Albedo | Specular | Transparency | Reflectivity | Future special effect |
| --- | --- | --- | ---: | ---: | ---: | --- |
| Grass | `grass_top` / `grass_side` / `dirt` | `(1.00, 1.00, 1.00)` | 0.05 | 0.00 | 0.02 | None required |
| Cobblestone | `cobblestone` on all faces | `(1.00, 1.00, 1.00)` | 0.08 | 0.00 | 0.03 | Normal mapping planned |
| Obsidian | `obsidian` on all faces | `(0.90, 0.90, 1.00)` | 0.55 | 0.00 | 0.35 | Reflection planned |
| Glass | `glass` on all faces | `(0.90, 0.97, 1.00)` | 0.80 | 0.85 | 0.15 | Refraction planned |
| Lava | `lava` on all faces | `(1.00, 0.95, 0.90)` | 0.10 | 0.00 | 0.05 | Emission planned |

Phase 2 stores all parameters but only texture, albedo, and specular currently affect shading.
Glass remains opaque to primary and shadow rays; obsidian does not launch reflection rays; lava
does not emit light; and cobblestone still uses its geometric AABB normal. Refraction, reflection,
emission, and normal mapping remain Phase 3 work.

## Performance

CPU raytrace duration and presentation FPS are separate measurements. The application rerenders
only after startup or a camera change, while Raylib continues presenting the current texture
each window frame.

On the current development machine, the 320×180 canonical-material showcase with directional
lighting and hard shadows rendered in approximately 3.10 ms in the latest audit release startup;
an earlier showcase startup measured 3.24 ms. The earlier hard-shadow comparison runs measured
3.57 ms, 2.77 ms, and 2.93 ms. These small samples are useful as local sanity checks, not formal
benchmarks; later phases will substantially increase scene complexity.

## Testing

The current suite contains 150 tests covering vector arithmetic and normalization, ray invariants,
AABB construction and edge cases, camera basis/ray generation/orbit limits, texture sampling and
registration, P6 parsing and malformed input, material face selection, cube-face UV orientation,
scene closest-hit behavior, ambient/Lambert/Blinn-Phong behavior, renderer lighting and texture
resolution, canonical material registration and texture selection, shadow-ray occlusion and origin
bias, framebuffer and color conversion, PPM output, and presentation-independent camera and RGBA
conversion helpers. During Phase 2 every AABB is an opaque shadow blocker, including materials
whose transparency value is reserved for Phase 3.

The audit validation also regenerates the runtime assets with `scripts/prepare_assets.sh`, then
runs `cargo fmt --check`, `cargo test`, `cargo check`, `cargo build --release`, and
`cargo run --release`.

## Project Constraints

- Raytracing algorithms and scene geometry are implemented in project code.
- The raytracer is CPU-only; GPU rendering or GPU acceleration is not used for ray generation,
  intersection, shading, or scene traversal.
- Raylib is the only direct third-party dependency and is isolated to presentation, input, timing,
  and platform responsibilities under the current project interpretation.
- The project does not currently use Raylib 3D primitives, models, lighting, or shaders for the
  scene.

## Planned Features

The following belong to later phases and are not yet implemented:

- reflection, refraction, and normal mapping;
- emissive lava;
- a sunset/night skybox;
- deterministic procedural 16×16 floating islands and configurable seeds;
- ores and the EggWars battle-aftermath scene;
- a voxel grid and 3D DDA traversal;
- dynamic tile-based CPU multithreading;
- performance benchmarking and profiling.

## Course Rubric Mapping

| Target | Status |
| --- | --- |
| 3D CPU raytracing foundation | Implemented in Phase 1 |
| Orbital viewing and zoom | Implemented in Phase 1 |
| Five textured materials | Canonical definitions and temporary five-material showcase implemented |
| Lighting, shadows, reflection, refraction | Direct lighting and hard shadows implemented; advanced effects planned |
| Normal mapping and emissive lava | Planned |
| Sunset/night skybox | Planned |
| Procedural floating-island terrain | Planned |
| Voxel traversal and parallel rendering | Planned |

## Video

Final project video: to be added during the delivery/polish phase.
