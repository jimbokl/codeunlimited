# Version 2.2: explicit work packets and complete attempt records

Version 2.2 adds an opt-in managed-plan path to the bounded runtime. A plan is
validated and snapshotted before execution. Its declared dependencies, group,
risk, scope set, and packet cap determine which tasks enter each worker start;
`run packet` exposes that selection without calling a provider. Runs without a
plan retain the existing behavior.

Managed completion is deliberately strict. A response may accept only an
ordered prefix of the selected packet, and each accepted packet must pass the
one frozen verification command. The runtime derives the remaining queue from
the plan. Scope paths are planning metadata, not an operating-system sandbox,
and freezing verifier argv does not freeze mutable tests or process environment.

## Accounting and recovery

An intent is written before provider dispatch. Every completed, failed, or
recovered attempt remains in the immutable ledger, including unknown usage.
`run ledger` reports coverage and normalized counters without converting
missing data to zero. The optional total-token cap controls whether another
worker may start; it does not terminate a current call and can be exceeded by
that call.

Recovery is for an interrupted or ambiguous dispatch. Stop any old worker,
inspect the repository, then acknowledge the accepted state. A normal
`blocked` result is terminal rather than a resumable pause. Resolving that
blocker and starting a new run is an operator decision.

## Offline evidence

The reproducible fixture invokes only the public CLI and a local deterministic
worker. With a packet cap of one, four accepted tasks require four successful
fixture process starts. With a cap of four, the same tasks require one start.
Both arms produce the same four independently checked files.

```bash
cargo build --release --locked
python3 scripts/benchmark_packets.py --binary target/release/codeunlimited --json
```

The saved report is synthetic offline evidence. Its process counts are not
model request counts. Its prompt-byte totals are rendered `run prompt` output,
not token counts or hidden provider traffic. No provider/model call or competent
native-agent comparison was run, so real token savings remain unmeasured. The
binary hash records the caller-supplied artifact but does not attest that it was
built from the reported source revision.

Larger packets can avoid repeated process startup but may increase repair cost.
A matched-quality, increasing-horizon experiment against a native agent is still
required before enabling packets by default or claiming realized savings.
