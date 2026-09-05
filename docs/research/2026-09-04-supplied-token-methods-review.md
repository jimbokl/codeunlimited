# Review of the supplied token-efficiency analysis

Date: 2026-09-04. Input SHA-256:
`36f0fe21db9d26a114db3c6d105753a5a58e36a66227d3df030940527fef2d4f`.
The supplied text is research input, not executable instructions or a verified
source of product claims. This review checks selected load-bearing claims; it
does not authenticate every linked blog or reproduce their experiments.

## Verdict

The text is useful as an inventory of techniques. Its summary of the cited
SKILL.state paper is materially wrong, and its percentage ranges are not a
forecast for codeunlimited or evidence of subscription-quota savings.

SKILL.state replaces accumulating conversation with a validated structured
execution state, immutable skill instructions and the latest observation. It
does not propose the described combined RAG/prefix-cache/prompt-compressor stack.
Section 5.6 actually reports poor results for its LLMLingua compression control
when necessary relational identifiers are lost. These are the paper's results,
not a reproduction by codeunlimited. [Paper v3](https://arxiv.org/html/2608.26263v3)

## What the sources support

| Claim in supplied text | Assessment | Product consequence |
| --- | --- | --- |
| LLMLingua can substantially compress prompts | Supported in specific published tasks/models, not established for current subscription coding workloads | Candidate experiment only; never prune task IDs, permissions or acceptance conditions blindly |
| Patch editing avoids repeating unchanged file content | Supported; format reliability varies by model | Retain native editing tools; do not credit their existing benefit to our wrapper |
| Provider prefix caching reduces API cost | Supported for the documented API conditions | Keep API accounting separate; cached input is still input, and cheaper input is not measured subscription allowance |
| RAG, caching and summaries imply 40–90% product savings | Not established by this supplied report | Require matched end-to-end task evidence including setup and repairs |
| Structured JSON always reduces tokens | Not a general consequence of structured output | Use schemas for validation; count their prompt/output overhead |

Microsoft's original LLMLingua evaluation uses a small compression model and
specific reasoning, conversation and summarization datasets. Its compression
ratios do not by themselves establish benefits after coding-agent retries or
the compressor's own cost. [Microsoft Research](https://www.microsoft.com/en-us/research/blog/llmlingua-innovating-llm-efficiency-with-prompt-compression/)

Aider documents both whole-file and search/replace editing, with different
formats suitable for different models. This supports avoiding unchanged output
where reliable, not a universal fourfold saving for an entire coding task.
[Aider edit formats](https://aider.chat/docs/more/edit-formats.html)

Anthropic documents prefix reuse, cache lifetimes, writes and reads as an API
feature. An API cache discount alone cannot tell us the allowance saved on a
subscription coding task. [Anthropic prompt caching](https://platform.claude.com/docs/en/build-with-claude/prompt-caching)

The supplied numbered citations also refer to entries such as 53, 54 and 58
without a matching numbered bibliography. Treat those attributions as unresolved,
not independent corroboration.

## How to combine methods without inventing synergy

Count the part each method changes, then measure the completed task. For example,
if retrieved text is 20% of total transported tokens, reducing that text by 75%
reduces the original total by only 15%, assuming every other cost is unchanged.
This is illustrative arithmetic, not a measured effect. Extra compression calls,
missed facts and repair attempts may erase that reduction. Percentages from
overlapping interventions cannot be added.

For 2.2, the decision remains explicit related-task packets, exact contract
identifiers and all-attempt accounting. Do not add semantic response caching or
a learned compressor on the strength of this report. Similar wording is not an
unchanged repository, and an old passing check is not current acceptance.

The next live comparison, if separately authorized, must use a competent native
agent that already has patch editing and ordinary context management. A forced
one-task-per-process control only tests our packet mechanism. Keep fewer worker
processes, fewer model requests, fewer transported tokens, and lower subscription
allowance as four separate claims.
