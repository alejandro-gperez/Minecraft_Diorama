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
- Phase 3 — Raytracing Effects: in progress.
- Mission 14 — procedural sunset/night environment: complete.
- Mission 15 — bounded recursive ray infrastructure: next.
- Current suite: 158 passing tests.
- Development resolution: 320×180, presented at 960×540.
- Recent release render observations: approximately 2.77–2.83 ms on the development machine.
  These are local development observations and are not universal benchmarks.
- Last completed feature commit: `d16a265 feat(environment): add procedural sunset skybox`.

The repository is expected to begin each mission from a clean checkpoint. Verify the actual
repository state instead of assuming this section is current.

## Mission boundary

The next intended implementation is **Mission 15 — bounded recursive ray infrastructure**. Its
purpose is to introduce controlled secondary-ray tracing infrastructure without activating
reflection or refraction behavior. The user and planning assistant will provide its exact prompt
and API. Do not design, implement, or anticipate Mission 15 during unrelated work.

Do not implement future missions early. Stop at mission boundaries.

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
- Do not introduce the voxel grid, 3D DDA, dynamic tiles, multithreading, SIMD, or final
  procedural terrain during Phase 3.

## Phase 3 roadmap

This is planning context, not authorization to implement future missions:

| Mission | Status |
| --- | --- |
| 14 — procedural sunset/night environment | Complete |
| 15 — bounded recursive ray infrastructure | Next |
| 16 — obsidian reflection | Planned |
| 17 — glass refraction + IOR | Planned |
| 18 — Fresnel composition | Planned |
| 19 — lava emission + local lava lighting | Planned |
| 20 — derived cobblestone normal mapping | Planned |
| Phase 3 audit | Planned |

Established Phase 3 decisions:

- The procedural environment is sampled in world space and should later be reused for secondary
  ray misses.
- Recursive rays require a strict maximum depth. Measure performance before increasing depth.
- Obsidian is the primary reflection demonstration.
- Glass is the primary refraction demonstration, with meaningful air/glass IOR behavior.
- Fresnel/Schlick belongs after basic refraction and must remain separate from Mission 15.
- Lava must be visibly emissive. The intended simple illumination model is emissive lava plus a
  small number of finite-radius local point lights and point-light shadow rays, not global
  illumination or path tracing.
- Cobblestone normal mapping should be derived deterministically from its texture/height
  interpretation rather than an unrelated external normal-map asset.

## Known non-blockers

Do not fix these during a documentation handoff:

- the fixed shadow bias is currently suitable for the unit-scale diagnostic scene;
- glass remains optically incomplete until its refraction mission;
- lava remains non-emissive until its emission mission;
- cobblestone still uses geometric normals until normal mapping;
- known `clippy::module_inception` warnings do not justify broad refactoring;
- final sun/directional-light artistic alignment is deferred until the EggWars scene exists.

## Handoff safety

Do not modify `AGENTS.md` to record progress. Do not implement Mission 15 from this handoff alone.
Do not change renderer behavior, lighting, environment behavior, materials, geometry, camera,
assets, or tests for the purpose of creating this handoff.
