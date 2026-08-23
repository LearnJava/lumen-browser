# Задача: WPT-RUN-doc-sync — синхронизировать docs/wpt-status.md с фактами

**Developer:** P2
**Ветка:** `p2-wpt-status-sync`
**Размер:** S
**Крейты:** — (только документация; код и тесты не меняются)
**ROADMAP:** строка `WPT-RUN-doc-sync` (`ROADMAP.md:759`, parent `WPT-RUN`, status `ready`)

## Контекст

`docs/wpt-status.md` — заявленный источник правды по охвату WPT («Живой документ готовности»),
но он разошёлся с фактами в трёх местах и именно из него выросла неверная оценка охвата
«2/277 категорий»:

1. Секция «Охват» до сих пор утверждает: «вендорены и гоняются две — `dom/nodes/` и `FileAPI/`…
   Остальные 275 категорий не вендорены». Это текст времён 2026-07-18.
2. Категорийный индекс показывает **25 категорий** как невендоренные (`—`, пустые заметки), хотя
   работа по каждой закрыта: строки `WPT-VENDOR-*` в `ROADMAP.md` в статусе `done`, каталоги
   существуют на диске, у четырёх есть полные заметки в `docs/wpt-vendor-notes/`.
3. Строка `mixed-content` помечена 🟡 без причины 🟡 в заметке (прогон остановлен вручную на
   78/388 id по методологии нулевой дисперсии — весь сэмпл TIMEOUT на TLS-гэпе).

Факт на 2026-08-23: **все 277 категорий верхнего уровня вендорены на диск**; трек `WPT-VENDOR`
в `ROADMAP.md` целиком `done` (289 строк). Единственная частично вендоренная категория — `dom`
(только `dom/nodes/`, остаток ведёт отдельная задача `WPT-VENDOR-dom-rest`).

## Пред-запуск

- [ ] Прочитать шапку и секции «Охват», «Легенда», «Как обновить этот файл»
      `docs/wpt-status.md:1-104`
- [ ] Убедиться, что ветка `main` чиста: `git status`

## Шаги

### 1. Создать ветку и worktree

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/wpt-status-sync -b p2-wpt-status-sync
cd .claude/worktrees/wpt-status-sync
```

### 2. Переписать секцию «Охват» (`docs/wpt-status.md:13-30`)

Заменить устаревший подсчёт («вендорены две… остальные 275 не вендорены») на фактическое
состояние:

- все 277 категорий верхнего уровня вендорены на диск; трек `WPT-VENDOR` в `ROADMAP.md`
  целиком `done`;
- единственное исключение по полноте вендоринга: `dom` — только `dom/nodes/`
  (остаток — открытая строка `WPT-VENDOR-dom-rest`);
- оговорка по полноте прогона: часть категорий прогонялась выборочно при нулевой дисперсии
  результата (методология `css`/`mixed-content`) — это ограничение замера, не вендоринга;
- абзацы про `test_driver.*` (WPT-RUN-2) и HTTPS-порт оставить — они про исполнителя,
  а не про охват, и остаются верными.

### 3. Проставить 25 строкам индекса «Вендорено ✅» + короткую заметку

Правила колонок — см. «Как обновить этот файл»: одно предложение (дата DONE, headline),
ссылка `[Подробности](wpt-vendor-notes/<slug>.md)` если файл есть, иначе — на строку ROADMAP
(`ROADMAP.md` поиском по id). Текст заметки брать из ноты соответствующей строки ROADMAP,
НЕ выдумывать. Номера строк зафиксированы 2026-08-23; при сдвиге искать по id.

| Категория | Строка ROADMAP | Дата DONE | Заметка wpt-vendor-notes |
|---|---|---|---|
| `content-security-policy` | `WPT-VENDOR-content-security-policy` (~L372) | 2026-07-25 | есть |
| `inert` | `WPT-VENDOR-inert` (~L447) | 2026-08-05 | есть |
| `intervention-reporting` | `WPT-VENDOR-intervention-reporting` (~L452) | 2026-08-05 | есть |
| `page-visibility` | `WPT-VENDOR-page-visibility` (~L494) | 2026-08-05 | нет |
| `pointerevents` | `WPT-VENDOR-pointerevents` (~L508) | 2026-08-05 | нет |
| `presentation-api` | `WPT-VENDOR-presentation-api` (~L511) | 2026-08-05 | нет |
| `proximity` | `WPT-VENDOR-proximity` (~L515) | 2026-08-05 | нет |
| `push-api` | `WPT-VENDOR-push-api` (~L516) | 2026-08-05 | нет |
| `referrer-policy` | `WPT-VENDOR-referrer-policy` (~L518) | 2026-08-05 | нет |
| `reporting` | `WPT-VENDOR-reporting` (~L520) | 2026-08-05 | нет |
| `requestidlecallback` | `WPT-VENDOR-requestidlecallback` (~L521) | 2026-08-05 | нет |
| `resize-observer` | `WPT-VENDOR-resize-observer` (~L522) | 2026-08-05 | нет |
| `resource-timing` | `WPT-VENDOR-resource-timing` (~L523) | 2026-08-05 | нет |
| `sanitizer-api` | `WPT-VENDOR-sanitizer-api` (~L524) | 2026-08-05 | нет |
| `savedata` | `WPT-VENDOR-savedata` (~L525) | 2026-08-05 | нет |
| `scheduler` | `WPT-VENDOR-scheduler` (~L526) | 2026-08-05 | нет |
| `screen-capture` | `WPT-VENDOR-screen-capture` (~L527) | 2026-08-05 | нет |
| `screen-details` | `WPT-VENDOR-screen-details` (~L528) | 2026-08-05 | нет |
| `screen-orientation` | `WPT-VENDOR-screen-orientation` (~L529) | 2026-08-06 | нет |
| `screen-wake-lock` | `WPT-VENDOR-screen-wake-lock` (~L530) | 2026-08-06 | нет |
| `server-timing` | `WPT-VENDOR-server-timing` (~L538) | 2026-08-06 | нет |
| `shared-storage` | `WPT-VENDOR-shared-storage` (~L542) | 2026-08-06 | есть |
| `speech-api` | `WPT-VENDOR-speech-api` (~L547) | 2026-08-06 | нет |
| `storage` | `WPT-VENDOR-storage` (~L548) | 2026-08-06 | нет |
| `storage-access-api` | `WPT-VENDOR-storage-access-api` (~L549) | 2026-08-06 | нет |

Для строк без заметки ссылка вида «[Подробности](../ROADMAP.md)» не нужна — достаточно
«Вендорена + прогнана DD.MM (DONE-нота в строке `WPT-VENDOR-<cat>` ROADMAP.md); headline из ноты».

### 4. Сверить две 🟡-строки

- `dom` — остаётся 🟡 (`dom/nodes/` только); убедиться, что заметка указывает на
  `WPT-VENDOR-dom-rest`.
- `mixed-content` — дописать причину 🟡 в заметку: прогон остановлен вручную на 78/388 id,
  100% TIMEOUT на TLS-гэпе WPT-RUN-2, продолжение признано нерентабельным
  (`docs/wpt-vendor-notes/mixed-content.md`). Статус не менять.

### 5. Не трогать генерируемый блок

Блок между `<!-- gen:dom/nodes:start -->` и `<!-- gen:dom/nodes:end -->` — вывод
`tests/wpt/gen_status_md.py`; его содержимое не редактировать.

## Проверка

```bash
# 1. Старое утверждение исчезло
grep -n "вендорены и гоняются две" docs/wpt-status.md        # пусто
# 2. Ни одной невендоренной категории в индексе
grep -cE '^\| `[a-z0-9-]+` \| [⬜🚫] \| —' docs/wpt-status.md # 0
# 3. Генерируемый блок не задет
git diff docs/wpt-status.md | grep -E '^[+-].*gen:dom'       # пусто
```

## Критерии готовности

- [ ] Интро описывает фактический охват (277/277 вендорены, оговорки перечислены явно)
- [ ] Все 25 строк индекса: ✅ + короткая заметка со ссылкой на источник
- [ ] 🟡-строки `dom`/`mixed-content` имеют объясняющие заметки
- [ ] `gen:dom/nodes`-блок побайтово не изменён
- [ ] Коммит влит в `main`, файл задачи удалён, строка удалена из `STATUS-P2.md`,
      `ROADMAP.md` `WPT-RUN-doc-sync` → `done` (DONE-нота одним предложением)
