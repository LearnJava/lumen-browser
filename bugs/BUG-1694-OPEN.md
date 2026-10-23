# BUG-1694 — Документ на ≈850 КБ с ≈17 000 `<span>` (`display:none`) загружается ≈21 с и обрывает навигацию родителя по таймауту

**Статус:** OPEN
**Заведён:** 2026-10-23 (P2, WPT-ENC-SCOUT)
**Область:** html-парсер/DOM/layout (загрузка большого документа с десятками тысяч элементов), шелл (ожидание `DocumentReady`)

## Симптом

`encoding/legacy-mb-*/*-decode-*.html` (например `korean/euckr-decode-korean.html?1-1000`) встраивают iframe `*_chars-*.html` — один документ ≈850 КБ с ≈17 000 `<span>` с `display:none`. В одиночном запуске ответ приходит в 1:50.8, а «Распарсено» — в 2:11.8, то есть документ грузится **≈21 с**. Под 10 параллельными браузерами это не укладывается в 30 с ожидания `DocumentReady` (`bidi-server/src/protocol.rs:68`), и родительская страница получает ERROR: `browsingContext.navigate … wait timeout: DocumentReady`.

В WPT-RUN-16 это 321 из 385 id кластера C (`legacy-mb-*/*-decode-*`) со статусом ERROR; при перепрогоне с меньшей нагрузкой 134 из 157 разошедшихся id перешли `ERROR → OK`. То есть сама навигация — перф-дефект, а не нехватка декодеров (их отсутствие — отдельная фича [ENC-CJK](../ROADMAP.md)).

## Проба

`python tests/wpt/run_corpus.py --binary <dev-release> --prefixes encoding/legacy-mb-korean --processes 1` и разбор лога шарда: интервал между «ответ получен» и «Распарсено» для `*_chars-*.html`. Профилировать лучше на самом iframe-документе, открытом напрямую: `--dump-layout` / `LUMEN_PERF`-метки (см. `docs/perf/`).

## Как найдено

WPT-ENC-SCOUT, кластер C (сам перф-дефект блокирует измерение остальных 383 id; потолок кластера +383 балла после ENC-CJK).

## Что делать

Профилировать загрузку документа с 17 000 элементов (парсер → DOM → cascade → layout для `display:none`-потомков); цель — порядка секунды. Затем перепрогнать `encoding/legacy-mb-korean/euckr-decode-korean.html?1-1000` с `--processes 10`: ни одного ERROR навигации.

## Как проверить

Сгенерировать HTML с 17 000 `<span style="display:none">` (≈850 КБ) и измерить время до `DocumentReady`; регрессия — перф-тест с порогом.
