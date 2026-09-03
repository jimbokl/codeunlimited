# Local benchmarking

`scripts/benchmark_local.py` measures the installed Rust binary against local
logs without copying them or retaining audit findings. Performance evidence and
product-outcome evidence are intentionally separate: a faster scan does not by
itself prove that an agent completed more work.

## Run the performance benchmark

Build the exact source under test and run three samples per scenario:

```bash
cargo build --release --locked
python3 scripts/benchmark_local.py \
  --binary target/release/codeunlimited \
  --runs 3 \
  --days 30 \
  --project /absolute/path/to/project \
  --output benchmark.json
```

An existing output is refused. Add `--force` to replace it atomically. Omit
`--output` to print the JSON to stdout. The script uses a temporary
`CODEUNLIMITED_HOME`, so benchmark index state does not replace the user's
normal index.

The scenarios are:

- `fixture`: the repository's small synthetic fixture, with indexing disabled;
- `full_no_index`: all available local logs, with indexing disabled;
- `bounded_time_warm_index`: `--days N` after an unmeasured index warm-up;
- `scoped_no_index`: one project without the index;
- `scoped_warm_index`: one project after an unmeasured index warm-up.

Each sample records only the child exit code, wall-clock seconds, maximum RSS,
Claude/Codex request counts, and scan counters. It does not retain token totals,
projects, paths, models, findings, prompts, responses, child stdout, or child
stderr. The platform block contains OS, architecture, and Python version but no
host name. A failed scenario is still written in this redacted form and makes
the harness exit non-zero.

`wall_seconds.median` is the median measured with `time.perf_counter`;
`wall_seconds.p95` is the observed nearest-rank 95th percentile. RSS comes from
`/usr/bin/time -lp` on macOS or `/usr/bin/time -v` on Linux; unsupported systems
record `null`. `max_rss_bytes` is the largest available RSS among the samples.

## 1.6 baseline and 1.7 acceptance targets

The pre-1.7 baseline was measured on 2026-09-03 on an Apple M4 with 16 GiB RAM,
macOS 26.6.2, against 2,747,957 recognized usage records in about 30.2 GiB of
Codex JSONL. These are local observations, not cross-machine guarantees.

| Scenario | 1.6 wall time | 1.6 maximum RSS |
|---|---:|---:|
| Synthetic fixture | <0.01 s | ~7.2 MB |
| Full audit | 21.39 s | 1,681,604,608 B |
| Last 30 days | 19.38 s | 1,379,155,968 B |
| Project-scoped, no index | 18.86 s | 188,792,832 B |

The fixed 1.7 targets, chosen before measuring the implementation, are:

| Scenario | Target |
|---|---:|
| Warm indexed 30-day audit | <=5 s and <=350 MiB |
| Second warm project-scoped audit | <=1 s and <=200 MiB |
| Full unindexed audit | <=20 s and <=800 MiB |
| Indexed vs unindexed result | Exact semantic parity after removing `scan` |

The dated JSON under `docs/benchmarks/` records hits and misses without moving
these targets after the result is known.

### Measured 1.7 result

The final three-sample run on the same machine completed successfully. The
history was still active during measurement, so retained request counts changed
slightly between samples; timings and RSS are therefore reported as observed,
not as laboratory constants.

| Scenario | Median | p95 | Maximum RSS | Target result |
|---|---:|---:|---:|---|
| Synthetic fixture | 0.0065 s | 0.0078 s | 7,340,032 B | smoke pass |
| Full, no index | 19.6015 s | 19.7042 s | 1,054,916,608 B | time hit; RSS miss |
| 30 days, warm index | 6.8883 s | 6.8888 s | 423,116,800 B | time miss; RSS miss |
| Project scope, no index | 18.5268 s | 18.8338 s | 275,562,496 B | diagnostic only |
| Project scope, warm index | 0.0260 s | 0.0276 s | 8,323,072 B | time and RSS hit |

Against the 1.6 observations, the full median improved by about 8% and maximum
RSS by about 37%; the 30-day median improved by about 64% and RSS by about 69%.
The fixed 5-second/350-MiB bounded targets and 800-MiB full-memory target remain
recorded as misses. The selected worktree had no matching retained sessions in
the scoped run, so its warm-index result is specifically the all-files-skipped
case (660 indexed files skipped). See
[`docs/benchmarks/2026-09-03-m4-16gb.json`](benchmarks/2026-09-03-m4-16gb.json)
for the redacted samples and scan counters.

## Measure whether the utility helps

The product outcome must be measured over real work, separately from scanner
speed. Use at least a 7-day baseline and a 7-day post-adoption period with a
similar task mix, models, and team. Run `init` at the boundary, keep task success
criteria fixed, and record every completed or failed task rather than selecting
only successful examples.

Primary KPI:

```text
completed tasks / million input tokens
```

Guardrails are task success rate, independent quality score, task wall time,
cache-read ratio, context growth, and retry rate. Report the raw before and
after values, workload changes, and uncertainty. A before/after movement is an
observation, not proof that codeunlimited caused it; stronger causal evidence
requires a contemporaneous control or randomized task assignment.
