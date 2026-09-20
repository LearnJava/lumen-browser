# BUG-939 — фоновая картинка, назначенная из скрипта (`el.style.backgroundImage = 'url(…)'`), не запрашивается ни разу; парсерный `style=` и правило таблицы стилей — запрашиваются

**Статус:** FIXED 2026-09-20 (P3)
**Тип:** дефект реализованного кода — сбор фоновых подресурсов есть и работает для двух путей из трёх.
**Заведён:** 2026-09-01 (WPT-RUN-6, срез 30 — живой замер, вариант `css-background-fetch`)
**Область:** shell/layout (сбор запросов за фонами идёт из прохода разбора документа, `collect_image_requests`; мутация `style` из JS этот сбор не перезапускает)
**Владелец:** P3.

## Симптом

Три способа задать `background-image` на одной странице, один сервер, один
прогон:

| путь | запрос на сервере |
|---|---|
| `<div style="background-image:url(a.png)">` (парсер) | есть |
| `#sheet { background-image: url(b.png) }` в `<style>` | есть |
| `el.style.backgroundImage = "url(c.png)"` из скрипта | **нет** |

При этом сам стиль применён: `getComputedStyle(made).backgroundImage`
отвечает `url("images/black-rectangle.png")`. То есть каскад мутацию видит,
а сеть о ней не узнаёт никогда — ни через 2,5 с, ни к концу страницы.

## Прямое измерение

`tests/wpt/verify_replaced_content_gaps.py --variant css-background-fetch`
(2026-09-01, dev-release, Linux, `main` = `287562e61`):

```
script-bg = url(images/black-rectangle.png)
computed-bg = url("images/black-rectangle.png")
[server saw: GET /images/anim-gr.png, GET /images/apng.png]
                  ↑ таблица стилей    ↑ парсерный style=
                                        images/black-rectangle.png — нет
```

Доказательство — сервер пробы, а не страница и не лог браузера: лог печатает
строку о хинте на запрос, которого не было ([BUG-826](BUG-826-FIXED.md)
касалась ровно этой ловушки), а страница о фоновых картинках не узнаёт
ничего в принципе.

Найдено побочно: в варианте `svg-image-href` контрольный `<div>` со
скриптовым фоном молча не появился в списке сервера, и это оказалось не
шумом.

## Кого это держит

Прямых id в остатке WPT-RUN-5 у этого дефекта нет — он найден как контроль
внутри чужого замера. Заведён потому, что это третий по счёту путь ресурса,
который жив у парсера и мёртв у скрипта ([BUG-885](BUG-885-FIXED.md) —
под-документы, [BUG-938](BUG-938-FIXED.md) — битмапы канвы), и любая проба,
которая ставит фон из JS, будет молча мерить его вместо своего предмета.

## Направление починки

Пересобирать запросы за фонами при мутации стиля из JS — там же, где
[BUG-443](BUG-443-FIXED.md) научил движок пересобирать каскад по
фингерпринту. Проверять: `--variant css-background-fetch` должен показать на
сервере все три URL.

## Фикс (P3, 2026-09-20)

Направление подтвердилось без изменений, только точка перезапуска — не
каскад, а единственная точка сбора фоновых картинок:
`fetch_and_decode_background_images` (`crates/shell/src/page_pipeline.rs`)
вызывается ровно один раз, в начальном проходе pipeline, и ни один
последующий релейаут её не перезапускал (`frames.rs` знал об этом ограничении
и документировал его как известный, общий со страницей, — этот фикс закрывает
обе стороны разом только для top-level документа; у фреймов то же ограничение
остаётся).

Новый `Lumen::spawn_dynamic_background_image_loads` (`crates/shell/src/page_load.rs`)
вызывается из того же пост-релейаутного хука, что и `spawn_dynamic_image_loads`
(BUG-730, `crates/shell/src/relayout.rs`) — единственной точки, через которую
проходит любой релейаут, каким бы producer-ом он ни был запущен. Собирает URL-ы
через `collect_background_image_requests` по свежепостроенному layout-дереву и
заворачивает каждый в `lumen_layout::ImageRequest` с сентинел-`node_id`
(`NodeId::from_index(0)` — тем же, что `collect_bg_image_inner` уже использует
для `content: url(...)`-сегментов без DOM-узла), чтобы переиспользовать
fetch/decode/dedup-конвейер `spawn_image_requests` без дублирования кода:
`node_id` в нём нигде не читается, а единственный читатель поля,
`apply_stream_intrinsic_sizes`, обходит только `<img>`-запросы
(`collect_image_requests`), так что сентинел на фоновых записях ни на что не
влияет. Дедуп URL идёт через тот же `stream_images_requested`, что и `<img>` —
общее пространство URL, так что фон, совпавший с уже запрошенной картинкой, не
дублирует сетевой запрос.

## Проверка

Живой прогон `tests/wpt/verify_replaced_content_gaps.py --variant
css-background-fetch --seconds 25` (dev-release, v8):

```
[server saw: GET /images/anim-gr.png, GET /images/apng.png, GET /images/black-rectangle.png]
```

Все три URL — было `anim-gr.png`/`apng.png` без `black-rectangle.png`.
`cargo clippy --workspace --all-targets -- -D warnings` чист;
`scripts/scoped-test.sh` — единственный красный таргет `-p lumen-driver --test
all` (BUG-1008, посторонний дрейф CPU-эталонов, сигнатура побайтово совпадает
с уже задокументированной).
