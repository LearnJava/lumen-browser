# BUG-1018 — `<iframe src="about:blank">` рисовал страницу «Не удалось загрузить фрейм»

**Статус:** FIXED 2026-09-06
**Крейт:** shell (`crates/shell/src/frames.rs::fetch_iframe_source`)
**Найден:** P1, 2026-09-06, при ревизии [BUG-885](BUG-885-FIXED.md)

---

## Симптом

Форма записи меняла результат. Живой замер, три фрейма на одной странице:

```
bare:  cw=object cd=object location.href=about:blank URL=about:blank body=""
blank: cw=object cd=object location.href=about:blank URL=about:blank
       body="Не удалось загрузить фрейм about:blank загрузка 'about:blank' …"
real:  cw=object cd=object location.href=http://…/child.html            body="child"
```

`<iframe>` без атрибута давал пустой документ — правильно. `<iframe src="about:blank">`
давал **видимую коробку с ошибкой** внутри фрейма. По HTML §7.6 это один и тот же
под-документ, форма записи не должна значить ничего.

В stderr это выглядело так:

```
iframe: загрузка 'about:blank' не удалась: network error: unsupported scheme: about
```

## Механизм

`fetch_iframe_source` (`crates/shell/src/frames.rs`) отсеивала `javascript:` и `data:`
явными ранними ветками, а `about:blank` — нет: строка доходила до
`base.resolve(src)` → сетевого клиента → `lumen_network` отвечал
`unsupported scheme: about`. Ошибка возвращалась как `FetchError`, а вызывающая
сторона (`spawn_frame`, ветка `Some(Err(e))`) по правилу FRAME-4 среза 2 подставляет
вместо под-документа синтетическую страницу `frame_error_document` и ставит
`load_failed = true`.

Пустой `src` при этом обрабатывался правильно (`FrameSource::Inline(String::new())`,
первая же строка функции), и ветка `None` в `spawn_frame` помечает такой под-документ
адресом `about:blank` — то есть правильное поведение уже было рядом, просто явная
форма до него не доходила.

## Фикс

Ранняя ветка той же формы, что у `javascript:`/`data:`, но с `Ok`, а не `Err`:
`about:blank` (плюс хвост `?…`/`#…` — это по-прежнему about:blank-документ)
возвращает пустой `FrameSource::Inline`. Прочие `about:`-адреса (`about:config`
и т. п.) отказывают, как и раньше — спека выделяет только `about:blank` и
`about:srcdoc`, а у второго свой путь через атрибут.

## Проверка

- `fetch_iframe_source_treats_about_blank_as_an_empty_document`
  (`crates/shell/src/tests/scripts_and_frames.rs`): четыре формы записи
  (`about:blank`, `ABOUT:BLANK`, `?x=1`, `#frag`) дают пустой `Inline`; пустой `src`
  даёт то же самое; `about:config` по-прежнему `Err`.
- Тот же живой замер после фикса: `blank: … body=""` — совпадает с `bare`, строки
  `unsupported scheme: about` в stderr больше нет.

## Область, которой это НЕ касается

Голдены не затронуты: ни одна страница в `graphic_tests/` и `samples/` не использует
`about:blank` во фрейме (проверено грепом), а поведение фрейма без `src` не менялось.
