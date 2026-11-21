# Terrain precision audit of the actual official revisions

Compared user-associated official sources: v3 `411af5f530934aa21d8cb5dfff35259219595159` (4.81) and v4 `5b783d19f138239033be32b527318f9247a9f9b8` (4.65). These results establish behavior-changing source semantics, **not the cause of the official score difference**. Scorer-version and official match traces remain unavailable.

## Mechanism

V3's `terrain_z` is already O(1): it reads four neighboring heights from the original terrain array and bilinearly interpolates. V4's `terrain_z_fast` looks up one cached value sampled at the center of a 2 m cell. This is a constant-factor speed/precision tradeoff, not an asymptotic optimization.

In frozen v4 source, `src/map.rs:876` defines the snapped lookup. `is_blocked_at` uses it for both center and offset terrain heights at lines 988/998; v3 uses bilinear heights at the same points. `src/brain.rs:1166` also uses the snapped value to decide whether a movement candidate receives a 14-point zero-clearance navigation penalty. A different center height can change this grounded/airborne classification.

## Executed witnesses

The final diagnostic ran actual v4 map code in preserved builder `mechbot:research-builder-5b783d1`, plus the exact collision function extracted from the pinned v3 commit under a diagnostic name. It executed three tests: one deterministic real-map scan and two synthetic linear-ramp fixtures. **All three passed.** No production source, image, or simulator was changed; the added test module existed only inside a disposable offline container.

Uniform-ramp witnesses, body radius 2.5 m:

- Gradient 1.0 (45 degrees), position `[9.9,10,9.9]`: v3 accepts, v4 blocks. Exact center/forward heights are 9.9/12.4 (rise 2.5), but cached values are 9/13 (rise 4), exceeding the collision test's 3.125 m threshold. The cached approximation invents an impassable slope on a uniform walkable ramp.
- Gradient 1.4 (about 54.5 degrees), position `[10.5,10,14.7]`: v3 blocks, v4 accepts. Exact rise is 3.5 m; cached rise is about 2.8 m. The same quantization can miss a steep slope.

The real-map scan used 742,712 points on a 1 m grid offset from arena bounds by `(8.137,8.271)`, excluding an 8 m boundary margin. These counts describe this artificial sampling lattice, **not bot encounter frequency**:

| Query radius | V3 clear / v4 blocked | V3 blocked / v4 clear |
| --- | ---: | ---: |
| 1.8 m (stock micro-steering's `2.5 * 0.72`) | 584 | 787 |
| 2.5 m | 682 | 734 |

Mean absolute terrain error was 0.0680 m, p95 0.1740 m, p99 0.5905 m. The worst sampled error was **+36.2028 m** at `(-304.2430,-6.8190)`: bilinear 3.9603 versus cached 40.1631. The small average conceals large local errors near sharp terrain changes.

Concrete real-map obstruction witness: center `(-300.2430,-14.8190)`, feet z=-4.7950, radius 1.8 m. The north sample's bilinear height is -4.7626, whereas its cached height is +3.2760. V3 reports clear; v4 reports blocked. Other recorded witnesses show the reverse direction.

Concrete ground-penalty witness: position `(-261.2430,-11.8190,23.4)` has bilinear ground 23.4 and cached ground 20.8738. Its navigation cell has zero clearance, while both body checks at radius 2.5 m report clear. V3's grounded gate is true; v4's is false because 23.4 exceeds 20.8738+1.5. Thus the 14-point penalty is omitted for a zero-clearance movement candidate in that cell. The scan found 471 such positions that were also v3-body-clear. This is a heuristic-classification witness, not a proof of official physical legality or of the ultimately selected movement action.

## Scope and experiment recommendation

Restoring bilinear terrain reads specifically in collision and the brain ground gate is the strongest precision-preserving experiment. The cache can remain for preprocessing or soft canopy scoring. Check deadline performance rather than assuming the old constant-time interpolation is too slow. Separate this from the canopy policy change.

The local simulator calls the production map's `is_blocked_at` for opponent steering and movement (`sim/src/main.rs:1286,1454-1460`). A naive v3/v4 source swap therefore changes both bot behavior and simulated physics. Root's separate comparison harness freezes environment geometry; this audit itself does not run matches or infer score effects.

Reproduce from the repository root:

```sh
python research/v3-v4-terrain-audit/run.py
```

Files: `audit.rs` is the test module; `legacy_method.rs` is the pinned v3 function extracted by the runner; `results.json` contains full precision samples, synthetic witnesses, source commits, image ID and input hashes; `output.log` records the successful test run. The runner uses a readonly kit/audit mount, `--network none`, one CPU and 1 GB memory. Results remain approximate diagnostics, with the official cause unproven.
