# BUG-651: `file://` URL passed as CLI page arg fails to load — `PageSource::from_arg` never strips the scheme

**Статус:** OPEN
**Компонент:** shell (`crates/shell/src/main.rs:3424-3438` — `PageSource::from_arg`)
**Найден:** P2, WPT-VENDOR-permissions-request (live-probe setup), 2026-08-05

## Симптом

Passing a `file://` URL directly as the CLI page argument fails to load, on
every entry point that resolves its source via `PageSource::from_arg`
(`--dump-layout`, `--dump-display-list`, `--screenshot`, `--print-to-pdf`, and
the initial positional `<src>` of `--mcp-live-port N <src>`/`--bidi-port N
<src>` when it is not `about:blank`):

```
$ lumen.exe --dump-layout 'file:///D:/RustProjects/lumen-browser/samples/page.html'
Ошибка dump file:///D:/RustProjects/lumen-browser/samples/page.html: Синтаксическая ошибка
в имени файла, имени папки или метке тома. (os error 123)
```

The same file loads fine when passed as a bare path (`--dump-layout
samples/page.html`), and a `file://` URL loads fine when passed to the
`navigate` MCP/BiDi tool *after* startup instead of as the initial CLI arg.

## Причина

`PageSource::from_arg` (`main.rs:3424-3438`, used by `--dump-layout`/
`--dump-display-list`/`--screenshot`/`--print-to-pdf` and by `parse_cli`'s
`OpenWindow` case at `main.rs:3759`, which is what the initial `--mcp-live-port
N <src>`/`--bidi-port N <src>` argument goes through) has no `file://` case at
all — anything that isn't `http(s)://`/`about:blank`/the chrome-preview URL
falls through to `PageSource::File(PathBuf::from(s))` with the **whole**
string, scheme included. On Windows, `PathBuf::from("file:///D:/...")` is not
a valid path (colon after the drive letter's own colon, treated as an
alternate-data-stream separator) and `File::open` returns `ERROR_INVALID_NAME`
(os error 123); on POSIX the literal path `file:///abs/path` simply doesn't
exist, so it would fail as a plain "not found" instead.

A sibling function, `page_source_for_automation_url` (`main.rs:562-583`,
already carries a doc comment recording exactly this Windows drive-letter
gotcha and works around it — used for JS-initiated navigation and BiDi/MCP
`navigate` calls), does this correctly: it strips the `file://` prefix and, on
a drive-letter path, the extra leading `/` too. `from_arg` never reuses it.

In practice every script in this repo that drives `--mcp-live-port`/
`--bidi-port` with a local file (`graphic_tests/run.py`, `scripts/scroll_perf.py`,
`scripts/mem_perf.py`, `scripts/input_perf.py`, `scripts/mt_stall_bench.py`,
`scripts/scroll_blit_accept.py`) works around this by starting the process
with `about:blank` and then calling the `navigate` MCP/BiDi tool with the real
`file://` URL — none of them pass the URL as the initial positional CLI arg.
That convention is precisely why this gap has stayed unnoticed: every
production caller happens to avoid the broken path. Found only because a
one-off live-probe script (WPT-VENDOR-permissions-request, checking
`navigator.permissions.request`) passed a `file://` URL directly as the
initial arg instead of following that convention.

## Как воспроизвести

```
lumen.exe --dump-layout 'file:///D:/RustProjects/lumen-browser/samples/page.html'
# or
lumen.exe --mcp-live-port <N> 'file:///D:/RustProjects/lumen-browser/samples/page.html'
```
Both fail; the same path without the `file://` prefix, or `about:blank`
followed by an MCP/BiDi `navigate` call to the `file://` URL, both work.

## Возможный фикс

Have `PageSource::from_arg` delegate its `file://` handling to (or share the
same logic as) `page_source_for_automation_url`'s drive-letter-aware
stripping, instead of duplicating a narrower special-case set.
