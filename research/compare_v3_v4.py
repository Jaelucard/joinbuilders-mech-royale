"""Bounded v3/v4 practice ablation with frozen v4 simulator geometry."""
import argparse
import hashlib
import json
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
V3 = "411af5f530934aa21d8cb5dfff35259219595159"
V4 = "5b783d19f138239033be32b527318f9247a9f9b8"


def blob(revision, path):
    return subprocess.check_output(["git", "show", f"{revision}:{path}"], cwd=ROOT).decode("utf-8")


def sha(text):
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("variant", choices=["v3", "v4", "terrain-only", "canopy-only"])
    parser.add_argument("--matches", type=int, default=48)
    args = parser.parse_args()
    if args.matches < 1:
        parser.error("--matches must be positive")
    revision = V3 if args.variant == "v3" else V4
    names = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", revision, "src"], cwd=ROOT, text=True).splitlines()
    source = {name: blob(revision, name) for name in names}
    if args.variant == "terrain-only":
        assert source["src/map.rs"].count("+ canopy_bonus;") == 1
        source["src/map.rs"] = source["src/map.rs"].replace("+ canopy_bonus;", "+ 0.0;")
    if args.variant == "canopy-only":
        assert source["src/map.rs"].count("self.terrain_z_fast(") == 4
        assert source["src/brain.rs"].count("self.map.terrain_z_fast(") == 1
        source["src/map.rs"] = source["src/map.rs"].replace("self.terrain_z_fast(", "self.terrain_z(")
        source["src/brain.rs"] = source["src/brain.rs"].replace("self.map.terrain_z_fast(", "self.map.terrain_z(")
    environment = blob(V4, "src/map.rs")
    simulator = (ROOT / "sim/src/main.rs").read_text(encoding="utf-8")
    assert simulator.count("use mechbot::map::ArenaMap;") == 1
    simulator = simulator.replace("use mechbot::map::ArenaMap;", "use mechbot::protocol;\nmod environment_map;\nuse environment_map::ArenaMap;")
    marker = "    let elapsed = t0.elapsed().as_secs_f64();"
    assert simulator.count(marker) == 1
    simulator = simulator.replace(marker, marker + '''
    for (i, r) in results.iter().enumerate() {
        println!("PROXY_RESULT {}", serde_json::json!({
            "seed":1000+i as u64*7919,"mode":mode,"place":r.place,"points":r.points,
            "kills":r.kills,"survival_s":r.death_t,"loadout":r.loadout,
            "killed_by":r.killed_by,"gun_shots":r.gun_shots,"gun_hits":r.gun_hits,
            "laser_shots":r.laser_shots,"laser_hits":r.laser_hits
        }));
    }
''')
    manifest = (ROOT / "sim/Cargo.toml").read_text(encoding="utf-8")
    manifest = manifest.replace('[dependencies]', '[dependencies]\nserde = { version = "=1.0.219", features = ["derive"] }')
    lock = (ROOT / "sim/Cargo.lock").read_text(encoding="utf-8")
    old = 'name = "mechsim"\nversion = "0.1.0"\ndependencies = [\n "mechbot",\n "serde_json",\n]'
    assert lock.count(old) == 1
    lock = lock.replace(old, old.replace(' "serde_json",', ' "serde",\n "serde_json",'))
    image = subprocess.check_output(["docker", "image", "inspect", "--format", "{{.Id}}", "mechbot:research-builder-5b783d1"], text=True).strip()
    out = ROOT / "research/evaluation"
    label = "scored-" + args.variant
    with tempfile.TemporaryDirectory(prefix="scored-ablation-") as tmp:
        stage = Path(tmp)
        files = {**source, "Cargo.toml":blob(revision,"Cargo.toml"), "Cargo.lock":blob(revision,"Cargo.lock"),
                 "sim/src/main.rs":simulator, "sim/src/environment_map.rs":environment,
                 "sim/Cargo.toml":manifest, "sim/Cargo.lock":lock,
                 "sim/hello_config.json":(ROOT/"sim/hello_config.json").read_text(encoding="utf-8")}
        for name, content in files.items():
            target = stage / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content, encoding="utf-8", newline="\n")
        command = ["docker", "run", "--rm", "--network", "none", "--cpus", "1", "--memory", "1g",
                   "--mount", f"type=bind,source={stage},target=/snapshot,readonly",
                   "--mount", f"type=bind,source={ROOT/'kit'},target=/build/kit,readonly",
                   "-e", f"MATCHES={args.matches}", image, "sh", "-c",
                   'set -eu; cp -r /snapshot/. /build/; cargo build --release --locked --offline --manifest-path /build/sim/Cargo.toml; VERBOSE=1 /build/sim/target/release/mechsim practice "$MATCHES"']
        with (out/f"{label}.log").open("w",encoding="utf-8") as log:
            result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    log_text = (out/f"{label}.log").read_text(encoding="utf-8")
    if result.returncode:
        print(log_text[-6000:])
        raise SystemExit(result.returncode)
    outcomes = [json.loads(line.removeprefix("PROXY_RESULT ")) for line in log_text.splitlines() if line.startswith("PROXY_RESULT ")]
    assert len(outcomes) == args.matches
    inputs = ["sim/src/main.rs","sim/hello_config.json","kit/arena/v2/world.json","kit/arena/v2/terrain.i16"]
    record = {"kind":"approximate local practice proxy; not official scoring", "label":label,
              "checked_at_utc":datetime.now(timezone.utc).isoformat(),
              "source_revision":revision,"variant":args.variant,"builder_image_id":image,
              "compiled_source_sha256":{k:sha(v) for k,v in source.items()},
              "environment_revision":V4,"environment_map_sha256":sha(environment),
              "simulator_harness_sha256":sha(simulator),"simulator_manifest_sha256":sha(manifest),"simulator_lock_sha256":sha(lock),
              "input_sha256":{p:hashlib.sha256((ROOT/p).read_bytes()).hexdigest() for p in inputs},
              "outcomes":outcomes,"limitations":["Fixed approximate v4 environment for all variants; not the official engine.",
                 "Variant trees are retained but canopy score is disabled for terrain-only; startup overhead is not an official timing test.",
                 "Random HashMap iteration can change reruns; capped survival metrics are not elapsed survival."]}
    (out/f"{label}.json").write_text(json.dumps(record,indent=2)+"\n",encoding="utf-8")
    print(label, "practice mean", sum(r["points"] for r in outcomes)/len(outcomes), flush=True)


if __name__ == "__main__":
    main()
