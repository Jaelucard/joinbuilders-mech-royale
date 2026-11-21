"""Build and verify the frozen selected source without editing the workspace bot."""
import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path
from run_suite import HERE, ROOT, V3, blob, compose, sha


def execute(name, command):
    path=HERE/(name+'.log')
    with path.open('w',encoding='utf-8') as log:
        result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
    print(name,'exit',result.returncode,flush=True)
    if result.returncode:
        print(path.read_text(encoding='utf-8')[-6000:])
        raise SystemExit(result.returncode)
    return path


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--conformance',action='store_true')
    args=parser.parse_args()
    selected=json.loads((HERE/'selection.json').read_text(encoding='utf-8'))
    source=compose(tuple(selected['options']))
    assert {k:sha(v) for k,v in source.items()}==selected['source_sha256']
    if args.conformance:
        manifest=json.loads((HERE/'selected-manifest.json').read_text(encoding='utf-8'))
        assert manifest['source_sha256']==selected['source_sha256']
        execute('selected-conformance',[sys.executable,str(ROOT/'kit/check/check.py'),'--map-dir','/arena','--',
            'docker','run','--rm','-i','--network','none','--cpus','1','--memory','512m','--memory-swap','512m',
            '--pids-limit','128','--cap-drop','ALL','--read-only','--tmpfs','/tmp:rw,size=64m','--user','65534:65534',
            '--mount',f'type=bind,source={ROOT/"kit/arena"},target=/arena,readonly',manifest['runtime_image_id']])
        print((HERE/'selected-conformance.log').read_text(encoding='utf-8'))
        return
    with tempfile.TemporaryDirectory(prefix='selected-bot-') as folder:
        stage=Path(folder)
        files={**source,'Cargo.toml':blob('Cargo.toml'),'Cargo.lock':blob('Cargo.lock'),'Dockerfile':blob('Dockerfile')}
        for name,content in files.items():
            path=stage/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(content,encoding='utf-8',newline='\n')
        execute('selected-builder-build',['docker','build','--target','builder','-t','mechbot:experiment-selected-builder',str(stage)])
        execute('selected-runtime-build',['docker','build','-t','mechbot:experiment-selected',str(stage)])
    execute('selected-tests',['docker','run','--rm','--network','none','--cpus','1','--memory','1g',
        '--mount',f'type=bind,source={ROOT/"kit"},target=/build/kit,readonly','mechbot:experiment-selected-builder','cargo','test','--locked','--offline'])
    witness=execute('selected-cooldown-witness',['docker','run','--rm','--network','none','--cpus','1','--memory','1g',
        '--mount',f'type=bind,source={ROOT/"research/regression-audit"},target=/probe,readonly',
        '--mount',f'type=bind,source={ROOT/"sim"},target=/input,readonly','mechbot:experiment-selected-builder','sh','-c',
        'mkdir -p /build/src/bin && cp /probe/cooldown_witness.rs /build/src/bin/cooldown_witness.rs && cargo run --quiet --offline --locked --bin cooldown_witness'])
    witness_text=witness.read_text(encoding='utf-8')
    for distance in [130,160]:
        assert re.search(rf'd={distance} decision=1 .*fire=\[true, false\]',witness_text), 'Known wasted-aim regression must not be reintroduced'
    assert re.search(r'd=100 decision=0 .*fire=\[false, true\]',witness_text),'Preserve useful mortar fire'
    image=lambda name:subprocess.check_output(['docker','image','inspect','--format','{{.Id}}',name],text=True).strip()
    actual=subprocess.check_output(['docker','run','--rm','--network','none','mechbot:experiment-selected-builder','sh','-c','cd /build && sha256sum src/*.rs'],text=True)
    hashes=dict((path,h) for h,path in (line.split() for line in actual.splitlines()))
    assert hashes==selected['source_sha256']
    manifest={'id':selected['id'],'options':selected['options'],'baseline_commit':V3,'source_sha256':hashes,
              'builder_image_tag':'mechbot:experiment-selected-builder','builder_image_id':image('mechbot:experiment-selected-builder'),
              'runtime_image_tag':'mechbot:experiment-selected','runtime_image_id':image('mechbot:experiment-selected'),
              'tests_pass':True,'official_candidate_score':None,'publication_state':'Not pushed; conditional score/conformance gate still required.'}
    (HERE/'selected-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')


if __name__=='__main__':main()
