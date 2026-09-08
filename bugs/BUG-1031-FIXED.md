# BUG-1031 — `soft-navigation-heuristics`: `browsingContext.navigate` тихо не навигирует после того, как соседний тест перехватил чужой `navigate`-эвент

**Статус:** FIXED 2026-09-08 (P3)
**Заведён:** 2026-09-07 (P2, WPT-RUN-7 срез 25)
**Область:** shell (`crates/shell/src/lumen/navigation.rs::navigate_to`/`navigate_to_forced`,
`crates/shell/src/app/about_to_wait.rs` — `AutomationCommand::Navigate`/`NewTab`), js
(`crates/js/src/navigation_api.rs` — `NavigateEvent.intercept()`/`canIntercept`)
**Владелец:** P3 (фикс закрыт)

## Симптом

`--update-expected --all --root soft-navigation-heuristics --recursive --processes=6`
прошёл штатно за 1:12 (21/94 harness OK, 0/49 сабтестов — категория экспериментальная,
большинство тестов TIMEOUT/ERROR даже в baseline, это ожидаемо и не сама находка). Два
последовательных `--check` на том же бинаре и том же свежезаписанном baseline, без
изменений между прогонами (`--processes=6` в обоих):

- прогон 1: **21 регрессия**, 6 unexpected pass (narrow expectations), 0 other deviations;
- прогон 2: **15 регрессий**, 7 unexpected pass (narrow expectations), 0 other deviations.

Полные логи не сохранялись отдельным файлом (только хвост stdout), поэтому точное
пересечение множеств регрессий между прогонами не подсчитано — но ни один из видимых в
хвосте регрессий прогона 1 (`image-src-change.html`, `video-src-change.html`) не повторился
в хвосте прогона 2, что уже отличается от растущего/пересекающегося счётчика BUG-1022/1024
(там был явный общий знаменатель-файл). Baseline `soft-navigation-heuristics` не
закоммичен — `.ini`-файлы, записанные `--update-expected`, откачены `git clean -fd
tests/wpt/metadata/soft-navigation-heuristics/` до исходного (отсутствующего) состояния
этим же срезом.

## Почему это важно

Тот же класс находки, что [BUG-1003](BUG-1003-OPEN.md)/[BUG-1004](BUG-1004-OPEN.md)/
[BUG-1005](BUG-1005-OPEN.md)/[BUG-1011](BUG-1011-OPEN.md)/[BUG-1022](BUG-1022-OPEN.md)/
[BUG-1024](BUG-1024-FIXED.md) («N `--check` подряд без изменений между ними дают N разных
наборов регрессий»), теперь на маленькой (94 id) и в основном не проходящей категории —
показывает, что механизм не зависит ни от размера категории (BUG-1003/1004 тоже были
небольшими), ни от доли PASS в baseline (здесь harness OK всего 21/94, большая часть
baseline и так уже FAIL/ERROR/TIMEOUT). Расширяет выборку категорий, задетых этим классом,
но новых зацепок к локализации не добавляет.

## Воспроизведение

```
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root soft-navigation-heuristics --recursive \
  --processes=6 --update-expected
tests/wpt/.venv/bin/python tests/wpt/run_report.py \
  --binary target/dev-release/lumen --all --root soft-navigation-heuristics --recursive \
  --processes=6 --check
# повторить --check ещё раз без изменений между прогонами — набор регрессий не совпадает
```

## Что не проверялось

- Логи `--check` не сохранялись через `--log-raw`/redirect в файл — только хвост stdout,
  поэтому точный overlap между прогонами (как в BUG-1022/1024) не подсчитан.
- Третий `--check` подряд — есть только две точки.
- Изоляция конкретных задетых файлов через `run_smoke.py` в цикле (как для BUG-1011) — не
  делалась.

## Срез P3 2026-09-08 — механизм BUG-1024 (каскадная паника → отравленный Mutex) проверен и отклонён

Гипотеза: та же дыра, что чинил [BUG-1024](BUG-1024-FIXED.md) (непроверенный `doc.get(nid)` в
JS-натив, доступном напрямую со страницы, паникует на чужом/устаревшем `NodeId`, отравляя
`Mutex<Document>` — все последующие `.lock().unwrap()` того же документа тоже паникуют
каскадом, что и выглядит как «плавающий» набор регрессий между прогонами). Аудит нашёл и
починил шесть ещё непроверенных путей этого же класса в `dom_core.rs`/`platform.rs` —
[BUG-1036](BUG-1036-FIXED.md).

**Результат: гипотеза отклонена.** Тот же бинарь (пересобран с фиксом BUG-1036) на том же
репро — `--update-expected`, затем два `--check` подряд — дал 18 и 21 регрессию
соответственно, непересекающиеся так же, как до фикса. Симптом воспроизводится один в один —
причина BUG-1031 не эта дыра, а другой, всё ещё не найденный механизм. Остаётся `OPEN`,
владелец не изменился.

## Срез P3 2026-09-08 — найден и починен настоящий механизм: интерцепт-эвент, не отравленный `Mutex`

### Локализация

`--log-raw` на baseline-прогоне и на двух последующих `--check` (не только хвост stdout, как
в первой находке — полный JSON-лог wptrunner) поймал точный симптом на каждой регрессии:

```
ERROR browsingContext.navigate(http://localhost:18300/soft-navigation-heuristics/<файл-N>.html)
reported success but the document was never replaced
(still at http://localhost:18300/soft-navigation-heuristics/<файл-N-1>.html); the page did not load
```

138 таких строк в baseline-прогоне (46 уникальных пар «застрял на / должен был перейти на»,
retry ×3 — дефолт wptrunner). Ключевая закономерность: файл, «на котором застрял» воркер, в
каждом из трёх независимых прогонов (baseline + 2×`--check`) — всегда один из шести:
`navigation-api-back.tentative.html`, `navigation-api-hash.tentative.html`,
`navigation-api-precommit-handler.html`, `navigation-api-prevent-default.window.html`,
`navigation-api-rejected.tentative.html`, `navigation-api.tentative.html` — то есть сами тесты
Navigation API, специально проверяющие `intercept()`/`preventDefault()` у эвента `navigate`.
Какой из шести и сколько тестов после него ловят ошибку — зависит от того, какому воркеру
`SingleTestSource`/`hash(test.id) % processes` назначит этот файл (BUG-1011's находка — между
независимыми прогонами `python3` это не воспроизводится), поэтому набор регрессий каждый раз
разный — но корень один и тот же на все три прогона.

### Причина

`navigate_to` (`crates/shell/src/lumen/navigation.rs`) — единая точка входа и для навигации,
которую инициирует сама страница (клик по ссылке, `history.pushState`, `location.href=`,
`navigation.navigate()`), и для навигации, которую инициирует автоматика (WebDriver BiDi
`browsingContext.navigate`, `AutomationCommand::NewTab`). Перед каждой навигацией она
безусловно диспатчит JS-эвент `navigate` на **текущем** (ещё не выгруженном) документе с
захардкоженным `canIntercept=true` (`_lumen_dispatch_navigate('push', url, true, false)`) и,
если страница вызвала `event.intercept()`, реальная загрузка нового документа не запускается
вовсе — `navigate_to` возвращает управление раньше, чем доходит до `self.reload()`
(`crates/js/src/navigation_api.rs::NavigateEvent.intercept()` не проверяла вообще ничего,
`canIntercept` был мёртвым параметром — ни разу не читался).

Ровно это и тестируют файлы `navigation-api-*.html`: они вешают на `window.navigation`
слушатель `navigate`, который перехватывает событие. WPT-раннер переходит к СЛЕДУЮЩЕМУ тесту
категории через тот же самый `browsingContext.navigate` — то есть через `navigate_to` на всё
ещё живом документе предыдущего теста. Если у него остался (не снят явно, не эфемерный)
слушатель `navigate`, он перехватывает и эту, совершенно постороннюю навигацию — реальный
переход на страницу следующего теста никогда не происходит. `AutomationCommand::Navigate`'s
обработчик (`about_to_wait.rs`) при этом безусловно отвечает `AutomationReply::Ack`, а
`bc_navigate`'s `live.wait(WaitCondition::DocumentReady, …)` (`crates/bidi-server/src/
protocol.rs`) читает `document.readyState` **старого**, так и оставшегося текущим документа —
оно уже `"complete"`, поэтому ожидание не падает и не подвисает. BiDi репортит успех, контекст
навсегда остаётся на старой странице, и КАЖДЫЙ следующий тест того же воркера до конца прогона
(если он тоже полагается на реальную загрузку, а не на что-то, что сработает и на чужой
странице) молча проваливается — набор пострадавших файлов зависит только от порядка очереди
воркера, отсюда «разные регрессии на каждом прогоне» из первой находки.

Это НЕ спек-соответствие: HTML LS §7.8.1 требует, чтобы `navigate`-эвент навигаций,
инициированных браузером/автоматикой (набранный URL, WebDriver), был некэнселебл и
неперехватываем (`canIntercept`/`cancelable` = `false`) — именно для того, чтобы страница не
могла держать браузер в заложниках. Lumen такого различия не делала вовсе.

### Фикс

- `crates/js/src/navigation_api.rs`: `NavigateEvent` теперь несёт настоящий `canIntercept`
  (проброшен в конструктор `Event`'s `cancelable`, так что и `preventDefault()` для
  неперехватываемых навигаций — no-op по базовому классу `Event`), `intercept()` бросает
  `DOMException('InvalidStateError')`, когда `canIntercept` ложно (тот же паттерн, что
  `credentials.rs`/другие натив-шимы уже используют для спек-исключений — не новый примитив).
  `_lumen_dispatch_navigate` игнорирует исход перехвата целиком, когда `canIntercept` ложно.
- `crates/shell/src/lumen/navigation.rs`: `navigate_to` разбит на тонкие обёртки
  `navigate_to`/`navigate_to_forced` вокруг общего `navigate_to_inner(source, can_intercept)`.
  `navigate_to` (все 20+ прежних вызывающих — клики, `pushState`, адресная строка и т. д.) не
  изменилась, `can_intercept=true`. Новая `navigate_to_forced` (`can_intercept=false`) читает
  результат интерцепта только когда он разрешён — при `false` эвент диспатчится для
  наблюдаемости (страница может слушать `navigate` в информационных целях), но его исход не
  влияет на то, произойдёт ли реальная загрузка.
- `crates/shell/src/app/about_to_wait.rs`: `AutomationCommand::Navigate`/`NewTab` вызывают
  `navigate_to_forced` вместо `navigate_to` — единственные два места автоматики, которые раньше
  шли через перехватываемый путь.

`navigate_replace`/`navigate_back`/`navigate_forward` не тронуты — автоматика их не вызывает
(`live.navigate()`/BiDi `browsingContext.navigate` маппится только на `AutomationCommand::
Navigate`/`NewTab` → `navigate_to`).

### Верификация

Пересобранный бинарь на исходном репро (`bugs/BUG-1031-FIXED.md`'s §Воспроизведение):

- `--update-expected`: **48/94 harness OK** (было 20–21/94 на трёх прогонах до фикса — почти
  вдвое больше тестов теперь реально загружаются вместо того, чтобы застрять на чужой
  странице);
- три последовательных `--check` подряд без изменений между ними: **0 регрессий, 0 unexpected
  pass, 0 other deviations, байт-в-байт идентичный результат все три раза** (48/94 harness OK,
  0/87 сабтестов каждый раз) — сравнить с 21/15/28/18 регрессий на четырёх прогонах до фикса.
  Класс находки «N `--check` подряд дают N разных наборов» для этой категории закрыт.
- `--log-raw` на верификационных прогонах — ни одной строки «the document was never replaced».
- Baseline `soft-navigation-heuristics` (94 `.ini`) закоммичен впервые для категории этим
  фиксом.

### Гейты

`cargo build -p lumen-shell -p lumen-js --profile dev-release --bin lumen` — чисто.
`cargo check -p lumen-shell -p lumen-js --all-targets` — чисто.
`cargo test -p lumen-bidi-server --lib navigate` (8/8) и
`cargo test -p lumen-shell --bins navigat` (20/20) — все зелёные, включая
`navigate_with_live_window_executes_real_navigate`/`navigate_updates_url_and_returns_navigation`
(прямые regression-тесты на `browsingContext.navigate`, не задеты сменой `can_intercept`).
`cargo clippy --workspace`/`-p lumen-image` не проходит на этой машине независимо от диффа —
системный `rustc 1.98.x` вместо пина 1.97.0 красит `crates/engine/image/*`
(`chunks_exact_to_as_chunks`, линт, которого нет в 1.97) ещё на этапе сборки зависимостей,
воспроизведено и на чистом `main` тем же прогоном — тот же случай, что BUG-1024/1030/1036.

### Не проверено / дальше

Тот же класс «N `--check` подряд дают N разных наборов» открыт ещё на нескольких категориях —
[BUG-1003](BUG-1003-OPEN.md)/[BUG-1004](BUG-1004-OPEN.md)/[BUG-1005](BUG-1005-OPEN.md)/
[BUG-1011](BUG-1011-OPEN.md)/[BUG-1022](BUG-1022-OPEN.md). Этот срез объясняет и чинит один
конкретный механизм (застрявшая навигация после интерцепта чужого `navigate`-эвента) — он
правдоподобный кандидат и для части их регрессий (`html/rendering`/`html/semantics`/
`resize-observer`/`close-watcher`/`input-events` не тестируют Navigation API напрямую, но любая
категория, где хотя бы один файл вызывает `navigation.navigate()`/`history.back()` и
перехватывает `navigate` без явного `removeEventListener`, воспроизведёт тот же символ), но
это НЕ проверено повторным прогоном ни на одной из них — следующий шаг для владельца каждого
бага: пересобрать бинарь с этим фиксом и повторить их собственное §Воспроизведение.
