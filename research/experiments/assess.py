"""Choose once on screening data; evaluate the predeclared prospective push gate."""
import argparse
import json
import random
import statistics
from pathlib import Path

HERE=Path(__file__).resolve().parent
BASELINE="n-baseline_c-baseline_b-baseline"


def read(path):return json.loads(path.read_text(encoding="utf-8"))


def interval(deltas):
    rng=random.Random(481)
    n=len(deltas)
    values=sorted(sum(rng.choices(deltas,k=n))/n for _ in range(10000))
    return [values[249],values[9749]]


def comparison(base,candidate,mode):
    assert base["harness_sha256"]==candidate["harness_sha256"]
    assert base["map_sha256"]==candidate["map_sha256"]
    a={r["seed"]:r for r in base["outcomes"] if r["mode"]==mode}
    b={r["seed"]:r for r in candidate["outcomes"] if r["mode"]==mode}
    assert a.keys()==b.keys()
    deltas=[b[s]["points"]-a[s]["points"] for s in sorted(a)]
    return {"n":len(a),"baseline_mean":statistics.mean(x["points"] for x in a.values()),
            "candidate_mean":statistics.mean(x["points"] for x in b.values()),
            "delta":statistics.mean(deltas),"paired_bootstrap_95":interval(deltas),
            "better":sum(x>0 for x in deltas),"equal":sum(x==0 for x in deltas),"worse":sum(x<0 for x in deltas)}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage",choices=["select","gate"])
    args=parser.parse_args();plan=read(HERE/'plan.json')
    if args.stage=="select":
        assert not (HERE/'selection.json').exists(),"Winner already frozen; do not reselect after holdout"
        rows=[read(p) for p in sorted((HERE/'results/screen').glob('*.json'))]
        assert len(rows)==plan['screen']['combinations']
        base=next(r for r in rows if r['id']==BASELINE)
        eligible=[]
        for r in rows:
            assert r['harness_sha256']==base['harness_sha256']
            assert r['map_sha256']==base['map_sha256'] and r['image_id']==base['image_id']
            assert r['matches']==48 and r['start_index']==0
            n,c,b=r['options']
            if n in plan['promotion_eligible_options']['navigation'] and c in plan['promotion_eligible_options']['combat'] and b in plan['promotion_eligible_options']['brain']:
                eligible.append(r)
        eligible.sort(key=lambda r:(-r['means']['practice'],sum(x!='baseline' for x in r['options']),r['id']))
        best=eligible[0]
        selection={'id':best['id'],'options':best['options'],'source_sha256':best['source_sha256'],
                   'baseline_source_sha256':base['source_sha256'],'map_sha256':base['map_sha256'],'image_id':base['image_id'],
                   'harness_sha256':best['harness_sha256'],'screen_comparison':comparison(base,best,'practice'),
                   'eligible_ranking':[{'id':r['id'],'mean':r['means']['practice']} for r in eligible],
                   'all_screen_means':{r['id']:r['means']['practice'] for r in rows},
                   'selection_rule':plan['selection'],'holdout_results_seen':False}
        (HERE/'selection.json').write_text(json.dumps(selection,indent=2)+'\n',encoding='utf-8')
        print(json.dumps({k:v for k,v in selection.items() if k not in ['all_screen_means','source_sha256','harness_sha256']},indent=2))
    else:
        chosen=read(HERE/'selection.json');checks={}
        for suite in ['validation','repeat']:
            base=read(HERE/'results'/suite/(BASELINE+'.json'))
            cand=read(HERE/'results'/suite/(chosen['id']+'.json'))
            assert cand['source_sha256']==chosen['source_sha256']
            assert base['source_sha256']==chosen['baseline_source_sha256']
            for record in [base,cand]:
                for key in ['harness_sha256','map_sha256','image_id']:
                    assert record[key]==chosen[key],key
            assert cand['matches']==96 and cand['start_index']==72
            modes={m:comparison(base,cand,m) for m in ['practice','ffa']}
            modes['score_gate_pass']=modes['practice']['delta']>=0.5 and modes['practice']['paired_bootstrap_95'][0]>0 and modes['ffa']['delta']>=-0.25
            checks[suite]=modes
        result={'candidate':chosen['id'],'repeats':checks,'score_gate_pass':all(v['score_gate_pass'] for v in checks.values()),
                'remaining_gate':'Regression tests and Docker conformance must also pass before any push.',
                'official_candidate_score':None}
        (HERE/'gate.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
        print(json.dumps(result,indent=2))


if __name__=='__main__':main()
