# CPU Raytraced EggWars Diorama

## Overview

This is a Computer Graphics course project written in Rust. It is a CPU raytracer for a
Minecraft-inspired EggWars diorama, drawing on the visual language of the classic Minecraft
1.6–1.8 era. The intended final scene depicts the aftermath of a battle: floating islands,
bridges, damaged defenses, opened chests, exposed resources, and lava beneath a sunset-to-night
sky.

Phases 1–3 provide the correct, testable 3D foundation, the audited surface and lighting system,
and the raytracing effects: a procedural sunset/night environment, bounded recursive reflection,
Snell refraction with Schlick Fresnel for glass, emissive lava with a local point light, and a
derived normal map for cobblestone. Phase 4 (performance architecture) is implemented and has
passed its final technical audit: a dense voxel grid, 3D DDA traversal, hybrid voxel + AABB scene
queries, and a deterministic tile renderer whose tiles scoped CPU worker threads claim dynamically.
Phase 5 (the EggWars world) is in progress: Mission 28 added a deterministic, seeded procedural
floating-island generator, Mission 29 uses it to build the five-island EggWars arena (a 24×24
central island and four 16×16 outer islands) in one global voxel grid from one seed, and Mission 30
builds the arena's architecture on that terrain: four bridges in their narrative states (complete,
damaged, severed, abandoned), a small base with a nine-block obsidian egg defense on each outer
island, and a ruined central structure with glass windows. Eggs, chests, arrows, lava, and the
final lighting and composition do not exist yet.

## Current Status

| Phase | Status |
| --- | --- |
| 1 — Core Raytracer | Complete and audited |
| 2 — Materials, Textures, and Lighting | Complete and audited |
| 3 — Raytracing Effects | Complete and audited |
| 4 — Performance Architecture | Complete and audited (see Phase 4 Audit) |
| 5 — EggWars World | In progress: Missions 28 (procedural floating island), 29 (five-island arena), 30 (bridges, structures, and egg defenses), 30.5 (nine-block egg defenses and a 640×360 internal resolution), 31 (battle-aftermath decorations), and 31.25 (spatial grouping of decorative AABBs) complete |

The rendering pipeline is: every radiance and shadow ray queries one hybrid scene (3D DDA over a
voxel grid plus a scan of arbitrary AABB objects that skips any spatial group whose bounds the ray
misses), tiles of the framebuffer are claimed
dynamically by scoped CPU worker threads, and primary-ray misses sample a project-owned procedural
sunset/night environment in world space. Every radiance ray flows through one bounded, depth-aware
trace path; reflective materials launch recursive reflection rays and transparent materials launch
recursive Snell refraction rays whose split with reflection follows Schlick's Fresnel
approximation. Lava is self-luminous and lights nearby surfaces through a finite local point light.
Cobblestone is normal mapped from a map derived offline from its own texture. The scenes that exist
are the default five-block showcase, the small voxel diagnostic scene, one seeded procedural
floating island, and the seeded five-island EggWars arena with its bridges, bases, egg defenses,
central ruin, and battle-aftermath props: two surviving eggs, shell fragments, open chests, arrows,
and block fragments (no lava yet; final lighting and composition are pending).

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
- angle-dependent Schlick Fresnel composition of glass reflection and refraction, with total
  internal reflection routed to reflection.
- validated per-material emission, used by canonical lava as visible self-radiance.
- finite-radius local point lights with squared falloff, Lambert diffuse, Blinn-Phong specular, and
  hard point-light shadow rays.
- a dense, contiguous voxel grid of `BlockType` identities with integer world coordinates and a
  configurable, possibly negative origin.
- auxiliary dirt and coal/iron/gold/diamond ore materials registered once at startup, and an
  exhaustive `BlockType -> MaterialId` mapping covering every block type.
- one renderer-facing `SceneHit` built from a shared `SurfaceHit` for AABB objects and voxel faces
  (`VoxelHit`), with UVs from a single shared cube-face table.
- 3D DDA voxel traversal with nearest-hit and any-hit (shadow) queries.
- hybrid scene queries: every radiance and shadow ray tests the voxel grid by DDA and arbitrary
  AABB objects by a scan in insertion order, and receives the nearest hit or any blocker.
- flat spatial groups of AABB objects (Mission 31.25): the 54 decorative boxes form 8 groups, each
  behind one exact bounding box, so rays that miss a group skip its objects with identical results.
- a deterministic tile renderer: the framebuffer is split into 16×16 tiles.
- dynamic tile-based CPU multithreading: one scoped worker thread per available CPU claims tiles
  from an atomic counter, with output byte-identical to the sequential renderer.
- a deterministic procedural floating-island generator: a seeded 16×16 radial-mask island of
  grass, dirt, and a tapered cobblestone underside with exposed ores, generated once into the
  scene's voxel grid and selected with `--procedural-island` / `--seed <u64>`.
- the five-island EggWars arena: a 24×24 central island and four 16×16 outer islands on the
  cardinal axes, each with its own seed derived from one global seed, generated into one global
  voxel grid and selected with `--eggwars [--seed <u64>]`.
- deterministic EggWars construction on that terrain: four terrain-attached cobblestone bridges
  (complete, damaged, severed, incomplete), outer bases whose nine-position obsidian egg defenses
  (eight blocks around the egg and one above it) hold 9, 7, 5, and 3 blocks, a ruined central structure with an obsidian floor inlay and two glass
  windows, and metadata (egg cells, defense slots, bridge ends, central open area) that later
  missions build on.
- battle-aftermath props placed from that metadata as 59 project-owned AABB parts, 54 of them
  instantiated as scene objects (the sealed North egg is story metadata only): surviving
  eggs in the North and West defenses, shell fragments in East and South, two open chests in the
  central ruin, seven axis-aligned arrows stuck in blocks, and eight block fragments at the breaks,
  using seven decorative solid-color materials plus the canonical cobblestone and obsidian.

## Architecture

```text
Source PNG assets                      Seed + IslandConfig
        |                                      |
        | offline preparation                  | procedural generation, once per scene
        v                                      v
Runtime PPM assets -> TextureRegistry -> Material / MaterialId    VoxelGrid (one, global)
                                              |                        |
                                              v                        v
          Scene -> AABB objects (grouped scan) ----+
                   VoxelGrid + BlockMaterials (DDA) -+-> nearest SurfaceHit -> SceneHit
                                                                                  |
                                                   +------------------------------+
                                                   v
       OrbitalCamera + Lighting + Environment -> CPU Renderer
                                                   |
                                                   | 16x16 tiles claimed via AtomicUsize
                                                   v
                                     scoped CPU worker threads
                                                   |
                                                   | short Mutex-protected tile copy
                                                   v
                                           CPU Framebuffer
                                             /       \
                                            v         v
                                          PPM      Raylib window
```

Raylib does not perform raytracing or render the 3D scene. It is isolated to the application
boundary and currently handles window creation, keyboard/mouse input, frame timing, and display
of the framebuffer produced by the CPU renderer. Core modules do not expose Raylib types. Worker
threads run only project-owned CPU raytracing; none of them calls Raylib.

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
Reflection and refraction are the secondary radiance rays; each spawns at `depth + 1`, so a
transparent hit below the maximum depth branches into two child rays (only the reflected one
under total internal reflection). A transparent hit at `MAX_RAY_DEPTH` spawns neither branch and returns its full,
unweighted local shading rather than black. Local shading includes emission and point lights, so a
terminal hit on lava still glows. Emission spawns no rays of its own.
Hard-shadow rays are any-hit visibility queries, not radiance rays, and do not consume depth.
Recursion uses only small stack values and shared borrows, with no per-ray allocation.

## Reflection

An opaque hit (`transparency == 0`) whose material has `reflectivity > 0` and whose depth
satisfies `can_spawn_secondary_ray(depth)` traces one mirror ray at `depth + 1`. For a normalized
incident direction `D` and the outward geometric normal `N`:

```text
R      = D - 2(D·N)N
origin = hit.position + N * REFLECTION_RAY_ORIGIN_BIAS      (D·N < 0, arriving from outside)
origin = hit.position - N * REFLECTION_RAY_ORIGIN_BIAS      (D·N >= 0, arriving from inside)
color  = local * (1 - reflectivity) + reflected * reflectivity
```

`REFLECTION_RAY_ORIGIN_BIAS` is `1.0e-4`. A reflected ray stays in the medium the incident ray
travelled through, so the origin is offset onto the incident side. Exterior reflections are
unchanged; a ray reflecting inside glass stays inside instead of being pushed out and immediately
re-entering the face it reflected from. The same entering/exiting classification drives
reflection, refraction, and Fresnel.

The reflected ray goes through the same `trace_ray` as every other ray: a miss samples the
world-space procedural environment, and a hit receives texture, albedo, lighting, hard shadows,
and further reflection while depth permits. A hit at `MAX_RAY_DEPTH` is locally shaded only. The
bias is numerically equal to the shadow bias but is a separate constant, and the source object is
never excluded. For opaque materials the reflectivity coefficient is constant per material, with
no Fresnel term.

Obsidian (`0.35`) is the primary demonstration: it mirrors the sunset horizon and neighboring
blocks. Grass, cobblestone, and lava reflect only their small stored values. Glass reflection is
Fresnel-controlled instead; see below.

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
reflection no refracted ray exists; see Fresnel composition below. Glass (`transparency = 0.85`)
is the primary demonstration: the grass, neighboring blocks, and sky are visible through it,
displaced by the refraction, and the displacement changes with the viewing angle. Materials with
zero transparency never trace refraction rays.

## Fresnel Composition

Transparent materials split their optical contribution between reflection and refraction with
Schlick's approximation of the Fresnel reflectance. Using the interface indices and oriented
normal `n` from the classification above:

```text
R0        = ((eta_i - eta_t) / (eta_i + eta_t))²
cos_theta = clamp(-D·n, 0, 1)
F         = R0 + (1 - R0)(1 - cos_theta)⁵        F = 1 under total internal reflection

optical   = reflected * F + refracted * (1 - F)
color     = local * (1 - transparency) + optical * transparency
```

For air and glass (`1.0` / `1.5`), `R0 = 0.04` in both directions, so glass viewed head-on is
about 96% transmissive, while `F` rises toward `1` at grazing incidence and the glass mirrors the
sky and neighboring blocks. `transparency` is the fraction of the surface that behaves as a clear
optical interface; for canonical glass, 15% of the textured, lit local appearance remains and the
85% optical portion is split by Fresnel. The helper `schlick_reflectance` validates its indices
the same way materials do and rejects non-finite input.

For transparent materials the stored `reflectivity` is not applied: Fresnel already decides how
much of the optical portion reflects, and blending the constant on top would count reflection
twice. Canonical glass keeps its stored `0.15` as material data, but it has no optical effect.
Opaque materials keep the constant-reflectivity blend above.

Under total internal reflection the transmissive portion becomes pure reflection:
`color = local * (1 - transparency) + reflected * transparency`, and no refracted ray is traced.
Previously the whole surface fell back to its reflection-blended local color there, so
inside-glass edges glowed with the lit texture. Below `MAX_RAY_DEPTH`, reflection is
traced whenever `F > 0` and refraction whenever `F < 1` and Snell gives a direction; both use
`depth + 1`. At `MAX_RAY_DEPTH` neither branch is traced and the hit keeps its local shading.

Schlick is evaluated at the incident-side cosine in both directions. Inside glass this makes `F`
stay near `R0` until the critical angle (about 41.8°) and then jump to `1`; the physical Fresnel
curve rises steeply but continuously there.

Known limitations:

- Shadow rays still treat every AABB, including glass, as an opaque blocker, so glass casts a full
  hard shadow. Transparent or colored shadows, absorption, and dispersion are not implemented.
- Every glass AABB is assumed to be surrounded by air, and no medium stack is tracked. Two touching
  glass blocks therefore behave as glass → air → glass, with an internal interface between them.
  Merging connected glass is a concern for the later voxel world.
- Faces struck from inside glass are locally lit with their outward geometric normal.

## Normal Mapping

Canonical cobblestone is the normal-mapped material. The effect changes only the **shading
normal** used for local lighting; the AABB, its silhouette, hit position, face, and UVs are
untouched, so a rough-looking face is still perfectly flat in outline and in shadow. Grass,
obsidian, glass, and lava have no normal map and shade with their geometric normals exactly as
before.

### Offline derivation

`scripts/prepare_assets.sh` (through `scripts/prepare_assets.py`) derives
`assets/textures/cobblestone_normal.ppm` from the runtime cobblestone texture; it is not an
unrelated external asset, is not painted by hand, and is a 16×16 P6 image like every other texture.
The script is deterministic (it uses only IEEE arithmetic and `math.sqrt`) and validates the
output's size.

```text
height(x, y) = 0.2126 R + 0.7152 G + 0.0722 B          (Rec. 709 luminance, brighter = higher)
dx = height(x + 1, y) - height(x - 1, y)
dy = height(x, y + 1) - height(x, y - 1)
n  = normalize(-strength * dx, -strength * dy, 1)
texel = round((n * 0.5 + 0.5) * 255)                   (flat surface = 128, 128, 255)
```

Differences wrap at the 16×16 borders, because Minecraft block textures tile seamlessly and a
wrapped edge produces no artificial seam. The chosen `strength` is `2.0`. `3.0` crushed a large
share of texels to near-black, while `1.0` read as a faint grain, so `2.0` gives a clearly
dimensional surface without looking violently crumpled. A test re-derives the map from
`cobblestone.ppm` and checks that the committed asset follows this derivation.

### Runtime

A material may hold an optional `TextureId` for its normal map (`Material::with_normal_map`);
materials default to none, so existing construction is unchanged. The map is an ordinary texture
registered once in the scene's texture storage and shared by every cobblestone object; it is
never copied per object. At a hit the renderer samples it with nearest-neighbor filtering at the
same UV as the color texture, so the map stays attached to the same local mapping and there is no
bilinear filtering.

The sample is decoded as `n = 2 * rgb - 1`, normalized, and rejected when it is non-finite,
degenerate, or has `z <= 0.05` (within about 3° of the surface plane or pointing into it). A
rejected texel falls back to the face's geometric normal instead of being clamped, so a corrupt
texel can never produce NaN or an inward-facing normal. Any accepted normal has positive `z`, so
the transformed normal stays in the geometric hemisphere.

### Tangent basis

Tangent space follows the project's UV convention: `+X` is increasing `u`, `+Y` is increasing
`v` (image down, so nothing is flipped between the image and the surface), and `+Z` is the outward
geometric normal. A height field `P + h N` has perturbed normal `(-dh/du, -dh/dv, 1)` along the
actual `du` and `dv` directions, which holds for either handedness, so the mirrored faces need no
special case. The world-space normal is `T * n.x + B * n.y + N * n.z` with `T` and `B` taken from
the UV mapping of each face:

| Face | `u` | `v` | Tangent `T` (u+) | Bitangent `B` (v+) | Normal `N` |
| --- | --- | --- | --- | --- | --- |
| `+X` | `1 - z` | `1 - y` | `-Z` | `-Y` | `+X` |
| `-X` | `z` | `1 - y` | `+Z` | `-Y` | `-X` |
| `+Y` | `x` | `z` | `+X` | `+Z` | `+Y` |
| `-Y` | `x` | `1 - z` | `+X` | `-Z` | `-Y` |
| `+Z` | `x` | `1 - y` | `+X` | `-Y` | `+Z` |
| `-Z` | `1 - x` | `1 - y` | `-X` | `-Y` | `-Z` |

A test hits every face of a non-unit, translated box and confirms that moving along `T` or `B`
increases only `u` or `v` respectively.

### Geometric normal versus shading normal

| Uses the **shading** normal | Keeps the **geometric** normal |
| --- | --- |
| directional Lambert diffuse | AABB intersection, hit position, `CubeFace`, UVs |
| directional Blinn-Phong specular | shadow-ray origin bias and direction (directional and point) |
| point-light Lambert diffuse | whether a light is on the visible side of the face at all |
| point-light Blinn-Phong specular | reflection direction and origin, refraction, Fresnel, medium entering/exiting |

A light arriving from behind the geometric face never reaches the surface, so a bump facing the
light on a turned-away face stays dark; the Lambert term also gates the specular highlight.
Reflection and refraction continue to use the geometric interface normal: this mission does not
add bump-mapped reflection. Shadows remain geometric occlusion, and lava emission, glass behavior, and
Fresnel are unchanged. Normal mapping is cheap: one extra texture lookup, a decode, and a basis
transform per normal-mapped hit, with no allocation.

## Emission and Local Lava Lighting

Lava is the project's primary emissive material. Two deliberately separate mechanisms make it
visible in the scene: the material's own **emission**, which is how the lava surface appears, and
a small number of explicit **point lights**, which are how lava lights its surroundings. Nothing
here is global illumination, path tracing, photon mapping, or area-light sampling.

### Material emission

`Material` stores an `emission_color` and a non-negative `emission_strength` (default: none).
Emission is validated: the color must be finite and non-negative per channel, the strength must be
finite and `>= 0`, and their product must be finite. Invalid values are rejected, not clamped.
Unlike albedo, emission is not limited to `[0, 1]`: it is radiance, and intermediate `Color`
values may exceed the displayable range. The framebuffer already clamps only when converting to
RGB8, so no HDR or tone-mapping stage was added. Tone mapping is not implemented.

```text
emission = texture_sample * emission_color * emission_strength
local    = ambient
         + directional_visible * (directional_diffuse + directional_specular)
         + sum over point lights of point_visible * (point_diffuse + point_specular)
         + emission
```

Emission is self-radiance added to the local result. It is not multiplied by Lambert diffuse and is
not shadowed, so lava stays bright on its unlit side, inside a shadow, and in the dark. The
texture modulates the emission (albedo does not), which keeps the painted lava detail readable
instead of flattening it into one saturated color. Because emission is part of local shading, a
reflection or refraction ray that reaches lava sees the same emissive radiance through the
ordinary `trace_ray` path; there is no lava-specific reflection code.

Canonical lava uses `emission_color = (1.00, 0.88, 0.72)` and `emission_strength = 1.5`. The lava
texture averages about `(0.85, 0.41, 0.10)`, so an average texel emits roughly `(1.3, 0.5, 0.1)`:
bright texels saturate while the darker crust remains readable. Grass, cobblestone, obsidian, and
glass do not emit, and none of their other optical parameters changed.

### Point lights

`PointLight` has a position, a color, an intensity, and a radius. The position must be finite, the
color finite and non-negative, the intensity finite and `>= 0` (zero is an inert light), and the
radius finite and `> 0`. A point light is not geometry and never blocks rays. The scene owns a small
list of them; each shaded hit iterates that list without allocating, cloning, or dynamic dispatch.

For a hit at `P` with shading normal `N` (the geometric normal unless the material has a normal
map) and a light at `L`:

```text
distance    = |L - P|
attenuation = clamp(1 - distance / radius, 0, 1)²
radiance    = color * intensity * attenuation
diffuse     = base_color * radiance * max(N·L_dir, 0)
specular    = radiance * material_specular * max(N·H, 0)^32
```

The falloff is intentionally not inverse-square: it gives a predictable artistic radius, exactly
zero influence at and beyond it, and a cheap evaluation. The specular term reuses the directional
light's Blinn-Phong model and shininess. Lights beyond the radius, inert lights, lights on the
surface, and back-facing surfaces return before any shadow ray or specular work; materials with
zero specular skip the highlight. The geometric normal admits a light and offsets its shadow ray; the diffuse and specular terms
use the shading normal, which only differs for normal-mapped materials (see Normal Mapping).

Each potentially lit hit casts one any-hit shadow ray from `P + N * POINT_LIGHT_SHADOW_BIAS`
(`1.0e-4`, a unit-scale assumption like the other biases) toward the light, limited to the remaining
distance to it. The interval is finite because the light is a point, not a surface: a blocker
beyond the light cannot shadow it. The shadow ray reuses the existing `Scene::is_occluded` query.
As for the directional light, glass is an opaque blocker of point-light shadow rays; transparent
or colored shadows are not implemented.

### Representing lava regions

The intended future architecture is many connected lava voxels reduced to one or a few
representative point lights per region, not one light per lava voxel. The procedural region
extraction is not implemented. The showcase places a single warm light, color `(1.00, 0.45, 0.12)`,
intensity `3.0`, radius `5.0`, at `(2.3, 1.0, -1.15)`, just outside the lava block's face toward the
default camera so the block does not shadow its own light. It visibly warms the nearby grass and
fades with distance; glass and obsidian inside its radius receive a fainter contribution.

## Voxel Grid Foundation (Phase 4)

`src/voxel/` holds the logical Minecraft-style world that 3D DDA traversal and future procedural
terrain use. Scene queries traverse it (see Hybrid Voxel + AABB Traversal below); the default
showcase owns no grid and still renders byte-identically from its five AABB blocks.

- `BlockType` (`repr(u8)`, one byte) is a block's identity: grass, dirt, cobblestone, obsidian,
  glass, lava, and coal/iron/gold/diamond ore. There is no separate stone block; cobblestone is
  the rock material. It is separate from `MaterialId`, which describes optical behavior.
  `BlockMaterials` resolves each block type to a registered `MaterialId` with an exhaustive
  `match`; voxels store no material and lookup involves no strings, hashing, or allocation.
- `Voxel` is `Empty` or `Block(BlockType)`, still one byte because `Empty` uses a spare
  discriminant value.
- `VoxelPosition` is an integer world voxel coordinate (`i32` per axis). Voxel `(x, y, z)` owns
  the half-open unit cell `[x, x + 1) × [y, y + 1) × [z, z + 1)` on the usual axes (`+X` east,
  `+Y` up, `+Z` south). AABB intersection semantics are unchanged; exact ray/boundary handling
  is defined by the DDA (see below).
- `VoxelGrid` owns one contiguous `Vec<Voxel>` of `width × height × depth` cells, indexed
  `x + width * (z + depth * y)` (X fastest, then Z, then Y). Its integer `origin` is the world
  voxel at local `(0, 0, 0)`, so `local = world - origin` and a grid can be centered on the world
  origin, for example `origin = (-8, -4, -8)` for a 16 × 8 × 16 grid. Construction rejects zero
  dimensions, volume overflow, grids reaching past the `i32` world range, and storage beyond the
  allocation limit. Reads outside the grid return `None` and writes return `VoxelOutOfBounds`
  without changing anything; negative offsets are rejected rather than wrapped to `usize`.
- No voxel becomes an `Aabb`. Fully enclosed interior voxels remain ordinary logical occupancy:
  DDA stops at the first occupied cell along a ray, so hidden voxels need no geometry or
  culling pass.

A 16 × 16 × 16 grid stores 4 KiB of voxels and a 64 × 32 × 64 grid 128 KiB, plus a 64-byte
header.

## Voxel Scene Integration (Phase 4)

This step prepared the scene, materials, and hit representation for hybrid traversal without
changing what was traversed; Mission 24 then made scene queries traverse the grid.

### Auxiliary terrain materials

Dirt and the four ores get world materials that are deliberately not rubric materials: they live
in `AuxiliaryMaterials`, separate from `CanonicalMaterials`, and are not part of the five-material
table below.

| Material | Texture | Albedo | Specular | Transparency | Reflectivity | IOR | Emission / normal map |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| Dirt | `dirt` (shared with grass bottom) | `(1.00, 1.00, 1.00)` | 0.03 | 0.00 | 0.00 | 1.0 | None |
| Coal / iron / gold / diamond ore | `coal_ore` / `iron_ore` / `gold_ore` / `diamond_ore` | `(1.00, 1.00, 1.00)` | 0.08 | 0.00 | 0.00 | 1.0 | None |

White albedo keeps the original textures readable. Reflectivity is exactly zero because these
blocks will make up most of the terrain, and any non-zero value spawns a recursive reflection ray
from every hit. The ores share cobblestone's rock specular and differ only in texture. At startup
the four ore PPMs are loaded once; dirt reuses the `TextureId` already registered for the grass
bottom face, so no texture is loaded twice.

### Block-to-material mapping

| Block type | Material |
| --- | --- |
| Grass, Cobblestone, Obsidian, Glass, Lava | Canonical material of the same name |
| Dirt, CoalOre, IronOre, GoldOre, DiamondOre | Auxiliary material of the same name |

`BlockMaterials::new(canonical, auxiliary)` builds the mapping. `Scene::set_block_materials`
rejects a mapping that names any unregistered material, so a voxel can never resolve to an unknown
`MaterialId`.

### Scene ownership

`Scene` optionally owns one primary `VoxelGrid` next to its AABB `SceneObject`s, plus the
`BlockMaterials` that resolve it. Both are `Option`s, so scenes and tests without voxels need no
grid, and `set_voxel_grid` requires block materials first, so a grid is never stored without a way
to resolve its blocks. The showcase registers its block materials but no grid. `closest_hit` and
`is_occluded` test both the grid and the AABB objects.

### Common hit representation

Every geometry source reports a `geometry::SurfaceHit` (`t`, world position, outward geometric
normal, `CubeFace`, UV). This is the former `AabbHit` under a source-neutral name: an AABB hit and
a voxel-face hit carry exactly the same data, so a second type would only duplicate it.

- `Aabb::intersect` returns a `SurfaceHit`.
- `voxel::VoxelHit` is a 64-byte `Copy` value holding a `SurfaceHit` plus the voxel coordinate,
  `BlockType`, and resolved `MaterialId`. `VoxelHit::try_new` builds one from a voxel, face, hit
  position, and `t`. It derives the normal from the face, computes the UV, and resolves the
  material through `BlockMaterials`. It intersects nothing: finding the face is the DDA's job. It rejects non-finite input and positions more than `1.0e-3` from the claimed face of the
  claimed cell.
- `SceneHit` holds `geometry: SurfaceHit`, `material_id`, and a lightweight
  `source: HitSource` (`Object` or `Voxel { position, block }`). Shading reads only `geometry` and
  `material_id` and never branches on `source`; the source keeps the crossed voxel and block
  available for debugging, traversal, and future connected-glass logic. `SceneHit` is 64 bytes and
  `Copy`. There are no trait objects and no allocation.

`CubeFace::uv` is the single UV table for axis-aligned surfaces. `Aabb` computes box-normalized
local coordinates and passes them to it; `VoxelHit` passes the position relative to the voxel's
minimum corner. Tests check that a voxel hit at a translated, negative, and asymmetric coordinate
equals the `SurfaceHit` of the equivalent unit AABB exactly, for all six faces, including oblique
rays.

Touching glass voxels behave as glass → air → glass: `HitSource::Voxel` and `VoxelHit` keep the
voxel, block, and face so connected-glass handling remains possible later, but nothing uses that
yet. Lava voxels map to canonical lava, but no point light is derived from them; procedural lava
regions will later get one or a few representative lights.

## 3D DDA Voxel Traversal (Phase 4)

`VoxelGrid::intersect(ray, &block_materials, t_min, t_max) -> Option<VoxelHit>`
(`src/voxel/traversal.rs`) returns the first occupied voxel surface along a ray, and
`VoxelGrid::intersects(ray, t_min, t_max) -> bool` is its any-hit counterpart for shadow rays.
Both run one shared private DDA (`first_occupied`), so they cannot diverge: `intersects` equals
`intersect(..).is_some()` but stops at the first occupied cell without snapping a position,
computing a UV, resolving a material, or building a `VoxelHit`. Every block type, glass and lava
included, is an opaque blocker. `Scene` calls both; see the next section.

How it works:

- **Grid-bound clipping.** One slab test against the whole grid, `[origin, origin + size)` per
  axis, gives the entry and exit parameters. A ray that misses the grid, or only touches its
  boundary, does no stepping at all; space outside the grid is empty, and traversal stops as soon
  as the ray leaves it.
- **Stepping.** Amanatides–Woo state per axis: the current integer cell, a step direction, and the
  parameter `t_next` of the next cell boundary. Each iteration moves to the cell whose boundary
  comes first and reads one byte from contiguous storage through an index maintained alongside
  the cell coordinates. Cost is proportional to the cells crossed (at most `width + height +
  depth`), not to the grid volume or the number of blocks. There is no per-voxel `Aabb`, no
  allocation, no hit list or sorting, and no dynamic dispatch; a block's material is resolved only
  once, for the hit. `t_next` is recomputed as `(plane - origin) / direction`, the slab test's own
  expression, rather than accumulated from `t_delta`, so there is no drift and a voxel hit's `t`
  is bit-identical to the equivalent unit `Aabb`'s.
- **First occupied voxel.** Entering an occupied cell from empty space reports the face crossed
  (moving `+X` hits `NegativeX`, moving `-Y` hits `PositiveY`, and so on). Interior voxels are
  never reached because traversal stops at the first occupied cell.

Conventions:

| Situation | Behavior |
| --- | --- |
| Searched segment | Starts at `max(t_min, grid entry)`; with `t_min = 0` and the origin inside the grid, at the origin |
| Interval | Inclusive `[t_min, t_max]`, `t_max` may be infinite; NaN bounds, `t_min > t_max`, or a non-finite origin return `None`, like `Aabb::intersect` |
| Origin strictly inside an occupied voxel | That voxel's first exit face, never a `t = 0` hit (needed for refraction inside glass) |
| `t_min` falling inside an occupied voxel | That voxel's exit, like an `Aabb` whose entry is clipped |
| Origin exactly on a cell plane | Direction-aware half-open ownership: at `x = 1.0` a `+X` ray is in cell 1 and a `-X` ray in cell 0. The cell being left is never reported; starting exactly on the entry face of an occupied cell reports that face at `t = 0`, as the slab test does |
| Edge and corner crossings | All tied axes step together straight to the diagonal cell; cells touched only along an edge or at a corner are not entered |
| Face priority on ties | X, then Y, then Z (the axis order of `Aabb`'s slab test), for entry and exit faces alike |
| Zero or near-zero direction components | `\|d\| <= 1e-8` (the slab test's own threshold) never steps and uses floor ownership; no division by zero or NaN state |
| Grid origin | Any integer origin, including negative ones; rays cross world zero normally |

The hit position is `ray.at(t)` with only the coordinate along the face normal snapped to the
voxel's exact integer plane, so the hit always lies on the claimed face; in-face coordinates and
therefore UVs stay ray-derived. The `VoxelHit` is built through `VoxelHit::try_new`, which uses the
shared `CubeFace::uv` table.

Adjacent glass voxels are still independent cells: a refraction ray biased inside one glass voxel
exits at the interface with its glass neighbor (glass → air → glass), unchanged from the AABB
behavior. Lava is an ordinary occupied voxel; traversal creates no lights.

Tests compare the DDA against a test-only brute-force oracle of one unit `Aabb` per occupied voxel
over about 27,000 deterministic rays and four intervals on a mixed 5 × 4 × 6 grid: hit or miss,
voxel, `t`, face, normal, and UV agree exactly and the position agrees to `1e-5`. The few rays
whose nearest contact lies on a voxel edge, where the closed-box slab test and half-open ownership
legitimately differ, are counted and excluded (12 of about 107,000 queries).

Visited cells versus a naive scan (explanatory only, not a render benchmark), in a 64 × 16 × 64
grid whose lower half is solid (32,768 blocks):

| Ray | Cells visited by DDA | `Aabb` tests in a naive scan |
| --- | ---: | ---: |
| Straight down onto the surface | 9 | 32,768 |
| Steep oblique camera ray | 28 | 32,768 |
| Grazing oblique camera ray | 59 | 32,768 |
| Across the empty grid, axis-aligned | 64 | 0 blocks to test |
| Across the empty grid, oblique | 134 | 0 blocks to test |

## Hybrid Voxel + AABB Traversal (Phase 4)

`Scene::closest_hit` and `Scene::is_occluded` query both geometry sources with the caller's ray
interval, so every renderer ray — primary, reflected, refracted, directional shadow, and
point-light shadow — sees voxels and arbitrary AABB objects without any renderer change:

```text
Tracer::trace_ray / shade_hit
        |
        v
Scene::closest_hit / Scene::is_occluded
        |
   +----+-----------------+
   |                      |
AABB objects           VoxelGrid
linear slab scan       3D DDA
   |                      |
   +----------+-----------+
              v
    nearest hit (or any blocker) -> SceneHit
```

- **Nearest hit.** Objects are scanned first, shrinking the interval as usual; the nearest object
  `t` (or the caller's `t_max` if none) becomes the DDA's `t_max`, so voxel traversal stops at an
  object in front instead of walking on through the grid. Objects go first because the DDA's cost
  is proportional to the cells it crosses, so it gains from a shorter interval, while a slab test
  costs the same whatever its bounds. No hit arrays or sorting.
- **Object-wins tie rule.** A voxel replaces the object hit only when its `t` is strictly smaller;
  at equal `t` the AABB object wins, with zero tolerance. Explicit objects may deliberately overlay
  or sit on voxel terrain and must stay visible there. Exact comparison is enough because both
  sources compute a plane crossing as `(plane - origin) / direction`: a voxel face and a coplanar
  object face give bit-identical `t`. The DDA's inclusive `t_max` can report a voxel at exactly the
  object's `t`; the strict comparison discards it. The `1e-3` `VoxelHit` face-validation tolerance
  plays no part, and an object even `1e-4` farther loses.
- **Occlusion.** Objects are tested first, then `VoxelGrid::intersects`, both with the interval
  unchanged; the first blocker returns `true`. No `SceneHit`, UV, or material work is done for a
  blocker. Directional shadow rays (`t_max = ∞`) are now blocked by voxels; point-light shadow rays
  keep their finite `t_max`, so a voxel between the surface and the light blocks it and one beyond
  the light does not. The lighting equations, biases, and early-outs are unchanged.
- **Metadata.** Object hits carry `HitSource::Object` and voxel hits
  `HitSource::Voxel { position, block }`. The renderer never branches on it.
- **Invariant.** A grid is stored only after block materials, so a voxel hit always resolves to a
  registered material. A scene without a grid takes the object path only and behaves exactly as
  before. Do not add `SceneObject`s duplicating occupied voxels: that doubles traversal for the
  same surfaces and creates tied surfaces.

Voxel hits receive exactly the AABB shading pipeline: per-face texture selection, normal mapping
(voxel cobblestone uses the derived map), ambient, directional and point-light diffuse/specular,
hard shadows, emission (voxel lava glows, but creates no light), recursive reflection (voxel
obsidian), and Fresnel refraction (voxel glass; a refracted ray biased inside a glass voxel reports
its exit face, as Mission 23 specified). Dirt and the ores render through their auxiliary
materials. Tests check that each of the ten block types traces to the same color as the unit `Aabb`
in its cell, under full lighting with a point light, at the primary and the maximum depth.

### Voxel diagnostic scene

`cargo run --release -- --voxel-diagnostic` renders a small hybrid scene instead of the showcase
(exported to `output/voxel_diagnostic.ppm`): an 8 × 6 voxel slab of grass over dirt with the four
ores exposed on the camera-facing side, a two-block voxel lava pool with one hand-placed point
light, voxel cobblestone, obsidian, and glass blocks, and two AABB objects (a thin obsidian post
and a cobblestone half slab) standing on the voxel ground. No surface is represented twice. The
default showcase is unchanged and still the Phase 3 regression reference.

## Deterministic Tile Renderer (Phase 4)

Mission 25 reorganized the sequential pixel loop into tiles, rendered one after another on a single
thread, as the work division for the multithreaded scheduler that Mission 26 added (next section).

- `Tile` (`src/render/tile.rs`) is a small `Copy` value describing a half-open rectangle
  (`x_start <= x < x_end`, `y_start <= y < y_end`) in global framebuffer pixels. It holds no
  pixels and no references.
- `TileGrid` derives tile boundaries arithmetically from the framebuffer and tile dimensions, with
  overflow-safe ceiling division and checked tile counts; no tile list is stored, and `tiles()`
  is an allocation-free iterator. Zero tile dimensions and tile-count overflow are returned as
  errors (`TileGridError`, surfaced as `RenderError::InvalidTileConfiguration`), never panics.
- Tiles are indexed **row-major**: `tile_x = index % columns`, `tile_y = index / columns`; tile 0
  is the upper-left tile and indices advance left to right, then top to bottom. The same indices
  are claimed dynamically by worker threads.
- The initial tile size is 16×16 (`TILE_WIDTH`, `TILE_HEIGHT`). At the original 320×180 that was
  20 × 12 = 240 tiles with a 4-pixel last row; at the current 640×360 it is 40 × 23 = 920 tiles and
  360 is not a multiple of 16, so the last tile row is 8 pixels high. Edge tiles are clipped,
  so framebuffer dimensions need not be multiples of the tile size, and every pixel belongs to
  exactly one tile.
- `Renderer::render` keeps its signature; the application knows nothing about tiles. On the
  sequential path each tile traces its pixels and writes straight into the single CPU framebuffer.
  Raylib remains presentation only and sees none of this.
- A pixel's primary ray always uses its **global** framebuffer coordinates and the same pixel-center
  convention as before, so the image does not depend on tile size or tile order. Tracing, lighting,
  hybrid traversal, and all secondary rays are untouched.

Verification: a test-only copy of the previous row-major loop is compared byte for byte with the
tiled renderer on a reflective/refractive/emissive voxel-and-AABB scene, the five-block showcase,
and the voxel diagnostic scene, at 320×180 and at non-divisible sizes (including 1×1), for tile
sizes 1×1, 8×8, 16×16, 32×16, 32×32, 5×7, 7×3 and one larger than the frame; reversed and
strided tile orders also give identical images. The default showcase and voxel diagnostic PPMs
are byte-identical to the pre-mission renders.

Release timings through the application binary, 320×180, nine launches per run, interleaved
against a copy of the pre-Mission-25 binary (the initial CPU render time the app prints): default
showcase 5.3–5.6 ms before and 5.3–5.5 ms after (medians about 5.5 and 5.35 ms); voxel diagnostic
5.5–5.7 ms before and 5.2–5.5 ms after. The difference is small and well inside code-layout noise,
so no speedup is claimed: single-thread performance is approximately unchanged, as expected from
reordering the same work. Close-view timings were not re-measured. These are local development
observations, not universal benchmarks.

## Dynamic CPU Multithreading (Phase 4)

Mission 26 renders the 16×16 tiles in parallel on CPU threads from the Rust standard library, and
Mission 27B simplified how finished tiles reach the framebuffer. It adds no dependency, no GPU
work, and no `unsafe` code, and it does not change tracing, traversal, materials, or lighting:
workers call the same `Tracer` the sequential renderer uses.

### Worker lifecycle

`Renderer::render` stays synchronous. Each call opens a `std::thread::scope`, spawns a fixed set of
workers, and returns only after the scope has joined every one of them. There is no persistent
thread pool, no background rendering, and no thread per tile. Workers borrow the scene, lighting,
environment, and camera immutably through the scope; nothing is cloned per worker.

### Dynamic tile scheduling

Workers share one `AtomicUsize` holding the next unclaimed tile index. A worker claims a tile with
`fetch_add(1, Ordering::Relaxed)`, traces it, and claims again until the index reaches the tile
count. Every index is handed out exactly once, so tiles are never skipped or traced twice, and fast
workers simply take more tiles. `Relaxed` is sufficient because the counter only assigns work; it
never publishes pixel data. Tile geometry comes from `TileGrid` arithmetic; no tile list is built.

### Framebuffer integration

There is exactly one framebuffer, a contiguous row-major `Vec<Color>`. A tile is not a contiguous
slice of it, so a worker first traces into a private buffer and then copies the finished tile in:

1. Each worker owns **one** reusable tile-sized `Vec<Color>` (allocated once per worker per render,
   sized for the largest tile, and reused for every tile it claims, edge tiles included).
2. It traces a claimed tile into that buffer, holding no lock.
3. It locks the framebuffer, which sits behind a standard-library `Mutex` borrowed through the
   scope (no `Arc`), calls `Framebuffer::write_tile` to copy the tile row by row into its global
   position, and releases the lock before claiming the next tile.

The lock covers only that copy; primary-ray generation, intersection, shading, recursive rays, and
texture sampling all run outside it. Tiles never overlap and nothing is blended, so the order in
which workers acquire the lock cannot change the image. The calling thread only waits for the
workers to finish. Mission 26 instead moved buffers to the calling thread over `mpsc` channels,
with two buffers per worker; replacing that with the lock removed the channels, the buffer-return
protocol, and the second buffer, and was also faster with few workers (see Measurements).

### Worker count and sequential fallback

- Automatic (`Renderer::render`): `std::thread::available_parallelism()`, falling back to one worker
  when it fails. The application prints this count at startup.
- Explicit (`Renderer::render_with_worker_count`): any positive count, for testing and
  benchmarking. Zero is `RenderError::InvalidWorkerCount`.
- Every request is bounded by the tile count and by `MAX_WORKERS = 64`, so an excessive request
  never spawns more threads than tiles or than 64.
- One effective worker (requested, single-CPU systems, or one-tile frames) runs the Mission 25
  sequential tile loop directly on the calling thread, with no thread and no buffers.

### Errors and panics

A worker whose tile fails stops and returns that tile's index and `RenderError`, and a shared stop
flag keeps the other workers from claiming new tiles; tiles already claimed are finished, so nothing
blocks and the scope joins cleanly. Tiles are claimed in increasing index order, so every tile
before the failed one is still traced, and after the join the renderer returns the lowest-index
failure: the same error the sequential renderer reports. A failed render never returns `Ok`.

A worker panic is not swallowed: the join re-raises its payload on the calling thread, so a panic
never becomes `Ok`. A worker that panics while the framebuffer lock is held poisons it; the other
workers then panic on the poisoned lock instead of continuing, and the poisoned frame is never
replaced or recovered. A tile that does not fit the framebuffer, which breaks the grid's partition
contract, panics outside the lock.

### Exact equivalence

Each pixel is traced independently from its global coordinates; there is no shared random state,
no cross-pixel accumulation, and no floating-point reduction across workers. Tests compare color
values and RGB8 bytes exactly against the test-only row-major reference and the single-worker path
for 1, 2, 3, 4, 5, 7, 8, and 16 workers (and requests up to `usize::MAX`), tile sizes 1×1 through
4096×4096 including 8×8, 16×16, 32×16, and 32×32, non-divisible frames, the optics test scene, the
showcase, and the voxel diagnostic scene. Deliberately delayed early tiles, which complete far out of
index order, give the same image. The application's default showcase and voxel diagnostic PPMs are
byte-identical to the Mission 25 renders, and a release sweep of 5,760 camera poses (both scenes,
four targets, 24 yaw steps, six pitches, five radii, at 64×36) gave identical images for 2, 3, and
automatic workers against one worker.

### Measurements

Apple M3 Pro, 11 CPUs (5 performance and 6 efficiency cores), `available_parallelism() = 11`,
release build, 320×180, 16×16 tiles. Mission 27B was compared with the Mission 26 channel
implementation as two binaries that differ only in `parallel.rs`, run alternately (six rounds in
each order, 3 warm-up and 15 measured renders per configuration and round); the table gives the
median of those 12 rounds in ms. Automatic is 11 workers. The close-glass view targets the glass
block at radius 2, so most primary rays spawn reflection and refraction rays. The voxel proxy is a
throwaway scene of five voxel islands (48×20×48 grid) with obsidian, glass, and lava, orbited from
a distance; it stands in for the planned EggWars world and is not part of the repository.

| Scene | Workers | Mission 26 | Mission 27B | Change |
| --- | ---: | ---: | ---: | ---: |
| Default showcase | 1 | 4.99 | 5.12 | +2.4% |
| | 2 | 3.11 | 2.67 | −14.0% |
| | 4 | 1.64 | 1.40 | −14.4% |
| | 8 | 1.08 | 1.00 | −7.6% |
| | automatic | 0.92 | 0.90 | −2.2% |
| Voxel diagnostic | 1 | 5.06 | 5.18 | +2.5% |
| | 2 | 3.15 | 2.68 | −14.8% |
| | 4 | 1.65 | 1.41 | −14.5% |
| | 8 | 1.07 | 0.99 | −7.9% |
| | automatic | 0.92 | 0.90 | −1.8% |
| Close glass | 1 | 30.22 | 31.89 | +5.5% |
| | 2 | 16.72 | 16.33 | −2.4% |
| | 4 | 8.81 | 8.39 | −4.8% |
| | 8 | 5.36 | 5.51 | +2.7% |
| | automatic | 4.56 | 4.71 | +3.2% |
| Voxel proxy | 1 | 10.69 | 10.80 | +1.0% |
| | 2 | 6.43 | 5.61 | −12.8% |
| | 4 | 3.40 | 2.92 | −14.0% |
| | 8 | 1.95 | 1.85 | −5.0% |
| | automatic | 1.59 | 1.60 | +0.5% |

Interpretation:

- The lock removes most of the cost with few workers: 13–15% less time at two and four workers on
  every scene except close glass, where tracing dominates. At the automatic count the difference
  is within noise on every scene, so no speedup is claimed there.
- The one-worker path is the unchanged sequential renderer in both binaries, yet it differs by up to
  5.5% (close glass), so differences of that size are code-layout noise, not synchronization. The
  close-glass rows at 8 and automatic workers fit that explanation: each binary's speedup over its
  own one-worker time is the same (6.6× against 6.8× at 11 workers), so the lock adds no
  measurable cost there.
- Scaling is sublinear. Six of the eleven CPUs are efficiency cores, and every render pays for
  thread creation and tile copies. Two workers scale worse than the rest in every scene. An
  efficiency-core placement was the first guess, but the Mission 27A investigation gathered
  evidence against it and did not establish the mechanism; see Phase 4 Audit. It is not a Phase 4
  blocker.
- These are local development observations, not universal benchmarks. Parallelism is not
  guaranteed to help on every machine or for very small frames.

### Memory

`size_of::<Color>()` is 12 bytes (three `f32`). One 16×16 tile buffer holds 256 × 12 = 3,072 bytes,
and each worker owns one: 11 workers use 33 KiB (33,792 bytes) and the 64-worker maximum 192 KiB
(196,608 bytes), half of the two-buffer Mission 26 design (66 and 384 KiB) and with no channel
allocations. Thread stacks are not included. All of it is freed when the render returns. No worker
allocates a framebuffer or clones scene resources.

## Phase 4 Audit

The final technical audit of Phase 4 (Missions 21–27B, checkpoint `d0dcaac`) read the voxel, scene,
traversal, tile, parallel, and renderer sources and their tests, and ran the full validation suite.
It found no implementation defect, so it changed no runtime code and added no tests: the suite is
unchanged at 529 passing tests.

### Final architecture

```text
Scene ── AABB objects ── linear slab scan ──┐
     └─ VoxelGrid + BlockMaterials ─ 3D DDA ┴─> nearest SurfaceHit -> SceneHit / any blocker
                                                        │
Renderer::render ─ TileGrid (16×16, row-major) ─ AtomicUsize claim ─ scoped workers
                                                        │ one tile buffer per worker, no lock while tracing
                                                        └─ Mutex<&mut Framebuffer>: lock only for write_tile
```

### What was verified

| Area | Result |
| --- | --- |
| Voxel grid | One contiguous `Vec<Voxel>` indexed `x + width * (z + depth * y)`; `Voxel` and `BlockType` are one byte each (asserted by a test); negative origins, world/local conversion in `i64`, bounds checks, and volume/world-range overflow checks are covered; lookups allocate nothing; no per-voxel `Aabb`, material, or visibility; interior voxels stay stored |
| Block/material mapping | An exhaustive `match` maps every `BlockType` to a registered `MaterialId` (checked on insertion); canonical material parameters are unchanged since Phase 3, including the small reflectivity of grass (0.02), cobblestone (0.03), and lava (0.05), which stay by decision with no reflection threshold; textures are owned once by the scene and referenced by ID, with no string lookup or file access while rendering |
| 3D DDA | One grid-bounds slab clip, then Amanatides–Woo stepping over contiguous storage; positive, negative, and parallel directions, integer boundaries, edge/corner ties (X > Y > Z), finite and infinite intervals, starts inside empty and solid cells, and a step bound are all handled; no per-cell `Aabb`, no allocation; plane parameters stay `(plane - origin) / direction` |
| DDA oracle | A test-only brute-force reference of one unit `Aabb` per occupied voxel agrees with the DDA on hit/miss, voxel, `t`, face, normal, UV, position, block, and material over about 27,000 rays × 4 intervals; only nearest contacts on edges or corners, where closed-box and half-open ownership legitimately differ, are excluded |
| Hybrid queries | `closest_hit` scans objects, then runs the DDA with the nearest object `t` as `t_max`; the object wins exact ties; caller intervals are respected; scenes without a grid take the object path only; a grid cannot be stored without block materials. `is_occluded` tests objects, then the any-hit DDA (no `VoxelHit`, UV, or material work), with early exit; point-light shadow rays keep their finite interval, and glass and lava voxels are opaque blockers |
| Recursive effects | The renderer has no voxel-specific branch and never reads `HitSource`; reflection, Fresnel refraction, total internal reflection, lava emission, normal mapping, directional and point lighting, and the environment all go through `Scene`, with `MAX_RAY_DEPTH = 3` unchanged. Tests trace every block type against its unit-`Aabb` equivalent |
| Tiles | Row-major indices; 16×16 default, 240 tiles at 320×180 with a 4-pixel last row (920 at 640×360 with an 8-pixel last row, since Mission 30.5); overflow-safe arithmetic, no stored tile list; primary rays use global pixel coordinates; output is independent of tile size and order |
| Parallel renderer | `std::thread::scope`, `available_parallelism()` default, `MAX_WORKERS = 64`, one effective worker runs the sequential loop on the calling thread; tiles are claimed once each with `fetch_add(1, Relaxed)` (assignment only, never publication); every worker is joined before `render` returns; no pool, no thread per tile, no `unsafe`, no GPU |
| Mutex integration | One `Framebuffer` behind `Mutex<&mut Framebuffer>`; one reusable tile buffer per worker; no channels, buffer-return protocol, staging frame, or framebuffer clone; the lock covers only `write_tile`, so tracing never holds it |
| Errors and panics | A worker error stops further claims, claimed tiles finish, and the lowest-index failure is returned (the sequential renderer's error); a worker panic is re-raised after the join, so a panic or poisoned lock can never yield `Ok`; zero workers and invalid tile sizes are rejected; failures and panics in every tile terminate without deadlock |
| Determinism | Parallel output is byte-identical to the sequential reference for every tested worker count, tile size, partial edge tile, deliberately uneven tile duration, and scene (optics test scene, showcase, voxel diagnostic); there is no shared random state and no floating-point reduction across workers |
| Hot path | No per-ray, per-hit, per-pixel, or per-tile allocation, no cloning of textures or scene, no logging, mutable RNG, or filesystem access while tracing. The remaining allocations are per render: one tile buffer per worker, one thread handle list, and one result list |
| Raylib boundary | Raylib appears only in `src/app/app.rs`; core modules and worker threads never see its types |

Clippy reports only the known `clippy::module_inception` warnings (7) and two
`assertions_on_constants` warnings in test code. `cargo fmt --check`, `cargo check`, and
`cargo build --release` are clean with no compiler warnings, and `cargo run --release` runs both the
default showcase and `--voxel-diagnostic` with stable PPM output between runs.

Observations that are not defects: if a worker panics, the remaining workers keep claiming tiles
until the frame is exhausted before the panic propagates, which only delays an already fatal
outcome; and the window title still reads "Phase 3".

### Performance baseline

Apple M3 Pro, 11 CPUs (5 performance and 6 efficiency cores), release build, 320×180, 16×16 tiles,
at the audited commit. A throwaway harness outside the repository (a copy of the sources plus a
benchmark mode) rendered each configuration for 3 warm-up and 15 measured renders per round, over 8
rounds interleaved across scenes and worker counts. Each cell is the median of the 8 round medians in
ms, with the range of round medians in parentheses; "automatic" is 11 workers.

| Scene | 1 worker | 4 workers | Automatic | Speedup at 4 / automatic |
| --- | ---: | ---: | ---: | ---: |
| Default showcase | 5.13 (5.04–5.26) | 1.39 (1.38–1.41) | 0.89 (0.85–1.22) | 3.7× / 5.8× |
| Voxel diagnostic | 5.18 (4.98–5.35) | 1.40 (1.38–1.50) | 0.89 (0.87–0.99) | 3.7× / 5.8× |
| Close glass | 31.69 (30.89–32.01) | 8.34 (8.08–8.87) | 5.30 (4.82–5.68) | 3.8× / 6.0× |
| Voxel proxy | 14.08 (13.67–14.32) | 3.75 (3.63–3.90) | 2.00 (1.95–2.14) | 3.8× / 7.0× |

The voxel proxy is a different throwaway scene from the one in the Mission 27B table: five tapered
voxel islands in a 48×20×48 grid (grass, dirt, cobblestone, iron ore, obsidian pillars, glass, a lava
row, one point light) orbited at radius 40. It is not in the repository and its rows are not
comparable with Mission 27B's. The other three rows reproduce the Mission 27B observations
(default 1.40 ms at four workers and 0.90 ms automatic; close glass 31.9 ms with one worker). Close
glass at the automatic count measured 5.30 ms here against 4.71 ms then; the one-worker time
matched and no runtime code changed between the two, so this is treated as run-to-run and harness
variation, not a regression. These are local development observations, not universal benchmarks.

### Mission 27A conclusions

The read-only performance investigation (its raw data is not stored in the repository) concluded:

- 16×16 tiles are appropriate and the tile size stays at its default.
- The automatic worker count (`available_parallelism()`) performs well.
- Scoped-thread creation costs too little to justify a persistent thread pool.
- The DDA is already efficient; replacing the per-step plane computation with incremental `t_delta`
  or reciprocal multiplication was rejected because the gain was too small and reciprocal
  multiplication changed pixel output.
- No general BVH is currently justified; the remaining linear AABB scan may become expensive if
  the final scene holds many decorative objects. (Mission 31 confirmed this; Mission 31.25 added
  one flat level of spatial object groups, still no BVH.)
- The two-worker scaling anomaly (about 1.6× against about 3× at four workers) is not explained.
  Evidence went against the efficiency-core placement hypothesis, and the mechanism was not
  established. It is not a Phase 4 blocker and was not investigated further.

These rejected optimizations should not be reopened without new evidence.

### Guidance for Phase 5

Recommendations only; none of this is implemented.

- Build block-aligned terrain and structures as voxels, and reserve AABB objects for special
  non-block geometry such as arrows, chest parts, and eggs.
- Size the `VoxelGrid` tightly around the scene: DDA cost follows the cells a ray crosses, and memory
  is one byte per cell.
- Keep decorative AABB objects few, since each ray scans all of them. Consider grouping them by
  island only if the measured object count and render time in the real scene warrant it.
- Light each connected lava region with one or a few representative `PointLight`s, never one per
  lava voxel; every shaded hit iterates every light.
- Preserve the established material reflectivity values, including grass, cobblestone, and lava.
- Measure the real final scene before optimizing further.

## Procedural Floating Islands (Phase 5, Mission 28)

`src/world/` generates Minecraft-style floating islands into the scene's single global
`VoxelGrid`. Generation runs once while the scene is built; the renderer, tracer, DDA, scene
queries, lighting, and materials are unchanged and never sample noise. Camera orbit and zoom only
rerender the same grid.

### API

```rust
pub struct IslandConfig {
    pub center: VoxelPosition, // x/z: footprint center line; y: surface (flat grass) level
    pub footprint_width: u32,  // X extent, default 16
    pub footprint_depth: u32,  // Z extent, default 16
    pub thickness: u32,        // surface level down to the deepest possible block, inclusive
}

IslandConfig::new(center, thickness)            // a standard 16×16 island
IslandConfig::bounds() -> Result<IslandBounds, IslandError>
generate_island(&mut VoxelGrid, &IslandConfig, seed: u64) -> Result<IslandStats, IslandError>
```

The footprint covers world `x` in `center.x - width / 2 ..` for `width` cells (likewise `z`), so a
16-wide island at `center.x = 0` spans `-8..=7`. `bounds()` is the box every block of the island lies
in for any seed: the full footprint, from `center.y - (thickness - 1)` up to `center.y + 1`. The
seed is a separate argument, so one placement can be regenerated with any variation, and future
islands can share one grid at different centers, including negative coordinates.

**Validation before writing.** `generate_island` rejects a zero footprint (`ZeroFootprint`), a
thickness below 5 (`ThicknessTooSmall`: a rim column needs grass, dirt, and rock), any cell of the
region beyond the `i32` range (`CoordinateOverflow`), and a region not entirely inside the grid
(`OutsideGrid`). All checks run before the first write, so an error leaves the grid untouched and an
island is never clipped. Only the island's own blocks are written; other cells keep their contents.

### Determinism

Every random decision is a pure hash of `(seed, domain, integer coordinates)`: SplitMix64's
finalizer folds in the seed, an eight-byte domain tag, and each coordinate in turn. There is no
RNG stream, no global state, no hash map, and no time or OS randomness, so results do not depend on
call order, on how many values another feature draws, or on the grid's previous contents.
Coordinates are local to the footprint, so a translated island is the same island. Smooth
variation is value noise (hashed lattice values blended with smoothstep weights) using only
`+ - * / floor round sqrt` on `f32`, which IEEE 754 defines exactly; no `atan2`, `sin`, or `pow` is used, so
a seed gives bit-identical terrain on every platform for a fixed code version. A test pins the
fingerprint of the reference seed's voxels. Future generator changes may change historical seeds;
there is no versioning.

Independent domains: silhouette (`isl-silh`) and its detail octave (`isl-sdet`), surface height
(`isl-surf`), dirt depth (`isl-dirt`), underside jitter (`isl-undr`) and tip offset (`isl-utip`),
and ore clusters (`isl-orec`) and ore fill (`isl-oref`). A new feature takes a new tag and cannot
shift existing terrain or ores.

### Shape

The generator evaluates every one of the 16 × 16 footprint cells and plans one vertical run of
blocks per cell.

- **Radial mask.** For cell `(i, j)`, `nx = (i + 0.5 - W/2) / (W/2)` and likewise `nz`, so the
  footprint edge is at 1. The cell is land when `sqrt(nx² + nz²) <= R(direction)`, where
  `R = 0.96 + 0.12·n₅ + 0.05·n₉` and `n₅`, `n₉` are smooth periodic noises in `[-1, 1)` with 5 and 9
  control points around the outline. Direction is a "diamond angle" (one division instead of
  `atan2`). `R` stays in `[0.79, 1.13]`: corners are always empty, everything within 0.79 is land,
  and the island spans at least 12 (typically 14–16) cells on each axis. Because `R` depends only on
  direction, the silhouette is star-shaped around the center: no detached cells, no spikes.
- **Surface.** With `edge = distance / R`, the grass is exactly at the surface level for
  `edge < 0.45` (a flat center for future structures). Outside it, value noise at a 4-cell scale
  raises coherent patches by one block (`noise > 0.62`, only for `edge < 0.85`) and sinks patches
  at the rim by one (`noise < 0.38`, only for `edge > 0.8`). Grass is always at level −1, 0, or +1.
- **Dirt.** `1 + floor(2.2 · noise)` blocks (4-cell scale): mostly one or two, occasionally three,
  always leaving at least one rock block below.
- **Underside.** Depth below the surface level falls from the full thickness under a tip offset by
  up to 0.2 of the half extent (per seed) to 4 at the rim:
  `depth = 4 + (thickness - 5) · t · (0.4 + 0.6 t) ± 1.5 · t · noise`, with `t = 1 - distance to tip / R`.
  The profile lies between a cone and a spike, giving wide shoulders and a small irregular tip;
  the jitter fades toward the rim.
- **Coherence.** Every column contains the level just below the surface, so the 4-connected
  footprint forms one face-connected landmass with no cavities or floating blocks, and below the
  lowest possible grass each layer is a subset of the layer above: the underside tapers
  monotonically.

### Layers and ores

Each column is grass (canonical grass material: top, side, and dirt-bottom textures), then dirt
(auxiliary dirt material), then cobblestone (canonical, normal mapped) down to its bottom. No new
material, texture, glass, or lava is generated.

Ores replace only cobblestone that has a side (`±X`/`±Z`) face open to the air, the rock surface an
orbiting camera sees; interior and downward-only faces would hide them. Local space is divided into
2 × 2 × 2 cluster cells. Each cluster cell draws one ore or none, and each eligible block inside it
becomes that ore with the ore's fill probability, giving small clusters without order-dependent
spreading:

| Ore | Cluster probability | Fill | Mean per island (seeds 0–255) |
| --- | ---: | ---: | ---: |
| Coal | 10% | 75% | 14.6 |
| Iron | 6% | 65% | 7.3 |
| Gold | 3% | 55% | 3.5 |
| Diamond | 1.5% | 35% | 1.2 |

No island is guaranteed every ore; across seeds the rarity order always holds.

### Diagnostic scene and CLI

```bash
cargo run --release -- --procedural-island --seed 1847
cargo run --release -- --seed 1847        # --seed alone implies --procedural-island
cargo run --release -- --procedural-island # default seed 42
```

The island is `IslandConfig::new((0, 0, 0), 12)`, generated into a 16 × 13 × 16 grid sized
exactly to its bounds (3,328 cells), with no AABB objects and no point lights, under the existing
ambient and directional light and sunset environment. The camera orbits the island's visual center
(the footprint center, a third of the thickness below the grass) from the lit south-east side at
pitch 0.25 and radius 22, high enough to see the grass and low enough to see the underside.
Default seed 42 was chosen because its initial view shows coal, iron, and diamond ore on the lit
rock face. The startup log prints the seed, block and ore counts, grid size, and generation time;
the window overlay shows the active seed; the first render is written to
`output/procedural_island_seed_<seed>.ppm`. The same seed rebuilds the same island after any
restart, with no saved world files.

The parser uses the standard library only. A seed is ASCII decimal digits (leading zeros allowed)
from `0` to `18446744073709551615`. A missing value, a negative, signed, or non-decimal value,
overflow, a repeated option, an unknown argument, or combining `--voxel-diagnostic` with the island
options prints an error and the usage line and exits with status 2; a malformed seed never falls
back to the default. No arguments still renders the showcase.

Inspected seeds 1847, 2026, 42, 0, 7, and 99 (initial view, from below, from above, and close up)
all show a round, irregular grass top with a flat center, one-block terraces, a grass/dirt rim, a
cobblestone underside tapering to a small tip, visible ore clusters on side faces, and no holes,
detached blocks, or square outline.

## Five-Island EggWars Arena (Phase 5, Mission 29)

`src/world/arena.rs` lays out the classic EggWars cross of floating islands and generates all five
with Mission 28's `generate_island`, once per island, into one global `VoxelGrid`. There is no
second terrain algorithm, no per-island grid, no chunking, and no AABB per block; the renderer
traverses the arena like any other grid. Bridges, structures, chests, eggs, lava, and decorations
are not part of this mission.

### Layout

```text
                    North 16×16
                     (0, -32)

  West 16×16       Center 24×24       East 16×16
   (-32, 0)          (0, 0)            (32, 0)

                    South 16×16
                     (0, 32)
```

| Island | Footprint | Center `(x, y, z)` | Thickness | Region (inclusive) |
| --- | --- | --- | ---: | --- |
| Center | 24 × 24 | (0, 0, 0) | 15 | x −12..11, y −14..1, z −12..11 |
| North | 16 × 16 | (0, 0, −32) | 12 | x −8..7, y −11..1, z −40..−25 |
| South | 16 × 16 | (0, 0, 32) | 12 | x −8..7, y −11..1, z 24..39 |
| East | 16 × 16 | (32, 0, 0) | 12 | x 24..39, y −11..1, z −8..7 |
| West | 16 × 16 | (−32, 0, 0) | 12 | x −40..−25, y −11..1, z −8..7 |

Axes follow the project convention (`+X` east, `+Y` up, `+Z` south), so north is `−Z` and west
`−X`. All five islands share the surface level `y = 0`: the flat grass of every island is at the
same height, so future bridges can run level. The central island's underside reaches three blocks
deeper (thickness 15 versus 12); its top does not tower over the others.

**Separation.** `ISLAND_SEPARATION = 32` blocks from the center of the central island to the center
of each outer island. The nominal gap between square footprints is `32 − 12 − 8 = 12` blocks; the
round silhouettes (radius 0.79–1.13 of the half extent) widen the visible gap to roughly 12–17
blocks. That reads as open void in the initial view, stays a plausible Minecraft bridge length, and
keeps the whole arena framed without zooming far out. The layout is seed-independent: a seed
changes terrain, never where the islands are.

### API

```rust
pub enum IslandId { Center, North, South, East, West }   // Copy, Eq, Hash; IslandId::ALL
IslandId::direction() -> (i32, i32)    // unit (dx, dz) from the arena center; (0, 0) for Center
island_seed(global_seed: u64, IslandId) -> u64

ArenaLayout::eggwars()                 // the project layout (separation 32)
ArenaLayout::new(separation: u32) -> Result<ArenaLayout, ArenaError>
ArenaLayout::config(IslandId) -> IslandConfig
ArenaLayout::bounds() -> IslandBounds  // union of the five regions = the global grid
generate_arena(&ArenaLayout, global_seed: u64) -> Result<Arena, ArenaError>

pub struct Arena { pub grid: VoxelGrid, pub info: ArenaInfo }
pub struct ArenaInfo {
    pub global_seed: u64,
    pub separation: u32,
    pub islands: [ArenaIsland; 5],     // id, config, derived seed, IslandStats; index = IslandId
    pub grid_origin: VoxelPosition,
    pub grid_dimensions: (usize, usize, usize),
    pub occupied_voxels: usize,
}
```

Island identity is a five-variant enum used as a fixed array index, not a string or a registry.
`direction()` and each island's `config.center` are the anchors later bridge and structure missions
need: the bridge from an outer island runs along `-direction()` toward the center.

### Seeds

The arena takes one `u64` global seed. Each island's generator seed is
`hash3(global_seed, island_tag, 0, 0, 0)`, Mission 28's stateless SplitMix64-based hash with a
fixed eight-byte tag per island (`arn-cntr`, `arn-nrth`, `arn-soth`, `arn-east`, `arn-west`).
Derivation does not depend on generation order or on array positions, and no RNG stream exists,
so generating North before Center (or any other order) produces the identical grid, and every
island equals a standalone generation of that island with its derived seed. Seeds `0` and
`u64::MAX` are ordinary. Tests pin the five derived seeds and a fingerprint of the whole arena for
global seed 1847. The four outer islands share footprint and thickness but have independent seeds,
so they are related variations, never rotated copies.

### Validation and the global grid

`ArenaLayout::new` validates the complete layout before anything is allocated or written: every
island config (`IslandConfig::bounds`: footprint, thickness, `i32` range), and every pair of island
regions, which must leave at least `MIN_ISLAND_GAP = 1` empty cell between them. The check uses
the regions, the boxes every possible block of each island lies in, so it never relies on the
radial mask happening to keep islands apart. An invalid layout is an `ArenaError` (`Island`,
`Overlap`, or `Grid` if the enclosing grid cannot be created), and grid creation precedes the first
island write, so no partial arena ever exists. Separation 20 makes the footprints touch and is
rejected; 21 is the closest valid layout.

The global grid is exactly the union of the five regions: origin `(-40, -14, -40)`, 80 × 16 × 80 =
102,400 cells, 100 KiB at one byte per voxel. No margin is added: future bridges run at the surface
level between islands, inside this box. Structures that rise above `y = 1` need the grid's top
raised; Mission 30 does so (see EggWars Construction below), and `generate_arena` itself still
returns this exact grid. Cells outside the five regions, including every gap corridor, stay
`Voxel::Empty`; each island is exactly one face-connected landmass that never touches another.

### Scene, CLI, and camera

```bash
cargo run --release -- --eggwars --seed 1847
cargo run --release -- --eggwars            # default seed 42, shared with --procedural-island
```

The scene is the arena grid with block materials and no AABB objects or point lights, under the
existing ambient and directional light and sunset environment. Startup prints the global seed,
island count, total blocks and ores, grid size and origin, separation, and generation time, then
one line per island with its footprint, center, derived seed, and block and ore counts. The window
overlay shows `EggWars arena | seed <seed> | 5 islands`; derived seeds appear only in the startup
log. The first render is written to `output/eggwars_seed_<seed>.ppm`.

`--eggwars` combines only with `--seed`. Combining two of `--voxel-diagnostic`,
`--procedural-island`, and `--eggwars`, or giving `--seed` to the voxel diagnostic, is rejected with
the usage line and exit status 2, like every other malformed argument. `--seed` alone still selects
the procedural island.

The initial camera orbits the central island's center line, a third of its thickness below the
grass, at yaw 0.7 (from the lit south-east), pitch 0.35, radius 68. It shows all five islands
entirely inside the frame, the central island in the middle, the gaps between islands, the rocky
undersides, and the sunset band above the void. Orbit and zoom behave as before; there is no
camera collision yet, so zooming in far enough enters the central island (a known polish item).

### Visual results

Seeds 42, 1847, 2026, 0, 7, and `u64::MAX`, each from the initial view, orbits of 90° and 180°,
below, above, and radii 20, 40, and 110: all five islands visible and separate, the central island
clearly larger, the four outer islands distinct, round irregular silhouettes with no square
platforms, grass/dirt rims, tapered cobblestone undersides, ore clusters on side faces, no
overlapping terrain, no detached fragments, and a readable sunset/night sky. Across seeds 0–255 an
arena holds 7,169–8,771 blocks (central island 2,394–3,558, outer islands 985–1,502) and 104–204
ores, with mean counts per arena of 87.3 coal, 44.4 iron, 18.6 gold, and 6.0 diamond.

## EggWars Construction (Phase 5, Mission 30)

`src/world/construction.rs` (with `construction/bridges.rs` and `construction/structures.rs`)
turns the five-island terrain into a recognizable EggWars map. Construction is designed, not
generated: one fixed composition, written once into the same global `VoxelGrid` as ordinary voxels
(cobblestone, obsidian, and glass only). There is no AABB per block, no second grid, no template
framework, and nothing in the renderer, scene queries, DDA, or lighting knows about it.

### Pipeline

```text
global seed
  -> generate_arena_in(eggwars layout, seed, construction_bounds)   five islands (Mission 28/29)
  -> read anchors from the terrain-only grid
  -> plan bridges, outer bases and egg defenses, central ruin        (block list + metadata)
  -> validate the whole plan: inside the grid, empty cell, planned once
  -> write it                                                         one global VoxelGrid
  -> existing CPU renderer
```

`build_eggwars_arena(seed) -> Result<EggWarsArena, ConstructionError>` runs it once during scene
construction; camera movement never rebuilds anything. Planning reads only the terrain, so no
element depends on another or on planning or writing order, and validation precedes the first
write: construction never replaces terrain, never clips, and an error (`OutsideGrid`, `Occupied`,
`MissingTerrain`, or the arena's own error) leaves the grid untouched. Generation and construction
stay separate: the island generator and `generate_arena` are unchanged, and `generate_arena_in`
only generates the same islands into a larger grid.

### Grid expansion

The design's height requirements are: bridge decks at the grass level `y = 0` (one scaffold block
at `y = −1`); outer base floors at `y = 1`, eggs and defenses at `y = 2`, gate pillars up to `y = 3`;
the central floor at `y = 1`, walls up to `y = 4`, and the tallest pillar's obsidian cap at
`y = 5`. `CONSTRUCTION_TOP = 5`, and `construction_bounds` raises only the layout's top to it:
origin `(−40, −14, −40)`, 80 × 20 × 80 = 128,000 cells, 125 KiB at one byte per voxel (Mission 29:
80 × 16 × 80, 100 KiB). Bridges run inside the existing horizontal extent, so X and Z do not grow,
and the top layer is exactly the tallest block. Terrain is cell-for-cell the Mission 29 terrain:
islands, derived seeds, stats, and the pinned arena fingerprint are unchanged.

### Anchors

`surface_anchor(grid, x, z)` is the first free cell above the topmost block of a column, read from
the terrain-only grid, so it rests on real grass at that column's height (`y` 0, 1, or 2 for grass
at −1, 0, or +1), never inside terrain; it is `None` for empty columns and outside the grid. Every
structure column fills from its anchor up to its design level: a floor fills a sunken column up to
`y = 1` and leaves a raised grass block in place as part of the floor; pillars and walls start on
whichever is there. All structure columns lie inside the smallest possible silhouette
(`SILHOUETTE_MIN_RADIUS` of the half extent), so every one has ground for every seed. There is no
pathfinding.

Outer-island designs are written once in a local `(side, forward)` frame, `forward` pointing at
the center, and rotated a quarter turn at a time about the island's center point (a cell corner),
so the same shape and handedness appear on all four islands.

### Bridges

Each bridge is a two-block-wide cobblestone deck at `y = 0` on the lanes straddling the island's
center line (`x ∈ {−1, 0}` or `z ∈ {−1, 0}`), pointing straight at the center. Its ends come from
the terrain: starting in the void between the two islands' regions, each lane walks toward each
island across open void and across rim grass that sank below the deck (the deck rests on it), and
stops beside the first column whose ground reaches the deck. Each end block therefore touches
terrain face to face, so attached ends never have a gap and no deck block is buried.

| Bridge | State | Design |
| --- | --- | --- |
| North | `Complete` | Both lanes continuous from North's rim to the center's rim; one scaffold block hangs under the deck. |
| East | `Damaged` | Three cells missing from both lanes (the seed shifts the gap one cell either way); one lane's break is one cell longer, so the edge is ragged; both stubs stay attached. |
| South | `Severed` | Six cells missing from both lanes, plus one ragged cell; two separate segments, one attached to each island. |
| West | `Incomplete` | Built from West's rim six cells into the 12-cell void (one lane five), then nothing: no segment grows from the center. |

Damage is defined on the 12-cell void that separates the regions for every seed, so terrain can move
a deck's ends but never close a gap, reconnect the severed bridge, or complete the abandoned one.
Only North connects two landmasses.

### Outer bases and egg defenses

Each outer island has a 5 × 5 cobblestone floor around its future egg, two gate pillars flanking the
bridge's approach, and an obsidian defense. The egg cell is on bridge lane 0, two cells behind the
island's center, inside the always-flat middle, at `y = 2` on the floor. Mission 30 placed five
defense positions around it; Mission 30.5 replaced them with the nine-position Minecraft EggWars
enclosure, as `(side, forward, up)` offsets from the egg (`DEFENSE_TEMPLATE`, one template turned
toward the center on every island):

```text
   toward the center / bridge (attack)        O O O   forward +1
                                              O E O   forward  0   E: the egg, empty, with the
                                              O O O   forward -1      ninth block above it
```

Eight obsidian positions surround the egg at its own level and one sits directly above it (`up = 1`),
for nine; the egg's cell stays empty and there is no solid 3 × 3 × 3 enclosure. Template order is the
front row, the two flanks, the back row, then the cap (`DefenseSlot` index 8). All nine cells lie
inside the base floor and inside the grid (the cap is at `y = 3`, below `CONSTRUCTION_TOP`).
Missing blocks are never random: damage starts at the front, the side the attackers came from, so
every breach opens the egg toward the attack direction (the center).

| Island | Defense | Obsidian | Missing | Gates | Floor |
| --- | --- | --- | --- | --- | --- |
| North | `Intact` | 9 / 9 | none: sealed on every side and from above | 3 and 3 high | whole |
| West | `Damaged` | 7 / 9 | front-left and front blocks: a notch facing the bridge, cap still standing | 3 and 2 | whole |
| East | `Breached` | 5 / 9 | the whole front row and the cap: a U of two flanks and the back row open toward the bridge | 2 and 1 | one front corner gone |
| South | `Destroyed` | 3 / 9 | everything but an L-shaped back corner (left flank, back-left, back middle) | 1 and 0 (one stump) | front-left corner cratered |

The ring blocks stand on the floor with open tops, so obsidian's reflection stays visible, and
obsidian reflectivity is unchanged (0.35). An intact cap sits one block above the empty egg cell and
touches the ring only along edges, so it is the one construction block that is not face-connected
to a landmass (tests assert exactly the intact caps are such blocks). The egg cells stay empty in
the grid; Mission 31 places the two surviving eggs in them as AABB props (see Battle Aftermath
Decorations).

### Central ruin and glass

On the central island an 8 × 8 cobblestone floor (`x, z ∈ −4..3`) at `y = 1` holds a 2 × 2 obsidian
inlay at its middle, four corner pillars crumbling toward the south-east (4 high with an obsidian
cap, 3, 2, and 1), a partial north wall (3, 3, 3, 3, 2, 1 high), and a lower west wall (2, 2, 1, 1).
The south and east sides are open, so the default camera looks into the open 6 × 6 interior where
loot will be contested. Two glass panes sit in the north wall at `(−2, 3, −4)` and `(0, 3, −4)`,
one cell apart, with open air in front and behind and cobblestone on every other side; they touch no
terrain and no other glass. A refracted ray leaving a pane sideways still meets the known exit-bias
limitation, but the neighbor is uniformly textured cobblestone, so it is not visible in practice.

### Metadata

```rust
pub struct EggWarsArena { pub grid: VoxelGrid, pub terrain: ArenaInfo, pub construction: ConstructionInfo }
pub struct ConstructionInfo {
    pub bridges: [BridgeInfo; 4],        // OUTER_ISLANDS order: island, state, toward_center,
                                         // lanes (outer_end, center_end), missing_span
    pub bases: [EggBase; 4],             // island, defense_state, egg, attack_direction,
                                         // defense: [DefenseSlot { position, intact }; 9]
    pub central: CentralStructure,       // floor, inlay, open_area (empty cells for loot)
    pub surface_levels: [i32; 5],        // measured grass level at each island's center
    // + per-block-type counts: count(BlockType), total()
}
```

### Determinism and seeds

The same seed builds the identical arena (a test pins the constructed fingerprint for seed 1847).
The seed reaches construction only through terrain heights and silhouettes and three stateless
hashes on their own domains (`con-gaps`, `con-ragd`, `con-scaf`): the East gap's one-cell shift,
which lane is ragged, and where the North scaffold hangs. Different seeds keep every bridge state,
gap length, defense position and state, egg cell, and the central ruin; tests check the narrative
over 52 seeds.

### Scene, CLI, and camera

`--eggwars [--seed <u64>]` now renders the constructed arena; no CLI mode was added, and the other
modes' renders are byte-identical to Mission 29. Startup prints the arena line (total, terrain, and
ore blocks, grid, time), a construction line (block counts by type), the per-island lines, and one
line per outer island with its bridge and defense states. The default camera (yaw 0.7, pitch 0.35,
radius 68) still frames all five islands and every structure without clipping, so it is unchanged.

### Visual results

Seeds 42, 1847, 2026, and 0, from the initial view, above, low against the sunset, near the central
ruin, through the glass, and close to each defense: the five islands stay distinct with the central
island dominant; the bridges read as two-wide player bridges pointing at the center, with the East
gap, the long South break, and the abandoned West stub obvious from above and from the initial
view; the four defenses read as sealed, notched, breached, and destroyed; the panes refract the
sky and the ruin's interior without artifacts; nothing floats, nothing is buried, and the
structures are modest next to the grass. Across seeds 0–255 Mission 30's construction placed 269–310
blocks and an arena held 7,459–9,055 (always 19 obsidian and 2 glass); with the nine-block defenses
every seed holds 10 more construction blocks and 29 obsidian (24 in defenses), so 279–320 and
7,469–9,065.

## Egg Defense and Resolution Refinement (Phase 5, Mission 30.5)

Two independent changes, two commits.

**Nine-block egg defenses.** See Outer bases and egg defenses above: `DEFENSE_TEMPLATE` holds nine
`(side, forward, up)` positions, `DefenseState::intact_slots` returns `[bool; 9]`, and the four
states hold 9, 7, 5, and 3 blocks (North, West, East, South). Terrain, the arena layout, seed
derivation, bridges, the central ruin, the glass, the base floors, and the egg cells are unchanged,
so every terrain fingerprint (including the Mission 29 arena's) still passes; only the constructed
fingerprint for seed 1847 changed, to `0x9B34_718C_2742_6F74`, because the defense geometry
intentionally changed. Close-ups of all four defenses from the front, the side, and behind show the
nine-block enclosure, the U-shaped breach, the L-shaped fragment, and the front notch clearly.

**640×360 internal resolution.** `WIDTH × HEIGHT` in `src/main.rs` is now 640×360 (was 320×180),
presented in a 1280×720 window at an exact integer 2× scale (`WINDOW_SCALE` in `src/app/app.rs`;
the window size is derived from the framebuffer as `framebuffer × WINDOW_SCALE`). The Raylib texture
keeps point (nearest-neighbor) filtering, so every framebuffer pixel is an exact 2 × 2 block of
window pixels; the 16×16 texture assets are not resized. The framebuffer holds 230,400 pixels, the
reused RGBA upload buffer 921,600 bytes (allocated once), and the PPM is `P6 640 360 255` with
691,200 payload bytes. The camera aspect ratio comes from the framebuffer dimensions: it stays 16:9
(the same camera value as at 320×180), so field of view, yaw, pitch, radius, and target are untouched
and the initial composition does not move. There is still one primary ray per pixel at the pixel
center: no antialiasing, supersampling, or jitter. Tiles stay 16×16, giving 40 × 23 = 920 tiles with a
last row 8 pixels high. The renderer, parallel scheduling, DDA, materials, and lighting are unchanged.
The window title is now the phase-neutral "CPU Raytraced EggWars Diorama".

The PPM bytes intentionally differ from the 320×180 renders; at a fixed resolution output is still
byte-identical across repeated renders, worker counts (1, 3, automatic), and tile sizes (8, 16, 64).

Release timings, seed 1847 constructed arena, Apple M3 Pro (11 CPUs), medians of 20 renders after 5
warm-up renders (interactive 1 / 4 / automatic workers, ms), run twice with agreeing results:

| View | 320×180 | 640×360 | Slowdown |
| --- | ---: | ---: | ---: |
| Initial (radius 68) | 11.87 / 3.44 / 1.74 | 44.6 / 12.8 / 6.3 | 3.76× / 3.74× / 3.64× |
| Orbit +1.2 rad, radius 48 | 15.2 / 4.39 / 2.22 | 56.0 / 16.0 / 7.9 | 3.68× / 3.64× / 3.57× |
| Close (radius 28) | 20.1 / 5.70 / 2.84 | 73.0 / 20.7 / 10.2 | 3.63× / 3.63× / 3.59× |

The frame has 4× the primary rays but costs about 3.6–3.8× as much here, because per-frame fixed work
and the empty sky share do not scale with pixel count; this is a local measurement for this scene, not
a general rule. At the automatic worker count a render takes 6–10 ms, so orbiting and zooming stay
responsive at the 60 FPS target (the application's first render, which includes warm-up, measured
7.5 ms for the EggWars arena, 3.6 ms for the showcase, 4.1 ms for the voxel diagnostic, and 3.9 ms for
the island). No optimization was added; the single-threaded 45–73 ms is why the multithreaded renderer
matters at this resolution.

## Battle Aftermath Decorations (Phase 5, Mission 31)

`src/world/decorations.rs` (with `decorations/models.rs` and `decorations/validation.rs`) adds the
small props that tell the story of the finished match: two surviving eggs, the shell of the two
broken ones, two open chests in the central ruin, seven arrows stuck in blocks, and eight displaced
block fragments. Props are smaller than a block and do not align with the grid, so they are
project-owned AABBs, not voxels: the one global `VoxelGrid` is unchanged, and each prop is a handful
of `SceneObject`s that `Scene::closest_hit` and `Scene::is_occluded` scan alongside the DDA, exactly
like the showcase's objects. There are no meshes, no rotation system, no entity framework, and
nothing in the renderer, scene queries, DDA, or lighting knows about props.

### Pipeline

```text
build_eggwars_arena(seed)                  terrain + construction (Missions 28–30.5), unchanged
  -> place_decorations(&arena)             reads ConstructionInfo and the grid; never writes it
       anchors: egg cells, defense slots, bridge break edges, the ruin's open area, surface anchors
       models:  egg, chest, arrow, shell fragment, block fragment, built in the anchor's frame
       validate the whole set (below), then return Decorations { decorations, parts }
  -> eggwars_diorama_scene(grid, &decorations)
       register 7 decorative materials over one solid white texture, once
       add one SceneObject per part of every visible prop (54 of 59; the sealed North egg is
       skipped since Mission 31.25), in 8 spatial object groups (Mission 31.25)
  -> existing CPU renderer
```

`place_decorations(&EggWarsArena) -> Result<Decorations, DecorationError>` runs once during scene
construction. Every prop is placed relative to metadata or a measured surface, never at a hardcoded
world position in `main.rs`: eggs at `EggBase::egg`, base arrows in `EggBase::defense` blocks, debris
in the base frame (origin the egg cell, forward = `attack_direction`, the construction's own
quarter-turn convention), bridge debris at the break edges found by walking each lane's deck from
`BridgeLane::outer_end`/`center_end`, chests and wall arrows from `CentralStructure::open_area`, and
everything on grass at `surface_anchor`. A model is a list of boxes in sixteenths of a block in a
local `(side, up, forward)` frame; a quarter turn maps axis-aligned boxes to axis-aligned boxes, so
one model serves every orientation. The coordinates are multiples of 1/32, so they are exact in
`f32`.

### Eggs

| Island | Defense | Egg |
| --- | --- | --- |
| North | `Intact` (9/9) | survives, sealed: fully enclosed by the eight ring blocks, the cap, and the floor, so no ray reaches it |
| West | `Damaged` (7/9) | survives, visible through the broken front-left and front blocks under its standing cap |
| East | `Breached` (5/9) | destroyed: three shell fragments in the egg cell and the open front cell |
| South | `Destroyed` (3/9) | destroyed: three shell fragments scattered over the opened side of the enclosure |

`egg_survives(DefenseState)` is true exactly for `Intact` and `Damaged`. An egg is five stacked
square courses (pixel half-widths 4, 6, 5, 4, 2; heights 1, 5, 3, 3, 2), 12 px wide and 14 px tall
and widest in its lower third. It reads as the stepped pixel-art oval of the era, not a sphere or a
cube. It stands on the base floor inside its reserved cell, 2 px clear of every ring block and of
the cap, so it touches nothing but the floor. Its material is a pale warm cream (albedo
0.93, 0.88, 0.76) with a soft highlight (specular 0.25) and no reflection. Lit by the sun from the
open front and shaded under the cap, the West egg is clearly visible up close, and obsidian
reflections show it on the inner faces of the ring. The North egg exists (the narrative and the
tests require it) but its intact defense hides it completely. That is the intended reading: a
sealed defense the attackers never broke. No obsidian was removed to reveal it.

Since Mission 31.25 the North egg is placed and validated as before but marked `sealed`, and the
diorama scene does not instantiate its five parts (`Decorations::visible` skips sealed props): it
remains in `Decorations`, `egg_survives(Intact)` is still true, and its defense is still nine
obsidian blocks. Validation proves the enclosure for any sealed prop: all its parts lie in one cell
whose six face neighbors hold opaque blocks (anything but glass), here the four ring blocks beside
it, the cap above, and the cobblestone floor below, so the underside is closed too. A ray reaches
that cell only by crossing an opaque face, apart from the DDA's zero-measure edge convention (see
Known Limitations). Renders of seeds 1847, 42, 2026, and 0 from 18 poses each, ten of them close
around the North defense (sides, above, straight down, farther out), are byte-identical with and
without the egg, so removing it changed no pixel through primary, shadow, reflected, or refracted
rays. This is a single narrative decision, not hidden-surface removal: nothing else is culled.

Broken eggs leave no intact egg and nothing egg-sized: each shell fragment is a single flat cream
box at most 5 × 3 × 5 px, resting on the floor in the egg cell or a neighboring open cell.

### Chests

Two open chests stand on the central ruin's floor inside `open_area`: one against the north wall
east of both glass panes, facing south (so it never covers a pane), and one against the west wall,
facing east. Both fronts face the open south-east side and the default camera. Each is seven boxes:
four 2 px wooden walls (14 × 10 × 14 px outside) around a dark interior whose floor sits 3 px below
the rim, so the open top shows a recess; an upright lid, 14 px tall, standing on the back wall (an
axis-aligned stand-in for a lid swung open, since there is no rotation); and a small iron latch near
the top of the lid's front face. The lid reaches 1.5 blocks above the floor. Both chests stay within
their own cells, 1 px clear of the walls behind them. Materials: warm wood (0.62, 0.42, 0.20),
dark interior (0.20, 0.12, 0.06), and a pale iron latch with specular 0.6 and reflectivity 0.2,
the only reflective decorative material. It covers a few pixels, so its recursive rays are rare.

### Arrows

An arrow is three boxes along one axis: a 2.5 px flint head from 3 px inside the struck face to 1 px
outside it, a 1.5 px wooden shaft, and 2.5 px light fletching at the tail, 13 px from the face. The
head overlaps exactly its target block and also reaches out of it (visibly stuck, never buried),
while the shaft and fletching stay in the empty cell in front of the struck face. Arrows are
axis-aligned: `ArrowHeading` is `PositiveX`, `NegativeX`, `PositiveZ`, `NegativeZ`, or `Down` (an
arrow from a high shot stuck in the top of a block), and there is no arbitrary rotation. All five
headings appear:

| Site | Target | Heading |
| --- | --- | --- |
| West base | front-right defense block (obsidian), front face | from the bridge (`−X`) |
| East base | left flank (obsidian), front face, beside the breach | from the bridge (`+X`) |
| South base | last flank (obsidian), front face | from the bridge (`+Z`) |
| East bridge | island-side edge of the gap (cobblestone deck), top | `Down` |
| Center | north wall between the two panes (cobblestone), inner face | `−Z` |
| Center | west wall (cobblestone), inner face | `−X` |
| Center | grass south-east of the ruin, top | `Down` |

### Battle remnants

Eight fragments, cubes 7 px (obsidian) or 8 px (cobblestone) on a side, rest on block tops where the
fighting was: West, one obsidian piece in front of its notch; East, one obsidian piece in front of
the breach; South, an obsidian piece in front of the lost front row and a cobblestone piece in the
crater of its blasted floor (on the grass, at whatever height it has); the damaged East bridge, one
on the far edge of the gap; the severed South bridge, one on each side of the break; the center, one
beside the crumbled south-east pillar. They reuse the canonical cobblestone (normal mapped) and
obsidian (reflective) materials unchanged.

### Narrative placement

| Site | Eggs | Shell fragments | Arrows | Remnants |
| --- | ---: | ---: | ---: | ---: |
| North base (intact) | 1 | 0 | 0 | 0 |
| West base (damaged) | 1 | 0 | 1 | 1 |
| East base (breached) | 0 | 3 | 1 | 1 |
| South base (destroyed) | 0 | 3 | 1 | 2 |
| North bridge (complete), West bridge (abandoned) | 0 | 0 | 0 | 0 |
| East bridge (damaged) | 0 | 0 | 1 | 1 |
| South bridge (severed) | 0 | 0 | 0 | 2 |
| Center (contested; plus both chests) | 0 | 0 | 3 | 1 |

Damage grows from North to South, the abandoned and intact bridges stay clean, and the center
carries the loot and most of the arrows. Nothing is spread uniformly.

### Validation

Before returning, `place_decorations` checks the whole set; any failure is a `DecorationError` and
nothing is returned:

- every part is a finite, ordered box inside the grid;
- no part overlaps a block (touching faces is resting, not overlapping), except that an arrowhead
  overlaps exactly its target and also reaches out of it (`NotEmbedded` otherwise);
- a resting prop's lowest parts lie on the top of its support block, inside that block's column
  (`Unsupported` otherwise), so nothing floats;
- no part of one prop overlaps a part of another (`Overlap`);
- a sealed prop (only North's egg) lies inside one cell with an opaque block on each of its six
  faces (`NotSealed` otherwise).

Parts of one prop may touch or overlap: these are the only intended intersections (shaft against
head and fletching, walls around the chest interior, lid on the back wall, latch on the lid, egg
courses on each other), and tests check that every prop's parts form one connected piece.

### Materials

`material/decorative.rs` defines seven decorative materials: egg shell, chest wood, chest interior,
chest latch, arrow shaft, arrow head, and arrow fletching. They are separate from the canonical and
auxiliary sets, all opaque and non-emissive, with no normal map. `Scene::add_decorative_materials`
registers them once, in EggWars mode only, over one shared 1 × 1 white texture (`Texture::solid`):
each material's color is its albedo, and no image asset or loader was added.
`DecorationMaterials` resolves a part's `DecorationMaterial` to a `MaterialId` with an exhaustive
match, like `BlockMaterials`: no strings and no lookups during rendering. Shell fragments share the
egg's material, and block fragments share the canonical cobblestone and obsidian materials. The
59 objects use nine materials in total. The canonical materials are untouched: grass, cobblestone,
and obsidian reflectivity, glass IOR, and lava emission are the same. No light was added, and the
lighting and environment are not retuned.

### Determinism and seeds

Placement is a pure function of the constructed arena. The seed reaches it through terrain heights
(the crater fragment, the lawn arrow, the center fragment), bridge break positions (East gap shift,
ragged lanes), and three stateless hashes on their own domains (`dec-shrd`, `dec-rmnt`, `dec-arrw`)
that move each shell fragment, block fragment, and arrow by 0–2 px. Every seed has the same 25 props
at the same sites, with the same kinds, part counts, and headings. Tests check this over 64 seeds,
and placement succeeds on seeds 0–255. The grid is borrowed immutably: terrain and construction
fingerprints are unchanged (constructed seed 1847 is still `0x9B34_718C_2742_6F74`).

### Scene, CLI, and camera

`--eggwars [--seed <u64>]` now renders the decorated diorama (`eggwars_diorama_scene`). The
undecorated `eggwars_scene` remains as the arena stage that the voxel-level tests use. No CLI mode
was added. The showcase, voxel diagnostic, and procedural-island PPMs are byte-identical to Mission
30.5. Startup adds one line:

```text
  Decorations: 25 props (2 eggs, 6 shell fragments, 2 chests, 7 arrows, 8 remnants) as 54 AABB objects (5 parts sealed out of sight), placed in 0.023 ms, scene built in 0.758 ms
```

The default camera, resolution (640×360 in a 1280×720 window at 2×, nearest neighbor), controls,
and lighting are unchanged. From the default camera the props are small details (fewer than 1% of
the pixels change). Close-ups reveal the story.

### Visual results

Seeds 1847, 42, 2026, and 0 were inspected close to each base (front, sides, behind, low and high),
along the East and South bridges, in the ruin, and on the lawn beside it. The West egg reads clearly
through its notch and in the obsidian reflections. The East and South egg cells hold only small pale
shards, East's partly in the shadow of the U. The chests read as open Minecraft chests, with dark
recesses and upright lids. Arrows read as shaft, head, and fletching, with hard shadows on the walls
and floor. Fragments sit on the decks at the breaks and in the South crater. Nothing floats or is
buried, and on every seed the props follow the terrain.

## Spatial Grouping of Decorative AABBs (Phase 5, Mission 31.25)

Mission 31's props made every ray slab-test every decorative box, so an EggWars render cost about
3× the undecorated arena although most rays pass nowhere near a prop. Mission 31.25 adds the
smallest acceleration that fixes this: one flat level of spatial object groups inside `Scene`. It
also drops the sealed North egg from the scene (see Eggs above), so the diorama has 54 objects
instead of 59. There is no BVH, KD-tree, octree, hash grid, hierarchy, or per-cell object list, and
the renderer is unchanged: it still calls only `Scene::closest_hit` and `Scene::is_occluded`.

### Representation

`Scene` keeps owning one `Vec<SceneObject>` in insertion order (`objects()` is unchanged) and adds
a list of runs that partition it: each run is a `Range` of consecutive objects with optional
bounds.

```text
Scene::add(object)                  -> appended to an ungrouped run (no bounds, always tested)
Scene::add_object_group(objects)    -> appended consecutively as one run with bounds =
                                       exact union of the objects' boxes (Aabb::union)
objects: [ g0: South base | g1 | g2 | g3 | g4 | g5 | g6: ruin interior | g7: lawn ]
```

A group's bounds are computed once, from the objects' actual boxes, when the group is added: never
per frame, ray, tile, or camera move, and never by hand. Because each group is a contiguous range,
no object is copied, no index list is stored, and every object belongs to exactly one run. Later
`add` calls start or extend an ungrouped run and never touch a group's bounds, so objects added
after grouping are always traced and cast shadows (there is no stale metadata to invalidate). An
empty group adds nothing. The extra memory is one 48-byte run (a range and optional bounds) per
group: 8 for the diorama, 384 bytes. Textures and materials are not touched.

### Groups in the diorama

`eggwars_diorama_scene` cuts the visible props, in placement order, into consecutive runs that
share a `DecorationSite` and fit within `MAX_DECORATION_GROUP_EXTENT = 6.5` blocks on every axis.
Placement already visits one base, bridge, or the center at a time, so this follows the physical
layout without reordering anything. Every seed (checked over 0–255) yields the same 8 groups:

| Group | Props | Boxes | Largest extent (blocks) |
| --- | ---: | ---: | ---: |
| South base (shell fragments, arrow, two remnants) | 6 | 8 | 2.5–2.75 |
| East base (shell fragments, arrow, remnant) | 5 | 7 | 2.4–2.6 |
| West base (egg, arrow, remnant) | 3 | 9 | 2.7 |
| South bridge break, island side | 1 | 1 | 0.5 |
| South bridge break, center side | 1 | 1 | 0.5 |
| East bridge break (arrow and remnant) | 2 | 4 | 4.3–5.5 |
| Ruin interior (two chests, two wall arrows) | 4 | 20 | 5.1 |
| Lawn south-east of the ruin (arrow, remnant) | 2 | 4 | 1.2–1.8 |

The limit sits about a block from every measured size: the two ends of the South bridge's 6-block
break are at least 7.4 apart, and the whole center spans at least 8.6. One group per site (6
groups) left the ruin close-up about 40% slower, because the center's bounds then covered the whole
ruin floor; one group per prop (24 groups) was slower than both in wide views, because every ray
pays one bounds test per group. A limit of 8 saved about 2–3% more but put the South bridge in one
or two groups depending on the seed.

### Queries

```text
for run in runs (insertion order):
    if run has bounds and !bounds.overlaps_ray_interval(ray, t_min, current t_max): skip run
    test the run's objects exactly as before
then the voxel DDA, as before
```

`Aabb::overlaps_ray_interval` reuses the slab clipping of `Aabb::intersect` (now factored into one
`slab_interval` helper, same arithmetic) and asks whether the ray's `[entry, exit]` through the box
overlaps the query interval. It is not a surface test: a ray that starts inside the bounds, or a
segment lying wholly inside them, overlaps them. It is exact for culling: every plane crossing is
`(plane - origin) / direction`, which is monotonic in `plane` under IEEE rounding, so for a box
inside the bounds the bounds' `[entry, exit]` contains the box's, and the surface the box would
report (its entry or exit) lies in both intervals. A contained object that `intersect` or
`intersects` accepts can therefore never be culled, including at exact boundary contact, for
parallel directions (both use the same `1e-8` parallel rule), and for origins inside the bounds.
No epsilon expansion is needed, and none was added; a test checks this over 20,000 contained box,
ray, and interval combinations.

- **Closest hit.** The query interval for a group is `[t_min, nearest object t so far]`, inclusive,
  exactly the interval the linear scan would have used for its objects. Skipped groups hold no
  object that the linear scan would have accepted, and tested objects are visited in the same
  insertion order with the same strict `<`, so the result is identical: the same object, `t`,
  position, normal, face, UV, `MaterialId`, and `HitSource`. Two objects at an equal `t` keep the
  earlier-inserted winner, also across groups and with overlapping group bounds. The nearest object
  `t` still bounds the DDA, and an object still wins an exact tie against a voxel.
- **Shadows.** `is_occluded` skips a group whose bounds miss the caller's interval (infinite for
  the sun, finite for point lights) and otherwise tests its objects, returning at the first
  blocker. Bounds are never blockers and never produce a `SceneHit`, material, or pixel.
- **Hot path.** No allocation, sorting, or lookup per ray: a loop over a few runs and a slice of
  objects each.

### Verification

Before any code changed, the Mission 31 build rendered 75 reference PPMs at 640×360: the default
showcase, the voxel diagnostic, the procedural island (seed 1847), and seeds 1847, 42, 2026, and 0
of the EggWars diorama from 18 poses each (initial, ruin close-up, West egg close-up, low angle,
top-down, an orbit at radius 48, ten close views around the North defense, and the East and South
bases). After the North egg removal, and again after grouping, all 75 files are byte-identical to
the references (SHA-256). The app's own PPMs for the four modes are byte-identical to Mission 31's.
Tests additionally compare the grouped scene with the same objects added one at a time (the
linear scan) at 640×360 for all four seeds and from five poses, and compare `closest_hit` and
`is_occluded` with a test-only copy of the linear scan over thousands of rays.

## Known Limitations

These are deliberate scope boundaries of the current diagnostic renderer, not hidden defects:

- Shadow rays treat every AABB and every voxel, including glass, as an opaque blocker; there are
  no transparent or colored shadows, absorption, or dispersion.
- No nested-medium tracking: every transmissive AABB or voxel is assumed to sit in air, so touching
  glass boxes and adjacent glass voxels behave as glass → air → glass.
- A refracted ray leaving glass through a face that touches another block (for example a glass
  voxel resting on grass) is biased into that block, which reports its far face as seen from
  inside. Per-block AABBs hide or show this depending on insertion order, because the glass exit
  and the neighbor's entry tie; the DDA always reports the glass exit. Fixing it needs
  interface/medium handling, which is out of scope.
- Faces struck from inside glass are locally lit with their outward geometric normal.
- The origin biases (`1.0e-4`) and the point-light radius assume approximately unit-scale geometry.
- Normal mapping changes shading only, not silhouettes, shadows, reflection, or refraction.
- Lava regions are not procedurally converted into lights; the showcase hand-places one light.
- The recursion limit is `MAX_RAY_DEPTH = 3`; it was not raised because cost has not been
  profiled at greater depth.
- AABB objects are accelerated only by one flat level of spatial groups (Mission 31.25): every ray
  still tests every group's bounds, and every object of a group it reaches. That suits a few
  clusters of small props; many more objects or groups would need something else (a hierarchy
  was deliberately not built). Objects added one at a time with `Scene::add` are scanned linearly.
- Worker threads are created and joined on every render, and parallel speedup on the development
  machine is sublinear (about 5.8× on 11 CPUs for the default view). The Mission 27A investigation
  found 16×16 tiles appropriate and scoped-thread creation too cheap to justify a persistent pool.
- The DDA treats edge and corner contact as zero-measure: a ray exactly through the shared edge of
  two diagonally adjacent blocks passes between them, where the closed-box slab test would report a
  contact.
- Materials, light colors, and the sun direction are diagnostic values. Final artistic tuning
  belongs to the EggWars scene.
- The final EggWars scene is not finished: the arena has terrain, bridges, bases, defenses, the
  central ruin, and the battle-aftermath props, but no lava, and no final lighting, composition, or
  material tuning for it.
- The decorations still cost something: with 54 boxes in 8 groups an EggWars render is about
  1.25× (initial view) to 1.6× (props filling the frame) the cost of the undecorated arena, down
  from about 3× with Mission 31's 59 linearly scanned boxes (see Performance). The remaining cost
  is the 8 bounds tests every ray pays, plus the objects of every group a ray reaches; the ruin
  interior (20 boxes) and the West base (9) dominate their close-ups.
- Group bounds are only tested, never subdivided: a group's bounds include empty space (most of
  the ruin interior between the chests and the wall arrows), and rays through it test all its
  objects.
- Props are axis-aligned: arrows fly along ±X, ±Z, or straight down only, and the open chest lid
  stands upright rather than tilted. The North egg is fully hidden by its intact defense and is
  not instantiated as geometry (Mission 31.25); a camera zoomed inside its defense would not see
  it. The
  decorative colors are diagnostic values awaiting the final lighting pass.
- Construction is designed for `ArenaLayout::eggwars()` only; another separation would need its
  bridge damage repositioned.
- The island generator writes only the island's blocks and never clears other cells in its region;
  `ArenaLayout` validates that arena islands keep apart, while direct `generate_island` callers are
  responsible for overlap. Islands contain no glass or lava, and ores appear only on side-exposed
  rock (none hidden inside, none on downward-only faces).
- The constructed arena's grid ends at `y = 5`, the tallest structure; anything later built higher
  needs `CONSTRUCTION_TOP` raised. Most of the arena's single-threaded render cost is DDA stepping
  through empty cells, and the taller grid made it larger (see Performance); it is acceptable at
  the current resolution.
- The orbital camera has no collision: zooming in far enough places it inside an island.
- The resolution is fixed at 640×360 (window 1280×720) at compile time; there is no runtime
  resolution switching, and a single-threaded render at this size takes 45–73 ms.
- Intact defense caps (North and West) touch their ring only along edges, so a cap is a block that
  floats in the voxel-connectivity sense, as in Minecraft.
- Changing generator constants changes what every seed produces; seeds are reproducible for a fixed
  code version only.

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

Add `-- --voxel-diagnostic` to render the small hybrid voxel diagnostic scene instead of the
default five-block showcase, `-- --procedural-island [--seed <u64>]` (or just `-- --seed <u64>`)
to render one procedural floating island, or `-- --eggwars [--seed <u64>]` to render the
five-island arena; see Procedural Floating Islands and Five-Island EggWars Arena above.

Rendering uses one CPU worker thread per available CPU; the application prints the count at
startup. The development framebuffer is rendered at 640×360 and presented in a 1280×720 window using
an exact 2× scale with nearest-neighbor filtering. The application prints the PPM export path and shows CPU render duration,
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

or, with `--voxel-diagnostic`, to `output/voxel_diagnostic.ppm`, in procedural-island mode to
`output/procedural_island_seed_<seed>.ppm`, and in EggWars mode to
`output/eggwars_seed_<seed>.ppm`.

The `output/` directory is ignored by Git so generated renders remain local debugging artifacts.

## Texture Assets

Original PNG files under `assets/source/` are development inputs. The CPU raytracer does not
decode PNG at runtime. Prepare the 16×16 binary P6 PPM assets used by the renderer with:

```bash
scripts/prepare_assets.sh
```

The script validates every source dimension, uses macOS `sips` only for offline PNG decoding,
and writes runtime files under `assets/textures/`. Its Python helper uses only the standard
library and performs deterministic PPM conversion and tint arithmetic. The same command derives
`cobblestone_normal.ppm` from the cobblestone texture (see Normal Mapping).

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
| Cobblestone | `cobblestone` on all faces | `(1.00, 1.00, 1.00)` | 0.08 | 0.00 | 0.03 | 1.0 | Normal mapping (implemented) |
| Obsidian | `obsidian` on all faces | `(0.90, 0.90, 1.00)` | 0.55 | 0.00 | 0.35 | 1.0 | Reflection (implemented) |
| Glass | `glass` on all faces | `(0.90, 0.97, 1.00)` | 0.80 | 0.85 | 0.15 | 1.5 | Refraction + Fresnel (implemented) |
| Lava | `lava` on all faces | `(1.00, 0.95, 0.90)` | 0.10 | 0.00 | 0.05 | 1.0 | Emission + local lava light (implemented) |

Texture, albedo, specular, reflectivity, transparency, and index of refraction all affect
rendering, except that transparent glass's stored reflectivity is superseded by Fresnel. The
`1.0` IOR of the non-transmissive materials is the neutral default and has no optical effect.
Lava additionally stores `emission_color = (1.00, 0.88, 0.72)` and `emission_strength = 1.5`; the
other four materials do not emit. Cobblestone additionally references the shared derived
`cobblestone_normal` texture; the other four have no normal map. Glass still blocks shadow rays as
an opaque AABB.

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

With Fresnel composition, forty warmed 320×180 release renders per view in one process gave a
default-view median of about 4.33 ms (4.31–4.54 ms), against 4.08 ms (4.05–4.20 ms) for the
refraction build measured the same way, roughly 6% more. Close-up glass views filling much of the
frame measured medians of about 14.4 ms (head-on) and 15.0 ms (grazing), against 14.2 ms and
15.2 ms before. Six `cargo run --release` startups measured 4.36–4.75 ms (4.74, 4.44, 4.36,
4.75, 4.63, 4.43). The branch structure is unchanged: glass hits below the maximum depth still trace
a reflected ray and, unless total internal reflection occurs, a refracted ray; Fresnel changes
their weights. The extra default-view cost is plausibly from inside-glass reflections, which now
stay inside and hit glass again instead of escaping, but that attribution was not profiled.
These are local observations, not a formal benchmark.

With emission and the lava point light, forty warmed 320×180 release renders per view (ten warm-up
renders discarded) were measured in one process for the Mission 18 build and for the new build,
using the same harness and scene layout in the same session. Medians in ms:

| View | Mission 18 build | Emission + point light |
| --- | ---: | ---: |
| Default | 3.82 | 4.19 |
| Top-down | 5.52 | 6.47 |
| Back | 5.56 | 6.30 |
| Close lava | 9.57 | 12.99 |
| Close glass | 14.19 | 18.13 |

That is roughly 10–17% more for wide views and 28–36% more for close lava and glass views. The new
cost is the point-light evaluation and its shadow ray at every shaded hit inside the light's
radius, so it grows with the number of such hits (including secondary rays through glass and
reflections), the number of lights, and their radii. Six `cargo run --release` startups measured
4.47–4.95 ms (4.95, 4.47, 4.69, 4.48, 4.47, 4.50). These are same-session comparisons on the
development machine; the absolute values are not comparable with the earlier figures above, and
none of this is a formal benchmark.

With the derived cobblestone normal map active, medians of 40 renders at 320×180 (five warm-up renders
discarded) were measured in one session for the Mission 19 build and the Mission 20 build, using one
harness and scene layout and alternating the two builds over two rounds. Medians in ms, as
Mission 19 / Mission 20 per round:

| View | Round 1 | Round 2 |
| --- | ---: | ---: |
| Default | 4.17 / 4.26 | 4.39 / 4.54 |
| Top-down | 4.88 / 4.94 | 5.09 / 5.21 |
| Close lava | 16.28 / 17.14 | 16.92 / 17.10 |
| Close glass | 18.12 / 18.57 | 17.49 / 18.62 |

Normal mapping adds roughly 0.1–0.2 ms, about 1–5% depending on view, which is comparable to the
run-to-run variation between rounds. The absolute values differ from the previous table because the
camera poses of this harness are not identical to that one; only the paired comparison is
meaningful. A 6,840-pose camera sweep of the showcase scene (orbit, elevation, zoom, and three
targets) produced no render error and no non-finite pixel. These are local development
observations, not formal benchmarks.

### Pre-Phase-4 baseline

The Phase 3 audit recorded this baseline on the final showcase scene with real textures: 320×180,
release build, single-threaded, brute-force AABB traversal, one process, 40 measured renders per
view after 5 discarded warm-up renders. Medians in ms (range in parentheses):

| View | Median |
| --- | ---: |
| Default (orbit 0.55 + π, pitch 0.15, radius 10) | 4.88 (4.81–4.94) |
| Top-down | 5.20 (5.13–5.51) |
| Close cobblestone | 10.74 (10.61–11.40) |
| Close lava | 18.08 (17.91–18.27) |
| Close glass | 30.17 (28.61–30.50) |

Six `cargo run --release` startups of the default view measured 4.96–5.54 ms (5.54, 4.97, 4.96,
5.05, 5.05, 5.00). The machine was noticeably slower in this session than in the earlier tables,
so compare only within this table. Cost is dominated by close views filled with glass (two
recursive rays per hit plus shadow and point-light queries) and lava (point-light shadow rays).
A second audit sweep of 8,064 poses (four targets, 48 yaw steps, seven pitches, six radii from 0.8
to 25) at 64×36 produced no render error and no non-finite pixel. These are local development
observations, not universal benchmarks, and are the reference for Phase 4 speedups.

### Hybrid traversal (Mission 24)

Controlled comparison, explanatory rather than a benchmark: the same 4,114 blocks (a 32 × 4 × 32
grass/dirt/iron-ore terrain with lava cells and six small cobblestone, obsidian, and glass pillars)
represented once as 4,114 unit AABB `SceneObject`s and once as a 32 × 8 × 32 `VoxelGrid` with no
objects, rendered at 320×180 with the showcase lighting and one point light, release build,
single-threaded. Medians (naive: 5 renders, voxel: 20 renders, one warm-up each):

| View | Naive per-block AABBs | Voxel grid (DDA) | Speedup | Primary-ray work per ray |
| --- | ---: | ---: | ---: | --- |
| Wide (radius 30) | 2,431 ms | 9.8 ms | ~250× | 4,114 slab tests vs 6.8 cells |
| Close (radius 10) | 3,910 ms | 17.3 ms | ~225× | 4,114 slab tests vs 17.9 cells |
| Top-down (radius 25) | 3,210 ms | 13.2 ms | ~245× | 4,114 slab tests vs 6.5 cells |

With the glass pillars swapped for lava, the two representations produced identical images
(no pixel differing by more than `1e-4` across all three views). With glass, 0.2–0.6% of pixels
differ, all at refracted rays leaving glass into a touching block, where per-block AABBs tie (see
Known Limitations).

On the AABB-only showcase (no grid), hybrid traversal adds one `Option` check per query. Forty
warmed renders per view through the release application binary, three interleaved rounds against
the Mission 23 build in one session, gave medians within run-to-run noise: default 4.84–4.99 ms
versus 4.89–5.41 ms, close glass 36.3–36.4 ms (one noisy round: 39.0 ms) versus 35.7–36.1 ms. The voxel diagnostic scene
renders its default view in about 5.0 ms. A sweep of 8,064 camera poses (four targets, 48 yaw
steps, seven pitches, six radii from 0.8 to 25) at 64×36 over both the voxel diagnostic scene and
the showcase produced no render error and no non-finite pixel. These are local development
observations.

### Procedural island (Mission 28)

Apple M3 Pro, 11 CPUs, release build, 320×180, in one process: generation is the median of 200
runs including the grid allocation; renders are medians of 25 (one and four workers, automatic =
11) of the initial view.

| Seed | Blocks | Ores | Generation | 1 worker | 4 workers | Automatic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1847 | 1,089 | 29 | 0.040 ms | 4.99 ms | 1.40 ms | 0.83 ms |
| 2026 | 1,280 | 40 | 0.016 ms | 4.89 ms | 1.40 ms | 0.85 ms |
| 42 | 1,226 | 21 | 0.015 ms | 4.67 ms | 1.28 ms | 0.79 ms |
| 0 | 1,168 | 33 | 0.015 ms | 4.66 ms | 1.32 ms | 0.82 ms |
| 7 | 1,223 | 26 | 0.015 ms | 4.49 ms | 1.28 ms | 0.77 ms |
| 99 | 1,267 | 18 | 0.015 ms | 4.82 ms | 1.37 ms | 0.82 ms |
| `u64::MAX` | 1,232 | 31 | 0.016 ms | 4.86 ms | 1.33 ms | 0.80 ms |

Across seeds 0–255 an island has 966–1,452 blocks in its 3,328-cell grid. Seed 1847 under orbit and
zoom (automatic / one worker, ms): initial 0.83 / 4.87, orbit +90° 0.76 / 4.46, +180° 0.70 / 4.04,
from below 0.92 / 5.57, from above 0.81 / 4.69, radius 10 1.47 / 9.94, radius 5 0.69 / 3.82,
radius 45 0.53 / 2.33. Startups through `cargo run --release -- --procedural-island --seed 1847`
reported 0.02–0.07 ms of generation and 1.07–1.50 ms for the first render. Generation is negligible next to one frame, and the
renderer needed no new acceleration. These are local development observations, not universal
benchmarks.

### EggWars arena (Mission 29)

Apple M3 Pro, 11 CPUs, release build, 320×180, in one process: generation is the median of 100
runs including the grid allocation; renders are medians of 9 (one worker) or 25 (four workers,
automatic = 11) of the initial view. Every arena uses the same 80 × 16 × 80 grid.

| Seed | Blocks | Ores | Generation | 1 worker | 4 workers | Automatic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 1847 | 8,168 | 155 | 0.107 ms | 8.99 ms | 2.59 ms | 1.35 ms |
| 42 | 8,184 | 161 | 0.106 ms | 9.31 ms | 2.58 ms | 1.36 ms |
| 2026 | 8,081 | 179 | 0.106 ms | 9.47 ms | 2.60 ms | 1.36 ms |
| 0 | 7,756 | 160 | 0.103 ms | 9.51 ms | 2.62 ms | 1.35 ms |
| 7 | 8,438 | 178 | 0.108 ms | 9.56 ms | 2.63 ms | 1.40 ms |
| `u64::MAX` | 7,946 | 164 | 0.104 ms | 9.02 ms | 2.58 ms | 1.32 ms |

Seed 1847 under orbit and zoom (one / four / automatic workers, ms): orbit +90° 8.86 / 2.54 / 1.33,
+180° 9.01 / 2.48 / 1.31, from below (pitch −0.35) 11.08 / 3.21 / 1.67, from above (pitch 1.2)
7.20 / 2.07 / 1.13, radius 40 12.38 / 3.40 / 1.73, radius 20 15.44 / 4.39 / 2.27, radius 110
4.60 / 1.26 / 0.77. The other seeds are within a few percent of these. Startups through
`cargo run --release -- --eggwars --seed 1847` reported 0.39 ms of generation (a cold first run)
and about 2 ms for the first render.

**Empty-space traversal.** The dense grid is mostly air (8,168 of 102,400 cells are occupied for
seed 1847), and the DDA steps through every empty cell a ray crosses. To isolate that cost, the same
views were rendered with no grid, with an all-empty 80 × 16 × 80 grid, and with the arena (one /
automatic workers, ms):

| View | No grid | Empty grid | Arena |
| --- | ---: | ---: | ---: |
| Initial | 1.41 / 0.40 | 8.86 / 1.25 | 9.45 / 1.38 |
| From above | 1.40 / 0.47 | 6.87 / 1.06 | 7.57 / 1.12 |
| Radius 20 | 1.46 / 0.43 | 13.63 / 1.89 | 15.41 / 2.28 |
| Radius 110 | 1.40 / 0.44 | 4.31 / 0.74 | 4.26 / 0.76 |

Stepping through empty cells is most of the arena's cost: roughly 7.5 ms of the 9.5 ms
single-threaded initial view (about 130 ns per primary ray on average), while terrain hits,
shading, and shadow rays add 0.6–1.8 ms single-threaded across these views. It is not free, but at the automatic worker count the
arena renders in about 0.8–2.3 ms in every measured view, so the dense grid remains adequate and no
sparse grid, chunking, or empty-space skipping was added. If a later, larger grid (for example with
headroom for structures) is measured to be too slow, empty-space skipping is the first candidate.
These are local development observations, not universal benchmarks.

### Constructed arena (Mission 30)

Same machine and method as Mission 29 (release, 320×180; generation median of 100 runs including
grid allocation; renders medians of 9 or 25 runs of the initial view):

| Seed | Blocks (construction) | Generate + construct | 1 worker | 4 workers | Automatic |
| --- | ---: | ---: | ---: | ---: | ---: |
| 1847 | 8,461 (293) | 0.23 ms (first run, cold) | 12.27 ms | 3.47 ms | 1.74 ms |
| 42 | 8,482 (298) | 0.106 ms | 11.89 ms | 3.44 ms | 1.73 ms |
| 2026 | 8,372 (291) | 0.107 ms | 12.00 ms | 3.46 ms | 1.75 ms |
| 0 | 8,044 (288) | 0.103 ms | 12.10 ms | 3.49 ms | 1.75 ms |
| 7 | 8,728 (290) | 0.111 ms | 12.10 ms | 3.50 ms | 1.77 ms |
| `u64::MAX` | 8,243 (297) | 0.113 ms | 12.09 ms | 3.46 ms | 1.74 ms |

Planning, validating, and writing the construction alone takes about 5 µs; generation time is
unchanged within noise. Seed 1847, in several views, comparing the Mission 29 terrain in its exact
80 × 16 × 80 grid, the same terrain in the taller 80 × 20 × 80 grid, and the constructed arena
(one / four / automatic workers, ms):

| View | Mission 29 terrain | Terrain, taller grid | Constructed |
| --- | ---: | ---: | ---: |
| Initial (radius 68) | 8.82 / 2.57 / 1.40 | 12.27 / 3.36 / 1.72 | 11.90 / 3.45 / 1.74 |
| Central ruin (radius 22) | 10.94 / 3.12 / 1.64 | 16.45 / 4.73 / 2.38 | 17.78 / 5.12 / 2.57 |
| Glass close-up (radius 7) | 9.14 / 2.56 / 1.41 | 16.55 / 4.40 / 2.29 | 22.52 / 5.97 / 3.13 |
| East base (radius 24) | 11.78 / 3.37 / 1.75 | 17.09 / 4.59 / 2.30 | 16.49 / 4.71 / 2.39 |
| Top down (pitch 1.45) | 6.01 / 1.73 / 0.96 | 7.85 / 2.15 / 1.15 | 7.94 / 2.17 / 1.14 |
| Low, against the sunset | 10.25 / 2.95 / 1.55 | 12.73 / 3.62 / 1.87 | 12.84 / 3.61 / 1.87 |

The structures themselves cost little: nearly all of the increase comes from the four extra empty
layers, which rays from an elevated camera now cross cell by cell. The glass close-up adds the
recursive refraction and Fresnel reflection rays of the two panes. At the automatic worker count
every measured view renders in 1.1–3.2 ms, so no new acceleration structure was added;
empty-space skipping remains the first candidate if a later scene measures too slow. These are
local development observations, not universal benchmarks.

### Decorations (Mission 31)

Apple M3 Pro, 11 CPUs, release, 640×360. One process renders the Mission 30.5 scene (the same grid,
no objects) and the decorated diorama alternately, 5 warm-up and 15–20 measured rounds each; the
values are medians (1 / 4 / automatic workers, ms), seed 1847, run twice with agreeing results:

| View | Mission 30.5 | With 59 decorative AABBs | Change |
| --- | ---: | ---: | ---: |
| Initial (radius 68) | 47.4 / 12.9 / 6.4 | 142.4 / 37.4 / 20.4 | +200% / +191% / +218% |
| Orbit +1.2 rad, radius 48 | 60.9 / 16.1 / 8.1 | 180.2 / 47.2 / 25.8 | +196% / +193% / +220% |
| Close (radius 28) | 77.7 / 20.9 / 10.4 | 201.1 / 53.8 / 29.1 | +159% / +158% / +181% |
| Ruin and chests (radius 9) | 83.5 / 22.6 / 11.7 | 319.2 / 84.7 / 47.6 | +282% / +274% / +306% |
| West egg close-up (radius 2.4) | 87.2 / 23.1 / 12.0 | 343.1 / 89.8 / 49.3 | +294% / +289% / +310% |

Seeds 42, 2026, and 0 measure within 3% of these (initial view, automatic workers: 20.0, 20.4, and
20.3 ms). Placement takes about 4–5 µs warm (23 µs in the cold startup); registering the decorative
materials and adding the 59 objects is negligible next to loading the textures (both scene builds
take about 0.1–0.8 ms, dominated by file reads). There are 59 objects, 17 materials (10 + 7), and
13 textures (12 + 1).

The props are cheap to build and expensive to trace. `Scene::closest_hit` slab-tests every object
before the DDA, and `Scene::is_occluded` tests every object for each shadow ray, so the 59 boxes add
about 95 ms of single-threaded work to the initial view (about 1.6 ms per box per frame, roughly
4 ns per ray-box test). Every ray pays this, although most of a frame's rays never come near a prop.
Close views cost more because more of their rays hit geometry and cast shadow rays. The cost scales
with the object count: about 35 boxes would still roughly double the render time. At the automatic
worker count the diorama renders in 20–50 ms, so orbiting stays interactive but falls below the
60 FPS target, especially up close. As the mission required, no BVH, grouping, batching, or new
traversal was added. The decision to keep the 59 boxes and document the cost was made explicitly;
grouped acceleration for objects is the first candidate for a future, separately approved change.
These are local development observations, not universal benchmarks.

### Decoration grouping (Mission 31.25)

Apple M3 Pro, 11 CPUs, release, 640×360, seed 1847. Four test binaries built from the same
throwaway harness (never committed) ran interleaved, five rounds each; every value is the median of
the five round medians (each of 11 measured renders after 3 warm-ups), in ms for 1 / 4 / automatic
workers. "Stage" is the same arena with no objects (the Mission 30.5 scene, measured in the same
session); "North egg removed" is the first Mission 31.25 commit alone (54 boxes, linear scan).

| View | Stage (no props) | Mission 31 (59, linear) | North egg removed (54, linear) | Mission 31.25 (54, 8 groups) | Speedup vs Mission 31 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Initial (radius 68) | 48.7 / 13.0 / 6.5 | 142.9 / 37.4 / 20.4 | 134.9 / 35.4 / 19.2 | 61.2 / 16.2 / 8.2 | 2.33× / 2.31× / 2.49× |
| Ruin close-up (radius 9) | 87.2 / 23.0 / 11.8 | 320.9 / 84.0 / 45.8 | 301.5 / 78.9 / 43.0 | 128.4 / 33.8 / 17.6 | 2.50× / 2.49× / 2.60× |
| West egg close-up (radius 2.4) | 88.3 / 23.2 / 12.0 | 343.9 / 90.0 / 49.1 | 320.7 / 83.9 / 45.9 | 138.3 / 36.2 / 19.6 | 2.49× / 2.49× / 2.51× |

Round-to-round spread was below 1 ms in every cell except two single-worker Mission 31 cells
(2 ms), far below the differences reported. The props' added cost over the bare stage
falls from 94 / 24 / 13.9 ms to 12.5 / 3.2 / 1.7 ms in the initial view (87–88% of it recovered),
from 234 / 61 / 34 ms to 41 / 11 / 5.8 ms in the ruin close-up (82–83%), and from 256 / 67 / 37 ms
to 50 / 13 / 7.6 ms in the West egg close-up (80–81%). Removing the sealed North egg alone saves
5–7%, about what 5 of 59 linearly scanned boxes cost.

What remains is easy to account for. In the initial view almost no ray reaches a group, so the
remaining 12.5 ms single-threaded is the 8 bounds tests every primary and shadow ray now pays
instead of 59 box tests, consistent with Mission 31's measured cost of about 1.6 ms per box per
frame. In the close-ups most rays do reach the group in front of the camera and then test all its
boxes (20 in the ruin interior, 9 at the West base). Variants measured in the same harness: one
group per site (6 groups) 57.8 / 173 / 128 ms single-threaded for the three views, one group per
prop (24 groups) 82 / 168 / 166 ms, and, in a separate experiment binary, the extent-limited rule
with a limit of 8 (7 groups on seed 1847) 58 / 120 / 131 ms against 60 / 124 / 136 ms with 6.5.
The chosen limit trades those 2–3% for the same grouping on every seed. These are local observations, not universal benchmarks.

## Testing

The Phase 4 suite contained 529 tests covering vector arithmetic and normalization, ray invariants,
AABB construction and edge cases, camera basis/ray generation/orbit limits, texture sampling and
registration, P6 parsing and malformed input, material face selection, cube-face UV orientation,
scene closest-hit behavior, ambient/Lambert/Blinn-Phong behavior, renderer lighting and texture
resolution, canonical material registration and texture selection, shadow-ray occlusion and origin
bias, procedural environment regions, sun, stars, invalid directions, renderer miss integration,
ray-depth policy, reflection mathematics, origin bias, linear reflectivity blending, recursive
reflection depth behavior, canonical reflectivity, index-of-refraction validation, Snell
refraction and total internal reflection, entering/exiting classification and origin bias,
recursive refraction depth behavior, Schlick Fresnel reflectance and input validation, interface
orientation, inside and outside reflection origins, Fresnel transparency composition, total
internal reflection routing, canonical glass refraction, material emission validation and
self-radiance that survives zero diffuse and occlusion, point-light validation, falloff, diffuse,
specular, bounded shadow rays, glass blocking, recursive emissive lava, normal-map decoding, the
six-face tangent basis against the real UV mapping, the derived cobblestone asset, shading-normal
lighting with geometric shadows and reflection, a camera-pose render sweep, framebuffer and color conversion, PPM output,
presentation-independent camera and RGBA conversion helpers, and the voxel grid's construction
validation, world/local conversion with negative origins, contiguous layout, bounds-checked reads
and writes, `BlockType`-to-`MaterialId` mapping for every block type, auxiliary dirt and ore
registration and texture reuse, voxel surface hits with exact UV equivalence to unit AABBs at
negative and translated coordinates, `SceneHit` source metadata, 3D DDA traversal against a
brute-force AABB oracle, the voxel any-hit query against nearest-hit existence, hybrid nearest-hit
ordering, the object-wins tie rule and interval propagation, hybrid occlusion, renderer
equivalence of every block type with its unit AABB, voxel textures, normal mapping, emission,
reflection, Fresnel refraction, directional and point-light voxel shadows, and the voxel
diagnostic scene (no double geometry, camera sweep), and the tile renderer: tile construction and
validation, exact single coverage of every pixel for assorted frame and tile sizes, row-major
indexing, the 240-tile layout at 320×180, global primary-ray coordinates, and byte-identical
output against the sequential reference across tile sizes and tile orders, and the parallel renderer:
framebuffer tile writes, worker-count selection, bounding, zero-worker rejection and the `available_parallelism` fallback,
exactly-once tile claims, tile integration at global coordinates, tracing only on worker threads,
byte-identical output across worker counts, tile sizes, and uneven completion order, worker error
propagation with the sequential renderer's error, failures in every tile without deadlock, and
worker panic propagation with its message, exactly-once pixel coverage, one reused buffer per
worker, and a render that still works after a failed one. Every AABB and every voxel remains an opaque shadow blocker, including
glass.

Mission 28 adds 48 tests, for 577 passing: hash purity, seed and domain separation, `u64::MAX` and
zero seeds, smooth and periodic noise, and the diamond angle; island reproducibility, idempotent
regeneration, seed variation, order and prior-content independence, unrelated domain sampling, and
a pinned reference fingerprint; placement at the origin and at negative, translated, and extreme
coordinates with identical local shape, documented bounds, no writes outside the region, and every
validation error leaving the grid untouched; the full 16 × 16 evaluation domain, a round
non-square silhouette within its radial bounds, a flat occupied center, and a 4-connected
footprint; grass over one to three dirt over rock in every column, a single face-connected
landmass, one-level surface variation, and a monotonically tapering underside; ore rarity across
seeds, deterministic and seed-dependent ore arrangements, side-exposed ores only, and only terrain
block types; scene integration (DDA hits on grass with the canonical material, normal-mapped
cobblestone, and every ore's auxiliary material), the island camera target, unchanged terrain
under orbit and zoom, an assorted-viewpoint render sweep, and tiling equivalence; and the CLI
(valid, zero, and maximum seeds, missing, negative, invalid, and overflowing values, unknown and
repeated options, and the existing modes).

Mission 29 adds 36 tests, for 613 passing: the five island identities and their array indexing,
24 × 24 and 16 × 16 footprints and thicknesses, cardinal positions at one surface level, the fixed
separation and 12-block nominal gaps, pairwise disjoint regions, rejection of overlapping,
touching, and unrepresentable layouts and of an unallocatable grid; pinned derived seeds and a
pinned arena fingerprint, identical arenas for equal seeds, per-island variation across seeds with
an unchanged layout, generation-order independence, each island equal to its standalone
generation, distinct outer islands, and zero and maximum seeds; the global grid as the exact union
of the regions (negative origin, 80 × 16 × 80), every block inside exactly one region, empty gap
corridors, and each island one separate landmass; grass over dirt over rock with terrain block
types only, ore rarity across arenas, and a larger central island with a flat middle; scene
integration (one grid and no objects or lights, DDA hits on every island's grass, rays crossing the
void between islands, every ore's material, terrain shadows and open sky in the gaps), the initial
camera framing all five islands with none touching the frame edge, unchanged terrain under orbit
and zoom, an assorted-viewpoint render sweep, tiling equivalence, and a check that the renderer,
scene queries, voxel traversal, lighting, and environment contain no arena-specific code; and the
`--eggwars` CLI (default, explicit, zero, and maximum seeds, missing and malformed seeds, repeats,
and conflicting modes).

Mission 30 adds 38 tests, for 651 passing; the 613 earlier tests are unchanged. Grid and terrain:
the construction grid raises only the top to the tallest block (80 × 20 × 80, exact horizontal
extent), terrain in the taller grid is cell-for-cell the Mission 29 arena (same island stats and
the pinned fingerprint), construction never replaces terrain and adds only cobblestone, obsidian,
and glass, bounds that clip an island are rejected, and construction refuses to clip, overwrite, or
build twice without touching the grid; negative coordinates. Anchors: topmost-block anchors for
every column, never buried, measured surface levels, every designed column inside the smallest
silhouette, and empty future egg cells on the floor, open above and behind. Bridges, by voxel
occupancy: the narrative states and directions, a continuous North route connected to the center,
East's single three-cell gap with stubs attached to both islands, South's single six-cell gap with
two attached segments, West stopping short with a bare center end, ends touching terrain face to
face, level unburied decks, only North connecting two landmasses and nothing floating, and all four
states across 52 seeds. Defenses: 5/4/3/2 obsidian blocks in the grid, one template turned toward
the center on every island, missing blocks only in the front row and opening the egg toward the
attack, and exposed obsidian. Structures: base floors and gate heights, the central floor and
inlay, open interior, tallest pillar and partial walls, and exactly two exposed glass panes with
air behind and no terrain or glass contact. Reproducibility: identical arenas for equal seeds, a
pinned constructed fingerprint, a seed-independent layout, and independence from island generation,
planning, and write order. Rendering: the scene attaches one taller grid and no objects or lights,
DDA hits on every bridge end with normal-mapped cobblestone and misses through every gap, canonical
reflective obsidian on intact slots and the floor under broken ones, canonical refractive glass
whose panes change the image, construction shadows with open sky under the gaps, the initial camera
framing the constructed arena without clipping, an assorted-viewpoint sweep with an unchanged grid,
and tiling equivalence.

Mission 30.5 adds 14 tests, for 665 passing. Defenses, by voxel occupancy: the nine-position template
(eight ring cells plus one cap, no duplicates, no egg-cell position, no solid enclosure), 9 / 7 / 5 / 3
obsidian blocks counted both in the slot metadata and over the whole volume around each egg, a sealed
North, East's front row and cap missing, South's face-connected L fragment, West's front notch under
a standing cap, template rotation to four distinct orientations, positions in bounds, distinct, and
overwriting no terrain, the cap above the empty egg cell, and 29 obsidian blocks per arena; the
earlier floating-block test now allows exactly the intact caps. Resolution: 640×360, a 1280×720
window at an exact 2× scale, the 230,400-pixel framebuffer and 921,600-byte RGBA buffer (and rejection
of the old buffer size), identical 16:9 cameras at both resolutions, the `P6 640 360 255` PPM header
and exact payload, finite non-blank renders of all four scene modes, the 920-tile layout with an
8-pixel last row, pixel coverage at 640×360, and byte-identical EggWars output across repeats,
worker counts, and tile sizes. Smaller framebuffer sizes in the existing tests were left unchanged.

Mission 31 adds 40 tests, for 705 passing; the 665 earlier tests are kept, and two app-level tests
(finite renders of every mode at 640×360, and byte-identical EggWars output across repeats, worker
counts, and tile sizes) now render the decorated scene the app shows. Decorative materials: seven
distinct materials over one shared texture, opaque, non-emissive, only the latch reflective, a pale
warm shell, and a chest interior darker than its wood. Eggs: exactly two, in North and West; none in
East or South, whose egg cells hold only small shell fragments; each egg inside its reserved cell
on the floor, clear of (not even touching) every ring block and the cap with 2 px of air; the
stepped-oval profile; identical geometry in both bases and across runs. Chests: two, inside the
ruin's open area on the floor, overlapping no block; hollow walls around a recess below the rim, an
upright lid at the back in all four orientations, an open column above the interior, the latch on
the lid; lids against the walls; only chest materials; identical on every seed. Arrows: seven, all
five headings, head, shaft, and fletching end to end along the flight axis; each head embedded in
exactly its target and not buried; shaft and fletching in the open cell in front; base arrows from
the attack side into surviving defense blocks; cobblestone, obsidian, and grass targets. Remnants:
eight cubic cobblestone or obsidian fragments resting on their supports. Narrative: per-site tallies
for every base and bridge state, unchanged defense counts (9/3/5/7) and bridge states, and the
center's chests and arrows. Validity: the same story over 64 seeds, with offsets varying; 25 props
of 59 finite parts inside the grid; connected parts and no overlap between props; placement leaving
the arena and its fingerprint unchanged; decoration frames agreeing with the construction frames;
and validation rejecting an egg in obsidian, a lifted egg, two eggs in one cell, a prop outside the
grid, and a buried or detached arrow. Scene: one object per part with resolved, shared materials
(17 materials, 13 textures, no lights) over the same grid; the construction-stage scene unchanged;
`closest_hit` reaching the West egg through the notch and every prop from above; `is_occluded`
blocked by arrows and the chest lid; mirror rays off obsidian reaching the egg and changing the
render; assorted viewpoints around every prop; tiling equivalence; a small change to the initial
view; no prop words in the renderer or the scene queries; and per-seed reproducibility.

Mission 31.25 first drops the sealed North egg from the scene, adding 5 tests (710 passing): only
North's egg is sealed, on every seed, inside a cell closed by obsidian on four sides and above and
by the cobblestone floor below, while West's egg is not; the visible props are all but that egg,
in placement order (24 props, 54 parts, 59 parts in total); validation rejects sealing West's egg,
North's egg with its cap or a flank removed or its front block turned to glass, or a prop spanning
two cells; the scene holds no object in North's egg cell but still holds West's egg and the broken
eggs' fragments; and close renders around the North defense are pixel-identical with and without
the egg's five boxes.

The spatial grouping then adds 21 tests, for 731 passing; no earlier test was weakened. `Aabb`:
the exact `union`; `overlaps_ray_interval` accepting segments inside the box that `intersects`
rejects, inclusive boundary contact, grazing, edge, and corner rays, invalid intervals and origins,
and never missing a contained box that is hit (20,000 combinations of pixel-snapped boxes, aimed
rays with zero direction components, and finite or infinite intervals); and unchanged exact
`intersect` results after the slab refactor. `Scene`: runs that partition the objects in insertion
order (ungrouped, three groups, an empty group adding nothing, later objects ungrouped); bounds
equal to the exact union of their members and rebuilt identically; objects and materials unchanged;
`closest_hit` and `is_occluded` equal to a test-only linear reference on about 6,000 rays from
outside and inside group bounds, along face planes and edges, with zero direction components and
finite and infinite intervals, over a scene with voxels, touching and coplanar boxes, overlapping
group bounds, a duplicate box in two groups, and a box coincident with a voxel; finite shadow
segments with blockers inside and beyond a group; bounds that never hit or block; equal-`t` winners
kept by insertion order within and across groups; the object-wins tie against a voxel from all six
sides; and objects added after groups staying visible and casting shadows. App: the diorama's 8
groups (sizes 8, 7, 9, 1, 1, 4, 20, 4, each within one site and 6.5 blocks, covering every object
once, identical on rebuild) on four seeds and the same grouping on seeds 0–63; renders identical to
the linear scan from five poses (initial, ruin, West egg, low angle, top-down) and at 640×360 for
seeds 1847, 42, 2026, and 0; reflected, refracted, and shadow rays reaching grouped boxes in a
small glass-and-mirror scene with renders identical to the linear scan; and no grouping code in the
renderer.

The Phase 4 audit changed no runtime behavior and added no tests; the suite was unchanged at 529
passing tests.

The Phase 3 audit changed no runtime behavior and added no tests; the count was unchanged at 343.
It also regenerates the runtime assets with `scripts/prepare_assets.sh`, then
runs `cargo fmt --check`, `cargo test`, `cargo check`, `cargo build --release`, and
`cargo run --release`.

## Project Constraints

- Raytracing algorithms and scene geometry are implemented in project code.
- The raytracer is CPU-only; GPU rendering or GPU acceleration is not used for ray generation,
  intersection, shading, or scene traversal.
- Raylib 6.0.0 is the only direct third-party dependency (`Cargo.toml` is unchanged since the
  Phase 1 presentation commit) and is isolated to `src/app/app.rs`: presentation, input, timing, and
  platform responsibilities under the current project interpretation. `cargo tree` lists 39
  transitive packages, all pulled in by raylib and its `raylib-sys` build.
- Multithreading uses only the Rust standard library, and the source contains no `unsafe` code.
- The project does not currently use Raylib 3D primitives, models, lighting, or shaders for the
  scene.

## Planned Features

The following are not yet implemented. Phase 5 (EggWars world) is in progress; Mission 28 added
the procedural floating-island generator, Mission 29 the five-island arena, Mission 30 its
bridges, structures, and egg defenses, Mission 31 its battle-aftermath props, and Mission 31.25
spatial grouping for those props:

- transparent or colored shadows through glass, absorption, and nested or merged glass media;
- procedural lava-region extraction into representative point lights;
- lava and the final EggWars composition and lighting;
- visual storytelling, the final presentation and video, and final performance and visual polish;

## Course Rubric Mapping

"Implemented" below means the renderer feature exists and is demonstrated in the current
diagnostic showcase of five blocks. Rubric credit also requires each feature to appear
intentionally in the final EggWars diorama, which does not exist yet.

| Target | Status |
| --- | --- |
| 3D CPU raytracing foundation | Implemented in Phase 1 |
| Orbital viewing and zoom | Implemented in Phase 1 (arrow keys orbit, `W`/`S`/wheel zoom) |
| Five textured materials | Canonical definitions and temporary five-material showcase implemented |
| Lighting, shadows, reflection | Direct lighting, hard shadows, and bounded recursive reflection implemented |
| Refraction | Implemented: recursive Snell refraction through glass (IOR 1.5) |
| Fresnel | Implemented: Schlick reflection/refraction split for glass, TIR routed to reflection |
| Emissive lava | Implemented: visible self-radiance plus a local lava point light with hard shadows |
| Normal mapping | Implemented: derived cobblestone normal map lighting with a per-face tangent basis |
| Sunset/night skybox/environment | Procedural CPU environment implemented; shared miss path for every traced ray |
| Procedural floating-island terrain (16×16, seeded) | Implemented (Missions 28–29): seeded radial-mask islands with layered terrain and visible ores; the EggWars arena has a 24×24 central island and four 16×16 outer islands from one global seed |
| Voxel grid and 3D DDA | Implemented and audited; renders the voxel diagnostic, the procedural island, and the constructed five-island arena in one global grid |
| Tile-based CPU parallel rendering | Implemented and audited with scoped standard-library threads and dynamic tile claiming (about 5.8× on 11 CPUs for the default view, 6.0× for close glass); byte-identical to the sequential renderer |
| Visually complex, visually attractive scene | Pending: the final EggWars scene does not exist |
| Visual storytelling | In progress: bridge damage and egg-defense states (Mission 30); surviving and broken eggs, open chests, arrows, and debris (Mission 31); final composition pending |
| Final presentation and video | Pending (Phase 6) |
| Final performance and visual polish | Pending (Phase 6) |
| Final EggWars aftermath scene | Planned (Phase 5–6) |

The status column describes code and diagnostic scenes only. A feature earns rubric credit when it
is visibly and intentionally used in the final diorama, so none of the rows above is claimed as
final credit yet.

## Video

Final project video: to be added during the delivery/polish phase.
