"""Run matched approximate simulations against preserved baseline source in Docker.

Requires the ignored local sim/ and kit/ plus the preserved research builder image.
Each run compiles in a disposable container; it never changes production source.
"""
import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BASELINE = "5b783d19f138239033be32b527318f9247a9f9b8"
VARIANTS = {
    "baseline": [],
    "navigation": ["src/map.rs", "src/nav.rs"],
    "navigation-repair": ["src/map.rs", "src/nav.rs", "src/brain.rs"],
    "combat": ["src/combat.rs"],
    "brain": ["src/brain.rs"],
    "candidate": ["src/map.rs", "src/nav.rs", "src/combat.rs", "src/brain.rs"],
}

TREATMENTS = {
    "release-lost-target": ("src/combat.rs", "let hold = !urgent_move && charge + 0.10 < deadline;", "let hold = false;"),
    "legacy-charge-aim": ("src/combat.rs", "set_aim(if prepare_release { 100 } else { 40 }, desired);", "set_aim(100, desired);"),
    "legacy-charge-start": ("src/combat.rs", "set_aim(40, desired);", "set_aim(80, desired);"),
    "no-regen-urgent": ("src/brain.rs", "urgent_zone || doing_laser_duck || want_regen_retreat,", "urgent_zone || doing_laser_duck,"),
    "legacy-close-nav": ("src/brain.rs", "if on_roof || (target_is_higher && dist_to_goal <= 6.5)", "if on_roof || dist_to_goal <= 6.5"),
    "legacy-duck-exception": ("src/brain.rs", "if !urgent_zone && !charging_los_snipers.is_empty() {", "if !urgent_zone && !charging_los_snipers.is_empty() && !(me.weapons.iter().flatten().any(|w| w.kind == WeaponKind::Laser && w.charge.unwrap_or(0.0) >= 0.50) && lowest_vis_enemy_hp <= 360.0) {"),
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("variant", choices=VARIANTS)
    parser.add_argument("--matches", type=int, default=24)
    parser.add_argument("--start-index", type=int, default=0)
    parser.add_argument("--modes", nargs="+", choices=["practice", "ffa"], default=["practice", "ffa"])
    parser.add_argument("--label")
    parser.add_argument("--image", default="mechbot:research-builder-5b783d1")
    parser.add_argument("--base-commit",
                        help="Recorded source revision of the builder image (for isolated repairs on a preserved candidate)")
    parser.add_argument("--source-image", help="Read variant source from an available preserved builder image")
    parser.add_argument("--treatments", nargs="*", choices=TREATMENTS, default=[])
    args = parser.parse_args()
    if args.base_commit is None:
        if args.image != "mechbot:research-builder-5b783d1":
            parser.error("--base-commit is required when selecting a different builder image")
        args.base_commit = BASELINE
    if not re.fullmatch(r"[0-9a-f]{40}", args.base_commit):
        parser.error("--base-commit must be a full source commit SHA")
    if args.matches < 1 or args.start_index < 0:
        parser.error("matches must be positive and start-index nonnegative")
    label = args.label or args.variant
    if not re.fullmatch(r"[A-Za-z0-9_-]+", label):
        parser.error("label must contain only letters, numbers, underscores, or hyphens")
    output = ROOT / "research/evaluation"
    output.mkdir(exist_ok=True)
    image_id = subprocess.check_output(
        ["docker", "image", "inspect", "--format", "{{.Id}}", args.image], text=True).strip()
    image_hash_output = subprocess.check_output(
        ["docker", "run", "--rm", "--network", "none", image_id, "sh", "-c",
         "cd /build && sha256sum src/*.rs"], text=True)
    image_source_hashes = dict((filename, checksum) for checksum, filename in
                               (line.split() for line in image_hash_output.splitlines()))
    paths = ["sim/Cargo.toml", "sim/Cargo.lock", "sim/src/main.rs", "sim/hello_config.json",
             "kit/arena/v2/world.json", "kit/arena/v2/terrain.i16"] + VARIANTS[args.variant]
    hashes = {p: digest(ROOT / p) for p in paths}
    source = (ROOT / "sim/src/main.rs").read_text(encoding="utf-8")
    needle = "run_match(1000 + m as u64 * 7919, practice, &cfg, &map)"
    if source.count(needle) != 1:
        raise RuntimeError("Unexpected simulator seed loop; refusing an ambiguous patch")
    source = source.replace(needle, f"run_match(1000 + ({args.start_index} + m as u64) * 7919, practice, &cfg, &map)")
    # Extract full precision per-match outcomes without changing simulated behavior.
    marker = "    let elapsed = t0.elapsed().as_secs_f64();"
    if source.count(marker) != 1:
        raise RuntimeError("Unexpected simulator output location")
    output_code = '''    for (i, r) in results.iter().enumerate() {
        println!("PROXY_RESULT {}", serde_json::json!({
            "seed":1000 + (START_INDEX + i as u64) * 7919,
            "mode":mode,"place":r.place,"points":r.points,"kills":r.kills,
            "survival_s":r.death_t,"cubes":r.cubes,"loadout":r.loadout,
            "killed_by":r.killed_by,"gun_shots":r.gun_shots,"gun_hits":r.gun_hits,
            "laser_shots":r.laser_shots,"laser_hits":r.laser_hits
        }));
    }
'''.replace("START_INDEX", str(args.start_index))
    source = source.replace(marker, marker + "\n" + output_code)
    patches = {p:(ROOT / p).read_text(encoding="utf-8") for p in VARIANTS[args.variant]}
    if args.source_image:
        patches = {p:subprocess.check_output(
            ["docker","run","--rm","--network","none",args.source_image,"cat","/build/"+p],
            text=True) for p in VARIANTS[args.variant]}
        hashes.update({p:hashlib.sha256(s.encode("utf-8")).hexdigest() for p,s in patches.items()})
    for treatment in args.treatments:
        filename, old, new = TREATMENTS[treatment]
        if filename not in patches or patches[filename].count(old) != 1:
            raise RuntimeError(f"Treatment {treatment} does not match exactly one candidate location")
        patches[filename] = patches[filename].replace(old,new)
    shell = """set -eu
mkdir -p /build/sim/src
cp /candidate/sim/Cargo.toml /candidate/sim/Cargo.lock /candidate/sim/hello_config.json /build/sim/
cp /harness/main.rs /build/sim/src/main.rs
for file in $PATCH_FILES; do cp "/harness/$file" "/build/$file"; done
cargo build --release --locked --offline --manifest-path /build/sim/Cargo.toml
for mode in $EVAL_MODES; do VERBOSE=1 /build/sim/target/release/mechsim "$mode" "$EVAL_MATCHES"; done
"""
    with tempfile.TemporaryDirectory(prefix="mech-proxy-") as temp:
        harness = Path(temp)
        (harness / "main.rs").write_text(source, encoding="utf-8")
        for filename, patched in patches.items():
            target = harness / filename
            target.parent.mkdir(parents=True,exist_ok=True)
            target.write_text(patched,encoding="utf-8")
        cmd = ["docker", "run", "--rm", "--network", "none", "--cpus", "1", "--memory", "1g",
               "--mount", f"type=bind,source={ROOT.as_posix()},target=/candidate,readonly",
               "--mount", f"type=bind,source={(ROOT / 'kit').as_posix()},target=/build/kit,readonly",
               "--mount", f"type=bind,source={harness.as_posix()},target=/harness,readonly",
               "-e", "PATCH_FILES=" + " ".join(VARIANTS[args.variant]),
               "-e", "EVAL_MODES=" + " ".join(args.modes), "-e", f"EVAL_MATCHES={args.matches}",
               image_id, "sh", "-c", shell]
        print(f"Running {label}: {args.matches} matches per mode, index {args.start_index}", flush=True)
        with (output / f"{label}.log").open("w", encoding="utf-8") as log:
            result = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT, text=True)
    log_text = (output / f"{label}.log").read_text(encoding="utf-8")
    if result.returncode:
        print(log_text[-10000:])
        raise SystemExit(result.returncode)
    outcomes = [json.loads(line.removeprefix("PROXY_RESULT "))
                for line in log_text.splitlines() if line.startswith("PROXY_RESULT ")]
    if len(outcomes) != args.matches * len(args.modes):
        raise RuntimeError("Incomplete simulator results")
    # Candidate files were snapshotted into the immutable temporary harness before
    # Docker started. Later working-tree edits cannot alter the running variant.
    if any(digest(ROOT / p) != expected for p, expected in hashes.items()
           if p not in VARIANTS[args.variant]):
        raise RuntimeError("Inputs changed during evaluation; results not accepted")
    record = {"kind":"approximate local simulator; not official scoring", "variant":args.variant,
              "label":label,"checked_at_utc":datetime.now(timezone.utc).isoformat(),
              "baseline_commit":args.base_commit,"builder_image":args.image,"builder_image_id":image_id,"input_sha256":hashes,
              "base_source_sha256":image_source_hashes,
              "compiled_source_sha256":{**image_source_hashes,
                  **{p:hashlib.sha256(s.encode("utf-8")).hexdigest() for p,s in patches.items()}},
              "treatments":args.treatments,
              "candidate_source_image":args.source_image,
              "compiled_variant_sha256":{p:hashlib.sha256(s.encode("utf-8")).hexdigest() for p,s in patches.items()},
              "seed_start_index":args.start_index,"matches_per_mode":args.matches,
              "repeatability_note":"Bot HashMap tie ordering is not seed-controlled; matched seeds do not guarantee identical reruns.",
              "outcomes":outcomes}
    (output / f"{label}.json").write_text(json.dumps(record,indent=2)+"\n",encoding="utf-8")
    for mode in args.modes:
        rows = [r for r in outcomes if r["mode"] == mode]
        print(f"{label} {mode}: mean={sum(r['points'] for r in rows)/len(rows):.4f}, n={len(rows)} "
              "(survival_s is an uncorrected capped proxy; do not interpret as elapsed survival)",flush=True)


if __name__ == "__main__":
    main()
