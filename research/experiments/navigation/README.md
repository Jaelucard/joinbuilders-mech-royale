# Navigation experiment component

`research/experiments/navigation.py` exports `OPTIONS` and `apply(sources, option)`. Input is the exact v3 source dictionary, keyed by repository path. The function returns a copy and reads only immutable full-SHA Git blobs, never current production files. Apply navigation before the combat and brain components.

| Option | Change from supplied v3 |
| --- | --- |
| `baseline` | No changes. |
| `safe` | Repaired directed transitions, DDA shortcuts, safe smoothing, bounded actual-body recovery, and close-ground-goal/radius-aware brain caller. Keeps v3 map layout, interpolated terrain and no canopy preference. |
| `canopy` | V4 canopy preference and its grid machinery, with all four cached terrain reads replaced by v3 interpolation. Original v3 navigator and brain are byte-identical. |
| `safe_canopy` | Both options, with interpolated terrain throughout. |

Safe navigation is transplanted from `e3569754142f1cd8a9373226dc6ccd0a754d32a8`; canopy comes from `5b783d19f138239033be32b527318f9247a9f9b8`. The base is `411af5f530934aa21d8cb5dfff35259219595159`. The brain change only moves the existing `target_is_higher` declaration and replaces the waypoint block; drop holding, laser threats, micro-steering, all combat decisions, and bot names remain v3.

Regression verification:

```sh
python research/experiments/navigation/verify.py
```

All three nonbaseline variants compiled and passed offline Docker tests on preserved `mechbot:scored-v3-builder`:

- `safe`: 18 library tests plus four existing binary tests.
- `canopy`: two terrain-precision tests plus four binary tests.
- `safe_canopy`: 18 library tests plus four binary tests.

The safe variants retain all 16 existing navigation regressions, including the real-arena cliff case, legal stock/enlarged-body wall escape, actual overlap, wall crossing, slope checks, corner rules and brief-cell DDA traversal. Both new terrain tests require a uniform 45-degree ramp to remain passable and a steeper ramp to be rejected. Source-level assertions also prove that unrelated files, the old drop tracker and original laser-threat policy are not accidentally changed by this component. Image ID, exact compiled-source hashes and log paths are in `verification.json`.

These tests establish the isolated behavior and composition contract, not a scoring improvement. Fixed-size ordinary navigation and approximate physical collision remain limitations. Canopy is an imperfect directional-visibility heuristic; it is intentionally isolated for screening, not presumed beneficial.
