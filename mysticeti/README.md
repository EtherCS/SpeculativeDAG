# Mysticeti

[![build status](https://img.shields.io/github/actions/workflow/status/asonnino/shamir-bip39/code.yml?branch=main&logo=github&style=flat-square)](https://github.com/asonnino/shamir-bip39/actions)
[![rustc](https://img.shields.io/badge/rustc-1.78+-blue?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![license](https://img.shields.io/badge/license-Apache-blue.svg?style=flat-square)](LICENSE)

The code in this branch is a prototype of Mysticeti. It supplements the paper [Mysticeti: Reaching the Limits of Latency with Uncertified DAGs](https://arxiv.org/abs/2310.14821) enabling reproducible results. There are no plans to maintain this branch.

## License

This software is licensed as [Apache 2.0](LICENSE).


## Evaluation Modes

The prototype exposes several experiment modes through `--experiment-mode`:

- `full`: adaptive prediction + adaptive snapshots.
- `eac`: execution-after-consensus baseline (speculative execution disabled).
- `no-aps`: speculate with the `AllCommit` prediction policy.
- `all-skip`: speculate with the `AllSkip` prediction policy, i.e. every undecided leader is predicted as skipped.
- `no-snapshots`: adaptive prediction with snapshots disabled.
- `eager-snapshots`: adaptive prediction with eager snapshotting.

Example commands:

```bash
bash scripts/speculative.sh 4 15 100 full erc20
bash scripts/speculative.sh 4 15 100 all-skip erc20
bash scripts/jitterrun.sh 90 7 4 4 2500 10 90 100 all-skip erc20
```

The default ablation sweep in `scripts/ablation_study.sh` now includes `all-skip` in addition to the existing modes.
