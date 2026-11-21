"""Check frozen combat options on an otherwise unchanged v3 builder, offline."""
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
SPEC = importlib.util.spec_from_file_location("combat_options", HERE.parent / "combat.py")
COMBAT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COMBAT)
V3 = "411af5f530934aa21d8cb5dfff35259219595159"


def main():
    source = {"src/combat.rs":subprocess.check_output(["git","show",f"{V3}:src/combat.rs"],cwd=ROOT).decode("utf-8"),
              "src/brain.rs":"sentinel: prior navigation/brain transformation"}
    original = dict(source)
    assert COMBAT.apply(source, "baseline") == source
    image = subprocess.check_output(["docker","image","inspect","--format","{{.Id}}","mechbot:scored-v3-builder"],text=True).strip()
    records = {"base_revision":V3,"builder_image_id":image,"variants":{}}
    for option in ["shared", "window", "shot"]:
        changed = COMBAT.apply(source, option)
        assert source == original and changed["src/brain.rs"] == source["src/brain.rs"]
        with tempfile.TemporaryDirectory(prefix="combat-option-") as folder:
            stage = Path(folder)
            (stage/"combat.rs").write_text(changed["src/combat.rs"],encoding="utf-8",newline="\n")
            command = ["docker","run","--rm","--network","none","--cpus","1","--memory","1g",
                       "--mount",f"type=bind,source={stage},target=/variant,readonly",image,
                       "sh","-c","cp /variant/combat.rs /build/src/combat.rs && cargo test --lib --locked --offline combat::tests -- --nocapture"]
            result = subprocess.run(command,capture_output=True,text=True)
        (HERE/f"{option}-tests.log").write_text(result.stdout+result.stderr,encoding="utf-8")
        records["variants"][option] = {"exit_code":result.returncode,
             "source_sha256":hashlib.sha256(changed["src/combat.rs"].encode("utf-8")).hexdigest(),
             "test_result":[line for line in result.stdout.splitlines() if line.startswith("test result:")]}
        print(option,records["variants"][option],flush=True)
        if result.returncode:
            print(result.stdout[-6000:]+result.stderr[-3000:])
            break
    (HERE/"validation.json").write_text(json.dumps(records,indent=2)+"\n",encoding="utf-8")
    if any(r["exit_code"] for r in records["variants"].values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
