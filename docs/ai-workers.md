# AI workers (free Kilo Gateway models)

External LLM "workers" the assistant can delegate to via the Kilo Gateway API
(`https://api.kilo.ai/api/gateway/chat/completions`, OpenAI-compatible).
Delegation is opt-in: the user asks for it explicitly (see the mixed-programming
protocol in the assistant's memory); by default the assistant writes code itself.

Selection is based on a coding benchmark run 2026-07-02 across all 7 free
chat-capable models on the gateway (write a spec'd Rust function with unit tests +
one compiler-feedback round; fix a seeded `lower_bound` bug against ground-truth
tests; everything compiled and executed with real `rustc`).

## The three approved models

### 1. `stepfun/step-3.7-flash:free` — fast hands

**Use for:** mechanical edits by strict spec, bug fixes with externally supplied
ground-truth tests, code generation where the contract is fully pinned down.

- Fastest by a wide margin (~51 s per task vs 2–17 min for the others).
- Produced spec-correct function bodies in every benchmark round.
- Proven in production on BUG-228 ("Step 3.7 fixes, assistant tests" protocol).
- **Weakness:** ignores feedback about its own test data — it will not fix a wrong
  assertion even when shown the failure. Always supply tests externally.
- **Gotcha:** set `reasoning: {"effort": "low"}` or it burns tokens on thinking;
  output is often truncated (`finish_reason == "length"`) — request continuation.

### 2. `nvidia/nemotron-3-ultra-550b-a55b:free` — head

**Use for:** complex multi-step tasks, work that needs an honest error→fix cycle,
large-context jobs (1M-token window fits a big slice of the codebase).

- 550B MoE (55B active), hybrid Transformer-Mamba, frontier-reasoning tier.
- Only model (with Nano Omni) that converged to fully green after one feedback
  round — it fixed both its function and its own faulty test.
- Slower (~3 min per task); acceptable for hard tasks.
- Fallback if throttled: `nvidia/nemotron-3-super-120b-a12b:free` (same 1M
  context, mid quality — failed to compile round 1 but recovered on feedback).

### 3. `nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free` — eyes

**Use for:** describing screenshots (graphic-test diagnostics, Lumen vs Edge
comparison in words), any image/audio/video input. The only multimodal free model.

- Input: text + image + audio + video → text. Context 256K.
- Verified: described a `graphic_tests` screenshot (shapes, colors, magenta frame,
  absence of text) with full accuracy; a 20 KB PNG cost only ~770 prompt tokens.
- Image format: standard OpenAI content parts —
  `{"type": "image_url", "image_url": {"url": "data:image/png;base64,…"}}`.
- Code quality is decent too (fully green after one feedback round), so it can
  handle small "look at the picture, then edit" tasks end-to-end.

## Rejected models

| Model | Why not |
|---|---|
| `poolside/laguna-m.1:free` | 17 min/task; "fixes" failures by deleting the tests (green build, zero checks). Matches earlier production experience (empty worktrees, compile-error noise). |
| `cohere/north-mini-code:free` | Same test-deletion behavior on feedback. |
| `poolside/laguna-xs.2:free` | Only model whose code had a real runtime panic (subtract overflow), 4 failing tests in round 1. |
| `kilo-auto/free`, `openrouter/free` | Routers over random free models — not reproducible. |
| `nvidia/nemotron-3.5-content-safety:free` | Moderation guardrail, not a coder. |

## How to call

Client: `.tmp/kilo_client.py` (gitignored; recreate from this spec if missing).
API key: `.tmp/kilo.env` (**never commit**). Model is chosen via the `KILO_MODEL`
env var; default is Step 3.7 Flash.

```bash
KILO_MODEL="nvidia/nemotron-3-ultra-550b-a55b:free" \
  python .tmp/kilo_client.py <messages.json> <reply.txt>
```

Client behavior worth preserving on recreation: strict UTF-8 via
`ensure_ascii=False` bytes (Cyrillic breaks otherwise), progressive backoff on
403/429, auto-continuation on `finish_reason == "length"`,
`reasoning: {"effort": "low"}` in the request body.

## Ground rules

1. **Never trust the worker's own tests.** 6 of 7 models wrote self-contradictory
   test data (declared a valid input invalid). Ground truth comes from us.
2. **Run workers sequentially.** Free-tier throttling is per API key; parallel
   runs starve each other with 429s.
3. **NVIDIA free endpoints log requests** (trial terms, used to improve their
   products). Fine for Lumen code; do not send anything confidential.
4. **Always verify with the compiler.** Every model needed at least one
   `rustc`-feedback round; treat worker output as a draft, not a patch.
5. Benchmark artifacts: `.tmp/kilo_bench.py`, `.tmp/kilo_bench_results.json`
   (gitignored). Re-run when the gateway's free-model roster changes.
