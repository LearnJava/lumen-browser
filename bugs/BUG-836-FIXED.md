# BUG-836 — `sessionStorage` не переживает навигацию: каждый новый документ вкладки получает пустое хранилище (`localStorage` при этом сохраняется)

**Статус:** FIXED 2026-08-23 (P1)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 21 — найден живым замером, маркера намеренно нет)
**Область:** `crates/js/src/v8_runtime.rs:1358` (`install_dom` создаёт `ss_store` как `WebStorage::default()` без входного параметра — в отличие от `ls_store` строкой выше)
**Владелец:** P1/P3 (`lumen-js` + `lumen-shell`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
// документ 1
sessionStorage.setItem("k", "1");
localStorage.setItem("k", "1");
location.href = "next.html";
// документ 2 той же вкладки и того же origin
sessionStorage.getItem("k")   // → null   (должно: "1")
localStorage.getItem("k")     // → "1"    — корректно
```

## Прямое измерение

`tests/wpt/verify_navigation_form_import_gaps.py --variant session-storage-across-reload`
(2026-08-22, dev-release, Linux, коммит `762a0cad9`, `--seconds 14`,
обе страницы живы — 26 тиков суммарно):

| ожидалось | получено |
|---|---|
| `seen=1` на втором документе | документ 1: `seen=null local=null`; документ 2: `seen=null local=1` |

`local=1` во второй строке — это и есть контроль: механизм хранения работает
и переживает навигацию, отваливается именно session-половина.

Побочный эффект, найденный при этом же замере: первая редакция пробы
`nav-location-reload` использовала `sessionStorage` как тормоз против
повторной перезагрузки — и ушла в бесконечный цикл `location.reload()`,
потому что флаг после перезагрузки всегда `null`. Ровно то же произойдёт с
любой страницей, которая так защищается.

## Причина (локализована чтением кода)

```rust
let ls_store =
    ls_store.unwrap_or_else(|| Arc::new(Mutex::new(lumen_core::WebStorage::default())));
let ss_store: Arc<Mutex<lumen_core::WebStorage>> =
    Arc::new(Mutex::new(lumen_core::WebStorage::default()));   // v8_runtime.rs:1358
```

`ls_store` приходит аргументом (шелл держит его между документами), а
`ss_store` **не имеет соответствующего параметра вовсе** и создаётся пустым
на каждой установке DOM, то есть на каждый документ. HTML LS §12.2 требует
обратного: session storage привязан к browsing context (вкладке) и живёт,
пока жива вкладка. Юнит-тест `dom.rs:25046` фиксирует нынешнее поведение
(«sessionStorage is NOT shared; each runtime gets a fresh instance») — он
про изоляцию двух рантаймов, но при починке его формулировку придётся
уточнить: изоляция нужна между вкладками, а не между документами одной.

## Масштаб

Маркера в `timeout_audit.py` намеренно нет: 16 остаточных id `/webstorage`
— это 12 тестов `event_*`, ждущих событие `storage` из `<iframe>`
([BUG-480](BUG-480-OPEN.md), уже атрибутированы), и 4 квотных теста, где
причина другая. Заводится по прямому замеру.

Вне WPT цена высокая и тихая: `sessionStorage` — стандартное место для
токена формы, шага визарда, «уже показали баннер», защиты от повторной
перезагрузки. Всё это на Lumen молча теряется при каждом переходе, без
единой ошибки.

## Направление починки (не предписание)

Добавить `ss_store` в сигнатуру `install_dom` рядом с `ls_store` и хранить
его в шелле на уровне вкладки (там же, где `ls_store`), очищая при закрытии
вкладки, а не при смене документа. Ключевание — по origin, как требует
спека.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_navigation_form_import_gaps.py
   --variant session-storage-across-reload` — на втором документе ожидается
   `seen=1`.
2. WPT: `run_report.py --all --root webstorage --recursive`.

## Починено (2026-08-23, P1)

Владельцем `sessionStorage` сделана **вкладка**, а не документ.

* `lumen-js`: у `V8JsRuntime` появилось поле `ss_store` и строитель
  `with_session_storage(Arc<Mutex<WebStorage>>)` — по образцу уже
  существовавшего `with_sw_worker_store`. `install_dom` берёт хранилище оттуда
  и создаёт пустое, только если владелец ничего не передал. Сигнатура
  `install_dom` (11 аргументов, ~40 вызовов в юнит-тестах) не тронута: это
  строитель, а не ещё один параметр.
* `lumen-shell`: у `Lumen` и у снапшота вкладки появилась карта
  `ss_storage: HashMap<origin, Arc<Mutex<WebStorage>>>` — ровно рядом с
  `ls_storage` и с тем же ключом (`ss_store_for_base`, общий с ls хелпер
  `storage_origin_for_base`). Карта уезжает в снапшот при переключении вкладки,
  возвращается при переключении обратно и **обнуляется только при открытии
  новой вкладки** — то есть переживает любую смену документа и не утекает в
  соседнюю вкладку, как и требует HTML LS §12.2. `ss_store` протянут той же
  цепочкой, что `ls_store` (`PageSource::load` → `render_bytes` →
  `parse_and_layout` → `run_scripts_with_dom`, плюс `load_frame_sub_documents`
  и `hibernate::restore_js_context`); у `<iframe>` с opaque origin он
  отфильтровывается тем же `.filter(|_| !opaque)`.
* Путь bfcache: страница с живым рантаймом паркуется целиком (BUG-835), поэтому
  хранилище едет вместе с ней; `bfcache_thaw` (страницы без JS, ставит свежий
  рантайм) теперь тоже получает хранилище вкладки.
* Головные режимы (`--dump-layout`/`--screenshot`/`--print-to-pdf`,
  растеризация SVG) передают `None` — одноразовый рендер не browsing context.

### Замер после фикса

`verify_navigation_form_import_gaps.py --variant session-storage-across-reload
--seconds 8` (2026-08-23, dev-release, Windows):

| | было (2026-08-22) | стало |
|---|---|---|
| документ 1 | `seen=null local=null` | `seen=null local=null` |
| документ 2 | `seen=null local=1` | **`seen=1 local=1`** |

Побочный эффект («страница с флагом-тормозом в `sessionStorage` уходит в
бесконечный `location.reload()`») снят тем же фиксом: флаг теперь виден.

Юнит-тесты (`crates/js/src/dom.rs`): `session_storage_fresh_per_runtime`
переименован в `session_storage_fresh_per_runtime_without_owner` — изоляция
осталась требованием, но теперь это про **разные** browsing context'ы (никто не
передал хранилище); рядом добавлен
`session_storage_persists_across_documents_of_a_tab` на общий `Arc`.

Не входило в фикс: квоты (`QuotaExceededError`) — [BUG-870](BUG-870-OPEN.md),
событие `storage` между документами — отдельный механизм, и перенос карты
через T3-гибернацию (там на диск уезжают только байты страницы, у `ls_storage`
ровно то же ограничение).
