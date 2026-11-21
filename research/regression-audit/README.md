# Candidate regression witnesses

These fixtures reproduce the observed unfixed behavior in the preserved images. They are diagnostics, not acceptance tests for a repaired candidate, full physics simulations, or official scores. In particular, the navigation candidate assertion expects the unwanted stop. The combat harness reports real policy outputs with minimal two-tick aim/cooldown progression; it does not simulate projectile hits.

Run from the repository root with the preserved Docker builder images and ignored `sim/hello_config.json` available:

```sh
python research/regression-audit/run_witnesses.py baseline
python research/regression-audit/run_witnesses.py candidate
```

Each run records its image ID and writes logs here. The containers have no network access. Test source is copied/appended only inside disposable containers; repository production source and images are unchanged. No broad proxy matches are run.

`cooldown-output.txt` preserves the agent's first successful experiment. The source was recreated identically from its executed tool input after its original temporary directory was cleaned. The variant-specific logs are fresh executions of these archived fixtures.
