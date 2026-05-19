# Infrastructure

- Cargo workspace, edition 2024, resolver 3, MSRV 1.95.
- 14 crates in `crates/`: shell, core, network, storage, **knowledge**, bench, engine/{html-parser, css-parser, dom, layout, paint, font, encoding, image}.
- Bundled assets: `assets/fonts/Inter-Regular.ttf` (+ OFL.txt license).
- Test page: `samples/page.html` with embedded `<style>`.
- 5 allowed external dependencies: `winit = "0.30"`, `wgpu = "26"`, `rustls = "0.23"` + `webpki-roots = "0.26"` (active in lumen-network), JS engine (reserved), SQLite via `rusqlite` with feature `bundled` (reserved — will be activated at first persistent backend).
- Internal deps: workspace.dependencies on 11 crates.
- `.gitattributes` enforces LF for all text files; binary marker for `.ttf / .png / .woff2`.
- `.gitignore` ignores `/target`, `/*.zip`, `/*.tar*`, `.idea/`, `.vscode/`, swap files.

## Numbers

- **Total tests in workspace:** ~2310 (after find-in-page branch: +26 in lumen-shell — 24 in `find` module + 2 in keybinding for Ctrl+F).
- **`cargo clippy --workspace --all-targets -- -D warnings`** passes without warnings.
- **External runtime dependencies:** 3 active (winit, wgpu, SQLite via rusqlite/bundled) + 2 reserved (rustls active in lumen-network, JS engine).
- **Transitively via wgpu/winit:** ~200 crates.
