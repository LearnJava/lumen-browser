# BUG-1208 — `window.origin` отсутствует; `location.origin` srcdoc-фрейма пуст

**Статус:** FIXED
**Заведён:** 2026-09-28 (P6, попутно к [BUG-1198](BUG-1198-FIXED.md))
**Исправлен:** 2026-09-29 (P3)
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

## Исправление

- `install_dom` (`v8_runtime.rs`) получил параметр `origin_inherit_from: Option<&str>` —
  origin родителя для непесочного `about:blank`/`about:srcdoc`; инъекция глобала
  `_LUMEN_ORIGIN` в рантайм (ASCII-сериализация через новый `origin_serialization()`,
  `origin.rs`: tuple-origin — `scheme://host[:port]`, opaque — литеральная строка `"null"`).
- `window.origin`/`self.origin` объявлены в `web_api_shim_tail_b.js`.
- `origin_shim.js::globalRecord()` теперь строит запись реалма из `_LUMEN_ORIGIN`, а не из
  `location.href` — для `about:` адресов это разные вещи (наследуемый origin ≠ адрес документа).
- `location.origin` непарсуемого/opaque адреса — литеральная строка `"null"`
  (`web_api_shim_mid_b.js`), было — пустая строка.
- Воркеры (`worker.rs`/`shared_worker.rs`/`sw_worker.rs`): `globalThis.origin` строится тем же
  URL, что и `location` (мисксин общий для обоих контекстов; у воркера нет случая наследования).
- `run_scripts_with_dom` (`scripts.rs`) вычисляет `origin_inherit_from` из `ancestry`: только
  если фрейм не opaque (без `sandbox` без `allow-same-origin`) и адрес — `about:*`.
- Регрессионный тест: `crates/js/tests/cases/bug1208_window_origin.rs` (5 тестов — обычная
  страница, opaque-адрес, `about:srcdoc`/`about:blank` с наследованием и без).
