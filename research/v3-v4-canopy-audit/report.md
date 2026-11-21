# V3 to v4 canopy audit

Scope: isolate the canopy preference added between user-confirmed official revisions `411af5f530934aa21d8cb5dfff35259219595159` (4.81) and `5b783d19f138239033be32b527318f9247a9f9b8` (4.65). This audit establishes changed decisions, not the cause of the official score difference. No production files or preserved images were changed; no broad matches were run.

## What changed

V4 parses 1,560 trees from the supplied arena. All expected columns exist: x/y/z, height, crownR, crownBase, trunkR, crownX/crownY. The builder records local torso/eye overlap with nearby crowns and trunks in a 4 m grid, rounded and clamped to 0-10. No obvious schema mismatch was found.

`find_cover_pos` adds `0.35 * nav_canopy[cell]` when the mech is on the ground and the minimum distance to the first four considered threats exceeds 46 m. The bonus is at most 3.5 against a hard-LOS exposure penalty of 240 per threat. It is an additive preference, not merely an exact-score tie breaker. It does not add trees to hard LOS or collision. Emergency laser ducking separately requires the chosen cell to block hard LOS from all charging threats.

Among the actual map's 14,304 ground cells eligible by walkability/wall-distance for cover search, 2,804 have a nonzero canopy value. The audit records the full histogram. This is a grid statistic, not a frequency of in-game cover choices.

## Reproduced isolated chooser change

The probe runs the original v4 Rust code in preserved builder `mechbot:research-builder-5b783d1`, then changes only `nav_canopy` to zero between paired calls. It uses the real arena, a safe circle centered at `[0,0]` with radius 480, and the production regen search radius 44 m. All sampled origins and threats clear the bot's existing collision model. Terrain and all other scoring terms stay fixed.

One witness:

- Origin: `[281.619995, -323.089996, 0.065302]`.
- Threat eye: `[281.619995, -268.089996, 5.059100]`.
- Bonus disabled: current position, canopy value 8, 55 m from threat.
- Bonus enabled: `[281.619995, -327.089996, 0.422202]`, canopy value 10, 4 m travel, 59 m from threat.
- Both positions are exposed according to the unchanged hard-LOS checker.

The bounded scan found eight changed decisions after 638 synthetic origin/threat scenarios and intentionally stopped after eight examples. It is not a representative gameplay sample and does not estimate a change frequency. All retained examples use regen-style cover search. A different position can improve distance or later tactics, so the witness alone does not prove harm.

## Rules that limit the heuristic

The supplied [protocol](/C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/kit/protocol.md:367) says foliage visibility depends on the line through the crown; a local scalar density is not a directional visibility test. The world specifies `canopyDensity=0.35` and `canopyBlockT=0.25`, neither used by this cover bonus. Trunks stop sight but trees do not stop shots.

Base proximity sensors ignore canopy within 45 m, and the documented sensor upgrade adds 15 m. The hardcoded 46 m gate does not account for that 60 m upgraded range. In the witness, the preferred destination remains 59 m from the threat; foliage can therefore receive a concealment bonus where upgraded proximity ignores it. Other geometry could still block sight; no official optical visibility claim is made from the bot's hard-LOS check alone.

The same protocol documents five seconds of current-position tracking after sight is lost. Entering foliage does not immediately erase an already acquired target, and it does not stop incoming shots. These are reasons to evaluate concealment separately from ballistic safety, not reasons to treat every canopy move as a bug.

## Recommended isolated experiment

Run v4 with only the canopy score term disabled, keeping terrain lookup and map construction unchanged. Zeroing the score coefficient is the cleanest runtime policy ablation; zeroing `nav_canopy` reproduces that behavior in this probe. Compare against unchanged v4 using matched scenarios and collect cover-choice/route traces. This isolates the canopy policy from v4's separate terrain quantization changes.

Do not infer the official 0.16-point difference from a local proxy: the existing simulator does not implement official canopy visibility and can give hunters the hidden player's position. An official explicit-SHA ablation or real decision/replay evidence is needed to judge whether the policy helps actual concealment.

Reproduce this bounded diagnostic:

```sh
python research/v3-v4-canopy-audit/run_audit.py
```

[Probe source](/C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/v3-v4-canopy-audit/canopy_audit.rs), [runner](/C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/v3-v4-canopy-audit/run_audit.py), [structured results and image ID](/C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/v3-v4-canopy-audit/result.json), [raw output](/C:/Users/lowja/.t3/projects/joinbuilders-mech-royale/research/v3-v4-canopy-audit/output.log).
