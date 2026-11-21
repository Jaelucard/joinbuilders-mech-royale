<img src="assets/icon.svg" width="64" height="64" alt="">

# JoinBuilders Mech Royale (Rust)

A Rust bot for the **Mech Royale** (`mechfight/1`) 16-mech battle royale competition.

## Architecture (`src/`)

- [`src/main.rs`](src/main.rs) / [`src/lib.rs`](src/lib.rs) — Batched stdin reader (newest-observation rule, zero-race draining) and lockstep JSON-lines protocol loop.
- [`src/protocol.rs`](src/protocol.rs) — Forward-compatible `mechfight/1` message types, action sanitization, and debug drawing cap enforcement.
- [`src/map.rs`](src/map.rs) - Arena geometry, interpolated v3 terrain queries, hard line-of-sight, walkability and clearance grids, cover candidates and rooftop anchors. The selected candidate excludes the v4 cached-height and canopy-preference changes.
- [`src/nav.rs`](src/nav.rs) - Directed traversal checks, A* navigation, path smoothing, actual-radius near-wall recovery, and controlled jetpack escape.
- [`src/world.rs`](src/world.rs) — Persistent enemy tracking (18 s memory, laser charge/cooldown tracking, rear-damage tracking), kill-loot drop memory, pickups, cubes, crates, sound threat priorities, regen countdown, and Practice-squad vs. FFA detection.
- [`src/combat.rs`](src/combat.rs) - Shared weapon aiming, explicit laser release, gun return fire, conservative mortar scheduling within gun cooldown, loadout scoring, and upgrades.
- [`src/brain.rs`](src/brain.rs) - Zone rotation, evidence-based emergency laser ducking, cover and regeneration, capture-aware supply-drop waiting, loot collection, and tactical movement.

## Build & Conformance Check

The current source is the frozen finalist `n-safe_c-window_b-drop_threat` from the [experiment campaign](research/experiments/RESULTS.md), materialized exactly from its tested source hashes. It passed 52 tests and Docker conformance. The 64-configuration campaign and 3,840 local matches did **not** establish a significant score improvement. After reviewing that result, the user explicitly authorized publication anyway; see the [publication record](research/experiments/publication.json). No official candidate score has been measured.

The candidate preserves directed traversal, shared aim scheduling, explicit lost-target laser release, useful return fire, and capture-aware supply-drop waiting. It removes the unsupported lethal-laser exception and uses the confirmed v3 terrain queries. [Earlier implementation history](research/implementation-results.md) and the [current selected-source manifest](research/experiments/selected-manifest.json) distinguish this finalist from prior local candidates.

The user confirmed **4.81 on v3 `411af5f`** and **BB-488640B1 / 4.65 on v4 `5b783d1`**. The [scored-revision investigation](research/scored-revision-investigation.md) isolates terrain sampling and canopy preference as the actual code delta. Both baseline revisions remain preserved in Git history.

```sh
docker build -t mechbot .
python3 kit/check/check.py --map-dir /arena -- docker run --rm -i --network none --cpus 1 --memory 512m --read-only --tmpfs /tmp:rw,size=64m --user 65534:65534 -v "$PWD/kit/arena:/arena:ro" mechbot
```
