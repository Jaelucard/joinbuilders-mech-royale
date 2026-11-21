"""Frozen factorial screen and prospective validation; never changes production source."""
import argparse
import concurrent.futures
import hashlib
import itertools
import json
import subprocess
import tempfile
from datetime import datetime, timezone
from functools import lru_cache
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
V3 = "411af5f530934aa21d8cb5dfff35259219595159"
NAV = ["baseline", "safe", "canopy", "safe_canopy"]
COMBAT = ["baseline", "shared", "window", "shot"]
BRAIN = ["baseline", "drop", "threat", "drop_threat"]


def sha(value):
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


@lru_cache(maxsize=None)
def blob(path):
    return subprocess.check_output(["git", "show", f"{V3}:{path}"], cwd=ROOT).decode("utf-8")


def identifier(options):
    return "n-{}_c-{}_b-{}".format(*options)


BASELINE = identifier(("baseline",) * 3)


def compose(options):
    import navigation, combat, brain
    names = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", V3, "src"], cwd=ROOT, text=True).splitlines()
    sources = {name:blob(name) for name in names}
    for module, option in zip([navigation, combat, brain], options):
        sources = module.apply(sources, option)
    return sources


def harness():
    s = (ROOT / "sim/src/main.rs").read_text(encoding="utf-8")
    s = s.replace("use mechbot::map::ArenaMap;", "use mechbot::protocol;\nmod environment_map;\nuse environment_map::ArenaMap;")
    needle = "run_match(1000 + m as u64 * 7919, practice, &cfg, &map)"
    assert s.count(needle) == 1
    s = s.replace(needle, 'run_match(1000 + (std::env::var("START_INDEX").unwrap().parse::<u64>().unwrap() + m as u64) * 7919, practice, &cfg, &map)')
    assert s.count("r * 0.75") == 3
    s = s.replace("r * 0.75", "r")
    start = s.index("fn seg_hits_cylinder(")
    end = s.index("\nfn main()", start)
    s = s[:start] + '''fn seg_hits_cylinder(a: Vec3, b: Vec3, base: Vec3, r: f64, h: f64) -> bool {
    let d=[b[0]-a[0],b[1]-a[1],b[2]-a[2]];
    let q=[a[0]-base[0],a[1]-base[1]];
    let aa=d[0]*d[0]+d[1]*d[1];
    let bb=2.0*(q[0]*d[0]+q[1]*d[1]);
    let cc=q[0]*q[0]+q[1]*q[1]-r*r;
    let (mut lo,mut hi)=(0.0_f64,1.0_f64);
    if aa < 1e-12 { if cc>0.0 { return false; } }
    else {
        let disc=bb*bb-4.0*aa*cc;
        if disc<0.0 {return false;}
        lo=lo.max((-bb-disc.sqrt())/(2.0*aa));
        hi=hi.min((-bb+disc.sqrt())/(2.0*aa));
    }
    let (zlo,zhi)=(base[2]-0.3,base[2]+h+0.3);
    if d[2].abs()<1e-12 { if a[2]<zlo || a[2]>zhi {return false;} }
    else {
        let (u,v)=((zlo-a[2])/d[2],(zhi-a[2])/d[2]);
        lo=lo.max(u.min(v));hi=hi.min(u.max(v));
    }
    lo<=hi
}
#[cfg(test)] mod geometry_audit {
    use super::*;
    #[test] fn slanted_entry_is_detected() {assert!(seg_hits_cylinder([-5.,0.,0.],[5.,0.,20.],[0.,0.,0.],2.5,9.));}
    #[test] fn high_and_lateral_misses_stay_misses() {assert!(!seg_hits_cylinder([-5.,0.,20.],[5.,0.,20.],[0.,0.,0.],2.5,9.));assert!(!seg_hits_cylinder([-5.,4.,0.],[5.,4.,0.],[0.,0.,0.],2.5,9.));}
    #[test] fn vertical_entry_and_tangent_are_detected() {assert!(seg_hits_cylinder([0.,0.,-5.],[0.,0.,20.],[0.,0.,0.],2.5,9.));assert!(seg_hits_cylinder([-5.,2.5,4.],[5.,2.5,4.],[0.,0.,0.],2.5,9.));}
}
''' + s[end:]
    marker="    let elapsed = t0.elapsed().as_secs_f64();"
    assert s.count(marker)==1
    s=s.replace(marker, marker+'''
    for (i,r) in results.iter().enumerate() {
      println!("EXPERIMENT_RESULT {}",serde_json::json!({
       "seed":1000+(std::env::var("START_INDEX").unwrap().parse::<u64>().unwrap()+i as u64)*7919,
       "mode":mode,"points":r.points,"place":r.place,"kills":r.kills,"loadout":r.loadout,
       "killed_by":r.killed_by,"gun_shots":r.gun_shots,"gun_hits":r.gun_hits,
       "laser_shots":r.laser_shots,"laser_hits":r.laser_hits
      }));
    }
''')
    manifest=(ROOT/"sim/Cargo.toml").read_text(encoding="utf-8").replace('[dependencies]','[dependencies]\nserde = { version = "=1.0.219", features = ["derive"] }')
    lock=(ROOT/"sim/Cargo.lock").read_text(encoding="utf-8")
    old='name = "mechsim"\nversion = "0.1.0"\ndependencies = [\n "mechbot",\n "serde_json",\n]'
    assert lock.count(old)==1
    lock=lock.replace(old,old.replace(' "serde_json",',' "serde",\n "serde_json",'))
    return {"sim/src/main.rs":s,"sim/src/environment_map.rs":blob("src/map.rs"),
            "sim/Cargo.toml":manifest,"sim/Cargo.lock":lock,
            "sim/hello_config.json":(ROOT/"sim/hello_config.json").read_text(encoding="utf-8")}


def map_hashes():
    return {str(p.relative_to(ROOT)).replace('\\','/'):hashlib.sha256(p.read_bytes()).hexdigest() for p in [ROOT/'kit/arena/v2/world.json',ROOT/'kit/arena/v2/terrain.i16']}


def run_one(options, sources, common, image, suite, matches, start, modes, frozen_map):
    name=identifier(options);out=HERE/"results"/suite;out.mkdir(parents=True,exist_ok=True)
    path=out/(name+".json")
    source_hashes={k:sha(v) for k,v in sources.items()}
    common_hashes={k:sha(v) for k,v in common.items()}
    assert map_hashes()==frozen_map
    if path.exists():
        old=json.loads(path.read_text(encoding="utf-8"))
        assert old["source_sha256"]==source_hashes and old["harness_sha256"]==common_hashes
        assert old["matches"]==matches and old["start_index"]==start and old["modes"]==modes
        assert old["map_sha256"]==frozen_map and old["image_id"]==image
        return old
    with tempfile.TemporaryDirectory(prefix="mech-experiment-") as temp:
        stage=Path(temp)
        for filename, content in {**sources,**common,"Cargo.toml":blob("Cargo.toml"),"Cargo.lock":blob("Cargo.lock")}.items():
            p=stage/filename;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(content,encoding="utf-8",newline="\n")
        command=["docker","run","--rm","--network","none","--cpus","1","--memory","1g",
                 "--mount",f"type=bind,source={stage},target=/snapshot,readonly",
                 "--mount",f"type=bind,source={ROOT/'kit'},target=/build/kit,readonly",
                 "-e",f"START_INDEX={start}","-e",f"MATCHES={matches}","-e","MODES="+" ".join(modes),
                 "-e",f"CHECK_GEOMETRY={int(suite=='smoke' or (name==BASELINE and suite=='screen'))}", image,"sh","-c",
                 'set -eu; cp -r /snapshot/. /build/; if [ "$CHECK_GEOMETRY" = 1 ]; then cargo test --release --locked --offline --manifest-path /build/sim/Cargo.toml geometry_audit; fi; cargo build --release --locked --offline --manifest-path /build/sim/Cargo.toml; for mode in $MODES; do VERBOSE=1 /build/sim/target/release/mechsim "$mode" "$MATCHES"; done']
        with (out/(name+".log")).open("w",encoding="utf-8") as log:
            result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
    if result.returncode:raise RuntimeError(f"{name} failed; inspect {out/(name+'.log')}")
    assert map_hashes()==frozen_map,"Arena changed during a run"
    lines=(out/(name+".log")).read_text(encoding="utf-8").splitlines()
    rows=[json.loads(line.removeprefix("EXPERIMENT_RESULT ")) for line in lines if line.startswith("EXPERIMENT_RESULT ")]
    assert len(rows)==matches*len(modes)
    record={"id":name,"options":options,"suite":suite,"matches":matches,"start_index":start,"modes":modes,
            "completed_utc":datetime.now(timezone.utc).isoformat(),"source_sha256":source_hashes,"harness_sha256":common_hashes,
            "image_id":image,"map_sha256":frozen_map,
            "means":{m:sum(r["points"] for r in rows if r["mode"]==m)/matches for m in modes},"outcomes":rows,
            "kind":"approximate corrected local simulator; not official scoring"}
    path.write_text(json.dumps(record,indent=2)+"\n",encoding="utf-8")
    print(name,record["means"],flush=True)
    return record


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("suite",choices=["smoke","screen","validation","repeat"])
    parser.add_argument("--workers",type=int,default=3)
    args=parser.parse_args()
    combinations=list(itertools.product(NAV,COMBAT,BRAIN))
    if args.suite in ["validation","repeat"]:
        selection=json.loads((HERE/'selection.json').read_text(encoding="utf-8"))
        combinations=[("baseline",)*3,tuple(selection["options"])]
    if args.suite=="smoke":combinations=[("safe_canopy","shot","drop_threat")]
    # Freeze all source before any job begins; running jobs cannot observe later edits.
    snapshots={identifier(c):compose(c) for c in combinations}
    common=harness()
    image=subprocess.check_output(["docker","image","inspect","--format","{{.Id}}","mechbot:scored-v3-builder"],text=True).strip()
    frozen_map=map_hashes()
    if args.suite in ["validation","repeat"]:
        assert frozen_map==selection['map_sha256'] and image==selection['image_id']
        assert {k:sha(v) for k,v in common.items()}==selection['harness_sha256']
    for c in combinations:
        if args.suite in ["validation","repeat"] and identifier(c)!=BASELINE:
            assert {k:sha(v) for k,v in snapshots[identifier(c)].items()}==selection["source_sha256"]
    matches,start,modes=(1,0,["practice"]) if args.suite=="smoke" else ((48,0,["practice"]) if args.suite=="screen" else (96,72,["practice","ffa"]))
    errors=[]
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures={pool.submit(run_one,c,snapshots[identifier(c)],common,image,args.suite,matches,start,modes,frozen_map):c for c in combinations}
        for index,future in enumerate(concurrent.futures.as_completed(futures),1):
            try:future.result()
            except Exception as e:errors.append(str(e));print('ERROR',e,flush=True)
            print(f"{args.suite} completed {index}/{len(combinations)}",flush=True)
    if errors:raise SystemExit("\n".join(errors))


if __name__=="__main__":main()
