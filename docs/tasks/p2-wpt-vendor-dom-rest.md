# Задача: WPT-VENDOR-dom-rest — довендорить остаток категории `dom`

**Developer:** P2
**Ветка:** `p2-wpt-vendor-dom-rest`
**Размер:** M
**Крейты:** — (тулинг `tests/wpt/`, Python; Rust-кода правки не предполагаются)
**ROADMAP:** строка `WPT-VENDOR-dom-rest` (`ROADMAP.md:758`, parent `WPT-VENDOR`, status `ready`)

## Контекст

Категория `dom` — единственная, вендоренная частично: `tests/wpt/dom/` содержит только
`dom/nodes/` (168 файлов, курируемый S5/S6-гейт `run_suite.py`). Остальные подкаталоги
апстримной категории (`events/` — крупнейший: dispatch/EventTarget/Event-интерфейсы,
`ranges/`, `traversal/`, `collections/`, …) на диске отсутствуют и ни одной открытой строкой
бэклога не учтены. После закрытия всех 289 строк трека `WPT-VENDOR` это последняя дыра
вендоринга: без неё требование «охватить все тесты WPT» невыполнимо по определению.

Пин апстрима тот же, что у всего дерева: `35be3b44f3111c4d614b5b201e399493d20e7b38`
(`tests/wpt/VENDOR.md`) — НЕ перекачивать новый master.

## Пред-запуск

- [ ] Прочитать `tests/wpt/VENDOR.md` (метод вендоринга: committed snapshot через sparse-клон)
- [ ] Прочитать методологию разбора результатов: `docs/wpt-status.md:49-103`
      («Методология: не одна задача на тест» + регрессионный гейт TEST-3)
- [ ] Убедиться, что ветка `main` чиста: `git status`

## Шаги

### 1. Создать ветку и worktree

```bash
export PATH="/c/Users/konstantin/.cargo/bin:$PATH"
git worktree add .claude/worktrees/wpt-dom-rest -b p2-wpt-vendor-dom-rest
cd .claude/worktrees/wpt-dom-rest
```

### 2. Вендорить остаток `dom/` с запиненного коммита

Переиспользовать sparse-клон по процедуре `tests/wpt/VENDOR.md` (тот же приём, что у других
категорий: `git sparse-checkout add dom` поверх существующего временного клона на пине
`35be3b44f3111c4d614b5b201e399493d20e7b38`). Скопировать всё дерево `dom/` КРОМЕ уже
вендоренного `dom/nodes/` в `tests/wpt/dom/`. Файлы брать без изменений (verbatim upstream).

### 3. Довендорить внекатегорийные зависимости

```bash
python tests/wpt/find_missing_resources.py --root dom --ids
```

Отсутствующие пути довендорить с того же пина. Пути из общего бэклога `WPT-RUN-11`
(`/common/reftest-wait.js`, `/common/blank.html`, …) допускается довендорить точечно здесь,
если нужны для прогона `dom` — отметить это в DONE-ноте, чтобы RUN-11 вычел их из своего
списка. Новые записи инфраструктуры добавить в таблицу `tests/wpt/VENDOR.md`.

### 4. Прогнать категорию

```bash
export LUMEN_PROFILE=dev-release MSYS2_ARG_CONV_EXCL='/dom'
BIN=$(cygpath -w "$PWD/target/dev-release/lumen.exe")
tests/wpt/.venv/Scripts/python.exe tests/wpt/run_report.py \
  --binary "$BIN" --all --root dom --recursive --processes=4 \
  --out .tmp/wpt-report-dom-rest.html
```

`--processes=4` обязателен: `events/` крупный. Если прогон не помещается в сессию — резать
по подкаталогам (`--root dom/events` и т.д.), как делали срезы `html/*`; ни один подкаталог
не выбрасывать молча.

### 5. Разбор результатов по методологии

- Кластеры провалов группировать по первопричине: один `BUG-NNN` на причину с списком
  задетых тестов (известные гэпы — BUG-480 iframe, BUG-657 TLS, BUG-346/347/359 семейство
  относительных URL — переподтверждать в их файлах, а не заводить новые).
- Первый проход категории фиксирует факты, чинить движковые баги из этой задачи не нужно.

### 6. Зафиксировать результаты

- `docs/wpt-status.md`: строку `dom` 🟡 → ✅, короткая заметка (дата, число файлов/id,
  headline) + `[Подробности](wpt-vendor-notes/dom.md)`; полный отчёт — в
  `docs/wpt-vendor-notes/dom.md` (создать).
- `tests/wpt/VENDOR.md`: строка `tests/wpt/dom/` (кроме уже описанного `dom/nodes/`)
  по образцу соседних категорий.
- `ROADMAP.md`: строка `WPT-VENDOR-dom-rest` → `done` с DONE-нотой.
- Опционально (гейт TEST-3): `run_report.py --all --root dom --update-expected` +
  немедленный `--check` (exit 0). Каталог `tests/wpt/metadata/dom/nodes/` НЕ трогать —
  отдельный ручной гейт `run_suite.py` (S5/S6).

## Проверка

```bash
# Подкаталоги категории на диске
ls tests/wpt/dom/
# Ни один тест dom не ссылается на отсутствующий файл
python tests/wpt/find_missing_resources.py --root dom --ids   # пусто либо только ROUTED_NOT_VENDORED
```

## Критерии готовности

- [ ] `tests/wpt/dom/` содержит все подкаталоги пина, кроме сознательно исключённых
      (исключения перечислены в DONE-ноте)
- [ ] Прогон `run_report.py --all --root dom --recursive` дошёл до конца (или разбит на
      зафиксированные срезы по подкаталогам), результаты сведены в заметку
- [ ] `docs/wpt-status.md`, `tests/wpt/VENDOR.md`, `ROADMAP.md` обновлены;
      `metadata/dom/nodes/` не изменён
- [ ] Коммит влит в `main`, файл задачи удалён, указатель удалён из `STATUS-P2.md`
