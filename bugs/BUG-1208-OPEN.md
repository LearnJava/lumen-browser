# BUG-1208 — `window.origin` отсутствует; `location.origin` srcdoc-фрейма пуст

**Статус:** OPEN
**Заведён:** 2026-09-28 (P6, попутно к [BUG-1198](BUG-1198-FIXED.md))
**Область:** js (`crates/js/src/shim/web_api_shim_tail_b.js` — глобал `window`, свойство
`origin` не объявлено; `location.origin` документа `about:srcdoc`)

## Симптом

Проба в живом окне (`dev-release`, страница `http://127.0.0.1:<port>/probe.html`):

```
top:   window.origin=undefined self.origin=undefined ('origin' in window) === false
       location.origin=http://127.0.0.1:<port>
srcdoc-фрейм (без sandbox):             self.origin=undefined location.origin=''
srcdoc-фрейм (sandbox="allow-scripts"): self.origin=undefined location.origin=''
```

По HTML LS §8.1.3.5 `WindowOrWorkerGlobalScope.origin` — сериализация происхождения
настроек среды: у страницы — её tuple-origin, у srcdoc-фрейма без песочницы — происхождение
родителя (документ `about:srcdoc` наследует его, §7.4.x), у фрейма `sandbox` без
`allow-same-origin` — `"null"`. `location.origin` srcdoc-документа — сериализация
происхождения URL `about:srcdoc`, то есть `"null"`, а не пустая строка.

`Origin.from(globalThis)` внутри srcdoc-фрейма даёт непрозрачное происхождение (правильно для
песочницы, случайно — по URL `about:srcdoc`, а не по флагу), у не-песочного srcdoc-фрейма это
неверно: должен быть tuple-origin родителя.

## Что сделать

Объявить `origin` на глобале окна (и проверить воркеры — там он тоже `[Exposed=*]`), отдавать
происхождение документа с учётом наследования `about:srcdoc`/`about:blank` и флага песочницы;
тот же источник — для `Origin.from(globalThis)` (`origin_shim.js::globalRecord`) и
`location.origin` srcdoc-документа.
