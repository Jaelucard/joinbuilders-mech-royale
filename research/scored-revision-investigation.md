# Investigation of the two officially scored revisions

The user has now identified both sources: **4.81 belongs to v3 `411af5f530934aa21d8cb5dfff35259219595159`**, while **BB-488640B1 / 4.65 belongs to v4 `5b783d19f138239033be32b527318f9247a9f9b8`**. These are user-confirmed submission associations; official submission rows, scorer versions and per-game results have not been independently retrieved. The later local candidate was not involved in either run.

## Actual code change

The diff contains four files: README, bot-name strings, one brain ground-height lookup, and map code. Combat, weapons, supply drops, inventory and the navigator are unchanged. Two meaningful changes remain:

1. V4 replaces selected bilinear height queries with a cached height sampled at the center of a 2 m cell. V3's existing four-sample lookup was already O(1); this is a constant-factor optimization with reduced spatial precision, not an asymptotic improvement.
2. V4 adds a local canopy-density preference to cover selection. It does not add canopy to hard collision or ballistic protection.

The **terrain precision change is the stronger mechanistic suspect**, because it demonstrably changes physical-clearance and grounded decisions. It is not yet a proven cause of the official score difference. Canopy remains a separate policy risk.

## Reproduced terrain differences

A compiled Rust probe runs the preserved v4 map implementation against the exact extracted v3 collision method, holding the real arena constant. A deterministic scan covers 742,712 offset grid points; this is a map audit, not a gameplay-frequency sample.

- At radius 1.8 m, 584 sampled points changed from v3-clear to v4-blocked, and 787 changed in the other direction. Neither method is thereby proven to equal the official collision engine.
- A witness at approximately `(-300.243,-14.819)` is clear under v3 and blocked under v4. One nearby sample is -4.763 m using the interpolated query but +3.276 m using the cached cell center.
- At `(-261.243,-11.819,23.4)`, both collision methods allow the body, but cached ground is 20.874 m instead of 23.4 m. V4's brain therefore skips the existing 14-point penalty for moving through a zero-clearance navigation cell because it incorrectly appears more than 1.5 m above the cached ground relative to v3's model.
- Typical errors are much smaller than the extreme examples: mean absolute height difference is about 0.068 m, with a 99th percentile around 0.590 m. Near sharp terrain transitions the maximum sampled difference is 36.203 m. These statistics describe this sampling grid only.

[Terrain evidence and source](v3-v4-terrain-audit/results.json), [reproduction runner](v3-v4-terrain-audit/run.py). Three executable audit tests passed, including a uniform 45-degree ramp that v4 blocks but v3 permits, and a steeper ramp with the opposite result. The behavior change is therefore not dependent solely on complex arena geometry.

## Reproduced canopy difference

The tree schema is valid for all 1,560 trees. V4 adds up to 3.5 points to a cover candidate when threats are farther than 46 m. This is an additive preference, not only an exact-score tie breaker.

With v4 terrain held constant, a real-map witness moves the chosen destination 4 m toward denser canopy while both positions remain exposed under the unchanged hard-LOS check. The destination is 59 m from the threat, inside the documented upgraded proximity-sensor range of 60 m. The heuristic's fixed 46 m threshold does not account for that upgrade, direction through foliage, or tracking memory. This establishes a changed choice and a limitation, not a harmful official outcome.

[Canopy report](v3-v4-canopy-audit/report.md), [paired chooser results](v3-v4-canopy-audit/result.json).

## Controlled local ablation

The old simulator imports production map code for movement, so simply swapping v3/v4 would also change the simulated environment. The new [comparison runner](compare_v3_v4.py) instead compiles an independent frozen copy of v4 map code for the environment and NPCs. Only the player's bot code varies. The same protocol types, simulated opponents, configuration, arena and 48 practice seeds are used throughout. Generated source, environment, simulator manifest and lock hashes are recorded; comparison tooling now rejects mismatched frozen environments.

| Player variant | Mean local points | Difference from v3 | Paired proxy 95% interval |
| --- | ---: | ---: | --- |
| Original v3 | 5.417 | -- | -- |
| V4 terrain change only; canopy preference disabled | 5.229 | -0.188 | [-0.604, +0.125] |
| V4 canopy preference with v3 terrain queries restored | 5.521 | +0.104 | [-0.521, +0.750] |
| Original v4 | 5.313 | -0.104 | [-0.833, +0.646] |

These are **approximate simulator results, not official scores**. Every interval includes no difference. Uncontrolled HashMap iteration can affect repeated runs. The simulator does not model official optical canopy visibility and has an overly informative practice-hunter fallback, so canopy value is especially poorly represented. The preserved capped survival metric was not used as evidence. The observed directions support prioritizing a terrain rollback experiment, but cannot attribute the official 0.16-point difference.

[Comparison data](evaluation/scored-comparison.json), [chart](evaluation/scored-comparison.html). Each `scored-*.json` and log retains input hashes and every match. Reproduce with:

```sh
python research/compare_v3_v4.py v3
python research/compare_v3_v4.py v4
python research/compare_v3_v4.py terrain-only
python research/compare_v3_v4.py canopy-only
```

## Preserved baseline and next experiment

V3 is preserved on `baseline/scoring-411af5f`, with images `mechbot:scored-v3` and `mechbot:scored-v3-builder` built from the exact Git snapshot. Its four existing tests pass. Docker conformance passes 300/300 observations with no late, stale, wrong-tick or invalid answers; observed p99 is 1.6 ms, maximum 1.8 ms. This short trace is not a worst-case runtime proof. [Manifest](evaluation/scored-v3-manifest.json), [tests](evaluation/scored-v3-tests.log), [conformance](evaluation/scored-v3-conformance.log). All prior baseline and candidate images remain separate.

Use v3 as the known best comparison baseline. The next isolated official experiment should restore v3 terrain queries in v4 while retaining canopy, keeping every other policy unchanged. A separate canopy-off trial can follow if needed. Those component variants already have local diagnostic results; none has an official score. Compare explicit submitted SHAs under the same scorer version, with an unchanged v3 repeat if the submission budget allows. Do not fold in the later navigation/combat/drop bundle during this attribution test.

No production source was changed, no branch was pushed, and no submission was made in this investigation. The remaining limitation is official match-level/scorer evidence, not an unknown baseline commit.
