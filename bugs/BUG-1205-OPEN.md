# BUG-1205 — youtube: `CE connectedCallback (upgrade): Error: InjectionToken(PAGE_TOKEN)`, контент не рисуется

**Статус:** OPEN
**Компонент:** js — не локализовано (DI-контейнер youtube не находит провайдер `PAGE_TOKEN` при апгрейде custom element)
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
