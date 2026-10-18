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
