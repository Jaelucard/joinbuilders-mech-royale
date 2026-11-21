<img src="assets/icon.svg" width="64" height="64" alt="">

# JoinBuilders Mech Royale — `vanguard-v1` (Rust)

A Rust bot for the **Mech Royale** (`mechfight/1`) 16-mech battle royale competition.

## Architecture (`src/`)

- [`src/main.rs`](src/main.rs) — Batched stdin reader (newest-observation rule, zero-race draining) and lockstep JSON-lines protocol loop.
- [`src/protocol.rs`](src/protocol.rs) — Forward-compatible `mechfight/1` message types, action sanitization, and debug drawing cap enforcement.
- [`src/map.rs`](src/map.rs) — Loads `/arena/v2/world.json` (259 solid prisms) and `/arena/v2/terrain.i16` (1 m elevation grid), builds a 16 m prism spatial hash, exact 3D Line-of-Sight raycaster, 4 m walkability/clearance grid, and hard-cover finder.
- [`src/nav.rs`](src/nav.rs) — Reusable zero-allocation 8-connected A\* pathfinder with string-pull smoothing, caching, and stuck recovery.
- [`src/world.rs`](src/world.rs) — Persistent tracking of enemies, pickups, cubes, crates, sound threat priorities, regen countdown, and Practice-squad vs. FFA detection.
- [`src/combat.rs`](src/combat.rs) — Exact linear lead aiming (gun), hold-charge/aligned-release hitscan controller (laser + mortar cook-off priority), close-range cone/energy controller (lightning + anti-laser shorting), parabolic trajectory solver with obstacle clearance (mortar), weapon loadout swapping, and upgrade picker.
- [`src/brain.rs`](src/brain.rs) — Priority tactical decision engine (zone rotation, cover-regen loop, cube/weapon farming, supply drops, combat positioning), 16-direction micro-steering (bullet/mortar/charging-laser dodging), and rear-armor torso discipline.

## Build & Conformance Check

```sh
docker build -t mechbot .
python3 kit/check/check.py --map-dir /arena -- docker run --rm -i --network none --cpus 1 --memory 512m --read-only --tmpfs /tmp:rw,size=64m --user 65534:65534 -v "$PWD/kit/arena:/arena:ro" mechbot
```
