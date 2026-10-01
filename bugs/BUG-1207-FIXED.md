# BUG-1207 — youtube: `CE connectedCallback (upgrade): Error: InjectionToken(PAGE_TOKEN)`, контент не рисуется

**Статус:** FIXED 2026-10-02 (P3)
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` — `_lumen_ce_connect_descendants`, `_lumen_ce_maybe_connected`)
**Найден:** P6, живая проверка youtube к [BUG-1123](BUG-1123-FIXED.md), 2026-09-28

## Симптом

Видимое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, dev-release, `https://www.youtube.com/`:

```
[JS error] CE connectedCallback (upgrade): Error: md`InjectionToken(PAGE_TOKEN)
```

После 20 с: ~1260 элементов, `document.body.innerText` пуст, `ytd-app` есть, но без `shadowRoot`;
`ShadyDOM.inUse === true`. Ошибка была и до исправления BUG-1123 (лог пробы BUG-1167), то есть не
следствие `__shady_native_dispatchEvent`, который закрыт.

## Что известно

- Бросает `connectedCallback` элемента, апгрейднутого `customElements.define` (путь
  `_lumen_ce_upgrade_element`, `crates/js/src/shim/web_api_shim_mid.js`).
- Текст `md\`InjectionToken(PAGE_TOKEN)` — сообщение DI youtube о ненайденном провайдере; какой сигнал
  страницы (порядок апгрейда, данные `ytInitialData`/`ytcfg`, отсутствующий API) его вызывает — не
  установлено.

## Что сделать

Найти в скриптах youtube место, бросающее эту ошибку, и условие, при котором провайдер `PAGE_TOKEN`
не зарегистрирован к моменту `connectedCallback`; сравнить с Chrome порядок `define`/апгрейда и
значения, которые читает регистрация провайдера.

## Причина

`PAGE_TOKEN` регистрирует `ready()` элемента `ytd-page-manager` (`addProvider({provide: _.Ky, useValue: this})`).
`ytd-page-manager` лежит на несколько уровней ниже `ytd-app` в шаблоне, который ShadyDOM собирает отсоединённым
и присоединяет к хосту одним `appendChild`/`insertBefore`. Вставка звала поверхностный
`_lumen_ce_maybe_connected` только для самого вставляемого узла (HTML LS §4.2.3 «insert» требует обойти всех
потомков), поэтому вложенный элемент так и остался `HTMLElement`: `ready()` не отработал, а `attached`
у `ytd-app` через наблюдатель `guideIsVisibleButNotPersistentSelectorChanged` запросил `PAGE_TOKEN` и получил
`md`…``. Найдено трассой `define`/upgrade/connected по `ytd-app` и `ytd-page-manager` в живом окне.

## Исправление

- `_lumen_ce_connect_descendants(nid)`: после вставки обходит потомков подсоединённого поддерева и для каждого
  имени с дефисом зовёт `_lumen_ce_maybe_connected` (апгрейд либо `connectedCallback`). Вызывается из
  `appendChild`, `insertBefore` (в том числе для фрагмента) и `ShadowRoot.appendChild`. Страница без
  `customElements.define` платит одно сравнение счётчика.
- `_lumen_ce_maybe_connected` ставил `connectedCallback` и для отсоединённого элемента — вложенный
  конструированный элемент получал его дважды (при сборке отсоединённого дерева и при присоединении).
  Теперь только при `isConnected`.

Тесты: `crates/js/src/dom/tests/v8_bug1207_ce_nested_insert.rs`.

## Результат на youtube

Живое окно `--maximized`, `LUMEN_NO_ADBLOCK=1`, dev-release через `--proxy`: `[JS error]` исчез,
`ytd-page-manager` апгрейднут. Страница по-прежнему пуста (`innerText` 0, `ytd-page-manager` без детей,
`ytd-browse` нет, ошибок в логе нет) — отдельный дефект, [BUG-1239](BUG-1239-OPEN.md).
