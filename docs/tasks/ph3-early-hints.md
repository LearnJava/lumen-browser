# Задача: fetch Priority Hints + 103 Early Hints

**Developer:** P1
**Ветка:** `p1-early-hints`
**Размер:** M
**Крейты:** `lumen-network`, `lumen-js`, `lumen-core`

## Goal
1. **Priority Hints** (HTML LS §17.2.3 / Fetch §2.2): `fetchpriority="high|low|auto"` на
   `<img>`/`<link>`/`<script>` + `{priority}` в `fetch()`-init → влияние на порядок выборки.
2. **103 Early Hints** (RFC 8297): обработать informational 1xx-ответ с `Link: rel=preload`,
   начать preconnect/preload подресурсов до финального ответа.

## Current state (сверено с кодом 2026-07-05)
### Priority — частично есть на уровне ядра
- `crates/core/src/event.rs:66-88` — enum `FetchPriority { High=0, Medium=1, Low=2 }`
  + `for_kind()` (эвристика по типу подресурса: CSS/шрифт=High, script=Medium, img=Low).
- `crates/core/src/event.rs:109-113` — `Event::SubresourceHintFound { url, kind, priority }`
  (preload-сканер уже проставляет приоритет по типу).
- **НЕТ**: чтения HTML-атрибута `fetchpriority` (grep по `fetchpriority` в `crates/` — 0);
  приоритет считается только эвристикой по типу, автор-override игнорируется.
- **НЕТ**: `{priority}` в `fetch()`-init на JS-стороне.
- RFC 9218 `Priority:` заголовок в исходящем запросе: упоминается только в fingerprint-
  профиле Firefox (`crates/network/src/http/headers.rs:229`), не привязан к FetchPriority.

### 103 Early Hints — НЕТ (главный гэп)
- `crates/network/src/lib.rs:418-463` — `read_head()` читает ПЕРВУЮ status-line и трактует
  её как ФИНАЛЬНУЮ (`parse_status` на `lib.rs:429`). **1xx informational не пропускаются.**
  103-ответ будет ошибочно принят за финальный статус, тело сломается.
- Grep `103`/`Early.?Hints` по `crates/**/*.rs` → совпадения только в бинарных/несвязанных
  местах (qpack, hpack, icc); реальной обработки Early Hints нет.

## Entry points
- `crates/network/src/lib.rs:418` — `read_head()` (сюда добавить цикл пропуска 1xx + сбор 103).
- `crates/network/src/lib.rs:429` — `parse_status` (различить 1xx vs финальный).
- `crates/core/src/event.rs:66` — `FetchPriority` (сюда добавить `Auto`/author-override).
- `crates/core/src/event.rs:109` — `SubresourceHintFound` (учесть explicit fetchpriority).
- HTML-атрибут `fetchpriority`: искать место парсинга `<link rel=preload>`/`<img>` в
  preload-сканере (`SubresourceHintFound`-эмиттер) и в `lumen-dom`.
- `crates/js/src/dom.rs` — `fetch()` init-объект (добавить чтение `priority`).

## Срезы (декомпозиция)
### Срез 1 — S — `read_head` пропускает 1xx (кроме 103)
В `read_head` (`lib.rs:418`): если status ∈ [100,199] и ≠103 → отбросить заголовки этого
блока и читать следующую status-line (цикл). Это чинит и потенциальный `100 Continue`.
Юнит-тест: mock-сервер шлёт `100 Continue\r\n\r\n` перед `200 OK`.

### Срез 2 — M — Парсинг 103 Early Hints
В цикле среза 1: при status==103 собрать `Link:`-заголовки блока, вернуть их отдельно
(новое поле в `ResponseHead`/`Response`), затем продолжить чтение до финального ответа.
Юнит-тест: `103` с `Link: </a.css>; rel=preload; as=style` + затем `200 OK`.

### Срез 3 — S — Проброс Early Hints в preload-конвейер
Распарсить `Link: rel=preload/preconnect` из 103 → эмитить `SubresourceHintFound`
(переиспользовать существующий preload-путь) ДО получения финального body. Preconnect/
prefetch стартуют раньше — суть RFC 8297.

### Срез 4 — S — HTML-атрибут `fetchpriority`
Читать `fetchpriority` на `<img>`/`<link>`/`<script>` в preload-сканере/DOM; override
эвристики `FetchPriority::for_kind`. Добавить `FetchPriority::Auto` или отдельный
`Option<explicit>` слой. Юнит-тест: `<img fetchpriority=high>` → High, `low` → Low.

### Срез 5 — XS — `fetch(url, {priority})` на JS-стороне
Читать `init.priority` (`'high'|'low'|'auto'`) в `fetch()`-шиме `dom.rs`, прокинуть в
запрос (маппинг на RFC 9218 `Priority:` или внутренний приоритет). Юнит-тест наличия.

### Срез 6 — XS — Доки
`CAPABILITIES.md` (network/fetch) 🟡→✅ по частям; `ROADMAP.md:161` уточнить; `subsystems/`.

## Tests
- `lumen-network`: skip 1xx (`100 Continue`), парсинг 103 + `Link`, финальный статус корректен.
- `lumen-network`: 103 с несколькими `Link` → несколько preload-хинтов.
- `lumen-js`: `fetch(u,{priority:'high'})` не бросает; атрибут `fetchpriority` читается.
- Регресс: обычный ответ без 1xx работает как раньше (`read_head` не изменил семантику).

## Definition of done
- [ ] `read_head` пропускает informational 1xx, финальный статус читается верно.
- [ ] 103 Early Hints парсятся, `Link: rel=preload/preconnect` эмитят подресурс-хинты.
- [ ] `fetchpriority` HTML-атрибут переопределяет эвристику приоритета.
- [ ] `fetch()` init читает `priority`.
- [ ] Тесты зелёные; `CAPABILITIES.md`/`ROADMAP.md`/`subsystems/` обновлены.
