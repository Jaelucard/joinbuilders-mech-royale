"""Verify each pinned navigation variant without editing production source."""
import hashlib
import importlib.util
import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("navigation", HERE.parent / "navigation.py")
navigation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(navigation)


def main():
    names = subprocess.check_output(
        ["git", "ls-tree", "-r", "--name-only", navigation.V3, "src"], cwd=ROOT, text=True
    ).splitlines()
    source = {name: navigation._blob(navigation.V3, name) for name in names}
    assert navigation.apply(source, "baseline") == source
    original_brain = source["src/brain.rs"]
    image = subprocess.check_output(
        ["docker", "image", "inspect", "--format", "{{.Id}}", "mechbot:scored-v3-builder"], text=True
    ).strip()
    result = {"source_revision": navigation.V3, "image_id": image, "variants": {}}
    for option in navigation.OPTIONS[1:]:
        candidate = navigation.apply(source, option)
        assert source["src/brain.rs"] == original_brain
        assert candidate["src/combat.rs"] == source["src/combat.rs"]
        for name in ["src/world.rs", "src/protocol.rs", "src/lib.rs", "src/main.rs"]:
            assert candidate[name] == source[name]
        assert "my_laser_about_to_kill" in candidate["src/brain.rs"]
        assert "self.track_goal_progress(state_label, goal2, pos2, obs.tick, obs.t);" in candidate["src/brain.rs"]
        assert "holding_drop" not in candidate["src/brain.rs"]
        assert "self.terrain_z_fast(" not in candidate["src/map.rs"]
        assert "self.map.terrain_z_fast(" not in candidate["src/brain.rs"]
        if option == "canopy":
            assert candidate["src/brain.rs"] == original_brain
            assert candidate["src/nav.rs"] == source["src/nav.rs"]
        if option == "safe":
            assert "nav_canopy" not in candidate["src/map.rs"]
            assert "terrain_grid" not in candidate["src/map.rs"]
        with tempfile.TemporaryDirectory(prefix="nav-component-") as temporary:
            stage = Path(temporary)
            for name, contents in candidate.items():
                target = stage / name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(contents, encoding="utf-8", newline="\n")
            cmd = ["docker", "run", "--rm", "--network", "none", "--cpus", "1", "--memory", "1g",
                   "--mount", f"type=bind,source={stage},target=/snapshot,readonly",
                   "--mount", f"type=bind,source={ROOT/'kit'},target=/build/kit,readonly",
                   image, "sh", "-c", "cp /snapshot/src/*.rs /build/src/ && cargo test --locked --offline"]
            process = subprocess.run(cmd, capture_output=True, text=True)
        log_name = f"{option}-tests.log"
        (HERE / log_name).write_text((process.stdout + process.stderr).rstrip()+"\n", encoding="utf-8")
        result["variants"][option] = {
            "exit_code": process.returncode, "test_log": log_name,
            "compiled_source_sha256": {name: hashlib.sha256(text.encode()).hexdigest() for name, text in candidate.items()},
        }
        print(option, "exit", process.returncode, flush=True)
        if process.returncode:
            print((process.stdout + process.stderr)[-6000:])
            (HERE / "verification.json").write_text(json.dumps(result, indent=2)+"\n", encoding="utf-8")
            raise SystemExit(process.returncode)
    (HERE / "verification.json").write_text(json.dumps(result, indent=2)+"\n", encoding="utf-8")


if __name__ == "__main__":
    main()
