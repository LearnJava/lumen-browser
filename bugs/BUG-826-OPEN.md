# BUG-826 — `<link rel=preload|modulepreload|prefetch>` не грузится вообще: хинт пишется в stderr и выбрасывается, событий `load`/`error` нет

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 20 — 37 TIMEOUT остатка, механизм `preload-hint-never-fetched`)
**Область:** `crates/shell/src/main.rs:285` (единственный потребитель `Event::SubresourceHintFound` — логгер stderr), `crates/shell/src/main.rs:6807` (`dispatch_preload_hints`, доккоммент прямо говорит «в Phase 0 sink логирует в stderr; в будущем запустит fetch через HttpClient»), `crates/engine/html-parser/src/preload_scanner.rs:144` (`rel`-токены: знает `stylesheet`/`preload`/`preconnect`/`dns-prefetch`, `modulepreload` и `prefetch` не распознаются вовсе), `crates/js/src/dom.rs` (`_lumen_resource_track` — белый список `script`/`link`, но грузится только `rel=stylesheet`; см. собственный тест-охранник `dynamic_preload_link_fires_nothing`)
**Владелец:** P1/P3 (шелл + `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Ссылка-хинт не приводит ни к одному запросу и не сообщает о себе странице
ничем. Ни `load`, ни `error`, ни таймаут — тишина:

```js
// shadow-dom/declarative/…/shadowrootadoptedstylesheets-modulepreload-basic.html
const link = document.createElement("link");
link.rel = "modulepreload";
link.href = cssUrl;
const loadPromise = new Promise((resolve, reject) => {
  link.onload = resolve;
  link.onerror = reject;
});
document.head.appendChild(link);
await loadPromise;          // ← дальше этой строки тест не проходит никогда
```

Особенно обманчива парсерная форма: в stderr **есть** строка
`⤷ preload js [medium] http://…/psag-asset.js?parsed-preload`, то есть по
логу выглядит так, будто хинт отработал. Запроса при этом не было.

## Прямое измерение

`tests/wpt/verify_preload_script_audio_gaps.py` (2026-08-22, коммит
`79f7df91a`, `--seconds 5`, все пробы живы — по 9 тиков). Колонка «сервер
видел» — независимая половина замера: пробный http-сервер записывает каждый
запрошенный путь, поэтому «запроса не было» не зависит от того, что о себе
сообщает страница.

| проба | маркеры страницы | сервер видел |
|---|---|---|
| `link-stylesheet` (контроль) | `link-load rel=stylesheet` | `/psag-asset.css?stylesheet` |
| `link-preload-script` | только `link-appended` | ничего |
| `link-preload-style` | ничего | ничего |
| `link-modulepreload` | только `link-appended` | ничего |
| `link-prefetch` | ничего | ничего |
| `link-preload-parsed` | только `parsed-link rel=preload` (при этом в stderr `⤷ preload js [medium] …`) | ничего |
| `link-preload-404` | ничего | ничего |

Контроль важен вдвойне: `rel=stylesheet`, созданный тем же
`createElement`, и грузится, и стреляет `load` (BUG-722) — значит дефект не
в «созданных скриптом ссылках» и не в машинерии событий как таковой, а
именно в видах хинтов.

## Причина (локализована чтением кода)

Цепочка обрывается на первом же шаге. Preload-сканер
(`preload_scanner.rs`) находит хинт и складывает `PreloadHint`;
`dispatch_preload_hints` (`main.rs:6807`) резолвит URL, сортирует по
приоритету и эмитит `Event::SubresourceHintFound`. Единственный потребитель
этого события во всём воркспейсе — `match` в `main.rs:285`, который печатает
`⤷ preload …` в stderr. Fetch-а нет: доккоммент функции честно фиксирует это
как Phase 0. То есть preload-сканер сегодня — украшение лога.

Три независимые грани сверху:

1. **Скриптовый путь.** `_lumen_resource_track` пропускает `script`/`link`,
   но реально грузится только `rel=stylesheet`; в `dom.rs` для этого есть
   собственный тест-охранник `dynamic_preload_link_fires_nothing`, который
   *утверждает* нынешнее поведение («A `rel=preload` link must not be
   fetched behind the page's back — no event either way»). Починка обязана
   его переписать, а не обойти.
2. **Сканер не знает половины `rel`.** `modulepreload` и `prefetch` не
   попадают ни в один вариант `match` (`preload_scanner.rs:144`), поэтому
   для них нет даже строки в логе.
3. **`error` тоже никогда.** Проба `link-preload-404` не получила ничего:
   тесты, которые ждут отказа (`*_deny` в `connection-allowlist`,
   `modulepreload-failure`, CSP-шные `font-*-blocked`), виснут ровно так же,
   как ждущие успеха.

## Масштаб

Механизм `preload-hint-never-fetched` забирает **37 id** остатка снимка
WPT-RUN-5 (крупнейший механизм среза 20), а по всему снимку — 45 id, считая
те, что раньше висели на более слабых стадиях. Состав: `preload/*` 12,
`connection-allowlist/tentative` 10 (все 10 остатка категории — они опрашивают
серверный key-value store в `while (true)` до появления преload-нутого URL),
`shadow-dom/declarative/…/shadowrootadoptedstylesheets-modulepreload-*` 6,
`html/semantics/scripting-1/…/modulepreload-referrer*` 2,
`content-security-policy/font-src` 2, `resource-timing` 2, остальное россыпью.

Оценка снизу: реф-тесты вида `css/css-backgrounds/background-attachment-353.html`
(`<link rel=preload as=image onload="takeScreenshot()">`) сюда не входят —
у них нет harness-вывода вообще.

Вне WPT цена та же: сайт, который преload-ит шрифт или скрипт (обычная
практика), на Lumen не получает ни ускорения, ни события; а `modulepreload`
плюс `import` того же URL приводит к повторной загрузке.

## Направление починки (не предписание)

Провести `SubresourceHintFound` до сетевого слоя: тот же путь, по которому
шелл уже грузит `<link rel=stylesheet>`, с кэшированием ответа под URL, чтобы
последующий реальный запрос (`<script src>`, `import`, `<img>`) забирал тело
из preload-кэша, как того требует HTML LS §4.6.7 «link type preload».
Отдельно — уведомить JS-сторону: `load`/`error` на элементе `<link>` идут по
той же машинерии `_lumen_resource_*`, что уже работает для `rel=stylesheet`.
Минимальный первый шаг, дающий больше половины охвата: `preload` и
`modulepreload` (и токены в `preload_scanner.rs`), `prefetch` — потом.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_preload_script_audio_gaps.py
   --variant link-preload-script --variant link-modulepreload
   --variant link-preload-404 --variant link-preload-parsed` — в колонке
   «сервер видел» появляются запрошенные файлы, а страница печатает
   `link-load …` (и `link-error 404` для несуществующего).
2. WPT: `run_report.py --all --root preload --recursive` и
   `--root connection-allowlist` — семейства перестают висеть.
