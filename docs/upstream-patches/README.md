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

## A patch here does not unblock Lumen — only upstream can

Learned on GAP-DOCALLDDA, 2026-09-19, and true for every patch that adds a C
wrapper to `rusty_v8`'s `binding.cc`: **the consumer never compiles that file.**
`build_binding()` in the crate's `build.rs` runs only under `V8_FROM_SOURCE`;
an ordinary build calls `download_static_lib_binaries()` and links the prebuilt
`rusty_v8.lib` that denoland's CI produced, where the new wrapper does not
exist. So neither a `[patch.crates-io]` onto a patched fork nor a vendored copy
helps: the Rust side would call a symbol that is not in the library. Only a
published crate release (or a local `V8_FROM_SOURCE` build — `gn`/`ninja`/
`depot_tools`, hours, tens of GB) carries it.

What *does* work when the underlying **V8** function is already compiled into
that library — check with
`grep -oa '<Name>[A-Za-z0-9_@?$]*' target/<profile>/gn_out/obj/rusty_v8.lib` —
is binding it from a translation unit of our own, the way
[`crates/js/cpp/undetectable.cc`](../../crates/js/cpp/undetectable.cc) does. The
patch file stays here as the upstream contribution
([denoland/rusty_v8#2078](https://github.com/denoland/rusty_v8/pull/2078)); when
it ships in a release, the local wrapper is deleted in favour of the crate's.
