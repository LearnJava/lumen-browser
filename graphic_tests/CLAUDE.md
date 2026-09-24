# graphic_tests — context for the pixel pipeline

Loaded when a session works under `graphic_tests/`. Full manual — [`docs/graphic-tests.md`](../docs/graphic-tests.md).

- **The full run (`python graphic_tests/run.py --continue-on-fail`, ~20 min) is foreground only, in a focused window** — gdigrab captures the whole desktop. Backgrounded, it fails at TEST-00; with focus stolen mid-run, every later screenshot is garbage. A lone TEST-00 failure means re-run, not "my change broke everything".
- **Never edit a test page to work around an engine limit.** The pages are ground truth as Edge renders them; the only valid edit is a bug in the test itself.
- **Never change a threshold** — 0.5 % for every test. A page that cannot reach it gets a `KNOWN_DEBTORS` entry backed by an OPEN bug.
- **No screenshots in the repo.** Commit only `results/*.json` after a full run.
- The pipeline keeps its calibrated `--deterministic --viewport 1024x720` window — the `--maximized` rule for real-site testing does not apply here.
