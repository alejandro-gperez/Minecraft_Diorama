# Claude Code Working Instructions

This file is the Claude Code handoff for the CPU Raytraced EggWars Diorama repository.

## Authority and project context

- `AGENTS.md` is the authoritative project specification and must be read completely before any change.
- `CLAUDE.md` contains Claude Code working instructions and the verified current checkpoint.
- `README.md` is public-facing project documentation.
- Git history records the implementation history and must be preserved.
- If this file conflicts with `AGENTS.md`, follow `AGENTS.md`.

This is a Rust CPU raytraced Minecraft-inspired EggWars aftermath diorama, not a playable game.
The user controls only the external orbital camera and presentation/debug controls; there is no
player entity or gameplay system.

## Verified checkpoint

- Phase 1 — Core Raytracer: complete.
- Phase 2 — Materials, Textures, and Lighting: complete.
- Phase 3 — Raytracing Effects: complete (missions 14–20).
- Phase 4 — Performance Architecture: in progress.
- Mission 21 voxel grid foundation: complete. `src/voxel/` holds `BlockType`, one-byte `Voxel`,
  integer `VoxelPosition`, a dense `VoxelGrid` with a configurable origin, and `BlockMaterials`.
- Mission 22 voxel scene integration foundation: complete. `BlockType` adds `Dirt` (10 variants,
  still one byte; no Stone). Auxiliary dirt/ore materials (`AuxiliaryMaterials`, separate from
  `CanonicalMaterials`) are registered at startup; dirt reuses the grass-bottom texture.
  `BlockMaterials` maps every block type. `Scene` optionally owns block materials and one
  `VoxelGrid` (grid requires block materials); the showcase has no grid.
- Hybrid hit architecture: `AabbHit` became the source-neutral `geometry::SurfaceHit`;
  `voxel::VoxelHit` wraps a `SurfaceHit` plus voxel/block/material; `SceneHit` is
  `{ geometry: SurfaceHit, material_id, source: HitSource }`. `CubeFace::uv` is the one shared
  UV table. Shading must not branch on `source`. The renderer still traverses AABBs only.
- Mission 23 3D DDA voxel traversal: complete, tested in isolation, not renderer-integrated.
  `VoxelGrid::intersect(ray, &BlockMaterials, t_min, t_max) -> Option<VoxelHit>` in
  `src/voxel/traversal.rs`: one grid-bounds slab clip, then Amanatides–Woo stepping over contiguous
  storage; no per-voxel `Aabb`, no allocation. Segment starts at `max(t_min, grid entry)`; interval
  inclusive. Conventions: direction-aware half-open cell ownership (at `x = 1.0`, `+X` is in cell 1,
  `-X` in cell 0); the cell being left is never reported; starting strictly inside a solid reports
  its exit face; starting exactly on an occupied cell's entry face reports it at `t = 0`; tied axes
  step together to the diagonal cell; face priority X > Y > Z (Aabb slab order); `|d| <= 1e-8` is
  parallel. Plane parameters use `(plane - origin) / direction`, so `t` and UV match the unit
  `Aabb` bit-for-bit (oracle-tested). `Scene::closest_hit`/`is_occluded` do not call it yet.
- Current suite: 440 passing tests. The showcase PPM is byte-identical to the Mission 21 render.
- Development resolution: 320×180, presented at 960×540.
- Pre-Phase-4 baseline (local, single-threaded, brute-force AABB traversal): default view about
  4.9 ms, close cobblestone about 10.7 ms, close lava about 18 ms, close glass about 30 ms. These
  are local development observations, not universal benchmarks; the README has the full table.
- Compiler state: no rustc warnings; clippy reports only the known `clippy::module_inception`
  warnings (plus two pre-existing `assertions_on_constants` warnings in test code).
- Next work: Mission 24, hybrid DDA + arbitrary AABB traversal in `Scene` queries, as specified by
  the user.

The repository is expected to begin each mission from a clean checkpoint. Verify the actual
repository state instead of assuming this section is current.

## Mission boundary

Do not implement remaining Phase 4 work (renderer/scene traversal of the voxel grid, tiled
multithreading, SIMD) or Phase 5 (procedural terrain, the EggWars scene) from this handoff. The
user and planning assistant will provide the exact prompt and API for each future mission. Do not
implement future missions early. Stop at mission boundaries.

## Working protocol

Before coding:

1. Read `AGENTS.md` completely.
2. Read `CLAUDE.md`.
3. Inspect relevant source files, tests, and recent Git history.
4. Confirm the requested work belongs to the current phase.
5. Preserve existing behavior and avoid future-phase work.

During coding:

- Make the smallest coherent change.
- Preserve the Rust, CPU-only architecture.
- Keep Raylib limited to presentation, input, timing, and window lifecycle; never use Raylib
  3D rendering for scene geometry or raytracing.
- Do not add a third-party dependency without explicit approval.
- Keep core renderer modules independent from Raylib types.
- Prefer AABB/voxel-first geometry and avoid unnecessary allocations in hot paths.
- Preserve numerical conventions and use clearly justified epsilons.
- Avoid unrelated refactors.

Before committing, run at minimum:

```bash
cargo fmt --check
cargo test
cargo check
cargo build --release
```

Run `cargo run --release` when graphical or runtime behavior is relevant. Run
`scripts/prepare_assets.sh` only for asset-pipeline work. Inspect `git status` and `git diff`
before committing.

Use concise Conventional Commits. Keep each coherent implementation block atomic. Do not amend,
squash, rebase, force-push, rewrite existing history, or bundle unrelated changes unless the
user explicitly requests it.

## Project principles to preserve

- Rust stable and CPU raytracing only; no GPU acceleration.
- Raylib is presentation/input only and must remain replaceable.
- No additional dependency without explicit approval.
- AABB-first, voxel-friendly scene architecture.
- The final scene is a classic Minecraft 1.6–1.8-inspired EggWars battle aftermath.
- Five canonical materials already exist: grass, cobblestone, obsidian, glass, and lava.
- Original classic Minecraft texture assets are converted offline to runtime PPM and loaded by
  project-owned CPU code; do not recreate final block artwork procedurally without approval.
- Face-specific texture selection must remain possible.
- Terrain will later use a configurable deterministic seed.
- Future performance architecture is a voxel grid, 3D DDA, and dynamic tile-based CPU
  multithreading.
- The voxel grid stores block identity only: no per-voxel `Aabb` or `Material`, and interior
  voxels stay stored. Do not integrate DDA into scene queries or introduce dynamic tiles,
  multithreading, SIMD, or procedural terrain before their missions.

## Phase 3 summary

Mission status: 14–20 complete; Phase 3 audit complete. Established decisions to preserve:

- The procedural environment is sampled in world space and is the single miss path for every
  radiance ray, primary or secondary.
- Recursion is bounded by `MAX_RAY_DEPTH = 3` (depth 0 is primary). Shadow rays take no depth.
  Measure performance before increasing the depth.
- Opaque materials blend constant reflectivity; transparent materials use Schlick Fresnel with
  Snell refraction (glass IOR 1.5) and route total internal reflection to reflection.
- Emission is material-owned and unshadowed; lava lights its surroundings through a small number of
  finite-radius point lights with hard shadow rays, not global illumination.
- Cobblestone normal mapping uses a map derived offline from its own texture. The shading normal
  drives local lighting only; the geometric normal owns everything else.

## Known non-blockers

Do not fix these without an explicit mission:

- glass casts opaque hard shadows; there is no nested-medium tracking (touching glass is glass →
  air → glass); faces hit from inside glass are lit with the outward normal;
- origin biases and point-light radius assume approximately unit-scale geometry;
- normal mapping changes shading but not silhouettes, shadows, reflection, or refraction;
- lava regions are not converted into lights automatically;
- known `clippy::module_inception` warnings do not justify broad refactoring;
- final sun/directional-light artistic alignment and material tuning are deferred until the
  EggWars scene exists.

## Handoff safety

Do not modify `AGENTS.md` to record progress. Do not start a new mission from this handoff alone.
