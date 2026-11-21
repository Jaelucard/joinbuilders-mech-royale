<img src="assets/icon.svg" width="64" height="64" alt="">

# JoinBuilders Mech Royale — `vanguard-v4` (Rust)

A Rust bot for the **Mech Royale** (`mechfight/1`) 16-mech battle royale competition.

## Architecture (`src/`)

- [`src/main.rs`](src/main.rs) / [`src/lib.rs`](src/lib.rs) — Batched stdin reader (newest-observation rule, zero-race draining) and lockstep JSON-lines protocol loop.
- [`src/protocol.rs`](src/protocol.rs) — Forward-compatible `mechfight/1` message types, action sanitization, and debug drawing cap enforcement.
- [`src/map.rs`](src/map.rs) — Loads `/arena/v2/world.json` (259 prisms + 1,560 `RawTrees` canopy spheres) and `/arena/v2/terrain.i16` (1 m elevation grid), rasterizes separate 2 m `solid_top` (3D LoS), `building_top` (wall/roof), and O(1) `terrain_grid` (`terrain_z_fast`) height-fields with terrain-slope awareness (< 48° hills pass cleanly), builds a 4 m walkability/clearance/`nav_z` grid, 4 m `nav_canopy` concealment grid, and 252 climbable rooftop anchors, canopy-tie-broken hard-cover finder, slice-the-pie 1vMany isolation, and corner peek-pair finder.
- [`src/nav.rs`](src/nav.rs) — Reusable zero-allocation 8-connected cliff-aware A\* pathfinder with string-pull smoothing, caching, travel-only oscillation detection, and escalating wall/courtyard jetpack escape.
- [`src/world.rs`](src/world.rs) — Persistent enemy tracking (18 s memory, laser charge/cooldown tracking, rear-damage tracking), kill-loot drop memory, pickups, cubes, crates, sound threat priorities, regen countdown, and Practice-squad vs. FFA detection.
- [`src/combat.rs`](src/combat.rs) — Dual-motion-compensated hitscan laser release (with instant laser-duck slow abort and behind-cover pre-charging), exact linear lead aiming (`gun`), close-range arc-engagement (`lightning` + anti-laser shorting), over-cover/reload-window `mortar` solver, holistic 2-weapon loadout scoring, and upgrade picker.
- [`src/brain.rs`](src/brain.rs) — Priority tactical engine (`vanguard-v4`): zone rotation, emergency laser-duck reflex, memory-synced cover-regen holding (stays hidden past the 5 s `vision.memory_s` / `regen_delay` sync point to finish healing to 82–88% HP), safe loot/crate/kill-loot collection, supply-drop timing, slice-the-pie 1vMany isolation, laser corner-peeking, rooftop climbing/lateral rooftop strafing, unreachable-target blacklisting, 16-direction kinematic micro-steering with anti-lightning repulsion, and rear-armor torso discipline.

## Build & Conformance Check

The latest [experiment campaign](research/experiments/RESULTS.md) completed 64 configurations and 3,840 approximate local matches against the confirmed v3 baseline. The frozen finalist passed 52 tests and Docker conformance, but failed the predeclared improvement threshold in both validation process runs. **No candidate was pushed.** Its local gains are not official score predictions; production source in this checkout remains the earlier local R2 candidate.

The `fix/scoring-control` candidate adds directed navigation checks, shared weapon-aim scheduling, explicit lost-target laser release, and capture-aware supply-drop waiting. The unsupported lethal-laser duck exception is removed. See [implementation results](research/implementation-results.md) for separate commits, regression tests, matched proxy comparisons, and remaining limitations. The reported official baseline is **4.81**; no candidate official score has been measured.

The user confirmed **4.81 on v3 `411af5f`** and **BB-488640B1 / 4.65 on v4 `5b783d1`**. The [scored-revision investigation](research/scored-revision-investigation.md) isolates terrain sampling and canopy preference as the actual code delta. The later local candidate repairs pass 47 tests plus Docker conformance, but [repair validation](research/repair-validation.md) remains mixed; that branch has no official score.

```sh
docker build -t mechbot .
python3 kit/check/check.py --map-dir /arena -- docker run --rm -i --network none --cpus 1 --memory 512m --read-only --tmpfs /tmp:rw,size=64m --user 65534:65534 -v "$PWD/kit/arena:/arena:ro" mechbot
```
