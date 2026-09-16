# BUG-651: `file://` URL passed as CLI page arg fails to load — `PageSource::from_arg` never strips the scheme

**Статус:** FIXED 2026-09-17 (P3)
**Компонент:** shell (`crates/shell/src/page_source.rs::PageSource::from_arg`)
**Найден:** P2, WPT-VENDOR-permissions-request (live-probe setup), 2026-08-05

## Симптом

Passing a `file://` URL directly as the CLI page argument failed to load, on
every entry point that resolves its source via `PageSource::from_arg`
(`--dump-layout`, `--dump-display-list`, `--screenshot`, `--print-to-pdf`, and
the initial positional `<src>` of `--mcp-live-port N <src>`/`--bidi-port N
<src>` when it is not `about:blank`):

```
$ lumen.exe --dump-layout 'file:///D:/RustProjects/lumen-browser/samples/page.html'
Ошибка dump file:///D:/RustProjects/lumen-browser/samples/page.html: Синтаксическая ошибка
в имени файла, имени папки или метке тома. (os error 123)
```

The same file loaded fine when passed as a bare path (`--dump-layout
samples/page.html`), and a `file://` URL loaded fine when passed to the
`navigate` MCP/BiDi tool *after* startup instead of as the initial CLI arg.

## Причина

`PageSource::from_arg` (moved to `crates/shell/src/page_source.rs` by the
SPLIT track since the bug was filed against `main.rs:3424-3438`) had no
`file://` case at all — anything that isn't `http(s)://`/`about:blank`/the
chrome-preview URL fell through to `PageSource::File(PathBuf::from(s))` with
the **whole** string, scheme included. On Windows, `PathBuf::from("file:///D:/...")`
is not a valid path (colon after the drive letter's own colon, treated as an
alternate-data-stream separator) and `File::open` returned `ERROR_INVALID_NAME`
(os error 123); on POSIX the literal path `file:///abs/path` simply doesn't
exist, so it would have failed as a plain "not found" instead.

The sibling function `page_source_for_automation_url` (used for JS-initiated
navigation and BiDi/MCP `navigate` calls) already did this correctly via the
shared helper `crate::resource_base::file_url_to_path` (strips the `file://`
prefix and, on a drive-letter path, the extra leading `/` too). `from_arg`
never reused it.

## Фикс

`from_arg`'s fallback arm now tries `resource_base::file_url_to_path(s)`
first and only falls back to the bare-path `PathBuf::from(s)` when the string
isn't a `file://` URL — the same rule `page_source_for_automation_url` already
applied, now shared instead of duplicated-and-missing.

### Проверено

- `crates/shell/src/tests/cli.rs`: two new tests —
  `page_source_from_arg_file_url_strips_scheme` (Windows drive-letter form)
  and `page_source_from_arg_file_url_posix_style` (`file:///abs/path`);
  existing `page_source_from_arg_file`/`page_source_from_arg_url`/
  `page_source_from_arg_none_is_empty` still pass unchanged.
- `cargo clippy -p lumen-shell --all-targets -- -D warnings` — clean.
- `scripts/scoped-test.sh` — full pass (see commit).

### Что осталось

None — the fix is a straight reuse of the existing, already-correct helper;
no follow-up filed.
