"""Compile and run the brain component regressions in isolated offline containers."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
V3 = "411af5f530934aa21d8cb5dfff35259219595159"


def module(name):
    path = ROOT / "research/experiments" / f"{name}.py"
    spec = importlib.util.spec_from_file_location("experiment_" + name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def run(navigation, brain, combat="baseline", legacy_policy=False):
    paths = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", V3, "src"], cwd=ROOT, text=True).splitlines()
    sources = {path:subprocess.check_output(["git", "show", f"{V3}:{path}"], cwd=ROOT).decode("utf-8") for path in paths}
    original = dict(sources)
    sources = module("navigation").apply(sources, navigation)
    if combat != "baseline":
        sources = module("combat").apply(sources, combat)
    before = dict(sources)
    sources = module("brain").apply(sources, brain)
    assert all(sources[name] == text for name,text in before.items() if name != "src/brain.rs")
    if navigation == "safe":
        assert "next_waypoint_with_radius" in sources["src/brain.rs"]
    if brain == "drop":
        assert "my_laser_about_to_kill" in sources["src/brain.rs"]
    if brain == "threat":
        assert "drop_hold" not in sources["src/brain.rs"]
    if legacy_policy:
        # Keep the new tests but restore all legacy production brain behavior.
        suffix = sources["src/brain.rs"].split("#[cfg(test)]", 1)[1]
        sources["src/brain.rs"] = original["src/brain.rs"] + "\n#[cfg(test)]" + suffix
    name = f"{navigation}-{brain}" + (f"-{combat}" if combat != "baseline" else "") + ("-legacy-red" if legacy_policy else "")
    with tempfile.TemporaryDirectory(prefix="brain-experiment-tests-") as temp:
        stage = Path(temp)
        for path,source in sources.items():
            target = stage/path
            target.parent.mkdir(parents=True,exist_ok=True)
            target.write_text(source,encoding="utf-8",newline="\n")
        command = ["docker","run","--rm","--network","none","--cpus","1","--memory","1g",
                   "--mount",f"type=bind,source={stage},target=/snapshot,readonly",
                   "--mount",f"type=bind,source={ROOT/'kit'},target=/build/kit,readonly",
                   "mechbot:scored-v3-builder","sh","-c",
                   "set -eu; cp /snapshot/src/*.rs /build/src/; cargo test --lib --locked --offline"]
        with (OUT/f"{name}.log").open("w",encoding="utf-8") as log:
            result = subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
    print(name,"exit",result.returncode,flush=True)
    success = result.returncode == 0
    if legacy_policy:
        log_text = (OUT/f"{name}.log").read_text(encoding="utf-8")
        success = (result.returncode != 0 and
            "experiment_threat_tests::aimed_charger_is_not_ignored_for_an_unproven_own_kill ... FAILED" in log_text and
            "experiment_threat_tests::charger_facing_elsewhere_does_not_abort_our_charge ... FAILED" in log_text)
    if not success:
        print((OUT/f"{name}.log").read_text(encoding="utf-8")[-7000:])
    return {"navigation":navigation,"brain":brain,"combat":combat,"legacy_policy":legacy_policy,
            "exit_code":result.returncode,"expected_outcome_verified":success,"log":f"{name}.log"}


if __name__ == "__main__":
    combinations = [("baseline","drop"),("baseline","threat"),("baseline","drop_threat"),("safe","drop_threat"),
                    ("safe","drop_threat","window"),("safe","drop_threat","shot")]
    results = [run(*parts) for parts in combinations]
    results.append(run("baseline","threat",legacy_policy=True))
    (OUT/"verification.json").write_text(json.dumps(results,indent=2)+"\n",encoding="utf-8")
    raise SystemExit(any(not r["expected_outcome_verified"] for r in results))
