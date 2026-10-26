# BUG-1700 — `navigation.navigate("#frag")`/`back()`/`traverseTo()` между записями одного документа — полная навигация (повторная загрузка, новый realm), страница уходит в цикл перезагрузок

**Статус:** OPEN
**Заведён:** 2026-10-26 (P2, WPT-NAV-SCOUT)
**Область:** shell (`crates/shell/src/app/about_to_wait.rs` — разбор `take_nav_updates`, действия 0/1/2/3/4; `crates/shell/src/lumen/navigation.rs` — `navigate_to_inner`, `navigate_back`, `navigate_to_key`)

## Симптом

`navigation.navigate("#foo")` обрабатывается оболочкой как `navigate_to(PageSource::url(url))` — полноценный переход: GET документа (в логе браузера `Reload: …/page.html#foo` + `→ GET …#foo`), новый JS-realm, повторное выполнение всех скриптов страницы. Контроль: `location.hash = "#ctl"` в том же документе повторной загрузки не вызывает, realm сохраняется.

Для страницы, которая в своём скрипте безусловно зовёт `navigation.navigate("#foo")` (а так устроены ~100 тестов `navigation-api`), это вечный цикл: ≈230 мс на круг, каждый круг добавляет запись в `navigation.entries()`, через 20 с браузер не отвечает на BiDi-вызов (`LUMEN_WPT_HARD_CAP`) и тест получает TIMEOUT. Побочные эффекты цикла у тестов, дошедших до проверок: `entries().length` раздут («expected 1 but got 15»), `currentEntry.index` смещён.

Проба: `tests/wpt/navigation-api/currententrychange-event/history-back-same-doc.html` через `tests/wpt/run_smoke.py` — 14 строк `Reload: …#foo` за 3 с и TEST_END TIMEOUT; страница-проба с `sessionStorage`-флагом показывает, что после `navigate('#nav')` скрипт выполнился второй раз (`entries=4 idx=3` вместо 3/2), т.е. realm пересоздан.

## Как найдено

WPT-NAV-SCOUT, кластер A (`docs/wpt-vendor-notes/navigation-api.md`): 95 id с ≥10 перезагрузками в логе шарда (из них 88 — TIMEOUT «browser did not answer a BiDi call within 20s») + 30 id с раздутыми `entries()` (по тому же механизму, эвристика). Потолок +125 баллов (+0.18 п.п. корпуса); подкаталоги: `navigation-methods` 35, `navigate-event` 28, `scroll-behavior` 19, `ordering-and-transition` 16.

## Что делать

Навигация между записями, у которых совпадают URL без фрагмента и документ (фрагментная навигация, переходы `back`/`forward`/`traverseTo` между same-document записями, `navigate()` на тот же документ с `history: "replace"`), должна идти как same-document: без загрузки, с сохранением realm, с диспатчем `navigate`/`currententrychange`/`navigatesuccess` в текущем документе. В оболочке уже есть путь для `location.hash = …` — использовать его, а не `navigate_to`.

## Как проверить

Страница из пробы выше: `navigation.navigate("#foo")` → в логе нет `Reload:`/`GET …#foo`, `window.__marker` переживает вызов, `navigation.entries().length` растёт ровно на 1. `run_corpus.py --prefixes navigation-api/scroll-behavior` — ни одного TIMEOUT «BiDi call».
