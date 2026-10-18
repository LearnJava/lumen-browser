# BUG-1239 — youtube: после BUG-1207 страница пуста, `ytd-page-manager` без детей, ошибок в логе нет

**Статус:** OPEN
**Компонент:** js — не локализовано (youtube не строит `ytd-browse` главной)
**Найден:** P3, при закрытии [BUG-1207](BUG-1207-FIXED.md), 2026-10-02

## Симптом

Видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, dev-release, `--proxy` (прямое соединение с youtube в этой
сети рвётся на TLS), `https://www.youtube.com/`, 30 с: `[JS error]` нет, `ytd-app` и `ytd-page-manager`
апгрейднуты, но `ytd-page-manager.children` пуст, `ytd-browse`/`ytd-rich-grid-renderer` нет,
`document.body.innerText` пуст, ~1300 элементов (Chrome: 1529–1868). `ytInitialData` определён.

## Что сделать

Выяснить, кто должен создать `ytd-browse` в `ytd-page-manager` (обработчик `yt-page-data-fetched`/навигация
`ytd-app`), и где цепочка обрывается без исключения: `.tmp`-проба с трассой `define`/upgrade/connected, как в
BUG-1207, затем сравнить с Chrome.

## Повтор 2026-10-18 (Chat)

`target/release/lumen.exe` (сборка 2026-10-18 13:40), `--maximized --proxy http://127.0.0.1:12334`
(системный прокси Windows), `LUMEN_NO_ADBLOCK=1`, `https://www.youtube.com/`, ~35 с.

- Сеть в порядке: главная `200`, 318 ответов `200`. Пользователь видит пустую ленту без превью.
- **Превью не запрашиваются вовсе:** к `i.ytimg.com` ушёл только `generate_204`, ни одной картинки
  `/vi/…`. Значит, карточки (`ytd-rich-item-renderer` → `ytd-thumbnail`/`yt-image`) не создаются —
  это тот же обрыв, а не дефект загрузки или декодирования картинок.
- **Появилась ошибка, которой 2026-10-02 не было:**
  `[JS error] CE connectedCallback (upgrade): NotFoundError: Failed to execute 'insertBefore' on 'Node':
  The node before which the new node is to be inserted is not a child of this node.`
  Гипотеза: наш путь upgrade/connected (`_lumen_ce_upgrade_element` / `_lumen_ce_connect_descendants`
  в `crates/js/src/shim/web_api_shim_mid.js`) зовёт `connectedCallback` в момент, когда DOM ещё не тот,
  что видит Chrome (порядок реакций CE, HTML §4.13.6 «custom element reactions»), и `insertBefore`
  Polymer/Lit получает устаревший `refNode`. Не подтверждено: DOM посмотреть не удалось — живой
  `eval` не отвечает ([BUG-1633](BUG-1633-FIXED.md)).
- Лог прогона и проба: `.tmp/yt-live.log`, `.tmp/yt_probe.py` в основной копии (не в git) — проба
  считает `ytd-*`, `img` (src/complete), `ytInitialData…richGridRenderer.contents.length`.

Порядок работы: сначала [BUG-1633](BUG-1633-FIXED.md) (без живого `eval` обрыв не локализовать), затем
поймать стек `insertBefore` (кто вставляет, какой `refNode`, чей он ребёнок) и сравнить с Chrome.

## Ход работы P6 (2026-10-18, WIP, ветка `p6-bug-1239`)

**Сделано:**
- `NotFoundError … insertBefore` (повтор 2026-10-18) локализована и исправлена: `document.importNode(template.content, true)`
  возвращал Element-обёртку (nodeType 1) вокруг клонированного фрагмента. ShadyDOM (`webcomponents-sd`,
  `attachShadow({shadyUpgradeFragment})`) делает `host.__shady_native_appendChild(fragment)` — наш `appendChild` не
  распознавал фрагмент и вставлял его самого, поэтому корень `ytd-app` состоял из одного вложенного фрагмента, а
  `#content` оказывался не ребёнком хоста. Фикс: `importNode` в `web_api_shim_mid.js` возвращает
  `_lumen_make_node_by_nid(clone_nid)`. Тест `document_import_node_of_template_content_is_a_fragment`
  (`cargo test -p lumen-js --features v8-backend --lib document_import_node`). После фикса ошибка `insertBefore` ушла,
  `ytd-miniplayer` создаётся.
- `ytd-page-manager` по-прежнему пуст. Установлено: начальная навигация не стартует — в `CF(X,M)` (main.js, поиск
  `lazyPrepareCriticalPages`) вызов `X.polymerController.$["page-manager"].lazyPrepareCriticalPages(...)` падает
  `TypeError` (глотается async-обёрткой, в логе тишина): у DOM-элемента `ytd-page-manager`
  (`customElements.get('ytd-page-manager')`, ctor `p` из `Iee`/`P1l`, режим `mode:2`/`useSink`) на прототипе только
  `constructor, createElement`, методов класса `Z1w` (`lazyPrepareCriticalPages` и др.) нет. Они должны быть
  проброшены: `iYG` → `H4y(p, name)` для `getInitialMethods()` (методы `Z1w.prototype`) и `$Ql(p,g,V)` для
  `getPropsToForwardBeforeRegister()`. Значит, либо `iYG` не доходит до этих циклов (исключение в `W(p, …)`/`MRy`/`x9U`
  при создании `polymerController`), либо `H4y` определяет методы на экземпляре, а не на прототипе, и они теряются
  (например при `Object.defineProperty` на wrapper'е). Проверять: `pm.polymerController` (есть ли), `Object.keys(pm)` /
  `Object.getOwnPropertyNames(pm)` на наличие `lazyPrepareCriticalPages`, `pm.polymerController.lazyPrepareCriticalPages`.
  Также у `ytd-app`: `app.$` заполнен (15 ключей, `page-manager` есть).

**Следующий шаг:** в живом окне (скрипт ниже) выяснить, где `lazyPrepareCriticalPages` на самом деле лежит
(`pm`, `pm.polymerController`, `app.polymerController`), и почему `CF` его не видит; сверить с Chrome
(`chrome.exe --headless=new --remote-debugging-port` или `--dump-dom` на `.tmp/yt/repro.html`).

**Как воспроизводить (всё в `.tmp/` worktree `p6-work`, не в git):** `.tmp/run_js.py <файлы.js>` — запускает
`target/dev-release/lumen.exe --maximized --proxy http://127.0.0.1:12334 --mcp-live-port N <URL>` (`WAIT=50` секунд
ожидания загрузки — меньше нельзя: страница грузится ~40 с), затем `eval` каждого файла; `.tmp/probe.js` — счётчики DOM;
`.tmp/yt/` — `main.js` (kevlar_base, 10 МБ, `curl -x http://127.0.0.1:12334`), `sd.js` (ShadyDOM),
`repro.html` (минимальный повтор ShadyDOM `attachShadow({shadyUpgradeFragment})`, Chrome и Lumen сравнимы через
`document.body.getAttribute('data-out')`). Сборка: `cargo build --profile dev-release -p lumen-shell` (~80 с инкрементально).
Отладочный приём: временно дописать IIFE в конец `web_api_shim_tail_b.js` (логи `dispatchEvent`, `unhandledrejection`),
перед коммитом откатить `git checkout`.

**Побочные дефекты (завести в `BUGS.md`, не чинить в этом пункте):**
1. `ServiceWorker` `statechange`: событие приходит без `target` (`Cannot read properties of null (reading 'state')`,
   youtube `k6E`) — проверить `navigator.serviceWorker.register` заглушку.
2. `FontFace`: `TypeError: self._rejectLoaded is not a function` как `unhandledrejection`, десятки раз за загрузку
   youtube (`_lumen_font_face_try_sources`, `web_api_shim_*`) — вероятно связано с BUG-1634.
