# SITES-TRIAGE — превратить прогон корпуса в очередь починок

Решение пользователя 2026-10-28: источник работы — реальные сайты, а не WPT
(ADR-034 «уровень 0 — сайт работает»). Прогон COMPAT-CORPUS-2-BASE
(2026-10-20, 500 сайтов, `docs/perf/journal.md:791-933`) показал CPR 5,9 % на
abroad / 15,2 % на ru, `blank:near-empty` на 239 сайтах, TIMEOUT+HUNG на 168, —
и по нему не заведено ни одного бага. Эта дорожка закрывает разрыв.

## Данные

- Сырьё прогона (в git нет, `.tmp/` в `.gitignore`): копия в основном checkout
  `D:/RustProjects/lumen-browser/.tmp/compat-crawl-2026-10-20/{abroad,ru}.results.jsonl`
  (оригинал — `.claude/worktrees/p2-work/.tmp/compat-crawl-{abroad,ru}/`). Одна строка
  JSON на сайт. Поля: `slug, url, lumen_status, checks{status,walls,exceptions,pixels,a11y},
  runs[].lumen{status, exceptions, net_failures, stderr_error_sigs, text_len, ready_s, png}`.
  Нормализация исключений — `exc_sig` (`scripts/compat_crawl.py:168-182`), правило
  `blank:near-empty` — `compat_crawl.py:81-82`, таксономия статусов —
  `perf_audit.classify_status` (`scripts/perf_audit.py:177`).
- Вес сайта — колонка `вес` в `docs/perf/corpus-c.tsv` (3 = позиция 1–50 в сегменте,
  2 = 51–200, 1 = остальные). «Вес кластера» = сумма весов его сайтов.
- Global-сайты разбирать по точке abroad, RU-сайты — по точке ru (journal:838-850:
  на ru global завышен гео-блокировкой).

## Сайты пользователя — первыми

`docs/perf/corpus-user-ru.txt` — 30 сайтов: 6 названы пользователем (dzen, pikabu, mail.ru,
ozon, avito, habr), 24 — популярные RU-сайты. Это главная проверка: в каждом срезе 2–4 сначала
разбираются они, потом остальной корпус. Открывать **без прокси** (решение пользователя):
в `data/fingerprint.toml` нет `proxy =`, Chrome — `--chrome-arg=--no-proxy-server`.
Состояние на 2026-10-20 (точка ru): mail.ru BROKEN_RENDER, avito HUNG + blank, pikabu blank
31 с, ozon SITE_REFUSED (антибот), ok/lenta/kp/sports HUNG, rutube/banki TIMEOUT, gosuslugi и
gazeta BROKEN_RENDER; dzen и habr в корпусе C не было.

## Общие правила для срезов 2–4

- Каждый кластер, который чинится в движке, — строка `OPEN` в `BUGS.md` (номер — по
  `origin/main` прямо перед коммитом) + `bugs/BUG-NNN-OPEN.md` с полями: **Источник: сайты**
  (список slug, число сайтов, вес кластера), минимальная локальная репродукция
  (`tests/fixtures/…` или `.html` в брифе), ожидание по Chrome, предполагаемое место в коде.
- Указатель на новый баг — в конец `STATUS-P3.md`; отсутствующее CSS-свойство — строка в
  `CSS-SPECS.md` и указатель в конец `STATUS-P4.md`. Порядок дописывания — по весу кластера.
- Не чинить самому. Не заводить баг по одному сайту, если причина не подтверждена
  живым запуском (`--maximized`, `LUMEN_NO_ADBLOCK=1`, `docs/automation.md` §Flags).
- Сайт за антибот-стеной / гео-блоком / требующий логина — класс «не наш», бага нет.

## SITES-TRIAGE-1 — скрипт кластеризации и отчёт (P2, M)

**Files:** `scripts/compat_triage.py` (новый), `scripts/test_compat_triage.py` (новый),
`docs/perf/runs/2026-10-20-compat-triage.md` (новый), `docs/perf/journal.md` (строка-ссылка).

1. Скрипт читает один или два `results.jsonl` + `corpus-c.tsv`, для каждого сайта с
   `lumen_status != OK` назначает **первичный класс** по приоритету: `TIMEOUT`/`HUNG` →
   `NET_FAIL` → `blank:near-empty` → `BROKEN_RENDER` → `pixels` (только `differing`) →
   `a11y`. Сайты `SITE_REFUSED`/антибот (`walls.chrome_hits` или `hard:*`) — в класс «не наш».
2. Внутри класса группирует по причине: сигнатура первого нового исключения Lumen
   (`checks.exceptions.new_sigs`), отдельно — «нет API» (регексы `is not a function`,
   `is not defined`, `is not a constructor`, `Cannot read properties of undefined` с именем
   свойства), `net_failures` по типу, `stderr_error_sigs`. Сайт без сигнатуры — «без улик».
3. Отчёт markdown: таблица классов (сайтов / вес), затем кластеры по убыванию веса:
   причина, N сайтов, вес, 3–5 примеров slug+url, ready_s. Отдельно — топ-30 сайтов веса 3
   с их классом.
4. CLI: `python scripts/compat_triage.py --results A.jsonl [--results B.jsonl] --corpus
   docs/perf/corpus-c.tsv --out <md>`; тест на синтетических строках JSONL (3–4 сайта).
5. Свежий прогон сайтов пользователя без прокси:
   `python scripts/compat_crawl.py --corpus docs/perf/corpus-user-ru.txt --env LUMEN_NO_ADBLOCK=1
   --no-rerun --timeout 40 --chrome-arg=--no-proxy-server --out .tmp/compat-crawl-user-<дата>`
   (≈ 30 мин) и первым разделом отчёта — таблица по этим 30 сайтам: статус, класс, причина,
   `ready_s`. Сайт пользователя получает вес 5 при подсчёте веса кластера.

**Done when:** тест зелёный; отчёт закоммичен; в нём таблица 30 сайтов пользователя и ≥ 15
кластеров с весом и примерами, сумма сайтов по классам = числу не-OK сайтов.

## SITES-TRIAGE-2 — пустые страницы (P2, M; после SITES-TRIAGE-1)

Класс `blank:near-empty`. Взять кластеры из отчёта SITES-TRIAGE-1 по убыванию веса, пока
не разобраны 30 сайтов веса 2–3. Для каждого — живой запуск Lumen и Chrome, причина пустоты
одной строкой (JS-исключение / нет API / скрипт не загрузился / таймаут загрузки / раскладка
скрыла контент / прочее). Завести баги по правилам выше (ожидаемо 5–12 багов).

**Done when:** в отчёте добавлен раздел «Разбор blank» с причиной у каждого из 30 сайтов;
баги заведены, указатели дописаны в STATUS-P3/P4 по весу.

## SITES-TRIAGE-3 — зависания и таймауты (P2, M; после SITES-TRIAGE-1)

Классы `TIMEOUT`/`HUNG` (168 сайтов abroad, из них ≈105 global — дело в Lumen, journal:851).
То же, что срез 2, для 30 сайтов наибольшего веса: на каком этапе стоит загрузка (сеть /
стадия `parse_and_layout` по `--trace` / долгий скрипт / цикл перерисовки). Сверить с
открытыми BUG-1676, BUG-1646, BUG-1694 и задачами LOAD-* / COMPAT-HANG — не дублировать,
а дописать сайты в существующий баг.

**Done when:** раздел «Разбор зависаний» с причиной у 30 сайтов; новые баги заведены,
существующие дополнены списками сайтов.

## SITES-TRIAGE-4 — «открылось, но выглядит не так» (P2, M; после SITES-TRIAGE-1)

Сайты со статусом OK/DEGRADED и `checks.pixels.differing`, топ-30 по `вес × diff_pct`.
Снять пару кадров Lumen/Chrome, назвать первое видимое расхождение (шрифт, сетка, flex,
картинка, позиционирование, отсутствующее свойство), свести в кластеры, завести баги/строки
CSS-SPECS.

**Done when:** раздел «Разбор вёрстки» с причиной у 30 сайтов; баги и строки CSS-SPECS
заведены, указатели дописаны по весу.

## SITES-RECRAWL-1 — повторный замер (P2, L; после LOAD-2 и SITES-TRIAGE-2)

Повторить прогон точки abroad той же командой (journal:793) на свежем `dev-release` из
`main`, затем `compat_triage.py` на новом и старом `results.jsonl`. В журнал — CPR, число
`blank`, `TIMEOUT/HUNG` до/после и медиана `ready_s`. Проверить, что на D свободно ≥ 30 ГБ
до старта (прошлый прогон падал с ENOSPC, journal:814).

**Done when:** запись в `docs/perf/journal.md` с таблицей до/после; регрессии (сайт был OK,
стал не OK) — заведены багами.
