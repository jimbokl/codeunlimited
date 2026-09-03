# Contributing

Thanks for helping make subscription limits go further.

## Ground rules

- **Privacy is the product.** Detectors may only use token counts, models,
  timestamps and project names. PRs that read prompt or response content
  will be declined regardless of how useful the signal is.
- **No network.** The CLI never makes requests. Anything that needs a
  backend (benchmarks) ships as strict opt-in and lives in a separate
  component.
- **Conservative numbers.** Every estimate needs a documented assumption in
  `docs/ACCURACY.md` - ranges, not hype.
- **English only** in all product texts.

## Dev loop

```bash
cargo build
cargo test              # golden fixtures + detector unit tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

CI runs the same on Linux/macOS/Windows. `codeunlimited doctor` is the
fastest way to check the parsers against your local logs.

## Adding a detector

1. Prototype in the Python sandbox (`codeunlimited/`) if you like, but the
   product implementation lives in `src/detectors.rs`.
2. Return a `Finding` with honest `impact_lo`/`impact_hi` bounds and a
   one-line fix a user can actually apply.
3. Add a unit test with synthetic requests + extend `tests/golden.rs`.
4. Document the math in `docs/ACCURACY.md`.

## Adding a log source

Open an issue with a small anonymized sample of the log format first
(token counts only). Parsers we can't validate against real logs don't
ship - accuracy over coverage.
