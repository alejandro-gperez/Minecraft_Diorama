# CPU Raytraced EggWars Diorama

## Overview

This is a Computer Graphics course project written in Rust. It is a CPU raytracer for a
Minecraft-inspired EggWars diorama, drawing on the visual language of the classic Minecraft
1.6–1.8 era. The intended final scene depicts the aftermath of a battle: floating islands,
bridges, damaged defenses, opened chests, exposed resources, and lava beneath a sunset-to-night
sky.

Phase 1 currently provides the correct, testable 3D foundation and a small debug scene. The
larger EggWars world and advanced optical effects are planned work, not current functionality.

## Current Status

Phase 1 — Core Raytracer — is complete. Phase 2 — Materials, Textures, and Lighting — is now
underway, beginning with CPU texture and material data rather than final texture art or lighting.

The current implementation includes:

- custom `Vec3` arithmetic and defensive normalization;
- custom normalized `Ray` values;
- robust slab-based ray/AABB intersection;
- an orbital camera with yaw, pitch, zoom, and perspective ray generation;
- a CPU-owned framebuffer;
- closest-hit traversal over a small scene of AABBs;
- basic debug colors and deterministic background shading;
- binary PPM output;
- Raylib presentation of the CPU-generated framebuffer;
- interactive orbital rotation and zoom;
- dirty rendering that avoids raytracing unchanged frames;
- separate CPU render timing and presentation FPS diagnostics;
- automated unit tests for the mathematical and geometric foundation;
- CPU-owned, arbitrary-size textures with deterministic nearest-neighbor sampling;
- validated material properties and compact IDs shared by scene objects;
- face-aware, local AABB UV coordinates propagated directly from slab intersections.

## Architecture

```text
Scene + OrbitalCamera
        |
        v
   CPU Renderer
        |
        v
 CPU Framebuffer
      /     \
     v       v
   PPM     Raylib
           Window
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
later raytracing phase. Raylib is not used to load or sample surface textures.

## Performance

CPU raytrace duration and presentation FPS are separate measurements. The application rerenders
only after startup or a camera change, while Raylib continues presenting the current texture
each window frame.

On the current development machine, the initial 320×180 release render measured approximately
2.42 ms. Manual presentation use was observed at approximately 45–58 FPS. These are development
observations, not universal benchmarks; later phases will substantially increase scene
complexity.

## Testing

The current suite contains 102 tests covering vector arithmetic and normalization, ray invariants,
AABB construction and edge cases, camera basis/ray generation/orbit limits, textures, materials,
cube-face UV orientation, scene closest-hit behavior, framebuffer and color conversion, PPM output, and
presentation-independent camera and RGBA conversion helpers.

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

- Minecraft-style textures and five distinct materials;
- lighting and shadows;
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
| Five textured materials | Planned |
| Lighting, shadows, reflection, refraction | Planned |
| Normal mapping and emissive lava | Planned |
| Sunset/night skybox | Planned |
| Procedural floating-island terrain | Planned |
| Voxel traversal and parallel rendering | Planned |

## Video

Final project video: to be added during the delivery/polish phase.
