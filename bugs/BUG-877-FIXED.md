# BUG-877 — `host.shadowRoot` отдаёт новый объект на каждое чтение: `host.shadowRoot !== host.shadowRoot`

**Статус:** FIXED 2026-09-19
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 27 — живой замер, вариант `slot-detail2`)
**Область:** `crates/js/src/dom.rs:5190` — геттер `shadowRoot` каждый раз зовёт `_lumen_make_shadow_root(sr_nid, 'open', nid)`, то есть строит свежий литерал; та же беда у значения, возвращённого `attachShadow` (`:4715`)
**Владелец:** P1/P3 (`lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

```js
var root = host.attachShadow({mode: 'open'});
host.shadowRoot === root            // false
host.shadowRoot === host.shadowRoot // false
```

Каждое обращение к `host.shadowRoot` создаёт новую обёртку. Все три
сравнения обязаны быть `true`: DOM Standard §4.8 требует, чтобы `shadowRoot`
был одним и тем же объектом на всё время жизни узла.

Режим при этом обслужен правильно и здесь не при чём: `closed`-корень
геттеру не достаётся вовсе (нативный `_lumen_get_shadow_root` отдаёт `None`,
покрыто тестом `shadow_root_getter_null_for_closed`), поэтому захардкоженный
в геттере `'open'` ложных срабатываний не даёт.

## Прямое измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant slot-detail2`
(2026-08-23, dev-release, Linux, `main` = `34cbefd25`):

```
sd2-keys ["__nid__","__isShadowRoot__","mode","host","baseURI","innerHTML",
          "textContent","style","querySelector","querySelectorAll",
          "getElementById","appendChild","removeChild","addEventListener",
          "removeEventListener","dispatchEvent"]
sd2-shadowRoot host.shadowRoot=object same=false stable=false
```

## Цена по WPT

Своего кластера id у бага нет — он прячет чужие. Любой сценарий вида
«повесить слушателя на `host.shadowRoot`, дождаться события» не может
сработать: слушатель уходит в реестр одной обёртки, диспатч — в другую.
Тем же путём ломаются `WeakMap`-и по shadow root, `===`-сравнения в
`shadow-dom/`-хелперах и кэширование корня в тестовых утилитах. Соседний
дефект того же объекта — [BUG-676](BUG-676-FIXED.md) (литерал вместо
прототипной цепочки, нет `window.ShadowRoot`).

## Что дальше

Кэшировать обёртку по `sr_nid` (как это сделано для элементов через
`_lumen_element_wrappers`) и отдавать её из обоих мест — геттера и
`attachShadow`. Чистка кэша — тем же `_lumen_gc_collect`, что и у элементов
(осторожно: [BUG-849](BUG-849-FIXED.md) — он чистит только освобождённые nid).

## Исправлено 2026-09-19 (P3)

Найденная точка (`dom.rs`) с тех пор переехала в
`crates/js/src/shim/web_api_shim_mid.js::_lumen_make_shadow_root` — сам
дефект не поменялся: функция всегда строила новую обёртку. Сделано ровно то,
что предлагал предыдущий срез: `_lumen_make_shadow_root` теперь интернирует
результат в тот же `_lumen_element_wrappers`, которым уже пользуются
`_lumen_make_element`/`_lumen_make_doctype` — та же карта, тот же
`_lumen_gc_collect`, никакого нового кэша с собственной логикой очистки.
`host.shadowRoot === host.shadowRoot` и `attachShadow(...) === host.shadowRoot`
теперь оба `true`; захардкоженный `'open'` в геттере больше не проблема — при
попадании в кэш аргумент `mode` игнорируется, отдаётся объект, построенный
`attachShadow` с настоящим режимом. Живой фикс совпал с [BUG-895](BUG-895-FIXED.md)
(тот же файл, соседняя строка) — обе правки в одном коммите. Регресс-тесты
`shadow_root_getter_returns_same_object_on_repeated_reads`/
`shadow_root_wrapper_survives_a_weak_map_key`
(`crates/js/src/dom/tests/v8_bug877_895_shadow_root_wrapper.rs`). Гейты:
`cargo clippy -p lumen-js --all-targets --features v8-backend -- -D warnings`
чисто, `cargo test -p lumen-js --features v8-backend` 3897/3899 (два
предсуществующих флака `opener_postmessage_*` — общее состояние между
параллельными тестами, проходят 3/3 при `--test-threads=1`, не регрессия
этого фикса).
