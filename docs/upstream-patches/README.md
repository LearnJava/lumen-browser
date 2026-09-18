# Upstream patches

Lumen's working boundary ([`CLAUDE.md`](../../CLAUDE.md) §Working boundary) forbids writing to
anything outside `D:\RustProjects\lumen-browser` — that includes `rusty_v8`, which lives in a
separate GitHub repository (`denoland/rusty_v8`), not in this workspace. When a Lumen defect can
only be fixed by extending an external crate we don't own, the diff is drafted here, against a
named upstream version, for a human to apply, build, and submit as a PR from their own fork.

Each patch file is a plain unified diff, paths relative to the `rusty_v8` repo root. Apply with
`git apply <file>.patch` from a `rusty_v8` checkout at the version named in the file header.

| Patch | Target crate/version | Lumen ticket |
|---|---|---|
| [`rusty_v8-mark-as-undetectable.patch`](rusty_v8-mark-as-undetectable.patch) | `v8` (`rusty_v8`) 150.1.0 | GAP-DOCALLDDA (`ROADMAP.md`) |
