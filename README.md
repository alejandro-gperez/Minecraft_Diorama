# CPU Raytraced EggWars Diorama

## Overview

This is a Computer Graphics course project written in Rust. It is a CPU raytracer for a
Minecraft-inspired EggWars diorama, drawing on the visual language of the classic Minecraft
1.6–1.8 era. The intended final scene depicts the aftermath of a battle: floating islands,
bridges, damaged defenses, opened chests, exposed resources, and lava beneath a sunset-to-night
sky.

Phase 1 provides the correct, testable 3D foundation and Phase 2 provides the audited surface and
lighting system. Phase 3 is underway with a procedural sunset/night environment, bounded
recursive ray infrastructure, recursive reflections, and recursive glass refraction; Fresnel,
emission, normal mapping, and the larger EggWars world remain planned work.

## Current Status

Phase 1 — Core Raytracer — and Phase 2 — Materials, Textures, and Lighting — are complete and
audited. Phase 3 — Raytracing Effects — is underway. Primary-ray misses now sample a project-owned
procedural sunset/night environment in world space. Every radiance ray flows through one
bounded, depth-aware trace path; reflective materials launch recursive reflection rays and
transparent materials launch recursive Snell refraction rays. Fresnel, emission, and normal
mapping are not implemented.

The current implementation includes:

- custom `Vec3` arithmetic and defensive normalization;
- custom normalized `Ray` values;
- robust slab-based ray/AABB intersection;
- an orbital camera with yaw, pitch, zoom, and perspective ray generation;
- a CPU-owned framebuffer;
- closest-hit traversal over a small scene of AABBs;
- a bounded, depth-aware radiance trace path shared by primary and secondary rays;
- deterministic world-space sunset/night environment sampling;
- procedural sun, sparse seeded stars, and a dark lower-hemisphere void;
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
- bounded recursive mirror reflection with a constant per-material reflectivity.
- bounded recursive Snell refraction with a validated per-material index of refraction.

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
       OrbitalCamera + Lighting + Environment -> CPU Renderer
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

## Procedural Environment

Primary-ray misses are colored by `Environment::sample(world_direction)`. The horizon follows
world `+Y`, so camera orbit does not move the palette, sun, or stars. A 96-band piecewise gradient
transitions from a cool lower void through orange/magenta sunset colors into violet and deep navy.
The subtle direction-based quantization is independent of framebuffer resolution.

The visible sun uses `normalize(0.6, 0.2, 0.8)`, approximately
`(0.588, 0.196, 0.784)`. It shares the directional light's X/Z azimuth while staying lower for
the sunset composition; the Phase 2 light itself remains at its established higher elevation.
A dot-product threshold creates the disc and a wider smooth threshold creates its glow.

Sparse stars use a fixed seed, `0xE6677A2D`, and a hash of quantized world-direction cells. There
is no stored star collection or mutable RNG, so sampling is deterministic and allocation-free.
Stars are restricted to the darker upper sky. The environment uses no cubemap, image skybox,
filesystem access, Raylib sky API, shader, or GPU rendering. Every traced radiance ray that misses
the scene uses this sampler, whatever its depth.

## Ray Depth

The renderer traces every radiance ray through one private `trace_ray(ray, depth)` path. Depth
counts upward from the camera: primary rays are depth `0`, a ray spawned by a depth-0 hit is depth
`1`, and so on up to `MAX_RAY_DEPTH = 3`. A ray at the maximum depth is still intersected, locally
textured and lit, and shadow-tested, and it still samples the environment on a miss; it only may
not spawn a further secondary ray. Tracing above the maximum returns an explicit render error.

Local surface shading is separate from the point where recursive contributions are composed.
Reflection and refraction are the secondary radiance rays; each spawns at `depth + 1`, so a hit
that both reflects and refracts branches into two child rays. Lava does not emit.
Hard-shadow rays are any-hit visibility queries, not radiance rays, and do not consume depth.
Recursion uses only small stack values and shared borrows, with no per-ray allocation.

## Reflection

A hit whose material has `reflectivity > 0` and whose depth satisfies
`can_spawn_secondary_ray(depth)` traces one mirror ray at `depth + 1`. For a normalized incident
direction `D` and the outward geometric normal `N`:

```text
R      = D - 2(D·N)N
origin = hit.position + N * REFLECTION_RAY_ORIGIN_BIAS      (1.0e-4)
color  = local * (1 - reflectivity) + reflected * reflectivity
```

The reflected ray goes through the same `trace_ray` as every other ray: a miss samples the
world-space procedural environment, and a hit receives texture, albedo, lighting, hard shadows,
and further reflection while depth permits. A hit at `MAX_RAY_DEPTH` is locally shaded only. The
bias is numerically equal to the shadow bias but is a separate constant, and the source object is
never excluded. The reflectivity coefficient is constant per material; there is no Fresnel term.

Obsidian (`0.35`) is the primary demonstration: it mirrors the sunset horizon and neighboring
blocks. Glass also reflects at its stored `0.15`, in addition to refracting. The other materials
reflect only their small stored values.

## Refraction

Every material carries an index of refraction (`ior`), which must be finite and strictly
positive; zero, negative, NaN, and infinite values are rejected rather than clamped. Materials
default to `AIR_IOR = 1.0`, the neutral value kept by the non-transmissive canonical materials.
Canonical glass uses `GLASS_IOR = 1.5`, approximately ordinary glass.

A hit whose material has `transparency > 0` and whose depth satisfies
`can_spawn_secondary_ray(depth)` traces one refracted ray at `depth + 1`. Every transmissive AABB
is assumed to sit in air. The incident direction `D` is classified against the hit's outward
geometric normal `N`:

```text
entering (D·N < 0):  eta_i = AIR_IOR,  eta_t = material.ior,  n = N
exiting  (D·N >= 0): eta_i = material.ior,  eta_t = AIR_IOR,  n = -N

eta     = eta_i / eta_t
cos_i   = -D·n
sin²_t  = eta² (1 - cos_i²)          total internal reflection if sin²_t > 1
T       = eta D + (eta cos_i - sqrt(1 - sin²_t)) n
origin  = hit.position - n * REFRACTION_RAY_ORIGIN_BIAS   (1.0e-4)
```

The oriented normal `n` is used only for the Snell calculation; the stored outward normal still
drives lighting, reflection, face identity, and UVs. The origin lands on the transmitted side:
just inside the box when entering (`position - N * bias`) and just outside when exiting
(`position + N * bias`). A ray starting inside an AABB reports that box's exit face, so an
entering refracted ray reaches the opposite face, where it refracts back into air. The source
object is never excluded.

Refracted rays share the common trace path: misses sample the procedural environment and hits
receive full shading, reflection, and further refraction while depth permits. On total internal
reflection, no refracted ray exists and the surface keeps its reflection-blended color with no
transmitted contribution; energy is not redistributed to reflection yet.

Composition is a temporary, angle-independent two-step blend:

```text
base  = local * (1 - reflectivity) + reflected * reflectivity
color = base  * (1 - transparency) + refracted * transparency
```

Reflectivity and transparency are not renormalized to sum to one, and there is no Fresnel term;
Fresnel composition is planned for the next mission. Glass (`transparency = 0.85`) is the primary
demonstration: the grass, neighboring blocks, and sky are visible through it, displaced by the
refraction, and the displacement changes with the viewing angle. Its texture and albedo remain as
the local interface appearance. Materials with zero transparency never trace refraction rays.

Known limitation: shadow rays still treat every AABB, including glass, as an opaque blocker, so
glass casts a full hard shadow. Transparent or colored shadows, absorption, and dispersion are not
implemented.

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
retaining the cyan edge pattern as RGB; physical transparency is provided by recursive
refraction rather than texture alpha. At startup, the project-owned P6 loader validates headers, dimensions,
the `255` maximum channel value, and exact RGB payload size before registering textures. Materials
store compact texture IDs rather than owning or cloning texture data. Raylib is not used to load
or sample surface textures.

## Canonical Materials

The temporary Phase 2 showcase displays all five rubric materials simultaneously. Their optical
parameters are registered once at startup and resolved during rendering through compact
`MaterialId` and `TextureId` values; shading performs no string lookup or per-hit allocation.

| Material | Texture appearance | Albedo | Specular | Transparency | Reflectivity | IOR | Special effect |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| Grass | `grass_top` / `grass_side` / `dirt` | `(1.00, 1.00, 1.00)` | 0.05 | 0.00 | 0.02 | 1.0 | None required |
| Cobblestone | `cobblestone` on all faces | `(1.00, 1.00, 1.00)` | 0.08 | 0.00 | 0.03 | 1.0 | Normal mapping planned |
| Obsidian | `obsidian` on all faces | `(0.90, 0.90, 1.00)` | 0.55 | 0.00 | 0.35 | 1.0 | Reflection (implemented) |
| Glass | `glass` on all faces | `(0.90, 0.97, 1.00)` | 0.80 | 0.85 | 0.15 | 1.5 | Refraction (implemented) |
| Lava | `lava` on all faces | `(1.00, 0.95, 0.90)` | 0.10 | 0.00 | 0.05 | 1.0 | Emission planned |

Texture, albedo, specular, reflectivity, transparency, and index of refraction all affect
rendering. The `1.0` IOR of the non-transmissive materials is the neutral default and has no
optical effect. Glass still blocks shadow rays as an opaque AABB. Lava does not emit light and
cobblestone still uses its geometric AABB normal. Fresnel, emission, and normal mapping remain
Phase 3 work.

## Performance

CPU raytrace duration and presentation FPS are separate measurements. The application rerenders
only after startup or a camera change, while Raylib continues presenting the current texture
each window frame.

On the current development machine, four 320×180 release startups with the procedural environment
measured approximately 2.83 ms, 2.82 ms, 2.80 ms, and 2.77 ms. The Phase 2 baseline was roughly
3.1–3.3 ms. Because the diagnostic camera and miss coverage changed, this is only a regression
sanity check—not evidence of an optimization or a formal benchmark.

After the bounded trace-path refactor, eight interleaved startups each of the previous and new
builds measured typically 2.77–2.87 ms and 2.70–2.80 ms respectively, with a byte-identical image.
The infrastructure alone adds no measurable cost while no secondary rays are active.

With recursive reflection active, five 320×180 release startups of the diagnostic scene measured
approximately 3.59–3.92 ms (3.59, 3.92, 3.82, 3.82, 3.87), versus roughly 2.8 ms before. This is a
small local sample, not a formal benchmark. Cost now scales with the number of visible reflective
pixels (every material has `reflectivity > 0`) and with depth, since each reflective hit below
`MAX_RAY_DEPTH` traces one more ray plus its shadow query.

With glass refraction active, six 320×180 release startups measured approximately 4.64–4.85 ms
(4.64, 4.78, 4.84, 4.85, 4.77, 4.72). Startup timings were noisy in that session: the
reflection-only build measured 3.75–4.57 ms under the same conditions. Thirty warmed renders of
the default view in one process gave medians of about 3.54 ms before and 4.08 ms after, roughly
15% more. Glass hits below the maximum depth branch into a reflected and a refracted ray, so cost
depends on the number of visible glass pixels, how often rays inside glass hit total internal
reflection, the recursion depth, and what the secondary rays hit. These are local observations,
not a formal benchmark.

## Testing

The current suite contains 225 tests covering vector arithmetic and normalization, ray invariants,
AABB construction and edge cases, camera basis/ray generation/orbit limits, texture sampling and
registration, P6 parsing and malformed input, material face selection, cube-face UV orientation,
scene closest-hit behavior, ambient/Lambert/Blinn-Phong behavior, renderer lighting and texture
resolution, canonical material registration and texture selection, shadow-ray occlusion and origin
bias, procedural environment regions, sun, stars, invalid directions, renderer miss integration,
ray-depth policy, reflection mathematics, origin bias, linear reflectivity blending, recursive
reflection depth behavior, canonical reflectivity, index-of-refraction validation, Snell
refraction and total internal reflection, entering/exiting classification and origin bias,
recursive refraction depth behavior, transparency composition, canonical glass refraction, a
camera-pose render sweep, framebuffer and color conversion, PPM output, and
presentation-independent camera and RGBA conversion helpers. Every AABB remains an opaque shadow
blocker, including glass.

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

- Fresnel composition and normal mapping;
- transparent or colored shadows through glass;
- emissive lava;
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
| Lighting, shadows, reflection | Direct lighting, hard shadows, and bounded recursive reflection implemented |
| Refraction | Implemented: recursive Snell refraction through glass (IOR 1.5) |
| Fresnel | Planned |
| Normal mapping and emissive lava | Planned |
| Sunset/night skybox/environment | Procedural CPU environment implemented; shared miss path for every traced ray |
| Procedural floating-island terrain | Planned |
| Voxel traversal and parallel rendering | Planned |

## Video

Final project video: to be added during the delivery/polish phase.
