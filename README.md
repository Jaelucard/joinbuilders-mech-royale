<img src="assets/icon.svg" width="64" height="64" alt="">

# JoinBuilders Mech Royale — `vanguard-v2` (Rust)

A Rust bot for the **Mech Royale** (`mechfight/1`) 16-mech battle royale competition.

## Architecture (`src/`)

- [`src/main.rs`](src/main.rs) — Batched stdin reader (newest-observation rule, zero-race draining) and lockstep JSON-lines protocol loop.
- [`src/protocol.rs`](src/protocol.rs) — Forward-compatible `mechfight/1` message types, action sanitization, and debug drawing cap enforcement.
- [`src/map.rs`](src/map.rs) — Loads `/arena/v2/world.json` (259 prisms) and `/arena/v2/terrain.i16` (1 m elevation grid), rasterizes a 2 m solid height-field for ~260 ns 3D Line-of-Sight raymarching, builds a 4 m walkability/clearance grid + 252 climbable rooftop anchors, hard-cover finder, slice-the-pie 1vMany isolation, and corner peek-pair finder.
- [`src/nav.rs`](src/nav.rs) — Reusable zero-allocation 8-connected A\* pathfinder with string-pull smoothing, caching, and stuck recovery.
- [`src/world.rs`](src/world.rs) — Persistent tracking of enemies (including laser charge start times), kill-loot drop memory, pickups, cubes, crates, sound threat priorities, regen countdown, and Practice-squad vs. FFA detection.
- [`src/combat.rs`](src/combat.rs) — Slew-predicted 1-tick hitscan laser release + behind-cover pre-charging (`+60%` mortar cook-off priority), exact linear lead aiming (`gun`), close-range cone/energy firing (`lightning` + anti-laser shorting), parabolic trajectory solver (`mortar`), holistic 2-weapon loadout scoring, and upgrade picker.
- [`src/brain.rs`](src/brain.rs) — Priority tactical engine (zone rotation, emergency laser-duck reflex, active-circle cover-regen loop, kill-loot & cube snowballing, supply drops, slice-the-pie 1vMany isolation, laser corner-peeking, rooftop high-ground climbing/holding), 16-direction micro-steering, and rear-armor torso discipline.

## Build & Conformance Check

```sh
docker build -t mechbot .
python3 kit/check/check.py --map-dir /arena -- docker run --rm -i --network none --cpus 1 --memory 512m --read-only --tmpfs /tmp:rw,size=64m --user 65534:65534 -v "$PWD/kit/arena:/arena:ro" mechbot
```
