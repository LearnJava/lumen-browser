# BUG-397 — Global Privacy Control (GPC) полностью не реализован: ни `navigator.globalPrivacyControl`, ни `Sec-GPC`

**Статус:** OPEN
**Компонент:** js (`crates/js/src/surface_api.rs` — рядом с `navigator.doNotTrack`,
строки 116-121) + network (`crates/network/src/http/headers.rs::build_request_headers`,
все 7 `HttpProfile`)
**Найден:** P2, WPT-VENDOR-gpc (2026-07-28), прогон дал 0 сигнала —
находка получена прямым грепом по `crates/`, не прогоном

## Симптом

Категория `gpc` (`tests/wpt/gpc/`, 4 исполняемых файла) целиком
недостижима этим wptrunner-исполнителем: 3 файла используют кастомные
testdriver-методы `test_driver.get_global_privacy_control()`/
`set_global_privacy_control()` → `SKIP (Executor does not support
testdriver.js)`; `idlharness.any.js` падает на невендоренных
`/resources/WebIDLParser.js`+`idlharness.js` (404) → `TIMEOUT`. Прогон
сам по себе не даёт находки (тот же класс, что многие категории этого
backlog-а).

По правилу «пробуй даже без сигнала» — грепом по `crates/`:

```
grep -rli "globalPrivacyControl\|Sec-GPC\|sec_gpc" crates/ --include="*.rs"
```

— **ноль совпадений**. Ни `navigator.globalPrivacyControl` (JS-свойство,
Boolean, спека https://www.w3.org/TR/gpc/), ни HTTP-заголовок `Sec-GPC: 1`
не реализованы вовсе — ни в JS-шиме, ни в исходящих HTTP-запросах.

## Почему это находка, а не просто "вне скоупа"

Соседний, юридически более слабый сигнал `navigator.doNotTrack`
**реализован** (`surface_api.rs:116-121`, намеренно `null` —
Chrome-совместимое поведение). Заголовок `DNT: 1` **отправляется**
по умолчанию в HTTP-профиле Chrome/Edge (`headers.rs:143`,
`headers.rs:193` через тот же блок). GPC — юридически обязывающий
сигнал (California CCPA/CPRA, Colorado Privacy Act, Connecticut
Data Privacy Act прямо признают `Sec-GPC: 1` как валидный универсальный
opt-out-запрос), а Lumen позиционируется как «private, lightweight,
transparent browser» (`CLAUDE.md` — «What is this»), уже имеет
per-profile HTTP-фингерпринт-инфраструктуру (ADR-007,
`crates/network/src/http/headers.rs`) и отдельный `HttpProfile::Strict`
для privacy-режима — естественное место для сигнала уже существует и
используется для менее значимого DNT, но не для GPC.

Реальные браузеры расходятся: Chrome/Edge/Safari не поддерживают GPC
нативно (поэтому его отсутствие в `HttpProfile::Chrome`/`Edge`/`Safari`
— корректная имитация), но Firefox с расширениями, Brave и DuckDuckGo
Browser его отправляют, и в `HttpProfile::Lumen` (собственный
незамаскированный фингерпринт Lumen, `headers.rs:233-246`) и/или
`HttpProfile::Strict` (усиленный anti-fingerprint профиль,
`headers.rs:126` — общий блок с Chrome) отсутствие GPC — не имитация
чужого браузера, а неиспользованная возможность собственного
privacy-заявления.

## Что нужно сделать

1. `navigator.globalPrivacyControl` — read-only Boolean на
   `Navigator`/`WorkerNavigator` (спека: `true`, если пользователь
   включил сигнал; в реальных браузерах это либо статический `true`
   при поддержке, либо настраиваемый тумблер). Естественное место —
   рядом с `navigator.doNotTrack` в `surface_api.rs`.
2. `Sec-GPC: 1` — HTTP-заголовок на каждый исходящий запрос, когда
   сигнал включён. Естественное место — `headers.rs::build_request_headers`,
   в блоке `HttpProfile::Strict`/`HttpProfile::Lumen` (и опционально
   `HttpProfile::TorBrowser`, если реальный Tor Browser его тоже
   отправляет — сверить перед добавлением, чтобы не создать обратный
   fingerprint-разрыв).
3. Оба значения должны быть согласованы (JS-свойство `true` ⇔
   заголовок присутствует) — иначе сайт увидит противоречивый сигнал,
   что само по себе является fingerprinting-вектором.
4. Нужен ли тумблер в `about:settings` (Privacy tab, уже существует,
   `CAPABILITIES.md` строка `Настройки`) или включать по умолчанию —
   решение продукта, не техническое; зафиксировать в ADR при реализации.

## Связанные

* `navigator.doNotTrack` — `crates/js/src/surface_api.rs:19,116-121`
  (реализован, для сравнения).
* `DNT`/`Sec-Fetch-*`/UA per-profile фингерпринт —
  `crates/network/src/http/headers.rs` (ADR-007).
* Категория `gpc` сама по себе не даёт исполняемого сигнала —
  находка исключительно из прямого грепа по кодовой базе, тот же
  приём, что `geolocation-sensor` (но там проба дала отрицательный,
  то есть корректный, результат — здесь положительный, то есть пробел).
