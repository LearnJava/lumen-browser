# BUG-788 — `lumen_css_parser::parse` съедает сотни мегабайт на килобайте битого CSS (рост суперлинейный)

**Статус:** OPEN
**Заведён:** 2026-08-19 (TEST-1, второй прогон CI-джоба `fuzz`)
**Область:** css-parser (`crates/engine/css-parser` — только `parse`, разбор таблицы стилей)
**Владелец:** P3 (движок). Заведён P2 в ходе тулинговой задачи, здесь не чинится.

## Симптом

libFuzzer убил `fuzz_css_parser` по памяти:
`ERROR: libFuzzer: out-of-memory (used: 2179Mb; limit: 2048Mb)` на входе в
12 327 байт — прогон
[32283392133](https://github.com/LearnJava/lumen-browser/actions/runs/32283392133),
артефакт `fuzz-artifacts-32283392133`, файл
`oom-418975f2b8473f9634a446d40fe266f9d3999b61`.

Перепроверено вне фаззера, на Windows, сборкой `dev-release`, со счётчиком
аллокаций в качестве глобального аллокатора:

| Вход | Пик аллокаций | Время |
|---|---|---|
| исходные 12 327 байт | **912 МиБ** | 2.55 с |
| минимизированный, 676 байт | **50 МиБ** | 0.12 с |
| он же, повторённый дважды (1352 байта) | не дождались | **>300 с** |

Раздувание на минимизированном входе — ×74 000 к размеру входа. Удвоение
входа увеличивает время более чем в 2500 раз, то есть рост **не линейный и
не квадратичный**: это то, что делает вход в килобайт способным положить
процесс.

Затронут только разбор таблицы стилей. Две другие точки входа того же
харнесса на том же входе стоят микросекунды и не аллоцируют заметно:

```
parse:               peak=912 MiB, elapsed=2.5499496s
parse_inline_style:  peak=0 MiB,   elapsed=9.4µs
parse_selector_list: peak=0 MiB,   elapsed=5.7µs
```

## Минимизированное репро (676 байт, base64)

Минимизация — ddmin по байтовым блокам, критерий «пик ≥ 50 МиБ».
Раскодировать: `base64 -d` (или `python -c "import base64,sys; sys.stdout.buffer.write(base64.b64decode(sys.stdin.read()))"`).

```
bmQgeyBhfQoKLmFsLCBtYXggcywgbWlubnQsIG1hZSBoZWlnaHRzLCBtdCB7Cjo6Z25lIGN1IHs7
W2ZTZnQgeyBhZnQ7IH0KLmFsaWduLCBtYXggcywgbWluLWNvbnRlbnQsIG1heC1jb250ZSBoZWln
aHRzLCBtaW4tY29udCB7CiA6Ojo6eyBhOyB9Ci5hbGFmZUNlbnRlciB7OyAuc2FmZVJpZ2h0IHsg
YXNhaHQ7WyB9RW5kIHsgfQoKLmFsaWduZW50LCBtYXggaGVpZ2h0cywgbWluLWNvbnRlbnQsIG1h
eC1jb250ZSBoZWlnaHRzLCBtaW4tY29udCB7CiAgOm5Db250eEVuZCB7IH0KCi5hbGlnbmVudCwg
bWF4IGhlaWdodHMsIG1pbi1jb250ZW50LCBtaHRzLCBtaW4tY29udCB7CiAgOjp7IGFsbGY6IH0K
LmFsaWduZCB7IGEgbmQ7IH0KCi5hbGlnbmVudCwgbWF4IGhlaWdodHMsIG1pbi1jb250ZW50LCBt
YXgtY29udGUgaGVpZ2h0cywgbWluLWNvbnQgewogICAgaGVpOzo6bnRlbnQge2VuJQAACn0KCi5t
YXgtK2NvewogICAlCn0KCi5tYXgtK2NvbnRlbnQgewogICAgCn0KCi5tYXgtK2NvbnRlbnQgewog
ICAgaGVpZ24gfQoKLmFsaWduQyBudGVyIHsgYWxpZ24tIGNlbnRlcjsgfQouYWVsZlN0YXJ0IHsg
YWx0OyB9Ci5hbGlnblNlbGZTYWZlUmlnaHQgewouYWxpZ25pZ25TZWxmU2FmZUxlYWZ0IHsgYWZl
IGxlZnQ7IH0KLmZlTGVmdCB7IGFsaWdlIGxlZnQ7IH0KLmFsZXhFbmQgeyBhbGlnbg==
```

Декодированный текст — исковерканный фрагмент WPT-шного CSS про
`min-content`/`max-content` (seed-корпус фаззера собран в том числе из
`tests/wpt/`), с характерными чертами: **множество незакрытых блоков `{`**,
`::` и `::::` подряд, `[` без пары, NUL-байты. Правдоподобная гипотеза (не
проверялась): восстановление после ошибки на незакрытом блоке заставляет
разбор возвращаться назад и переразбирать хвост, откуда и суперлинейный рост
— но подтверждать её должен тот, кто чинит.

## Почему это важно

Класс — DoS, путь пользовательский: `parse` вызывается на любом внешнем
`<style>`/`.css`. Килобайта битого CSS достаточно, чтобы вкладка съела
гигабайт и встала. Битый CSS в интернете не редкость и не требует
злонамеренности.

## Что уже сделано

`fuzz_css_parser` внесён в `KNOWN_FAILING` воркфлоу `.github/workflows/fuzz.yml`
рядом с `fuzz_image` ([BUG-787](BUG-787-OPEN.md)) — таргет продолжает гоняться
и собирать репро, но не красит джоб. **Убрать его из списка в коммите с
фиксом.**

## Регрессионный тест

После фикса: положить репро в `fuzz/regressions/fuzz_css_parser-oom-unclosed-blocks`
(имя обязано начинаться с имени таргета — CI сверяет префикс со списком
`cargo fuzz list`) и добавить в `fuzz/corpus/fuzz_css_parser/`. Отдельно —
юнит-тест, фиксирующий верхнюю границу: разбор этого входа обязан
укладываться в разумную память и время. Счётчик аллокаций, которым сняты
цифры выше, — 25 строк (`GlobalAlloc`-обёртка над `System` с двумя
атомиками), его несложно воспроизвести в тесте.
