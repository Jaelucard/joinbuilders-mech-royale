# Scoring control candidate results

**Follow-up investigation:** the user reported BB-488640B1 at 4.65 and said it used the latest GitHub push. A fresh remote check found only `master` at `5b783d1`; the candidate below was still local. The actual submission row has not been independently inspected, so 4.65 must not be recorded as a candidate result. See [regression investigation](regression-investigation.md) for provenance, additional candidate risks, and simulator limitations. Those findings supersede the earlier readiness recommendation below pending targeted follow-up.

The requested control fixes are implemented on `fix/scoring-control`. The user's corrected official baseline is **4.81**; its exact submission commit is still unverified. The local baseline is preserved at `5b783d19f138239033be32b527318f9247a9f9b8` on `baseline/scoring-5b783d1`, with its original runtime and builder images.

The candidate fixes reproducible defects and passes all validation. It has **no measured official score**. Across 48 matched seeds per mode, its approximate practice mean was unchanged and its approximate FFA mean was higher, with uncertainty still spanning no aggregate improvement. It is ready for an official evaluation as a correctness candidate, not a proven replacement for the best-scoring official submission.

## Independently reviewable changes

| Commit | Scope | Result |
| --- | --- | --- |
| `b5c030e` | Navigation | A*, shortcuts and smoothing share directed slope/corner checks. Close goals and waypoint advancement cannot bypass them. Blocked raster starts receive bounded physical-clearance recovery. |
| `4c3d13a` | Combat | Shared torso aim schedules compatible weapon use; non-laser fire is checked at the appropriate first/second simulation tick. Gun tracks during early laser charge; laser owns release preparation. Lost targets cause explicit release at current aim rather than patrol aim. Gun-only behavior and legacy tolerances are preserved. |
| `7b2f154` | Brain | Drop landing/capture is separate from travel-stall detection; recapture survives a brief contest, while inactivity and an absolute deadline bound repeated resets. The unsupported lethal-laser duck exception is removed. Close ground movement uses navigation. |

The production-source revision tested is `7b2f154`; subsequent report commits do not change bot behavior. No dependencies were added. No code was pushed or competition submission made.

## Validation

**37 Rust tests pass:** four existing tests plus 33 regressions covering navigation, combat and brain behavior. Real-arena tests ran with the provided kit mounted. [Full test output](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/candidate-tests.log).

**Docker conformance passes:** 300/300 observations on time, zero bad lines, wrong ticks, stale answers, or debug-cap violations. The run used no network, one CPU, 512 MB, no swap, no capabilities, read-only storage and uid 65534. Observed p99 was 7.7 ms and maximum 8.1 ms against the 50 ms budget. This recorded stream does not cover every possible tactical state. [Conformance output](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/candidate-conformance.log).

The same standalone scenarios were compiled against both baseline and candidate:

| Scenario | Baseline | Candidate |
| --- | --- | --- |
| Gun plus charging laser; target 100 m away moving laterally at 19.5 m/s | Gun fires with 6.789 m lateral aim mismatch | Gun still fires; selected aim matches its lead, zero mismatch in the fixture |
| Arrive at drop at t=87; landing at t=95 | Abandons at t=91.4 | Waits through landing and capture |
| Real-arena 5.476592 m uphill transition over 4 m | Direct shortcut crosses rejected A* edge | Shortcut rejected; reachable detour returned |

The tests also check nonlethal laser ducking, lost-target releases, minimum-charge releases, slot order, gun reload/cooldown windows, laser shorts, corner occlusion, contested capture resets, stale countdowns, zone escape and invalid navigation endpoints. [Baseline probe output](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/baseline-probes.log), [candidate probe output](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/candidate-probes.log), [probe source](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/behavior_probes.rs).

## Matched local simulation results

These are **approximate local simulator results, not official scores or forecasts**. The simulator uses the same map code as the bot, approximate collision/weapon physics, and its own house-bot implementation. No opponent code or simulator physics was changed for these comparisons. The harness only selects seed indices and emits per-match records.

The follow-up audit also found that the proxy reports 480 seconds for every surviving player, including early match finishes, and prints `WIN` for any survivor even if timeout HP ranking places it below first. Archived survival-time means are therefore not actual elapsed survival measurements. Placement points are computed separately; this reporting defect alone does not invalidate their arithmetic. Raw historical outputs are retained unchanged.

Development seeds were `1000 + i × 7919` for i=0–23. A separate holdout used i=24–47. Each seed supplies the same scenario/slot generator to baseline and candidate. The candidate was revised using development results before the holdout was run; no changes were made in response to holdout outcomes.

| Suite | Mode | Games per version | Baseline mean | Candidate mean | Paired difference |
| --- | --- | ---: | ---: | ---: | ---: |
| Development | Practice proxy | 24 | 7.042 | 7.042 | 0.000 |
| Holdout | Practice proxy | 24 | 3.750 | 3.750 | 0.000 |
| Development | FFA proxy | 24 | 14.292 | 14.000 | -0.292 |
| Holdout | FFA proxy | 24 | 12.375 | 13.583 | +1.208 |
| Combined | Practice proxy | 48 | 5.396 | 5.396 | 0.000 |
| Combined | FFA proxy | 48 | 13.333 | 13.792 | +0.458 |

Paired bootstrap 95% intervals for the combined differences are approximately **[-1.146, +1.125]** in practice and **[-0.125, +1.188]** in FFA. These describe proxy sampling variation, not official-score uncertainty. The aggregate results do not establish a reliable score improvement. The development FFA decline remains a tradeoff to inspect in real games.

Across the 48 practice pairs, 12 improved, 21 tied and 15 worsened. Across FFA pairs, six improved, 38 tied and four worsened. An eight-seed repeat in each mode reproduced baseline placements exactly, but standard HashMap iteration is not explicitly seed-controlled, so determinism is not guaranteed for all scenarios.

Files: [development comparison](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/final-development.json), [holdout comparison](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/final-holdout.json), [combined comparison](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/final-combined.json). The corresponding `baseline`, `candidate-final`, `baseline-holdout`, and `candidate-holdout` JSON/log files contain every match outcome and input hashes.

[Comparison chart](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/comparison.html).

## Regressions found and removed

Isolated initial navigation changes were approximately neutral in practice (7.042 to 7.167 over 24 seeds). Initial combat changes reduced the practice mean to 5.125, and the initial brain changes reduced it to 4.500. The initial combined candidate scored 5.417. Correctness tests alone would have missed these gameplay effects.

Two broader policy changes were narrowed before final evaluation:

- Holding a charge after losing its target retained half-speed movement and fixed torso orientation. The final version releases at the current aim, preserving baseline discharge timing while preventing accidental patrol-direction release.
- Treating all regen mode as urgent movement suppressed laser charging even while still exposed. The final version preserves defensive laser use in that state; actual zone/duck escapes still request urgent release.

An independent review also found that retaining the highest capture progress across resets could abandon a nearly complete recapture. The final implementation tracks progress within the current attempt and uses a separate absolute deadline to prevent indefinite contest loops. Both resumed capture and repeated resets have regression tests.

The initial ablations and their input hashes remain in the evaluation directory. They apply to the initial experimental source, not the final source. Reproduction commands below evaluate the final candidate. Do not select whichever individual seed happens to favor a change or use higher hit percentage as a substitute for placement.

## Reproduction and submission notes

The ignored `kit/` and `sim/` directories must be provisioned separately on a fresh checkout. Neither is included in the submission image. The preserved baseline image is `mechbot:research-5b783d1`; its builder is `mechbot:research-builder-5b783d1`. Candidate images are `mechbot:candidate` and `mechbot:candidate-builder`. Exact image IDs and source hashes are saved in the [baseline manifest](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/baseline-manifest.json) and [candidate manifest](C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/evaluation/candidate-manifest.json).

With the preserved builder available, run from the repository root:

```sh
python research/run_proxy.py baseline --matches 24 --label baseline-recheck
python research/run_proxy.py candidate --matches 24 --label candidate-recheck
python research/run_proxy.py baseline --matches 24 --start-index 24 --label baseline-holdout-recheck
python research/run_proxy.py candidate --matches 24 --start-index 24 --label candidate-holdout-recheck
python research/analyze_proxy.py research/evaluation/baseline-recheck.json research/evaluation/candidate-recheck.json
```

The evaluator snapshots candidate source into disposable containers and records source/config/map hashes. It never edits production source. To rebuild the baseline builder on a new machine, build the `builder` target from the preserved baseline revision. The optional `--source-image` flag can use source from an available preserved builder rather than the working tree. Historical treatment flags require source matching their recorded experimental variant.

The next high-value experiment is an **official baseline/candidate comparison**, with the exact submitted revision recorded. Publish the candidate branch before giving its repository revision to the scorer. Re-run the known 4.81 submission if its commit can be identified, then evaluate this candidate and collect every available per-game result/replay. Compare placement, laser encounter outcomes, navigation stalls, and successful drop captures; do not infer official improvement from the local FFA gain.

Remaining risks: fixed-size navigation clearance and approximate geometry; more frequent ducking can forgo a genuinely lethal finish; a held charge behind an occluded but still selected target retains the game's normal slowdown; and the local evaluator does not reproduce official opponent behavior exactly. A future escape-specific charge policy or conservative lethal-shot proof should be a separate experiment, rather than reintroducing broad suppression or the old 360-HP assumption.
