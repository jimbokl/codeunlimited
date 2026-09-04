# Paired experiment schema

`scripts/analyze_paired_experiment.py` analyzes independent, comparable task
pairs. Each task appears once and contains exact observed input-token and
request totals for both arms:

```json
{
  "schema_version": 1,
  "pairs": [
    {
      "task_id": "task-01",
      "control": {"requests": 4, "input_tokens": 120000},
      "treatment": {"requests": 3, "input_tokens": 95000}
    },
    {
      "task_id": "task-02",
      "control": {"requests": 2, "input_tokens": 60000},
      "treatment": {"requests": 3, "input_tokens": 70000}
    }
  ]
}
```

The analyzer requires at least two unique pairs. Counters must be integers;
requests must be positive and input tokens non-negative. Inputs above 20
non-zero pairs are refused because exhaustive `2^n` enumeration is no longer
the intended analysis path.

The output contains no `task_id` values. It reports exact aggregate counters,
the number of pairs where treatment was lower/higher/tied, task-average
tokens-per-request medians, and an exact two-sided sign-flip p-value over
`treatment.input_tokens - control.input_tokens`.

The p-value addresses paired task differences under exchangeability of arm
labels. It does not repair poor randomization, unequal task completion,
different models/tools, selective exclusions, or unmeasured quality loss.
Publish the protocol and all exclusions alongside any result.
