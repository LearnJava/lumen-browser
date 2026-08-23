# BUG-877 — `host.shadowRoot` отдаёт новый объект на каждое чтение: `host.shadowRoot !== host.shadowRoot`

**Статус:** OPEN
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
дефект того же объекта — [BUG-676](BUG-676-OPEN.md) (литерал вместо
прототипной цепочки, нет `window.ShadowRoot`).

## Что дальше

Кэшировать обёртку по `sr_nid` (как это сделано для элементов через
`_lumen_element_wrappers`) и отдавать её из обоих мест — геттера и
`attachShadow`. Чистка кэша — тем же `_lumen_gc_collect`, что и у элементов
(осторожно: [BUG-849](BUG-849-OPEN.md) — он чистит только освобождённые nid).
