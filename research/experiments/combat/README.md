# Frozen combat experiment options

`research/experiments/combat.py` exposes `OPTIONS` and `apply(sources, option)`. It returns a new mapping and replaces only `src/combat.rs`; navigation and brain transformations are retained verbatim. It reads immutable Git blobs, never the changing production file.

| Option | Source / behavior |
| --- | --- |
| `baseline` | No change to supplied v3 combat. |
| `shared` | Combat from `7b2f1547e2e2a827fa3bbfb6085068ff73cf3b45`: shared aim validation and explicit laser state/release handling, before the mortar-window repair. |
| `window` | Combat from `3fbca8bd51f653a6cb4ab77f2e763b585a49954b`: a mortar handoff must produce an aligned shot and allow gun aim to return by the next ready tick. |
| `shot` | Same frozen source as `window`, but an immediate aligned mortar shot may delay the gun by up to one additional two-tick action under the fixed-lead timing projection. A parity-aware return budget retains a bound instead of removing the gun-return check entirely. |

The new `shot` option is a diagnostic split of the return-time constraint, not a claim of improvement or an explanation for seed 555330. It does not add persistent state or permit multi-decision preparation when the current action cannot fire the mortar. It continues to reject the known 130/160 m no-shot handoffs. It permits the 160 m second-tick mortar fixture to purchase an actual mortar shot at the cost of one later gun action. A separate test rejects excessive return slew, including the second-ready-tick boundary where continuous slew alone would underestimate action-level delay.

All options retain gun-only behavior, non-laser final-aim checks and explicit laser release tests. Changing targets or future lead invalidates a fixed-target delay projection; the bounded fixture is not a guarantee about every real encounter. No broad matches or official scoring were run by this component author.

## Validation

Run `python research/experiments/combat/run_tests.py` from the repository root. The script substitutes only combat into the preserved exact-v3 builder and runs offline with one CPU. It checks that non-combat input sources remain unchanged.

- `shared`: 12 combat tests pass.
- `window`: 16 combat tests pass.
- `shot`: 17 combat tests pass, including the intentional gun-delay tradeoff and excessive-return rejection.

`validation.json` records the immutable image ID, source hashes and exits. The per-option logs retain each named test. These options are frozen before root screening/holdout selection.
