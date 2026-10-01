# CPU Raytraced EggWars Diorama

## Overview

This is a Computer Graphics course project written in Rust. It is a CPU raytracer for a
Minecraft-inspired EggWars diorama, drawing on the visual language of the classic Minecraft
1.6–1.8 era. The intended final scene depicts the aftermath of a battle: floating islands,
bridges, damaged defenses, opened chests, exposed resources, and lava beneath a sunset-to-night
sky.

Phase 1 provides the correct, testable 3D foundation and Phase 2 provides the audited surface and
lighting system. Phase 3 is underway with a procedural sunset/night environment, bounded
recursive ray infrastructure, recursive reflections, recursive glass refraction, Schlick
Fresnel composition for glass, emissive lava with a local lava point light, and a derived normal map
for cobblestone. Every planned Phase 3 rubric effect is implemented and Phase 3 has passed a
correctness, architecture, and performance-boundary audit. Phase 4 (performance architecture) is
in progress: the dense voxel grid, the scene, material, and hit types it feeds, and 3D DDA voxel
traversal exist, and scene queries now traverse voxels and AABB objects together, while tiled
multithreading, the larger EggWars world, and the final scene remain planned work.

## Current Status

Phase 1 — Core Raytracer — and Phase 2 — Materials, Textures, and Lighting — are complete and
audited. Phase 3 — Raytracing Effects — is complete. Phase 4 — Performance Architecture — is in
progress with a logical voxel grid, complete block-to-material mapping, a common renderer-facing
hit type, and 3D DDA voxel traversal that every radiance and shadow ray now uses through hybrid
voxel + AABB scene queries. Primary-ray misses now sample a project-owned
procedural sunset/night environment in world space. Every radiance ray flows through one
bounded, depth-aware trace path; reflective materials launch recursive reflection rays and
transparent materials launch recursive Snell refraction rays whose split with reflection follows
Schlick's Fresnel approximation. Lava is self-luminous and lights nearby surfaces through a finite
local point light. Cobblestone is normal mapped from a map derived offline from its own texture.

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
  AABB objects by a linear scan, and receives the nearest hit or any blocker.

## Architecture

```text
Source PNG assets
        |
        | offline preparation
        v
Runtime PPM assets -> TextureRegistry -> Material / MaterialId
                                              |
                                              v
          Scene -> AABB objects (linear scan) -----+
                   VoxelGrid + BlockMaterials (DDA) -+-> nearest SurfaceHit -> SceneHit
                                                                                  |
                                                   +------------------------------+
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
- AABB objects are still scanned linearly; only voxel geometry is accelerated, by the DDA, so
  objects should stay few. Rendering is single-threaded; tiled multithreading remains Phase 4
  work.
- The DDA treats edge and corner contact as zero-measure: a ray exactly through the shared edge of
  two diagonally adjacent blocks passes between them, where the closed-box slab test would report a
  contact.
- Materials, light colors, and the sun direction are diagnostic values. Final artistic tuning
  belongs to the EggWars scene.

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
default five-block showcase.

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

or, with `--voxel-diagnostic`, to `output/voxel_diagnostic.ppm`.

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

## Testing

The current suite contains 483 tests covering vector arithmetic and normalization, ray invariants,
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
diagnostic scene (no double geometry, camera sweep). Every AABB and every voxel remains an opaque
shadow blocker, including glass.

The Phase 3 audit changed no runtime behavior and added no tests; the count is unchanged at 343.
It also regenerates the runtime assets with `scripts/prepare_assets.sh`, then
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

The following are not yet implemented. Phase 4 (performance architecture) is in progress and
Phase 5 (EggWars world) has not started:

- transparent or colored shadows through glass, absorption, and nested or merged glass media;
- procedural lava-region extraction into representative point lights;
- deterministic procedural 16×16 floating islands and configurable seeds;
- ore placement in terrain and the EggWars battle-aftermath scene;
- dynamic tile-based CPU multithreading;
- performance benchmarking and profiling.

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
| Procedural floating-island terrain (16×16, seeded) | Planned (Phase 5) |
| Voxel grid, 3D DDA, dynamic tile multithreading | Voxel grid, 3D DDA, and hybrid voxel + AABB scene traversal implemented and rendered (voxel diagnostic scene); tile multithreading planned (Phase 4) |
| Final EggWars aftermath scene | Planned (Phase 5–6) |

## Video

Final project video: to be added during the delivery/polish phase.
