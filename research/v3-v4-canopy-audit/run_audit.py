import json
import subprocess
from pathlib import Path

audit=Path(__file__).resolve().parent
root=audit.parents[1]
image="mechbot:research-builder-5b783d1"
image_id=subprocess.check_output(["docker","image","inspect","--format","{{.Id}}",image],text=True).strip()
command="mkdir -p /build/src/bin && cp /audit/canopy_audit.rs /build/src/bin/canopy_audit.rs && cargo run --quiet --release --locked --offline --bin canopy_audit"
proc=subprocess.run(["docker","run","--rm","--network","none","--cpus","1","--memory","1g",
    "--mount",f"type=bind,source={audit},target=/audit,readonly",
    "--mount",f"type=bind,source={root / 'kit/arena'},target=/arena,readonly",
    image_id,"sh","-c",command],capture_output=True,text=True)
(audit/"output.log").write_text(proc.stdout+proc.stderr,encoding="utf-8")
rows={"image":image,"image_id":image_id,"source_commit":"5b783d19f138239033be32b527318f9247a9f9b8","exit_code":proc.returncode,"witnesses":[]}
for line in proc.stdout.splitlines():
    if line.startswith("CANOPY_WITNESS "): rows["witnesses"].append(json.loads(line.split(" ",1)[1]))
    elif line.startswith("CANOPY_SUMMARY "): rows["grid_summary"]=json.loads(line.split(" ",1)[1])
    elif line.startswith("CANOPY_SCAN "): rows["scan_summary"]=json.loads(line.split(" ",1)[1])
(audit/"result.json").write_text(json.dumps(rows,indent=2)+"\n",encoding="utf-8")
print(proc.stdout)
if proc.returncode: print(proc.stderr[-5000:])
raise SystemExit(proc.returncode)
