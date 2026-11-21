# Completed experiment campaign: no candidate qualified for a push

The 64-configuration screen and both prospective validation runs are complete. The selected candidate passes regression tests and Docker conformance, but **fails the predeclared performance gate in both validation runs**. Nothing was pushed. Production source in this checkout remains the previously recorded local R2 candidate; the new finalist exists as reproducible source transformations and preserved Docker images, not as a promoted source revision.

The official comparison baseline remains **4.81 at `411af5f530934aa21d8cb5dfff35259219595159`**, as confirmed by the user. The user associated **4.65 / BB-488640B1** with `5b783d19f138239033be32b527318f9247a9f9b8`. No new official score was obtained. Local simulator points below must not be added to 4.81 or interpreted as an official score prediction.

## Decision and fresh results

The single frozen finalist is `n-safe_c-window_b-drop_threat`:

- V3 interpolated terrain queries, directed traversal checks and actual-radius near-wall recovery; no canopy preference.
- Shared weapon aim, explicit lost-target laser release, and conservative mortar scheduling within the gun's cooldown window.
- Landing/capture-aware supply-drop waiting, with the unsupported lethal-laser exception removed. Emergency ducking uses existing enemy aim/recent-damage evidence.

| Run and mode | Baseline mean | Candidate mean | Paired gain | Paired bootstrap 95% interval |
| --- | ---: | ---: | ---: | --- |
| Fresh validation, practice | 3.135 | 3.469 | +0.333 | [-0.323, +1.000] |
| Process repeat, practice | 3.052 | 3.448 | +0.396 | [-0.271, +1.052] |
| Fresh validation, FFA | 13.031 | 13.365 | +0.333 | [-0.198, +0.906] |
| Process repeat, FFA | 13.042 | 13.365 | +0.323 | [-0.208, +0.896] |

Each row uses 96 matched seeds. The second run repeats the **same 96 scenarios in new processes**; it is not another 96 independent scenarios. Practice outcomes were better/equal/worse on 30/48/18 seeds in each run. Bootstrap intervals use 10,000 resamples of paired seed differences, RNG seed 481.

The gate required, in **each** repeat, at least +0.5 mean practice points, a practice interval strictly above zero, an observed FFA decline no greater than 0.25, passing regression tests and passing Docker conformance. The practice magnitude and uncertainty requirements both fail. FFA, tests and conformance pass their checks. The result is insufficient evidence for a significant local improvement, rather than proof that the candidate is worse.

[Machine-readable gate](gate.json), [comparison chart](comparison.html), [selected source/image manifest](selected-manifest.json).

## Scope and selection controls

The [plan](plan.json) was committed as `8cdb1d8` before screening. Source transformations were frozen in [freeze.json](freeze.json). Selection and source/environment hashes were committed as `17c535d` before prospective validation.

The full factorial screen crossed:

| Component | Options |
| --- | --- |
| Navigation | baseline, safe traversal, canopy with v3 terrain, safe traversal plus canopy |
| Combat | baseline, shared aim, cooldown window, bounded immediate mortar shot |
| Brain | baseline, drop waiting, credible laser threat, both |

This is 4 x 4 x 4 = **64 configurations**, each on 48 shared practice seeds (indices 0-47). Six combinations were eligible for promotion: safe navigation with or without canopy, any of the three repaired combat versions, and both brain repairs. The other 58 were diagnostic controls. This eligibility rule preserves all the user's requested correctness fixes; incomplete controls could not become the submission candidate.

The highest eligible screening mean won; ties use fewer changed component families, then stable identifier. The chosen candidate tied safe-canopy/shared/drop-threat at 5.438 and won the stable-identifier tie break. Baseline screening mean was 4.396, giving +1.042 with interval [-0.188, +2.354]. These are selection results, not independent evidence. [All screening means and eligible ranking](selection.json).

Validation used previously unused seed indices 72-167, with seed `1000 + index * 7919`. Only the frozen finalist and baseline were evaluated. No source retuning, replacement winner or threshold changes followed holdout inspection. There were **3,840 local matches**: 3,072 screening matches plus 768 validation/repeat matches across two bots and two modes. A one-match smoke run is excluded from that count.

An independent pre-validation review added baseline-source, arena and builder-image bindings, pre/post arena checks, and immutable-image conformance invocation. `freeze.json.validation_hardening` records the driver hash change and reason. These were verification changes before selection/holdout; they did not change the already-running screening snapshots, source options, simulation model or criteria. All saved screening and validation records match the selected arena, harness and builder image.

## Model and verification

Every variant used an independent frozen v3 map module for simulated movement/NPC geometry. Player code varies separately. Before screening, the shared harness was corrected to intersect a shot segment with a vertical cylinder analytically and use the actual body radius for movement instead of 75% of it. Three geometry regression tests pass, including a previously missed slanted hit and true misses. These corrections change the model, so old proxy means from earlier reports are not directly comparable with this campaign.

Source transformations are independently reviewable in [navigation.py](navigation.py), [combat.py](combat.py) and [brain.py](brain.py). Their separate verification records are in [navigation](navigation/README.md), [combat](combat/README.md) and [brain](brain/README.md). Negative-control tests establish that the threat witnesses fail on the legacy behavior.

The selected build passes **52 existing and regression tests** (48 library, 4 binary). An independent cooldown witness confirms gun return fire at 130 m and 160 m and retains the useful immediate mortar shot at 100 m. [Full test log](selected-tests.log), [cooldown witness](selected-cooldown-witness.log).

Docker conformance passes **300/300 observations**, with zero late, stale, wrong-tick or malformed answers and zero debug-cap violations. Observed p99 response time was 1.9 ms and maximum 2.8 ms. The container used one CPU, 512 MB, no network, a read-only filesystem, dropped capabilities and UID 65534. This short trace is not a worst-case runtime guarantee. [Conformance log](selected-conformance.log).

The exact selected runtime image is `sha256:1d1eceb6941c9d510d24c72c9f3b5718958b56980b69b843a567b1378b2c5f82` (`mechbot:experiment-selected`). Its builder, all eight source hashes and baseline commit are recorded in [selected-manifest.json](selected-manifest.json). The scored v3/v4 images and baseline branches remain preserved. No finalist official score exists.

## Regressions, interactions and next experiment

The finalist's positive mean contains substantial repeatable losses. Three examples, identical in both process runs:

| Seed | Baseline -> candidate points | Observation |
| --- | --- | --- |
| 1085903 | 15 -> 2 | Baseline finishes with gun+laser; candidate with gun only and dies to gunfire. |
| 689953 | 9 -> 1 | Both finish with gun+laser; candidate dies much earlier in placement. |
| 737467 | 15 -> 7 | Baseline survives; candidate dies to lightning. |

These summaries identify replay targets, not causes. End-state equipment does not establish when equipment was acquired or why a drop was lost. [All consistent losses](diagnostics.json) retains the per-run outcomes.

Screening also shows that fixes are not uniformly additive. Averaged over all other screened options, drop-only scores 4.717 versus brain-baseline 4.850, while drop-plus-threat scores 5.074. Combat window averages 5.038, shared aim 4.815, and the relaxed shot variant 4.917. These are descriptive marginal means on reused screening seeds, with interactions and uncontrolled process iteration; they do not independently establish a policy effect or justify switching candidates.

The next useful local experiment is **decision-trace diagnosis on the repeatable losing seeds**, starting with drop acquisition and exposure on 1085903, then the gunfire and lightning losses. Compare movement targets, time in cover, drop ownership, loadout acquisition and firing decisions before changing policy. Any resulting candidate requires a newly preregistered evaluation on untouched seeds; the present holdout is now diagnostic data.

The simulator still lacks official optical canopy visibility, has an overly informative hidden-player fallback, and approximates combat/physics. Unseeded HashMap iteration can change repeated results. It does not reproduce the official private practice suite. These limits prevent attributing the official 4.81 -> 4.65 drop or guaranteeing a future official gain. The [scored-revision investigation](../scored-revision-investigation.md) remains the evidence for the separate terrain/canopy attribution question. An official terrain-only rollback trial would test that question more directly than this bundled finalist.

## Reproduction and artifact handling

`run_suite.py` composes immutable Git snapshots and validates cached runs against source, harness, arena and builder hashes. It runs simulations in disposable Docker containers; it does not modify production source. The saved results include every match and input hash.

```sh
python research/experiments/run_suite.py screen --workers 3
# selection.json is already frozen; do not delete it or select again.
python research/experiments/run_suite.py validation --workers 2
python research/experiments/run_suite.py repeat --workers 2
```

These commands validate and reuse matching existing records. To collect genuinely new evidence, create a separate campaign with a new plan and output namespace rather than deleting this campaign's records. `validate_selected.py` builds/tests the exact finalist and `validate_selected.py --conformance` checks the manifest's immutable runtime image; rerunning them overwrites their logs/manifest. `assess.py gate` recomputes the score gate and overwrites `gate.json`; its output is the scoring decision only, so the separately recorded test/conformance fields must be retained when preparing a new combined report.

**Publication decision: no push.** The conditional authorization requires a significant improvement; this campaign did not establish one.
