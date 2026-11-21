# BB-488640B1 regression investigation

**Provenance resolved:** the user identified 4.81 as v3 `411af5f` and 4.65 as v4 `5b783d1`. The [scored-revision investigation](scored-revision-investigation.md) supersedes the earlier missing-baseline uncertainty below and examines the actual scored code delta. The historical candidate witnesses remain valid but did not cause either submitted run.

Follow-up: both executable candidate defects have been repaired and reviewed; [repair validation](repair-validation.md) records the changes and mixed held-out results. The original evidence and preserved candidate below remain historical records.

Investigated 21 November 2026. User-reported results: previous official best **4.81**, BB-488640B1 **4.65**. Follow-up: the user explicitly confirmed that `5b783d19f138239033be32b527318f9247a9f9b8` belongs to **4.65 - BB-488640B1**. The source revision for 4.81 and the submission scorer versions remain unknown.

## Main finding: the candidate was not published

A fresh fetch, `git ls-remote --symref origin HEAD`, `git ls-remote --heads origin`, and the independent GitHub commit API agree: the only published branch is `master`, at **`5b783d19f138239033be32b527318f9247a9f9b8`**, vanguard-v4, committed at 2026-11-21 07:37:11 UTC. [Published commit](https://github.com/Jaelucard/joinbuilders-mech-royale/commit/5b783d19f138239033be32b527318f9247a9f9b8).

The local `fix/scoring-control` branch was at `9c1283c` when this investigation started. Its production-source revision is `7b2f154`, following navigation `b5c030e` and combat `4c3d13a`. None of those candidate commits appeared on the remote. The earlier implementation report explicitly recorded that no push or official submission had been made.

The user's explicit SHA confirmation associates BB-488640B1 with v4, **not the local candidate**. The official submission row has not been accessed independently; this is user-confirmed provenance. The exact revision behind 4.81 is still unknown. Consequently, the reported difference cannot be attributed to the candidate fixes, and 4.65 must not populate `official_candidate_score`.

This establishes a submission-provenance mismatch, not the cause of the numerical difference between the two official runs. No bot changes, pushes, or submissions were made during this investigation.

## What official evidence establishes

The current [official track](https://www.joinbuilders.org/buildathon/tracks/builders) documents scorer `builders-3`, 48 private practice seeds fixed for the event, and a 50 ms action deadline. Late responses are skipped; the page explicitly allows score variation for bots close to the deadline. Different practice seed sets are therefore not a supported explanation. Timing variance remains possible, not demonstrated here.

The observed [submission UI bundle](https://www.joinbuilders.org/_next/static/immutable/chunks/2-3wrzk3s4-02.js) says a blank commit selects the newest default-branch commit and displays a submitted commit prefix alongside repository, receipt, score, message and scorer-version status. Anonymous public access does not expose the user's submission rows. [Public standings](https://www.joinbuilders.org/buildathon/standings/builders) expose best score/team/time/rank, not receipt-to-commit mappings or game-level losses. No public receipt or replay lookup was found in the observed UI.

Remaining explanations requiring evidence are a different previous source revision, a scorer-version difference, or same-source runtime/decision variation. The baseline uses randomly seeded HashMaps and lacks stable tie-breaking in some selections, but no decision trace connects this to either official result. Its archived 300-observation conformance run had zero late actions and a 3.8 ms maximum; that short trace does not establish performance across every official match.

## Independently reproduced risks in the local candidate

Three agents audited provenance, combat, and navigation/evaluation separately. The following witnesses ran against both preserved builder images, offline, in disposable containers. They diagnose candidate behavior; they do not explain BB-488640B1.

The archived fixtures were then rerun: all four container executions exited successfully. [Reproduction instructions and logs](regression-audit/README.md) include the exact builder image IDs. Reanalysis of the saved 48 matched pairs reproduced all prior comparison fields exactly, adding only the survival-metric warning. Production bot source, historical match data, and preserved images remain unchanged.

### Near-wall navigation can stop despite a clear escape

In a flat fixture, a wall starts at x=16. The mech starts at `[13.45,10,0]`, with goal `[10,10]`. Its actual radius of 2.5 m clears the wall, and all 101 sampled points toward the goal are clear. The conservative 2.65 m clearance rejects the starting point, whose raster cell is blocked.

- Baseline waypoint: `[10,10]`, toward the reachable goal.
- Candidate waypoint: `[13.45,10]`, the current position.

`nearby_nav_recovery` checks sample zero with the fixed conservative radius (`src/map.rs:1462`); every route fails before moving away from the wall. `next_waypoint` then returns the current position (`src/nav.rs:165`). Close goals and cover labels can suppress stuck recovery (`src/brain.rs:458-462`), potentially prolonging this stop. Its frequency in real games is unmeasured.

Both isolated tests passed their variant-specific assertions. [Fixture](regression-audit/navigation_start_clearance.rs). A fix should allow physically legal, progressively improving clearance at the start while retaining wall, slope and corner checks.

### Gun/mortar aim transfer can delay useful gun fire

With a stationary target at 130 m, stock turn rates, gun cooldown 0.14 s, mortar ready, and aim already on the gun solution:

- Baseline keeps gun aim and fires in the second decision's two-tick window.
- Candidate turns toward mortar, but sends neither fire flag because mortar is not aligned on its firing tick. On the next decision the ready gun retakes aim, and neither weapon fires while turning back. Gun fire occurs one decision (0.1 s) later; mortar never fired.

The same pattern reproduces at 160 m. At 100 m, the candidate successfully fires mortar then gun, so simply banning mortar is not appropriate. The priority transfer at `src/combat.rs:627` considers gun cooldown but not whether the other weapon can fire within the available window.

The fixture executes real Rust combat policy. Aim/cooldown progression is a minimal two-tick harness, not a full match or hit simulation. [Fixture](regression-audit/cooldown_witness.rs), [captured output](regression-audit/cooldown-output.txt). Keep shared-aim validation; gate aim transfer on an achievable shot or a deliberate bounded preparation interval.

### Laser ducking remains an interaction risk

Removing the unsupported lethal-laser assumption is correct, but it exposes a broad duck trigger: any visible charging enemy within 275 m and LOS can cause cover seeking, without proving that enemy is aiming at us. Ducking passes urgent movement into combat, releasing a partial/misaligned charge and suppressing restart. That can sacrifice a valid finish. Existing development diagnostics show a limited adverse signal; they do not justify restoring the false 360-HP kill assumption. A replacement needs selected-target damage, alignment and incoming-threat evidence.

## Why previous local validation was insufficient

The final 48-pair practice proxy had equal aggregate points while 15 individual seeds worsened. Aggregate neutrality and 37 passing correctness tests did not rule out these multi-step behavioral regressions. The final data do not show a universal laser-firing collapse; rejected experimental variants must not be conflated with the final candidate.

The proxy has additional fidelity defects independent of the candidate:

- `sim/src/main.rs:799-802` records 480 seconds for all survivors, including early match finishes. Its `WIN` log label can also include a surviving timeout finisher below first. Historical survival averages are not measured elapsed survival times. Placement arithmetic is computed separately and is not invalidated by this reporting defect alone.
- Its cylinder intersection helper checks only the point closest in XY (`:2283-2297`). For segment `(-5,0,0)` to `(5,0,20)` and a radius-2.5, height-9 cylinder at the origin, it rejects z=10 at the XY closest point, although `(-2,0,6)` lies inside the cylinder.
- Movement uses 75% body radius (`:1454`), while shot origins lack documented wall clipping (`:1529`; compare `kit/protocol.md:106`).
- Practice hunters fall back to the player's exact hidden current position when shared memory expires (`:1250-1255`), limiting stealth/route conclusions.

The ignored simulator is not submitted in the bot image. Historical raw results are retained unchanged, with their limitations recorded here and in the implementation report. Future analyzer outputs now warn about the survival metric.

## Next experiment and disposition

Keep the published v4 and both preserved builder/runtime images intact. Treat the candidate as **under investigation**, superseding its earlier readiness recommendation. Do not push it as a response to this score report.

1. Record the actual commit and scorer version displayed for both official runs. Until the previous SHA is identified, 4.81 remains the user's official baseline but not a verified score for local `5b783d1`.
2. Fix the two executable candidate regressions separately, retaining both witnesses as multi-step regression tests. Preserve safe navigation, useful gun fire and shared-aim validation. Keep laser-threat/finish policy as a separate experiment.
3. Compare each isolated repair and their combination on matched local scenarios, explicitly labeling the simulator's limits. Inspect losing seeds and route/weapon decisions, not survival averages or just the combined mean.
4. Publish only a tested revision intended for evaluation, and submit its explicit SHA. An official repeat of a verified baseline followed by one isolated candidate is the next informative score comparison. Record receipt, SHA and scorer version at submission time; do not treat local proxy improvements as official gains.

The actual cause of the 4.81-to-4.65 official difference remains unverified without those records. This investigation establishes the source mismatch and two reproducible candidate defects, rather than assigning unsupported causality.
