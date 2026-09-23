# BUG-1101 — crates.io рендерится пустой белой страницей из-за `[unhandled-rejection] TypeError: Cannot read properties of undefined (reading 'get')`

**Статус:** OPEN
**Тип:** дефект (живой сайт) — гипотеза не подтверждена отладчиком, только пробой `--dump-layout`.
**Заведён:** 2026-09-23 (P6, RP-10 — повторный аудит реальных сайтов против Edge).
**Область:** не локализовано — JS-ошибка приходит из минифицированного бандла crates.io
(SvelteKit, `_app/immutable/chunks/*.js`), не из движка напрямую; кандидат со стороны
движка — реализация `fetch()`/`Response`/`Headers` (`crates/js/src/dom.rs`).
**Владелец:** P3.

## Симптом

Живой прогон `/lumen-perf-audit --mode compat` (RP-10, `d530491e1`): `crates.io` теперь
проходит TLS/антибот-проверку (`← 200 https://crates.io/`, антибот-список в
`docs/perf/corpus.txt` для этого сайта устарел — см. также обновление списка в этом же
коммите), но итоговый кадр — **пустая белая страница** (скриншот
`.tmp/perf-audit/20260923-022309/crates.png`, не коммитится, см. журнал), классификатор
даёт `BROKEN_RENDER`.

`--dump-layout https://crates.io/` (headless, тот же бинарь) воспроизводит идентично:
после серии `preload`/`GET` на чанки `_app/immutable/...` и двух API-запросов —

```
→ GET https://play.rust-lang.org/meta/crates
← 200 https://play.rust-lang.org/meta/crates
→ GET https://crates.io/api/v1/site_metadata
→ GET https://crates.io/api/v1/summary
[unhandled-rejection] TypeError: Cannot read properties of undefined (reading 'get')
```

— ошибка вылетает ДО того, как `site_metadata`/`summary` успевают вернуться (оба GET
ещё в полёте в момент отказа), поэтому подозрение падает на код, читающий что-то из
уже завершившегося `play.rust-lang.org/meta/crates` (например `resp.headers.get(...)`
на объекте `Response`, который в эту секунду уже разрешился). Итоговый `dump-layout`
даёт почти пустое дерево (`Block rect=(0,0,1024,70.79)` — один заголовочный блок,
`display=none` у соседа), что совпадает с белым скриншотом живого окна.

## Не проверено

- Точный вызывающий код — бандл минифицирован, sourcemap не подгружался; не установлено,
  `Response`/`Headers`-ли это метод движка спотыкается, или сам SvelteKit-код на
  legitimate `undefined` (в норме отловленный `try/catch`, которого в проде нет).
- Не проверено на `--mcp-live-port` с `LUMEN_PROFILE_TREE`/точечным брейкпоинтом в
  `Headers.get`.
- Не сравнивалось с Edge (сеть в этой среде даёт живому Edge/curl 200 на все три URL —
  внешнего отказа нет, дело в клиентской обработке).

## Как воспроизвести

```
lumen.exe --dump-layout https://crates.io/
```

headless, без `--mcp-live-port`; ошибка в stderr, дерево почти пустое.
