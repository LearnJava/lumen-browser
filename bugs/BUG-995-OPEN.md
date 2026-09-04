# BUG-995 — `focused_node` переживает навигацию: `Document::get()` крашит процесс в `text_input.rs`

**Статус:** OPEN
**Заведён:** 2026-09-04 (P2, WPT-RUN-7 срез 3 — генерация expectations baseline для `input-events`)
**Область:** `crates/shell/src/lumen/text_input.rs` (`typeable_field`), `crates/shell/src/lumen/keyboard.rs`
(вызывающие), `crates/shell/src/lumen/navigation.rs` (`navigate_to`/`navigate_replace` не сбрасывают `focused_node`)
**Владелец:** P3

## Симптом

Тот же класс паники, что закрыт в [BUG-986](bugs/BUG-986-FIXED.md) (`Document::get()` на чужом
`NodeId`), но **другой call site**, который фикс BUG-986 не накрыл — это не JS-граница
(`dom_core.rs`), а прямой вызов из shell:

```
thread 'main' (639584) panicked at crates/shell/src/lumen/text_input.rs:98:24:
BUG-986: NodeId 456 вне арены документа (len 42) — устаревший/чужой идентификатор,
переживший навигацию или пересёкший границу вкладки; вызывающий: crates/shell/src/lumen/text_input.rs:98:24
```

`#[track_caller]`-диагностика из фикса BUG-986 сработала штатно и назвала конкретного
вызывающего — то, что не удавалось сделать до фикса. Паника рвёт BiDi-сессию
(`navigate: live window closed before replying`), wptrunner помечает тест `ERROR`,
перезапускает окно.

## Воспроизведение

Детерминированно на пересобранном бинаре (с фиксом BUG-986, `commit f0a6847c0` и
позже): полный прогон категории `input-events`,

```
tests/wpt/run_report.py --binary target/dev-release/lumen --update-expected --all \
  --root input-events --recursive
```

падает на тесте
`input-events-get-target-ranges-deleting-in-list-items.tentative.html?Delete,ol`
(первый субтест в файле) при переходе к следующему варианту URL того же файла — паника
происходит стабильно **на каждом прогоне категории**, всегда с одним и тем же
`NodeId 456 вне арены (len 42)`. Изолированный смок только последнего варианта
(`run_smoke.py '...?Delete,ul'`) не воспроизводит — нужна последовательность
навигаций внутри одного окна теста, а не отдельный тест.

## Root cause

`Lumen::focused_node: Option<NodeId>` (`crates/shell/src/lumen/click.rs`,
`focus_tab.rs`) не сбрасывается ни в `navigate_to`, ни в `navigate_replace`
(`crates/shell/src/lumen/navigation.rs` — `grep focused_node` там не находит ни одного
присвоения). wptrunner переиспользует одно окно между вариантами URL одного `.html`-
файла (`?Delete,ol` → `?Delete,ul` — это перезагрузка **того же** файла с новым query,
т.е. `navigate_to`/`navigate_replace`, не новая вкладка): элемент был сфокусирован в
документе первого варианта, страница перезагрузилась на новый (меньший) документ, а
`focused_node` продолжает указывать на id из старой арены. Следующий инжект клавиши
(`_lumen_dispatch_key_event` в `keyboard.rs`, testdriver `send_keys`) читает
`self.focused_node`, `keyboard.rs:398` зовёт `typeable_field(nid)`
(`text_input.rs:96`), которая на `text_input.rs:98` делает **непроверенный**
`doc.get(nid)` вместо `doc.try_get(nid)` — ровно тот примитив, который BUG-986 ввёл
специально для таких границ.

Это ставит под вопрос все остальные `self.focused_node?` / `let Some(nid) =
self.focused_node` в `text_input.rs` (строки 145, 167, 199, 219, 234, 252, 273, 294,
333) и `text_input.rs:58`/`381` (`node_id = self.focused_node.map(|n| n.index())`) —
они читают `nid`, только когда `typeable_field`/`frame_typeable_field` уже вернули
`Some`, то есть если строка 98 перейдёт на `try_get`, они автоматически перестанут
получать чужой id (цепочка одна). `keyboard.rs:275/367/398/435/479` вызывают
`typeable_field`/`frame_typeable_field` напрямую с `self.focused_node` — тот же вопрос
для `frame_typeable_field`, не проверено в этой сессии.

## Предлагаемое направление (не реализовано, P3 решает)

Либо (а) `navigate_to`/`navigate_replace` сбрасывают `focused_node = None` при смене
документа (соответствует HTML LS: навигация снимает фокус), либо (б)
`typeable_field`/соседи в `text_input.rs` переходят на `Document::try_get`/`contains_id`
по образцу `dom_core.rs` из BUG-986 — деградация вместо паники на чужом id. (а) чинит
корень (переживший навигацию фокус — сам по себе спека-нарушение, не только источник
паники), (б) чинит защиту границы независимо от (а). Вероятно нужны оба.

## Влияние на WPT-RUN-7

Baseline категории `input-events` (`tests/wpt/metadata/input-events/`, срез 3)
зафиксировал крах как `expected: ERROR` — `--check` гейт зелёный, но это не «всё ОК»,
а «крах воспроизводится стабильно и учтён»: будущий регресс в другом месте по-прежнему
поймается, а фикс этого бага всплывёт как unexpected pass и потребует перегенерации
baseline для этого файла.

## Сырые данные

`.tmp/wpt-run7-slice3/input-events.run1.log` (прогон до фикса BUG-986, паника в
`dom/lib.rs:706` без диагностики вызывающего) и повторный прогон после пересборки на
`f0a6847c0` — тот же файл/id, теперь с точным call site.
