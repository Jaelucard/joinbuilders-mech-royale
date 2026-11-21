"""Run a bounded map scan inside the preserved v4 builder; no bot source edits."""
import hashlib
import json
import subprocess
from pathlib import Path

root=Path(__file__).resolve().parents[2]
out=Path(__file__).resolve().parent
source=subprocess.check_output(['git','show','411af5f530934aa21d8cb5dfff35259219595159:src/map.rs'],cwd=root).decode('utf-8')
start=source.index('    pub fn is_blocked_at(')
end=source.index('    /// Ultra-fast 3D Line-of-Sight',start)
method=source[start:end].replace('pub fn is_blocked_at(','fn audit_v3_is_blocked_at(',1)
(out/'legacy_method.rs').write_text('impl ArenaMap {\n'+method+'}\n',encoding='utf-8')
image='mechbot:research-builder-5b783d1'
image_id=subprocess.check_output(['docker','image','inspect','--format','{{.Id}}',image],text=True).strip()
cmd=['docker','run','--rm','--network','none','--cpus','1','--memory','1g',
     '--mount',f'type=bind,source={out},target=/audit,readonly',
     '--mount',f'type=bind,source={root / "kit/arena"},target=/input/arena,readonly',
     image_id,'sh','-c',
     'cat /audit/legacy_method.rs /audit/audit.rs >> /build/src/map.rs && cargo test --release --lib --locked --offline terrain_resolution_audit -- --nocapture']
with (out/'output.log').open('w',encoding='utf-8') as log:
    proc=subprocess.run(cmd,stdout=log,stderr=subprocess.STDOUT,text=True)
text=(out/'output.log').read_text(encoding='utf-8')
if proc.returncode:
    print(text[-6000:]);raise SystemExit(proc.returncode)
result=json.loads(next(line.split('AUDIT_JSON ',1)[1] for line in text.splitlines() if 'AUDIT_JSON ' in line))
result['synthetic_witnesses']=[json.loads(line.split('SYNTHETIC_JSON ',1)[1]) for line in text.splitlines() if 'SYNTHETIC_JSON ' in line]
result['v3_commit']='411af5f530934aa21d8cb5dfff35259219595159'
result['v4_commit']='5b783d19f138239033be32b527318f9247a9f9b8'
result['builder_image_id']=image_id
result['input_sha256']={str(p.relative_to(root)).replace('\\','/'):hashlib.sha256(p.read_bytes()).hexdigest() for p in [out/'audit.rs',out/'legacy_method.rs',root/'kit/arena/v2/world.json',root/'kit/arena/v2/terrain.i16']}
(out/'results.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
print(json.dumps({k:v for k,v in result.items() if k not in ['witnesses','input_sha256']},indent=2))
