"""Reproduce observed baseline/candidate defects in disposable offline containers."""
import argparse
import json
import subprocess
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("variant", choices=["baseline", "candidate"])
    args = parser.parse_args()
    audit = Path(__file__).resolve().parent
    root = audit.parents[1]
    image = {
        "baseline": "mechbot:research-builder-5b783d1",
        "candidate": "mechbot:candidate-builder",
    }[args.variant]
    image_id = subprocess.check_output(
        ["docker", "image", "inspect", "--format", "{{.Id}}", image], text=True
    ).strip()
    commands = {
        "navigation": (
            "cat /probe/navigation_start_clearance.rs >> /build/src/map.rs && "
            "cargo test --lib --locked --offline audit_startclearance -- --nocapture"
        ),
        "combat": (
            "mkdir -p /build/src/bin && "
            "cp /probe/cooldown_witness.rs /build/src/bin/cooldown_witness.rs && "
            "cargo run --quiet --offline --locked --bin cooldown_witness"
        ),
    }
    results = {"variant": args.variant, "image": image, "image_id": image_id, "checks": {}}
    for name, command in commands.items():
        proc = subprocess.run(
            ["docker", "run", "--rm", "--network", "none", "--cpus", "1", "--memory", "1g",
             "--env", f"AUDIT_VARIANT={args.variant}",
             "--mount", f"type=bind,source={audit},target=/probe,readonly",
             "--mount", f"type=bind,source={root / 'sim'},target=/input,readonly",
             "--workdir", "/build", image, "sh", "-c", command],
            capture_output=True, text=True,
        )
        log = audit / f"{name}-{args.variant}.log"
        log.write_text(proc.stdout + proc.stderr, encoding="utf-8")
        results["checks"][name] = {"exit_code": proc.returncode, "log": log.name}
        print(f"{args.variant} {name}: exit {proc.returncode}; {log.name}", flush=True)
    (audit / f"{args.variant}-run.json").write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    if any(check["exit_code"] for check in results["checks"].values()):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
