# BUG-924 — `<audio src>` не резолвится относительно базы документа: относительный URL умирает как `MEDIA_ERR_SRC_NOT_SUPPORTED`, запроса нет

**Статус:** FIXED (обнаружено как уже исправленное, P3, 2026-09-23)
**Заведён:** 2026-08-25 (P1, остаток [BUG-799](BUG-799-FIXED.md))
**Область:** js (`crates/js/src/audio_element.rs:264-283` — `startLoad(url)` передаёт значение атрибута в `__lumen_audio_load` как есть)
**Владелец:** P1/P3

## Симптом

Три `<audio>` на одной странице, поданной с `http://127.0.0.1:8796/p11.html`,
три написания одного и того же файла:

```
EV abs:loadstart      @1ms      EV abs:loadeddata @54ms          rs=4
EV rootrel:loadstart  @3ms      EV rootrel:error  @58ms code=4   rs=0 ns=0
EV rel:loadstart      @7ms      EV rel:error      @58ms code=4   rs=0 ns=0
```

`http://127.0.0.1:8796/sine440.mp3` грузится; `/sine440.mp3` и
`sine440.mp3` — нет. `error.code` = 4 (`MEDIA_ERR_SRC_NOT_SUPPORTED`), то
есть отказ выглядит как «формат не поддержан», хотя это тот же самый файл.
**Собственный сервер пробы запроса не видит вовсе** — в его логе за этот
прогон одна строка `REQ /sine440.mp3` (абсолютный вариант), поэтому «не
дошло до сети», а не «сеть отказала».

## Механизм

`startLoad` (`audio_element.rs:281`) зовёт `__lumen_audio_load(_handle, url)`
значением атрибута без единого преобразования. Резолва относительно базы
документа нет ни на JS-стороне, ни в нативе — ровно та же дыра, что
[BUG-780](BUG-780-FIXED.md) закрыл в `xhr.rs` и что [BUG-858](BUG-858-OPEN.md)
держит открытой в `sendBeacon`. Причина повторения та же, что записана в
`CLAUDE.md`: `audio_element.rs` — свой `rt.eval`, до которого правка в
`WEB_API_SHIM` не доходит.

## Почему это стоит больше, чем выглядит

Это и есть причина, по которой вся ветка `audio-loading-*` в WPT не
загружает ничего: каждый её тест берёт `/media/sine440.mp3`. Проба под
живым `wptserve` (временная страница в дереве категории, удалена после
замера):

```
PROBE plain:loadstart @1ms   PROBE plain:error @53ms
PROBE pipe:loadstart  @2ms   PROBE pipe:error  @53ms
PROBE FINAL plain rs=0 ns=0 err=4 | pipe rs=0 ns=0 err=4
```

— то есть и `?pipe=trickle(d2)`, и голый файл падают одинаково и за 53 мс,
так что «медленная отдача» тут ни при чём.

**Поправка к формулировке остатка в [BUG-799](BUG-799-FIXED.md):** там этот
остаток записан как «`loadeddata` не приходит». Это симптом, а не механизм:
на абсолютном URL `loadeddata` приходит всегда и быстро — 284 мс на обычной
отдаче, 2 048 мс на отдаче, задержанной на две секунды по `Content-Length`,
2 046 мс на такой же по `Transfer-Encoding: chunked`. Событийная часть
исправна; не резолвится URL.

Вне WPT задет любой сайт, пишущий `<audio src="/media/x.mp3">` — то есть
обычное написание.

## Побочно измеренное (тем же прогоном, чинить вместе)

- `audio.duration` — `Infinity` у полностью загруженного файла (`rs=4`),
  вместо реальной длительности;
- `audio.currentSrc` — `undefined` (HTML LS §4.8.11.2 требует абсолютный URL
  выбранного ресурса; он же и есть естественное место для резолва).

## Как проверить фикс

`<audio src="/media/sine440.mp3">` и `<audio src="sine440.mp3">` на странице,
поданной по http, доходят до `loadeddata`, сервер видит по одному GET на
каждый, `currentSrc` — абсолютный URL, `duration` — число. Прогон
`html/semantics/embedded-content/the-audio-element` целиком: сегодня
`audio-loading-eager` FAIL, `audio-loading-lazy-in-scroller` FAIL,
`audio-loading-lazy-in-viewport` TIMEOUT.

**Ловушка при замере:** три теста этой категории
(`audio-loading-load-deferred`, `…-preload-auto-deferred`,
`…-preload-metadata-deferred`) сейчас **зелёные именно из-за этого бага** —
они проверяют `readyState === HAVE_NOTHING` через 1 000 мс, а у нас ресурс
не грузится никогда. После починки они станут красными по
[BUG-925](BUG-925-OPEN.md) (`loading=lazy` не реализован), и это движение
вперёд, а не регресс.

## Фикс

Резолв (и попутно `upgrade-insecure-requests`-переписывание) уже был внесён
в `startLoad` коммитом `d2e8b27b7` (GAP-CSPENF срез 51, 2026-09-20) —
изначально ради того, чтобы CSP `media-src` проверялся против абсолютного
URL, но переменная `_abs` заодно заменила собой `url` и в самом вызове
`__lumen_audio_load(_handle, _abs)` (было `__lumen_audio_load(_handle, url)`
до среза 51, см. срез 17: там резолв был только для CSP-проверки, а
загрузчик всё ещё получал сырой атрибут — сформулировано явным
комментарием-ограничением, снятым в срезе 51). Заявка на этот баг не была
отмечена закрытой, потому что фикс был найден по другой причине (CSPENF), не
по этому багу — задача P3 2026-09-23 обнаружила расхождение доков и
подтвердила пробой.

## Замер (P3, 2026-09-23)

`tests/wpt/verify_bug924_audio_src_resolve.py` — страница с тремя `<audio>`
(`http://host/sine440.mp3`, `/sine440.mp3`, `sine440.mp3`) на dev-release
бинаре:

```
requests seen by server: ['REQ /index.html', 'REQ /sine440.mp3', 'REQ /sine440.mp3', 'REQ /sine440.mp3']
PROBE abs:loadeddata     currentSrc=http://127.0.0.1:PORT/sine440.mp3
PROBE rootrel:loadeddata currentSrc=http://127.0.0.1:PORT/sine440.mp3
PROBE rel:loadeddata     currentSrc=http://127.0.0.1:PORT/sine440.mp3
```

Все три формы доходят до `loadeddata`, сервер видит ровно по одному GET на
каждую, `currentSrc` — абсолютный URL. Побочно измеренное в исходной заявке
разошлось: `currentSrc` уже не `undefined` (закрыто отдельно, [BUG-940]
(BUG-940-FIXED.md)), а `duration` всё ещё `Infinity` у полностью
декодированного файла — другой механизм (`rodio`'s MP3-декодер не считает
длительность), заведено отдельно как [BUG-1105](BUG-1105-OPEN.md), не
блокирует закрытие этого бага.
