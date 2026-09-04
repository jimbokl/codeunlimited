# Additional context-saving research inputs

Saved 2026-09-04 for the codeunlimited roadmap. User-supplied analysis is a
research lead, not implementation authority or product evidence. The supplied
SKILL.state analysis has SHA-256
`05ef5176c6c9fe9d74fb7469708de6e79807180868cafe2b513a4a91e88c788d`.

## Corrections checked against primary sources

- [SKILL.state v3, Tables 1, 2, and 5](https://arxiv.org/html/2608.26263v3)
  supports 65,408 versus 1,062,387 total tokens at horizon 100 (about 16.2x),
  for its warehouse experiment. The 6,175,509 total at horizon 200 belongs to
  the Memory/Summary baseline; LangGraph-style uses 5,041,164. The 0.18/0.22
  scores are budget-matched truncation/LLMLingua controls, not noise levels.
  The paper warns that statistical compression can remove essential identifiers.
- [arXiv:2606.09659](https://arxiv.org/abs/2606.09659) is *End-to-End Context
  Compression at Scale* (LCLMs), not the supplied “Context Cascade Compression”
  title. Its trained encoder/decoder architecture is not a text-only wrapper
  that can be installed around closed subscription models.
- [arXiv:2303.12570](https://arxiv.org/abs/2303.12570) is RepoCoder, not
  LLMLingua. [LLMLingua-2](https://arxiv.org/abs/2403.12968) is correctly
  identified by 2403.12968, but the supplied reference [56] links a different ID.
- The supplied “tokens per step” arithmetic mixes prompt averages, total
  tokens, and procedural horizon. Keep the paper's metric definitions; do not
  derive a per-model-call average by dividing by a different unit of steps.

## Implications for this product

Prioritize sufficient bounded state, evidence retention, focused repository
retrieval, and bounded observations. Keep raw artifacts available outside the
hot prompt so omitted details can be retrieved. Do not apply lossy statistical
pruning to code identifiers, evidence, constraints, or verification results by
default. AST/retrieval integration needs its own matched-quality evaluation.

State bounding and prefix caching can complement each other, but affect
different quantities: bounding can reduce transported input, while caching
reduces repeated processing or API price. Savings percentages do not add.
Account for boot prompts, state maintenance, retrieval, retries, output, and
verification on both sides. Subscription quota recovery cannot be inferred
from API prices. No paid experiment was run for these notes.

## Gemini Context Caching — saved, deferred

The user's separate Gemini note proposes an optional future Google API layer:
implicit prefix reuse plus explicit `CachedContent` objects, addressed by
`cached_content=cache.name`, with TTL management and a
`usage_metadata.cached_content_token_count` counter. It also proposes caching
PDF/audio/video context via uploads and accounting for cache storage over time.

The following are preserved as **unverified supplied claims**, not release
constants: a 90% read discount, full-price cache creation, $0.50–$1.00 per
million token-hours of storage, 2048/4096-token thresholds, implicit retention
of 5 minutes to 24 hours, default explicit TTL of 60 minutes, and effectively
unbounded extendable TTL. Example supplied prices for Gemini 2.5 Pro were
$1.25/M input, $0.125/M cache read, and $1.00/M token-hours storage. The supplied
model names included Gemini 2.5 Flash/Pro, 3.5 Flash, 3.1 Pro, and 3.8 Flash;
availability and each model's terms must be verified before implementation.

The supplied Python flow uses `google-genai`: `client.files.upload`, then
`client.caches.create(model=..., config=CreateCachedContentConfig(contents=[file],
system_instruction=..., ttl="900s"))`, then `client.models.generate_content`
with `GenerateContentConfig(cached_content=cache.name)`. The example's metadata
lookup also requires SDK verification. Do not implement or execute it in 2.1.

Preserved official links for that later investigation:

- [Gemini API caching guide](https://ai.google.dev/gemini-api/docs/caching)
- [Gemini implicit caching announcement](https://developers.googleblog.com/en/gemini-2-5-models-now-support-implicit-caching/)
- [Vertex AI context caching overview](https://cloud.google.com/blog/products/ai-machine-learning/vertex-ai-context-caching)
- [Vertex AI cached-content documentation](https://docs.cloud.google.com/vertex-ai/generative-ai/docs/context-cache/context-cache-overview)
- [Google platform pricing](https://cloud.google.com/gemini-enterprise-agent-platform/generative-ai/pricing)

Future acceptance criteria: separate subscription and API accounting, include
storage/write/read/output costs, test expiry and cache invalidation, preserve
unknown counters, and measure total cost per quality-matched completed task.
