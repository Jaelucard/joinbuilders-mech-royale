"""Summarize matched-seed local proxy runs, explicitly not official scores."""
import argparse
import json
import random
import statistics
from pathlib import Path


def interval(deltas):
    rng = random.Random(481)
    means = sorted(statistics.mean(rng.choices(deltas, k=len(deltas))) for _ in range(10000))
    return [means[249], means[9749]]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline")
    parser.add_argument("candidates", nargs="+")
    parser.add_argument("--output", default="research/evaluation/comparison.json")
    args = parser.parse_args()
    base = json.loads(Path(args.baseline).read_text())
    report = {"kind":"approximate local simulator; not official scoring", "official_user_reported_baseline":4.81,
              "official_candidate_score":None,"baseline_label":base["label"],"comparisons":[],
              "limitations":["Uncalibrated opponent/physics model; shared production geometry.",
                             "HashMap tie ordering can vary across repeats despite identical match seeds.",
                             "Archived proxy survival_s assigns 480 seconds to every survivor, including early wins; these means are not actual elapsed survival measurements.",
                             "Intervals describe paired proxy outcomes, not official score uncertainty."]}
    for path in args.candidates:
        cand = json.loads(Path(path).read_text())
        for shared in ["sim/src/main.rs","sim/hello_config.json","kit/arena/v2/world.json","kit/arena/v2/terrain.i16"]:
            assert base["input_sha256"][shared] == cand["input_sha256"][shared], shared
        for mode in ["practice","ffa"]:
            old = {r["seed"]:r for r in base["outcomes"] if r["mode"]==mode}
            new = {r["seed"]:r for r in cand["outcomes"] if r["mode"]==mode}
            seeds = sorted(old.keys() & new.keys())
            if not seeds:
                continue
            deltas = [new[s]["points"]-old[s]["points"] for s in seeds]
            row = {"label":cand["label"],"mode":mode,"n":len(seeds),
                   "baseline_mean":statistics.mean(old[s]["points"] for s in seeds),
                   "candidate_mean":statistics.mean(new[s]["points"] for s in seeds),
                   "mean_difference":statistics.mean(deltas),"paired_bootstrap_95":interval(deltas),
                   "better":sum(x>0 for x in deltas),"equal":sum(x==0 for x in deltas),"worse":sum(x<0 for x in deltas),
                   "baseline_survival_s":statistics.mean(old[s]["survival_s"] for s in seeds),
                   "candidate_survival_s":statistics.mean(new[s]["survival_s"] for s in seeds),
                   "baseline_low_placement":sum(old[s]["points"]<=2 for s in seeds),
                   "candidate_low_placement":sum(new[s]["points"]<=2 for s in seeds)}
            report["comparisons"].append(row)
            print(f"{row['label']} {mode} n={len(seeds)}: {row['baseline_mean']:.3f} -> {row['candidate_mean']:.3f}; "
                  f"delta {row['mean_difference']:+.3f}, paired proxy95% {row['paired_bootstrap_95']}; "
                  f"better/equal/worse {row['better']}/{row['equal']}/{row['worse']}")
    Path(args.output).write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")


if __name__ == "__main__":
    main()
