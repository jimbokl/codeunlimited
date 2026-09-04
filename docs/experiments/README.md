# Experiment evidence

The dated JSON artifacts in this directory are self-identifying snapshots of
bounded `codeunlimited experiment` runs. Reproduce one with the listed release
build and CLI commands, the recorded Git revisions, and the exact half-open
Unix or RFC 3339 windows.

Artifacts keep validated experiment names, Git revisions, task counts,
aggregate request/session/token counters, derived comparison values, and an
independent arithmetic check. They exclude local paths, machine identifiers,
content fields, raw log records, and finding details.

An exact observed counter total is not a causal estimate. Compare repeated,
similar tasks when possible, keep success criteria fixed, and treat an arm with
fewer than three completed tasks as low confidence.
