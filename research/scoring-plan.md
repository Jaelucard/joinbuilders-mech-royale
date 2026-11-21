# Mech Royale scoring improvement plan

Research date: 21 November 2026. Bot reviewed: `5b783d1`, matching fetched `origin/master`. Reported official baseline: **4.81**. Three native research agents worked concurrently, then reviewed the combined findings in a second pass. No sharing-capability error occurred.

Supporting artifacts: [executable diagnostic source](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/behavior_probes.rs), [public standings snapshot](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/standings-snapshot.json), and [score comparison chart](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/score-context.html).

Implementation follow-up: the verified control fixes are now implemented and evaluated. See [candidate results and submission notes](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/implementation-results.md). The findings below describe the original baseline; speculative lost-target holding and blanket regen-triggered laser suppression were rejected during evaluation.

The best next investment is a measured repair of verified control defects while preserving the strategy behind the reported 4.81 baseline. Keep Rust. The existing bot meets the decision deadline comfortably; further lookup optimizations are unlikely to change its in-game action frequency. First establish why it loses placement, repair reproducible behavior defects, then test practice-specific policies one at a time. No score increase or leaderboard win has been demonstrated yet.

## What the score requires

The official score is mean `16 − place`, so 4.81 corresponds approximately to mean place 11.19. This does **not** reveal when the bot dies. Placement alone does not identify which phase or mechanism causes a loss. Kills have no separate scoring bonus, but removing a practice hunter improves placement and supplies resources.

The [official track](https://www.joinbuilders.org/buildathon/tracks/builders) describes 48 practice games against 15 house bots that share vision, do not fight each other, and progressively hunt the entrant. Finals are FFA on new seeds. Practice therefore requires sustainable hunter elimination, recovery, and equipment acquisition; pure hiding cannot rely on opponents killing one another. FFA warrants a separately tested policy for avoiding unfavorable fights and exploiting third-party attrition.

The [public standings](https://www.joinbuilders.org/buildathon/standings/builders), captured at 2026-11-21 08:43:33 UTC, showed leader azing at **5.96**, team 2.3% at **5.60**, giddygiddyboy at **5.56**, and rank 32 at **5.04**. These are changing reference points, not promised targets. The user's team and scoring submission commit have not been independently identified. The 4.81 value is user-reported and has not been tied to `5b783d1`. The snapshot is saved in `standings-snapshot.json`.

## Evidence from the current bot

- A fresh Docker build from the current source passed the official conformance checker under no-network, one-CPU, 512 MB, read-only, unprivileged restrictions. All 300 observations were answered on time; no invalid lines, wrong ticks, stale queued answers, or excessive debug output. Observed p99 was 1.8 ms and maximum 2.0 ms. This sample establishes protocol health, not performance across all match states.
- All four existing Rust tests passed, including the real-arena navigation/LoS test. The 1,000-ray test took approximately 1.1 ms in this run.
- Standalone probes reproduced shared weapon aim and premature supply-drop abandonment against the current compiled library, plus the navigation shortcut inconsistency on the real arena. The combat/drop probes deliberately use an empty map to isolate those behaviors; they do not simulate full matches.
- Matches are lockstep. Faster answers below the 50 ms limit do not produce extra in-game decisions. See [kit/protocol.md:63](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/kit/protocol.md:63).
- `kit/check/check.py` replays fixed observations that do not respond to actions. Its recorded end placement is **not** a score for this bot.
- The local `sim/` and official `kit/` are ignored by Git. The simulator's staged hunter policy and many physical details are approximations. It shares `ArenaMap` geometry with the bot and even uses a reduced collision radius. Agreement between them is not independent validation.

## Priority order

### 1 Establish the failure distribution and preserve the baseline

Before strategic tuning, associate the 4.81 submission with its actual commit, scorer version, and available match results. Preserve `5b783d1` as a separate baseline candidate until that association is known. Record per-game placement and survival time independently.

Preserve the working strategy and instrument local/private-room decision traces with first contact, first damage, first secondary weapon, first hunter kill, cubes at 30/60/90 seconds, weapon damage and misses, HP before/after an encounter, regen attempts/completions, threats with line of sight, energy at escape, stuck seconds, and zone damage. Include the chosen macro action and target. Official stderr and debug drawings are not exposed through the track UI; obtain only the replay/feed data the service actually offers and collect detailed traces locally.

Inspect the first 30–60 seconds as one diagnostic slice, then inspect the final encounter before death. Categorize losses as unarmed exposure, failed aim/fire, movement stall, unsafe retreat, zone loss, or poor engagement selection. This prevents optimizing late-game drops if most matches end before the first drop, or optimizing opening routes if the actual problem is prolonged ineffective combat.

**Deliverable:** reproducible baseline manifest plus per-match outcomes and a ranked failure taxonomy. No simulated score should be labeled official.

### 2 Repair concrete control defects in separate changes

**A. Preserve traversability through every navigation stage.** A* rejects directed uphill steps that shortcuts and smoothing accept. [src/nav.rs:295](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/nav.rs:295) checks rise; [src/map.rs:1436](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/map.rs:1436) checks sampled walkable cells without the same transition constraint. The compiled Rust map confirmed adjacent walkable cells near `(-242.38, 48.91)` and `(-238.38, 48.91)` with a 5.476592 m rise over 4 m: beyond the A* threshold but accepted by `line_walkable`. On an 8 m route from `(-246.38, 48.91)` to `(-238.38, 48.91)`, `next_waypoint` returned the goal directly, bypassing that edge constraint. Use one swept, directed traversal predicate for A*, direct paths, and smoothing. Test ascents, descents, diagonal corners, narrow passages, and bridge clearance independently of the production map model. This can matter before any second weapon is acquired; the official-score effect is unmeasured.

**B. Schedule shared aim and weapon actions together.** At [src/combat.rs:561](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/combat.rs:561) and `:633`, gun and mortar firing are validated against their individual hypothetical aim directions. The final common aim is chosen later at `:646`; laser charging can win that choice. The compiled probe produced `fire=[true,true]`, selected yaw 0.00975, and gun-required yaw 0.07748 for a target 100 m away moving laterally at 19.5 m/s. The intended gun intercept and selected ray differ laterally by 6.789 m.

Select the final aim and predict actual torso slew for both simulation ticks before validating each shot. Allow gun tracking during laser charge, then transfer aim in time for release. Preserve useful simultaneous firing when aim solutions are compatible. Merely disabling every mismatched gun shot can improve accuracy while reducing total damage; measure damage per exposure and kill time too. Gun-only opening behavior should remain effective. This defect cannot explain losses occurring before the second weapon.

**C. Make laser release and retreat explicit states.** `fire=false` releases a charged laser; it is not a harmless cancellation. A generic fire-validity mask would create misfires. The no-target return at [src/combat.rs:395](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/combat.rs:395) supplies false triggers and no aim; lost targets and regen filtering therefore need deliberate hold/release behavior. Model idle, charging, holding, and release, including the automatic discharge deadline. At [src/brain.rs:332](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/brain.rs:332), regen retreat is not included in urgent movement, so a wounded escape can retain laser slowdown. Compare the value of a pending shot against reaching cover alive.

**D. Remove the unsupported lethal-shot assumption.** [src/brain.rs:219](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/brain.rs:219) suppresses laser ducking when any own charge is at least 0.50 seconds and the lowest visible enemy HP is at most 360. Base damage at 0.50 seconds is only 246.75; even a full unmodified shot deals 315 before armor against a non-mortar target. The check also does not bind the low-HP enemy to the selected target. Replace it with a conservative selected-target damage/readiness check, or remove the override in an ablation. A zero-cube, 0.50-second-charge, 350-HP enemy must not justify ignoring an incoming beam.

**E. Distinguish drop waiting from failed travel.** [src/brain.rs:84](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/brain.rs:84) blacklists DROP goals after 85 simulation ticks without a distance improvement greater than 4 m, approximately 4.3 seconds. `:804` allows arriving up to 12 seconds before landing. The compiled probe started at the drop at t=87 with eight seconds until landing and switched from DROP to PATROL at t=91.4, with 3.6 seconds still remaining. It blacklists the area for 28 seconds. Track travel, waiting, own capture, and contest separately. Preserve a deadline based on zone safety and enemy approach; do not wait indefinitely in a contested ring.

**F. Normalize live geometry once.** [src/brain.rs:1012](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/brain.rs:1012) multiplies reported radius by cube growth again; the observation contract describes current stats, and the local simulator already scales radius. Navigation uses fixed body dimensions, while cover checks use a single fixed-height point. Validate actual observed dimensions, remove duplicate scaling, and test body exposure and clearance at 0/5/10/20 cubes. Fixes should not reject valid paths or trust a head-hidden position with an exposed body.

Implement these independently so score changes can be attributed. Prioritize navigation and whichever combat defects appear in the loss traces; drop and large-body work remain valuable but may affect fewer games at the current baseline.

### 3 Run focused practice policy experiments

**Opening acquisition:** choose the best reachable secondary weapon using travel time, observed enemy exposure, elevation, and energy required to escape afterward. Compare current pickup behavior against a larger safe search radius, first-laser preference, and taking a nearby non-laser rather than waiting unarmed. Do not blindly sprint 240 m because a competitor permits that search radius. Current code already has some pickup priority before combat; improve the unsafe/missed cases instead of replacing it with another unconditional rule.

**Sustainable hunter combat:** separate voluntary pursuit from return fire. Gate closing distance, flanking, and elective engagements using estimated incoming damage, available cover, matchup, and recovery opportunity. Keep useful shots while already exposed or escaping. Test target persistence and low-HP finishing against current rapid rescoring. Measure damage received per hunter eliminated and HP/energy available for the next encounter.

**Economy during combat:** compare safe nearby cube/kill-loot collection against maintaining aim/range. Enemy-first target selection ([src/combat.rs:290](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/combat.rs:290)) and macro combat priority ([src/brain.rs:698](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/brain.rs:698)) can crowd out crates, drops, and kill-loot, although immediate pickups and some cubes already take precedence. Use a risk-adjusted resource value, including upgrade thresholds. Beyond ten cubes, HP/damage scaling caps while body growth continues: do not assume more farming is always better.

**Recovery:** require evidence that a retreat obtains hard cover and enough damage-free time to heal. Compare current peek/roof behavior with ground-level hunter fighting and clean cover retreats. Reject endless retreat/re-engage loops, firing suppression while exposed, and laser charging that prevents escape. Do not erase effective defensive shooting simply to keep the regen timer clean.

**Zone rotation:** replace automatic next-center routing ([src/brain.rs:504](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/brain.rs:504)) with candidate safe destinations evaluated at arrival time. Compare path ETA, zone damage, enemy exposure, and available cover. Urgent rotation currently overrides duck/regen and disables lightning repulsion. Retain tactical escape choices while giving true zone deadlines enough weight. A safer-looking path that incurs more lethal zone damage is a regression.

### 4 Tune FFA independently

The current mode heuristic ([src/world.rs:179](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/src/world.rs:179), `:264`, `:348`) can mistake zone/NPC deaths or a beam passing another mech for inter-mech combat, then retain that conclusion. With little evidence it defaults to practice. Only a small portion of strategy currently changes with this flag.

Maintain explicit practice and FFA configurations for controlled evaluation; where runtime mode is not provided by the protocol, use confidence-based evidence and a safe fallback. Do not infer hidden match seeds from `hello.seed`, which is a slot-specific bot RNG seed.

Test FFA engagement avoidance, third-party finishes, lower-exposure rotations, and canopy concealment separately. Normal unit-length walking at about 11.7 m/s is not silent: the sound threshold is 5 m/s. A real quiet option needs fractional movement plus braking/inertia handling. Trees conceal vision but do not stop shots. Practice and FFA gains must be reported separately.

### 5 Tune parameters only after the policies and evaluation are credible

Use small parameter groups: acquisition risk/energy reserve, target persistence, retreat thresholds, preferred ranges, and upgrade utility. Start with controlled sweeps or random search; use a more complex optimizer only if evaluation is stable and cheap enough. Include several opponent families, spawn positions, loadouts, and held-out seeds. Do not tune the whole policy against the same 48 local seeds repeatedly.

Derive loadout and upgrade utility from observed damage, exposure, recovery, and encounter distribution. The present fixed rankings should be revisited after shared-aim repair because the current scheduler can distort weapon-pair performance. Keep Rust and the existing protocol layer unless measured constraints identify a reason to change them.

## What public competitors actually support

Scores below are team standings at the saved snapshot, not a verified score for the linked source commit. README/team attribution supports the associations; public data does not identify the exact scoring commit or isolate which feature caused a result.

| Repository and inspected source | Language | Useful mechanism or evidence | Performance attribution |
| --- | --- | --- | --- |
| [charliescull/mech-royale-bot](https://github.com/charliescull/mech-royale-bot/blob/25ff2d949128dcc1520372d8b37304318d84ae7c/README.md) | Python brain, Go and TypeScript practice drivers | Current configuration selects Giddy for practice and its own FFA brain | README identifies 2.3%; official team score 5.60 |
| [justnau/giddygiddyboy-mech-royale](https://github.com/justnau/giddygiddyboy-mech-royale/blob/d03d32e030849f94dc7fb2f006041e8fb6a6d747/mech/brain.go#L1905) | Go | Squad parameters disable Ghost/Peek, encourage arming, target persistence, finishing, and combat loot | Matching team/name; official score 5.56 |
| [niranjankumarnk/mech-royale](https://github.com/niranjankumarnk/mech-royale/blob/1d04c65c73f4d0dcaf8735951024aaf00b9d753d/README.md) | Rust | Distinct hunter/FFA behavior; author reports worse practice results from rooftop and long-range fighting | README identifies BingBU; official score 4.58 |
| [jklein07/mech-royale-bot](https://github.com/jklein07/mech-royale-bot/blob/4fb47fe5e800a93bc0a089477d63ff2c4374c09b/README.md) | Rust | Private-room runner and spectator-feed metrics; approximate simulator | README identifies fortnite pirate token; official score 4.19 |
| [LanceHuang245/mech-royale](https://github.com/LanceHuang245/mech-royale/blob/6c321c52758e710d0d65bf151406c47a484f7cec/README.md) | Python | Paired FFA ablations, policy flags, explicit simulator/official discrepancies | Author-reported experiments; no verified team mapping |
| [enkyuan/aquarius](https://github.com/enkyuan/aquarius/blob/a6b3cd27a698c8bc6ccf2444a3806027b86e3837/bot/opening.py#L49) | Python | Full-body opening lanes, acceleration/braking and escape-energy checks, duel harness | No verified official score mapping |
| [Meows3s/mechfight-cpp](https://github.com/Meows3s/mechfight-cpp/commit/47fc9ebc30108bff97709f70ea612715a211b78a) | C++17 | Independent map/ballistics checks; documented overstrict fire-gating failure | Trace counts are author reports, not official improvement evidence |
| [varchaskukreja/TTUN](https://github.com/varchaskukreja/TTUN/blob/596f339454aa7e0c407618299315b7dc57b678a6/mech-royale/README.md) | TypeScript/Bun | Mode detection, compatible weapon use, FFA cover handling | Team attribution not independently established |
| [Warrior-hound/mech-royale](https://github.com/Warrior-hound/mech-royale/blob/c35061924570055e45fabd5bd11c2ba4740927b5/README.md) | Python | Focused combat/navigation scenarios; fire-suppression ablation | Approximate-simulator reports, no verified official mapping |

The C++ fire-gating example is a useful counterexample to blanket passivity: its author reports 87 withheld shots among 112 geometrically open, in-range opportunities. The fix kept movement gating while removing that trigger gate. Use the lesson to distinguish avoiding a fight from refusing to shoot while already in one.

Giddy's parameters form a coupled policy. Disabling peeking, extending combat range, increasing loot radius, and finishing weak hunters should be separate ablations in this bot before combinations are tested. Its Go implementation and the higher-scoring Python-wrapper team provide no evidence that a Rust rewrite or language migration would help.

## Experiment and promotion protocol

1. **Correctness:** add targeted fixtures for each defect, including negative cases. Laser tests must cover target loss, slot order, auto-release, slew over both ticks, and gun-only firing. Drop tests must cover early arrival, own capture, contest, unreachable travel, and zone interruption. Geometry tests must use an independent reference where possible.
2. **Protocol:** run current conformance under the actual runtime restrictions. Include worst-case tactical states when checking the 50 ms budget; the existing 300 observations do not cover every loadout/map state.
3. **Screening:** run baseline and candidate with matched seeds and slots in separate practice-like and FFA suites. Use the existing local simulator only as a labeled proxy. Its fixed seed generator `1000 + i × 7919` needs an explicit disjoint holdout mechanism before extensive tuning. Check deterministic repeatability and stable tie-breaking.
4. **Measurement:** primary result is mean placement points. Also record low-placement frequency, survival time, eliminations, arming time, damage, zone loss, and stalls. Report uncertainty from per-match paired differences where pairing is actually available. Preserve losses and their traces; do not cherry-pick favorable seeds.
5. **Official validation:** the current track permits four practice submissions per hour. Use an unchanged baseline, a best isolated candidate, a repeat/confirmation where needed, and a combined candidate or rollback test as the initial allocation. The current official track explicitly documents 48 private practice seeds fixed for the event; finals use new seeds. Fixed practice seeds do not remove runtime/deadline variance. Record the submitted commit, scorer version, timestamp, and all returned outcomes. If only an aggregate score is returned, do not invent a confidence interval from unavailable per-game data.
6. **Promotion:** keep a correctness repair after its invariant tests pass, but measure its gameplay tradeoffs. Promote a strategic policy only when improvement survives held-out tests and official validation. If a result is ambiguous, collect more evidence. Require a combined candidate to retain the individual gains; rollback interacting changes that lose them.
7. **Repeat by remaining loss cause:** after each accepted candidate, rebuild the failure taxonomy. Give the next agent wave the largest unresolved cause, a counterexample task, and independent competitor/evaluation review. Stop adding complexity when a change has no demonstrated benefit.

First implementation batch: evaluation manifest and traces; directed traversal repair; shared aim plus explicit laser-state regression tests; then the false-lethal override and drop waiting fix as separate changes. Reorder within that batch if actual loss traces show one defect dominates. First strategic batch: safe secondary acquisition, persistent recoverable hunter fights, and nearby combat-loot collection. Defer elaborate roof, stealth, and optimizer work until these experiments establish a stronger official baseline.

## Reproducing the research checks

Run the README Docker build and conformance command, retaining all match restrictions. The research images built for this pass are `mechbot:research-5b783d1` and `mechbot:research-builder-5b783d1`.

Run Rust tests in the builder image with `kit/` mounted read-only at `/build/kit`, then `cargo test --locked --offline -- --nocapture`.

`behavior_probes.rs` is a standalone diagnostic, not a Cargo test or a bot change. Mount it at `/input/probe.rs`, the existing ignored `sim/hello_config.json` at `/input/hello_config.json`, and `kit/arena` at `/input/arena`, all read-only, in the builder image with networking off. The optional map mount enables the navigation probe; do not mount it at `/arena`, because that would also activate the fallback map for the isolated drop probe. Compile and run inside that disposable container:

```sh
rustc --edition=2021 -C panic=abort -C lto=thin /input/probe.rs \
  --extern mechbot=target/release/libmechbot.rlib \
  --extern serde_json=$(find target/release/deps -name 'libserde_json-*.rlib' | head -n 1) \
  -L dependency=target/release/deps -o /tmp/probe
/tmp/probe
```

Pass `--baseline` to the probe executable when using the preserved baseline builder; omit it with `mechbot:candidate-builder` to assert repaired behavior. The ignored kit/config must be provisioned separately on a new checkout. Production changes and subsequent validation are documented in the implementation follow-up. No competition submission has been made.
