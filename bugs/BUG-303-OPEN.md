# BUG-303: github.com — JS-исполнение не завершается (≥240 с) и на V8

**Статус:** OPEN
**Дата:** 2026-07-17
**Компонент:** js (V8-рантайм / event loop) — точный виновник не установлен
**Найден:** перф-аудит `/lumen-perf-audit`, прогон 2026-07-17 (docs/perf/runs/2026-07-17.json)

## Симптом

`--dump-layout https://github.com/` (и `--screenshot`) не завершаются за
240 с таймаута, при этом `--dump-source` той же страницы — 0.7 с, т.е. сеть
и парсинг в норме; висит именно стадия JS/каскад/layout.

```
github  TIMEOUT  http=200  src=0.7s  lay=240.29s(timeout)  shot=240.29s(timeout)
stderr: module error: JS runtime error: Automatic publicPath is not supported in this browser
```

## Почему это отдельный баг

Июльский аудит (журнал docs/perf/journal.md, «Исторический контекст»)
списывал зависание github на QuickJS-интерпретатор без JIT. С тех пор дефолт —
V8 (ADR-018), но симптом воспроизводится 1-в-1 ⇒ гипотеза «медленный движок»
опровергнута. Похоже на незавершающийся цикл/ожидание: вероятно, webpack-бандл
падает или крутится из-за отсутствующего API (см. `Automatic publicPath` —
нужен `document.currentScript`/`import.meta.url`?), либо event loop не
дренируется.

## Диагностика (следующий шаг)

- `LUMEN_PROFILE_TREE=1` — где висим: cascade/layout или JS.
- Обрезать бандлы бинарным поиском через `--dump-layout` на сохранённой копии
  страницы; проверить наличие `document.currentScript`.
- stderr-лог стадии: `.tmp/perf-audit/20260717-121153/github.layout.stderr.log`.

## Ожидание

Загрузка завершается (пусть с деградацией фич) за разумное время; сторонний
сайт не должен уметь вешать пайплайн навсегда — возможно, нужен и общий
watchdog на JS-фазу загрузки.
