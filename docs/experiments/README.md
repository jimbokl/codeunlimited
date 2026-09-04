# Experiment evidence

The dated JSON artifacts in this directory are self-identifying snapshots of
bounded `codeunlimited experiment` runs. Reproduce one with the listed release
build and CLI commands, the recorded Git revisions, and the exact half-open
Unix or RFC 3339 windows.

`2026-09-04-v2.2-packets.json` is different: it is a synthetic offline
public-CLI fixture, not a token-log experiment. It records deterministic worker
process starts, accepted task IDs, rendered prompt-byte totals, final-tree
digests, and caller-supplied binary provenance. It makes no provider/model call
and reports no real token-savings percentage or native-agent comparison.

Artifacts keep validated experiment names, Git revisions, task counts,
aggregate request/session/token counters, derived comparison values, and an
independent arithmetic check. They exclude local paths, machine identifiers,
content fields, raw log records, and finding details.

An exact observed counter total is not a causal estimate. Compare repeated,
similar tasks when possible, keep success criteria fixed, and treat an arm with
fewer than three completed tasks as low confidence.

For controlled work, prefer independent task pairs and the format in
[PAIRED-SCHEMA.md](PAIRED-SCHEMA.md). The v1.9 analyzer reports aggregate
observed counters and an exact paired sign-flip result without exposing task
identifiers.
