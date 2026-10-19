# WPT `compat/` — регулярный прогон с reftest (COMPAT-CSS-WPT)

Набор `tests/wpt/compat/` (Compat Standard: `-webkit-*` алиасы, legacy flexbox, `-webkit-gradient`, `::-webkit-scrollbar`,
`-webkit-text-fill-color`) — 38 автоматизируемых id по манифесту. Строки-источники: `CSS-SPECS.md` §«Compat», подпункты `COMPAT-CSS-*`.

## Результат 2026-10-14 (первый прогон с reftest)

Бинарь: `main` e0c276e88 + `loading: false` в `crates/shell/src/chrome_ui.rs` (см. BUG-1630; без этой правки testharness
не исполняется). Три прогона подряд дали один и тот же результат. Снимок — [`runs/2026-10-14-compat.json`](runs/2026-10-14-compat.json).

| | значение |
|---|---|
| id по шкале wpt.fyi | **16 из 38 (41,68 %)** |
| testharness-id, harness OK | 13 (из них полностью зелёных 4) |
| reftest-id | 25 (**9 PASS**, 16 FAIL) — исполнялись впервые |
| подтестов | 50 из 138 |
| статусы | FAIL=16, OK=13, PASS=9 |

Прежний `run_report.py` видел 13 testharness-id из 39 и 9 из 71 подтеста; reftest в нём не было.

Полностью зелёные: `historical` 3/3, `webkit-box-removing-triggering-anonymous-merge` 1/1, `webkit-gradient-sign` 2/2,
`webkit-scrollbar-parse` 1/1; reftest — `webkit-background-origin-text`, `-box-align-001`, `-box-clamp-bottom-border`,
`-box-clamp-visibility-change`, `-box-fieldset`, `-box-z-index`, `-linear-gradient-diff-unprefixed`,
`-text-fill-color-property-003`, `-005`.

**Обновление 2026-10-18 (CSS-WEBKIT-GRADIENT):** `-webkit-linear/radial/repeating-*-gradient()` и `-webkit-gradient()` реализованы
(`style/parse/webkit_gradient.rs`); `webkit-linear-gradient-line-{top,right,bottom,left}`, `-diff-unprefixed`,
`webkit-gradient-comma` 2/2, `webkit-radial-gradient-radii` 3/3, `-gradient-sign` — зелёные, `.ini` этих id сняты.
По шкале wpt.fyi **22 из 38 (57,03 %)**, подтестов 103 из 138 (прогон `run_corpus.py --prefixes compat`, dev-release).

**Обновление 2026-10-18 (CSS-WEBKIT-BOX-LEGACY):** `-webkit-box-pack/-align/-flex/-direction/-ordinal-group` реализованы поверх flex (`style/cascade.rs`); `webkit-box-align-001`, `-vertically-centered`, `-horizontal-reverse-variants`, `-horizontal-rtl-variants`, `-rtl-flex`, `-ignores-flex-wrap.tentative`, `-blockify` — зелёные, их `.ini` сняты; `-item-shrink-001/002` сократили провалы до 5 и 2 подтестов. По шкале wpt.fyi **28 из 38 (74,82 %)**, подтестов 118 из 138 (прогон `run_corpus.py --prefixes compat`, dev-release).

## Провалы и привязка

| id | результат | строка CSS-SPECS / BUG |
|---|---|---|
| `css-style-declaration-alias-enumeration` | 51/51 | CSS-SPECS.md:781 (`-webkit-*` алиасы, COMPAT-CSS-ALIAS) |
| `webkit-mask-box-enumeration` | 6/6 | [BUG-1631](../../bugs/BUG-1631-FIXED.md) (`webkitMaskBoxImage*`, 2026-10-19) |
| `idlharness.window` | 23/29 (`window.orientation`, `onorientationchange` у Window и HTMLBodyElement) | [BUG-1632](../../bugs/BUG-1632-OPEN.md) |
| `webkit-box-item-shrink-001` 15/20 (подтесты 3, 9, 11, 13, 20), `-002` 16/18 (3, 11) | | CSS-SPECS.md:785 (legacy flexbox: нет «no automatic minimum size» у `input`/`textarea`/`select`/`iframe`/`video`) |
| `webkit-text-fill-color-property-001a/b/c/d, 002, 004, 006` (reftest FAIL); `webkit-text-fill-color-currentColor` 0/1 | | CSS-SPECS.md:786 (`-webkit-text-fill-color`) |

Каждый провал привязан к строке CSS-SPECS или к BUG; непривязанных нет.

## Блокер прогона: BUG-1630 (исправлен 2026-10-18)

На `main` testharness по BiDi стоял в `TIMEOUT` (первый плохой коммит 27e264dc5 UX-LOADING): спиннер загрузки хрома
гнал бесконечную цепочку перерисовок, и `about_to_wait` не вызывался. Исправлено в [BUG-1630](../../bugs/BUG-1630-FIXED.md);
прогон `compat/` воспроизводится на обычном бинаре без `loading: false`.

## Ожидания (`.ini`)

`tests/wpt/metadata/compat/` — testharness обновлён `run_report.py --update-expected`; для 16 FAIL-reftest заведены
`<id>.html.ini` с `expected: FAIL` и комментарием-привязкой (их читает `--check` только для testharness; reftest
исполняет `run_corpus.py`, ожидания там — документ).

## Повтор (после каждого среза P4)

```
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
<venv>/python tests/wpt/run_corpus.py --binary <lumen.exe> --prefixes compat \
  --out-dir .tmp/wpt-compat --processes 7 --run-json .tmp/wpt-compat/run.json
<venv>/python tests/wpt/run_report.py --binary <lumen.exe> --all --root compat --recursive --check
```

`MANIFEST.json` в git не лежит: первый прогон без `--skip-manifest-update` пересоздаёт его (~40 с). Интерпретатор с
патчем pywebsocket3 — `p2-work/tests/wpt/.venv`. Прогон `compat/` занимает ~14 с.
