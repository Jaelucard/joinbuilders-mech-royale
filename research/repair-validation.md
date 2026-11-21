# Candidate repair validation

The user explicitly confirmed **BB-488640B1 = 4.65 at `5b783d19f138239033be32b527318f9247a9f9b8`**. This is published v4, not the local candidate. The reported best remains **4.81**, with its revision and scorer version still unknown. These facts do not establish why the two official scores differ.

Two independently reviewable repairs are implemented in local source revision **`e3569754142f1cd8a9373226dc6ccd0a754d32a8`**:

| Commit | Repair |
| --- | --- |
| `3fbca8b` | Gun/mortar aim handoff must fit the mortar firing tick and return to gun aim before the gun's next ready tick. |
| `e356975` | A physically clear near-wall start can recover using the observed cube-scaled body radius. The safety margin returns within 0.5 m; directed, corner and DDA checks remain after grid reentry. Includes the brain caller integration. |

Three agents implemented and reviewed these changes with separate responsibilities. Review found and fixed a brief uphill-cell skip in recovery before integration. The prior supply-drop repairs, explicit laser releases, shared-aim validation, gun-only behavior, and removal of the unsupported lethal-laser assumption remain intact. No broader tactical tuning was added.

## Validation

- **47 Rust tests passed**: 43 library and four binary tests, including ten new regression tests. Real-arena tests ran with the kit mounted. [Test log](evaluation/repair-tests.log).
- Both original witnesses are now covered by acceptance tests: legal stock/enlarged-body escape succeeds; invalid body overlap, wall crossing, uphill/corner bypasses fail. Gun/mortar tests cover both slot orders, 130/160 m failure fixtures, useful 100 m handoff, first/second firing ticks and reload preparation.
- **Docker conformance passed**: 300/300 observations on time, zero missed deadlines, wrong ticks, bad lines or debug-cap violations. One older queued answer was discarded by the checker. Observed p99 1.5 ms, maximum 1.9 ms. Container: no network, one CPU, 512 MB/no swap, pids 128, all capabilities dropped, read-only, uid 65534. This short trace does not prove every official match stays within deadline. [Conformance log](evaluation/repair-conformance.log).
- Production Dockerfile builds succeeded with the locked dependencies. An initial network-disabled clean build lacked the crates.io cache; the normal build downloaded those locked dependencies. Tests and runtime validation then ran offline. No dependency or Dockerfile changes were needed.

Preserved images `mechbot:research-5b783d1`, `mechbot:research-builder-5b783d1`, `mechbot:candidate`, and `mechbot:candidate-builder` remain unchanged. New images are `mechbot:candidate-r2` and `mechbot:candidate-r2-builder`. Exact image IDs and source hashes: [R2 manifest](evaluation/candidate-r2-manifest.json).

The proxy harness normalizes selected source files to LF while Docker copies the working-tree bytes. Final image source hashes match the manifest; the proxy differences in brain/combat were verified to be CRLF-to-LF normalization only and are separately recorded.

## Matched approximate results

All numbers below are **local simulator means, not official scores or forecasts**. Existing seeds use indices 0-47; fresh seeds use 48-71, with seed `1000 + index * 7919`. Both modes use the same selected seeds/slots across variants. No source changes were made after viewing these results. Fresh reference and isolated repair runs followed the fresh v4/combined comparison to investigate attribution; they are diagnostics on that holdout, not additional unseen validation.

| Variant | Existing 48: practice | Existing 48: FFA | Fresh 24: practice | Fresh 24: FFA |
| --- | ---: | ---: | ---: | ---: |
| Published v4 | 5.396 | 13.333 | 4.583 | 14.250 |
| Original local candidate, fresh rerun | 5.292 | 13.792 | 3.542 | 14.458 |
| Original candidate + navigation repair | 5.875 | 13.792 | 4.375 | 14.083 |
| Original candidate + combat repair | 5.688 | 13.958 | 3.375 | 14.458 |
| Original candidate + both repairs | 5.979 | 13.938 | 4.083 | 14.125 |

The existing v4 results are the preserved matched data; the other existing-suite rows were freshly run. On a fresh rerun of the unchanged original candidate, practice placements differed from the earlier saved run on two of 48 seeds (64352: 13 to 12 points; 270246: 9 to 5). FFA placements were unchanged. Source/config/map hashes match. HashMap iteration is not seed-controlled; the observed variation must not be treated as proof of its exact cause, or of the cause of the official score drop.

Against v4, the repaired bundle's existing-suite practice difference is **+0.583**, paired bootstrap 95% interval **[-0.438, +1.625]**. On fresh seeds it is **-0.500**, interval **[-1.875, +0.750]**. Both include no gain. These intervals describe the recorded proxy pairs, not official uncertainty, and do not account fully for rerun variation or simulator error. [Existing comparison](evaluation/repair-v4-comparison.json), [fresh comparison](evaluation/repair-fresh-holdout-comparison.json), [chart](evaluation/repair-comparison.html).

The original candidate already loses more than the repaired bundle on the fresh practice set. Navigation repair improves on that original candidate in both sets. Combat repair is mixed: positive on the existing set, lower on one fresh practice match, and unchanged fresh FFA placements. Correctness of the witness does not establish superiority of the resulting policy.

## Interactions and remaining risks

Effects are not additive. Mean practice interaction, computed as `combined - navigation - combat + original candidate`, is -0.292 on existing seeds and -0.125 on fresh seeds. Routing changes affect subsequent fights and equipment opportunities, while uncontrolled rerun variation complicates attribution.

Specific observed losses worth tracing:

- Existing seed 349436: original 11, navigation 15, combat 15, combined 9 points. This is an interaction signal, not evidence for blindly combining individually favorable changes.
- Fresh seed 555330, gun/mortar: original 10, navigation 13, combat 6, combined 6 points. The conservative handoff policy can forgo multi-decision mortar preparation while a usable gun is cooling; that tradeoff needs an encounter trace rather than aggregate tuning.
- Fresh FFA seed 491978: original 15, navigation 6, combined 7 points. A navigation correctness repair can still route into a worse fight.

[Interaction records](evaluation/repair-interactions.json), [existing isolated comparison](evaluation/repair-comparison.json), [fresh isolated comparison](evaluation/repair-fresh-ablation-comparison.json). The corresponding JSON/log files retain every outcome and input hash.

Ordinary navigation still has a fixed-size raster; radius-aware recovery uses approximate collision geometry. Combat projections assume the existing 20 Hz/two-tick cadence and current target lead. The broad enemy-laser duck trigger still needs a separately tested threat/finish policy; the false lethal-laser assumption must not be restored. The proxy's collision, hidden-information, and survival-time defects remain as documented in the [investigation](regression-investigation.md). Survival averages were not used to choose a candidate.

## Disposition and next experiment

This is a technically conformant **experimental candidate**, not a demonstrated scoring upgrade. It remains local and unsubmitted. The published v4 and the original candidate are preserved. Do not promote the full bundle on the strength of the existing-seed mean while ignoring the fresh decline.

The next useful user evidence is the commit and scorer version for the 4.81 run. Once identified, use that exact revision as the official comparison baseline. Navigation is the more consistent of the two repairs locally; the next score experiment should isolate the navigation package on that known baseline, followed by combat only if the gun/mortar tradeoff is justified by traces. Publish the selected tested revision and submit an explicit SHA, recording its receipt and scorer version. If the 4.81 source is unavailable, label v4 as the known 4.65 reference rather than silently assigning it the best score.

For reproduction on the preserved original candidate, the harness now requires `--base-commit` with a nondefault builder, pins the builder image ID, and records all compiled source hashes:

```sh
python research/run_proxy.py baseline --image mechbot:candidate-builder --base-commit 7b2f1547e2e2a827fa3bbfb6085068ff73cf3b45 --matches 48 --label repair-reference-recheck
python research/run_proxy.py navigation-repair --image mechbot:candidate-builder --base-commit 7b2f1547e2e2a827fa3bbfb6085068ff73cf3b45 --matches 48 --label repair-nav-recheck
python research/run_proxy.py combat --image mechbot:candidate-builder --base-commit 7b2f1547e2e2a827fa3bbfb6085068ff73cf3b45 --matches 48 --label repair-combat-recheck
python research/run_proxy.py candidate --image mechbot:candidate-builder --base-commit 7b2f1547e2e2a827fa3bbfb6085068ff73cf3b45 --matches 48 --label repair-combined-recheck
```

Use `--matches 24 --start-index 48` for the fresh set. `navigation-repair` includes the radius-aware brain caller; the older `navigation` variant retains its original map/nav-only meaning. Full source changes and all validation here remain separate from official scoring.
