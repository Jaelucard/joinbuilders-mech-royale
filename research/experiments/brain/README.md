# Brain component experiments

`research/experiments/brain.py` exposes `apply(sources, option)`, returning a copied source dictionary. It changes only `src/brain.rs` through exact, asserted anchors, preserving navigation edits already present. Apply navigation first, combat second, brain last. Production files are never read or written by the transformer; inherited implementations come from immutable Git revisions.

| Option | Drop behavior | Laser-threat behavior |
| --- | --- | --- |
| `baseline` | Exact input behavior | Exact input behavior |
| `drop` | DropHold landing/capture/watchdog repair from `7b2f154` | Original v3 policy, including its unsupported own-kill exemption |
| `threat` | Original v3 behavior | Remove unsupported own-kill exemption; use credible charging-threat evidence |
| `drop_threat` | Both repairs | Both repairs |

Baseline and drop are isolation controls. They are not eligible as a complete implementation of the user's earlier requirement to remove the unsupported lethal-laser assumption.

## Threat policy

A charging enemy contributes to emergency laser ducking only with LOS, within the existing 275 m distance, and either:

- the existing `WorldState::aiming_at_me` predicate is true; or
- that actor damaged us within the existing three-second threat window.

The existing predicate is a 0.4-radian horizontal aim cone. This reuses already-established threat evidence rather than adding a new tuned threshold. The old own-charge/lowest-enemy-HP kill shortcut is removed entirely. No replacement guarantee of an imminent own kill is asserted. The ordinary kill-finish strategy, target selection, regen behavior, gun fire, lightning shorting and combat implementation are unchanged.

This is still a heuristic: a charger can turn toward us before the next observation, enemy turn-rate upgrades are not modeled here, the yaw cone ignores vertical aim, and recent damage can outlast the current threat. Tests prove the stated policy and preserved firing behavior, not that it improves official score. Source policy was frozen before viewing any experiment-screen outcomes.

## Drop policy

The isolated port retains the previously tested `DropHold` state and progress tracker. Waiting inside a legal landing/capture ring does not advance travel-stall detection. Recapture progress renews an inactivity deadline, while an independent absolute deadline bounds repeated contest/reset cycles. Unreachable targets still time out, and urgent zone escape still interrupts waiting. Navigation route selection and the terrain lookup remain whatever the input component selected.

## Verification

Run:

```sh
python research/experiments/brain/verify.py
```

The runner uses disposable, offline containers based on the preserved exact-v3 builder and does not mutate the image or workspace production source. It records six passing combinations:

- v3 + drop: 9 tests;
- v3 + threat: 5 tests;
- v3 + drop_threat: 14 tests;
- safe navigation + drop_threat: 32 tests;
- safe navigation + window combat + drop_threat: 48 tests;
- safe navigation + shot combat + drop_threat: 49 tests.

The five new threat tests cover an aimed enemy despite a nonlethal own charge, a charger facing elsewhere without aborting our 0.3-second charge, a recently damaging charger while turning, gun-only defensive fire, and exposed regen return fire. The nine drop tests come from the pinned repair source.

A separate negative control keeps the new threat fixtures while restoring the entire original v3 production brain. Three relevant tests fail there: nonlethal own charge, unrelated charger, and recent attacker. The two retained firing tests pass. Exit 101 for that control is the expected outcome; `verification.json` records `expected_outcome_verified: true` after verifying the specific failed-test names. The verification script itself exits zero when every positive and negative expectation is satisfied.

Evidence: `verification.json` and the adjacent per-combination logs. No broad match simulations, pushes or commits were performed by this component task.
