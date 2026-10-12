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
- Phase 4 — Performance Architecture: complete and audited (missions 21–27B).
- Phase 5 — Procedural EggWars Diorama: in progress (`AGENTS.md` names it current). Missions 28
  and 29 complete; Mission 30 (bridges and structures) is next and needs its own prompt.
- Mission 29 five-island arena: complete. `src/world/arena.rs`: `IslandId` enum
  (Center/North/South/East/West, `ALL`, index = discriminant, `direction()` = unit `(dx, dz)`
  outward, north `-Z`, east `+X`); `island_seed(global, id) = hash3(global, tag, 0, 0, 0)` with
  fixed tags `arn-cntr/nrth/soth/east/west` (order-independent, no RNG); `ArenaLayout::new(sep)`
  validates every config and every region pair (gap >= `MIN_ISLAND_GAP` = 1, region boxes, not the
  mask) before allocating; `ArenaLayout::eggwars()` = separation 32. Center 24×24 thickness 15 at
  the origin, outer 16×16 thickness 12 at ±32 on X/Z, all at surface `y = 0`; nominal gap 12
  (visible 12–17). `generate_arena(&layout, seed) -> Result<Arena { grid, info: ArenaInfo }>`
  allocates one grid exactly the regions' union — origin (-40, -14, -40), 80×16×80, 100 KiB, no
  margin (structures above `y = 1` will need the top raised) — then calls `generate_island` once
  per island. Pinned derived seeds and arena fingerprint for seed 1847.
- EggWars mode: `--eggwars [--seed <u64>]` (default 42, `cli::DEFAULT_SEED`, renamed from
  `DEFAULT_ISLAND_SEED`); two mode flags, or `--seed` with `--voxel-diagnostic`, exit 2; `--seed`
  alone is still the procedural island. Scene = arena grid, no objects/lights. Camera targets the
  central island (yaw 0.7, pitch 0.35, radius 68; all five islands fully in frame). Startup log has
  one arena line plus one line per island (derived seed); overlay shows only the global seed; PPM
  `output/eggwars_seed_<seed>.ppm`. Existing mode PPMs byte-identical to Mission 28.
- Mission 29 measurements (M3 Pro, 320×180, release): generation ~0.10 ms warm, 7,169–8,771 blocks
  over seeds 0–255; initial view ~9.0–9.6 ms at 1 worker, ~2.6 ms at 4, ~1.35 ms automatic; all
  measured views 0.8–2.3 ms automatic. An empty 80×16×80 grid alone costs 8.9 ms of the 9.5 ms
  single-threaded initial view (no grid: 1.4 ms): empty-cell DDA stepping dominates but is
  acceptable; empty-space skipping is the first candidate only if a larger grid measures too slow.
- Mission 28 procedural floating island: complete. `src/world/noise.rs`: stateless SplitMix64-style
  `hash3(seed, Domain, x, y, z)`, smooth 2D value noise, periodic 1D noise, and a "diamond angle"
  (no `atan2`/`sin`/`pow`: only exactly rounded float ops, so seeds are bit-reproducible across
  platforms). Each feature has its own 8-byte `Domain` tag (silhouette, detail, surface, dirt,
  underside, tip, ore cluster, ore fill); no RNG stream. `src/world/island.rs`:
  `IslandConfig { center, footprint_width, footprint_depth, thickness }` (`new` = 16×16; `center.y`
  is the flat grass level), `bounds()` (seed-independent region), and
  `generate_island(&mut VoxelGrid, &IslandConfig, seed) -> Result<IslandStats, IslandError>`, which
  validates everything (zero footprint, thickness < 5, i32 overflow, region outside grid) before
  writing, never clips, and writes only island blocks. Shape: per-column plan over all W×D cells;
  star-shaped radial mask `R ∈ [0.79, 1.13]` of the half extent; flat center (`edge < 0.45`); grass
  at −1/0/+1; dirt 1–3; cobblestone body to a depth tapering from `thickness - 1` under an offset tip
  to 4 at the rim; every column contains level −1 (one connected landmass), lower layers are subsets
  (monotonic taper). Ores replace only side-exposed cobblestone via 2×2×2 cluster cells (cluster /
  fill: coal 10%/75%, iron 6%/65%, gold 3%/55%, diamond 1.5%/35%). Local coordinates make it
  translation invariant. A test pins seed 1847's fingerprint; constant changes update it.
- Seed CLI (`src/cli.rs`, std only): no args = showcase; `--voxel-diagnostic`;
  `--procedural-island [--seed <u64>]` or `--seed <u64>` alone; default seed 42. Malformed,
  negative, overflowing, missing, repeated, unknown, or conflicting arguments exit 2 with usage.
  Island mode: `ISLAND_CONFIG = IslandConfig::new((0,0,0), 12)` in `main.rs`, one global grid sized
  to its bounds (16×13×16), no objects or lights, camera targets the island's visual center (yaw
  1.05, pitch 0.25, radius 22), PPM `output/procedural_island_seed_<seed>.ppm`, seed shown in the
  overlay (`run` takes an `overlay_label`). Showcase and voxel diagnostic PPMs are byte-identical
  to the previous commit; no renderer, DDA, scene-query, lighting, or material change.
- Mission 28 measurements (M3 Pro, 320×180, release): generation 0.015–0.04 ms; 966–1,452 blocks
  over seeds 0–255; initial island view ~0.8 ms automatic, ~1.3 ms at 4 workers, ~4.7 ms at 1.
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
  UV table. Shading must not branch on `source`.
- Mission 23 3D DDA voxel traversal: complete.
  `VoxelGrid::intersect(ray, &BlockMaterials, t_min, t_max) -> Option<VoxelHit>` in
  `src/voxel/traversal.rs`: one grid-bounds slab clip, then Amanatides–Woo stepping over contiguous
  storage; no per-voxel `Aabb`, no allocation. Segment starts at `max(t_min, grid entry)`; interval
  inclusive. Conventions: direction-aware half-open cell ownership (at `x = 1.0`, `+X` is in cell 1,
  `-X` in cell 0); the cell being left is never reported; starting strictly inside a solid reports
  its exit face; starting exactly on an occupied cell's entry face reports it at `t = 0`; tied axes
  step together to the diagonal cell; face priority X > Y > Z (Aabb slab order); `|d| <= 1e-8` is
  parallel. Plane parameters use `(plane - origin) / direction`, so `t` and UV match the unit
  `Aabb` bit-for-bit (oracle-tested).
- Mission 24 hybrid voxel + AABB traversal: complete. `Scene::closest_hit` scans objects first,
  then runs the DDA with `t_max` = nearest object `t`; a voxel wins only with strictly smaller `t`
  (object wins exact ties, zero tolerance, valid because both use `(plane - origin) / direction`).
  `VoxelGrid::intersects(ray, t_min, t_max)` is the any-hit DDA (shares the private
  `first_occupied` loop with `intersect`; no UV/material/`VoxelHit`). `Scene::is_occluded` tests
  objects, then the grid; every voxel, glass and lava included, is an opaque blocker. Voxel
  shadows (directional and finite point-light) and recursive reflection/refraction into voxels
  work with no renderer change; the renderer has no voxel-specific branch.
- Voxel diagnostic: `cargo run --release -- --voxel-diagnostic` (`voxel_diagnostic_scene` in
  `main.rs`, writes `output/voxel_diagnostic.ppm`); default showcase unchanged, no grid.
- Known Mission 24 limitation: a refracted ray leaving glass through a face touching another block
  is biased into that block and sees its far interior face (glass → air → block, like touching
  glass). Needs interface/medium handling; not fixed.
- Mission 25 deterministic tile renderer: complete, still single-threaded. `src/render/tile.rs` holds
  `Tile` (half-open `Copy` rectangle in global pixels, no storage) and `TileGrid` (arithmetic,
  overflow-safe, allocation-free `tiles()`; zero tile size / count overflow are `TileGridError`,
  surfaced as `RenderError::InvalidTileConfiguration`). Tile indices are row-major
  (`x = i % columns`, `y = i / columns`, 0 = upper left). Default 16×16 → 240 tiles at 320×180,
  last row 4 px high; edge tiles are clipped. `Renderer::render` is unchanged for the app and calls
  `render_with_tile_size`; `render_tile` writes directly into the one `Framebuffer` using global
  pixel coordinates for primary rays, so output is independent of tile size and order. The
  previous row-major loop survives only as a test reference (`tile_tests.rs`). Default showcase and
  voxel diagnostic PPMs are byte-identical to the pre-mission renders; release timings are
  unchanged within noise (~5.4 ms default, ~5.4 ms diagnostic at 320×180).
- Mission 26 dynamic CPU multithreading: complete. `src/render/parallel.rs`: per render,
  `std::thread::scope` spawns workers that claim tile indices with
  `AtomicUsize::fetch_add(1, Relaxed)` (assignment only, never publication). Mission 26 sent
  finished tile buffers to the calling thread over `mpsc` channels; Mission 27B replaced that (see
  below). No `unsafe`, no pool.
  `Renderer::render` = `available_parallelism()` workers (fallback 1);
  `render_with_worker_count` for tests/benchmarks; zero is `RenderError::InvalidWorkerCount`;
  bounded by tile count and `MAX_WORKERS = 64`; one effective worker runs the Mission 25 sequential
  loop on the calling thread. A failure sets a stop flag; the lowest-index failure is returned
  (equals the sequential error); worker panics propagate to the caller. Output is
  byte-identical to the sequential renderer for every worker count, tile size, and completion
  order; both app PPMs are byte-identical to Mission 25.
- Mission 26 measurements (M3 Pro, 11 CPUs = 5P + 6E, 320×180, release, medians): default
  5.08 → 0.89 ms automatic (5.7×), voxel diagnostic 5.13 → 0.91 ms, close glass 31.7 → 4.6 ms
  (6.9×); 4 workers ~3.0–3.4×. Two workers scale anomalously (~1.6×); see the Mission 27A
  conclusions below. The 1-worker path matches the Mission 25 binary within noise.
- Mission 27B simplified parallel tile integration: complete. `render_tiles_parallel` keeps the
  scoped threads and `AtomicUsize` claiming, but the one framebuffer now sits behind a
  `std::sync::Mutex` (borrowed through the scope, no `Arc`). Each worker owns exactly one
  tile-sized buffer (3 KiB; 33 KiB at 11 workers, 192 KiB at `MAX_WORKERS`), traces without any
  lock, and holds the lock only for `Framebuffer::write_tile`. No channels, no buffer return, no
  `BUFFERS_PER_WORKER`. Workers return `Option<(tile_index, RenderError)>`; after the join the
  minimum tile index wins (same error as sequential). Panics are re-raised with `resume_unwind`
  after the join; a poisoned lock panics, never recovers. The sequential 1-worker path and the
  worker-count rules are untouched; both app PPMs are byte-identical to Mission 26.
- Mission 27B measurements (same M3 Pro, 320×180, release, medians of 12 interleaved rounds
  against a Mission 26 binary): 2 and 4 workers 13–15% faster on default, voxel diagnostic, and a
  voxel-island proxy (e.g. default 3.11 → 2.67 ms and 1.64 → 1.40 ms), 8 workers 5–8% faster,
  close glass 2–5% faster at 2–4 workers; at the automatic 11 workers the change is within noise
  (default 0.92 → 0.90 ms, close glass 4.56 → 4.71 ms, which matches a +5.5% code-layout shift of
  the unchanged 1-worker path). No speedup is claimed at the automatic count.
- Phase 4 technical audit: no implementation defect found, no runtime change, no new tests (529
  passing). Verified voxel grid, material mapping, DDA and its AABB oracle, hybrid closest-hit and
  occlusion, recursive-effect integration (no voxel branch in the renderer), tiles, scoped-thread
  scheduling, the Mutex tile-copy design, errors/panics, determinism, hot-path allocations, the
  Raylib boundary (only `src/app/app.rs`), and dependencies (raylib 6.0.0 only; no `unsafe`).
  README has the full audit, baseline, and Phase 5 guidance.
- Audit baseline (M3 Pro, 11 CPUs, 320×180, release, median of 8 round-medians; 1 / 4 / automatic
  workers, ms): default showcase 5.13 / 1.39 / 0.89 (3.7× / 5.8×), voxel diagnostic 5.18 / 1.40 /
  0.89, close glass 31.7 / 8.34 / 5.30 (6.0×), a throwaway five-island voxel proxy 14.1 / 3.75 /
  2.00 (7.0×). Local observations, not universal benchmarks.
- Mission 27A conclusions to keep: 16×16 tiles and the automatic worker count are fine; a
  persistent pool, incremental `t_delta`/reciprocal DDA (it changed pixels), and a general BVH are
  not justified, and are not to be reopened without new evidence. The two-worker scaling anomaly
  (~1.6×) is unexplained; evidence went against the efficiency-core hypothesis. Not a blocker.
- Important known limitations: opaque glass/lava shadows; no medium tracking (touching glass is
  glass → air → glass; glass on a block exposes that block's interior face); `1e-4` biases assume
  unit scale; edge/corner contact follows the DDA's half-open conventions; AABB objects are scanned
  linearly (group them only if the real scene's measured object count warrants it); the arena is
  terrain only (no bridges, structures, lava, or props); no camera collision (zoom can enter an
  island). The window title in `src/app/app.rs` still says "Phase 3".
- Rubric: all raytracing effects, DDA, and parallel rendering exist in code and diagnostic scenes
  only; they count only when visibly and intentionally used in the final diorama (Phase 5–6).
- Current suite: 613 passing tests. The showcase PPM is byte-identical to the Mission 21 render.
- Development resolution: 320×180, presented at 960×540.
- Pre-Phase-4 baseline (local, single-threaded, brute-force AABB traversal): default view about
  4.9 ms, close cobblestone about 10.7 ms, close lava about 18 ms, close glass about 30 ms. These
  are local development observations, not universal benchmarks; the README has the full table.
- Mission 24 measurement: 4,114 blocks as a voxel grid render ~225–250× faster than as per-block
  AABBs (e.g. 9.8 ms vs 2.4 s at 320×180); the showcase is unchanged within noise. Measure the
  showcase through the release app binary: test-binary layout shifted it by ~10% with no code cause.
- Compiler state: no rustc warnings; clippy reports only the known `clippy::module_inception`
  warnings (plus two pre-existing `assertions_on_constants` warnings in test code).
- Next work: Mission 30 (bridges and structures), from the user's exact prompt. Not authorized by
  this checkpoint.

The repository is expected to begin each mission from a clean checkpoint. Verify the actual
repository state instead of assuming this section is current.

## Mission boundary

Phase 4 is complete and audited; Phase 5 is in progress with Missions 28 and 29 complete. Do not
start further Phase 4 optimization (SIMD, persistent pools, BVH, reciprocal DDA), Mission 30,
bridges, structures, lava, or decorations from this handoff. The user and planning assistant
will provide the exact prompt and API for each future mission. Do not implement future missions
early. Stop at mission boundaries.

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
- Terrain uses a configurable deterministic `u64` seed and stateless per-domain hashing; never
  introduce a shared mutable RNG stream or external randomness.
- Future performance architecture is a voxel grid, 3D DDA, and dynamic tile-based CPU
  multithreading.
- The voxel grid stores block identity only: no per-voxel `Aabb` or `Material`, and interior
  voxels stay stored. Never also add `SceneObject`s duplicating occupied voxels. All islands share
  one global `VoxelGrid` (no chunks, octrees, or per-island grids). Do not introduce persistent
  thread pools, unsafe framebuffer writes, or SIMD before their missions.

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

- glass (AABB or voxel) casts opaque hard shadows; there is no nested-medium tracking (touching
  glass is glass → air → glass, and glass resting on a block refracts into that block's interior);
  faces hit from inside glass are lit with the outward normal;
- origin biases and point-light radius assume approximately unit-scale geometry;
- normal mapping changes shading but not silhouettes, shadows, reflection, or refraction;
- lava regions are not converted into lights automatically;
- known `clippy::module_inception` warnings do not justify broad refactoring;
- final sun/directional-light artistic alignment and material tuning are deferred until the
  EggWars scene exists.

## Handoff safety

Do not modify `AGENTS.md` to record progress. Do not start a new mission from this handoff alone.
