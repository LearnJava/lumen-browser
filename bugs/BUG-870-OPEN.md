# BUG-870 — у `localStorage`/`sessionStorage` нет квоты: `setItem` не бросает `QuotaExceededError` никогда, и квотный тест крутится вечно

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 26 — живой замер, вариант `storage-quota`)
**Область:** `crates/core/src/web_storage.rs:38`–`44` — `WebStorage::set_item` пишет в `HashMap` без единой проверки размера; `crates/js/src/dom.rs:9921` — шимовый `setItem: function(k, v) { setItem(String(k), String(v)); }` тоже ничего не проверяет. Слова `quota` нет во всём `crates/core/src/web_storage.rs`
**Владелец:** P1/P3 (`lumen-js`/`lumen-core`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Web Storage (HTML LS §12.2) обязан иметь конечную квоту на origin и бросать
`QuotaExceededError` при её исчерпании — на этом построена вся защита от
страницы, заполняющей диск. Здесь предела нет: сколько влезет в память
процесса, столько и запишется.

Опаснее самого пробела его форма. Все четыре квотных теста WPT написаны
как **безусловный** цикл, который заканчивается только исключением:

```js
assert_throws_quotaexceedederror(() => {
    while (true) { localStorage.setItem("" + key + index, "" + val + index); index++; }
}, null, null);
```

Движок без квоты не проваливает такой тест, а виснет в нём навсегда,
забирая с собой остаток шарда (механизм `hung-browser`).

## Прямое измерение

`tests/wpt/verify_worker_port_storage_gaps.py --variant storage-quota`
(2026-08-23, dev-release, Linux, `main` = `c14b8068c`, `--seconds 8`).
Проба пишет значения по 1 КиБ, но цикл **ограничен** 20 000 итерациями —
иначе она измеряла бы сама себя:

```
sq-local   wrote=20000 ms=74 err=none length=20000
sq-session wrote=20000 ms=67 err=none length=20000
```

20 МиБ в каждое хранилище, ни одного исключения, 74 мс. Для сравнения:
типичная браузерная квота — 5–10 МиБ на origin, то есть предел должен был
сработать примерно на четверти этого объёма.

## Масштаб

4 id остатка снимка WPT-RUN-5 — ровно те, которые
[BUG-836](BUG-836-OPEN.md) в своей заметке отложил как «4 квотных теста»:
`webstorage/storage_local_setitem_quotaexceedederr.window.html`,
`storage_session_setitem_quotaexceedederr.window.html`,
`storage_local_quota_independent_from_session.window.html`,
`storage_session_quota_independent_from_local.window.html`. Последние два
дополнительно требуют, чтобы квоты двух хранилищ были независимы.

## Направление починки (не предписание)

Счётчик суммы `key.len() + value.len()` в `WebStorage` (отдельный на
`localStorage` и на `sessionStorage`, чтобы независимость выполнялась
сама), проверка в `set_item` перед вставкой, возврат ошибки наружу и
`QuotaExceededError` в шиме. Значение предела — вопрос политики, не
спеки; 5 МиБ на origin — общепринятое.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_worker_port_storage_gaps.py
   --variant storage-quota` — ожидается `err=QuotaExceededError` заметно
   раньше 20 000 ключей и независимые пределы у двух хранилищ.
2. WPT: `run_report.py --all --root webstorage --recursive`.
