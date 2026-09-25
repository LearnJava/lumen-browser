//! Content Security Policy enforcement — срез 1 (GAP-CSPENF): `script-src`
//! против инлайновых `<script>`/module-скриптов, взятых из `<meta
//! http-equiv="Content-Security-Policy">`. Срез 4 добавил `img-src`/
//! `default-src` против `<img src>` (host/scheme/`'self'`-источники, не
//! только keyword). Срез 5 добавил заголовок `Content-Security-Policy`
//! ответа: он доезжает до документа (`Document::csp_header`) и участвует
//! вместе с `<meta>`-политиками в [`document_csp_policy`] (срез 40 сделал их
//! независимыми политиками, не одной строкой), поэтому все точки enforcement
//! видят его без изменений в них самих. Срез 6 добавил
//! `script-src`/`default-src` против внешнего `<script src>` — та же
//! host/scheme/`'self'` проверка, что срез 4 сделал для `img-src`, теперь
//! останавливает fetch внешнего скрипта до сети.
//!
//! Срез 7 добавил `style-src`/`default-src` против внешнего `<link
//! rel=stylesheet>` — тот же host/scheme/`'self'` фетч-гейт, что срезы 4 и 6
//! дали `img-src`/`script-src`, применённый к `load_linked_stylesheets`
//! (`crates/shell/src/stylesheets.rs`); заблокированный лист не фетчится и
//! становится тем же `error`-исходом, что уже даёт сетевая неудача (BUG-804).
//!
//! Срез 9 закрыл последний непроверенный производитель картинок:
//! `loading="lazy"` (`Lumen::fetch_and_register_lazy_images`, `page_load.rs`)
//! теперь гейтится тем же `img_src_blocked`, что срез 4 уже дал eager- и
//! streaming-путям.
//!
//! Срез 10 добавил `connect-src` против `fetch()`/`XMLHttpRequest` — не в
//! этом файле: у JS-инициированного запроса нет точки кода с `&Document` под
//! рукой (в отличие от парсер-/страница-производителей выше), поэтому гейт
//! живёт в `lumen-network::HttpClient::fetch_request_impl`
//! (`with_connect_src_policy`, `crates/network/src/lib.rs`), а
//! `document_csp_policy` из этого модуля используется лишь один раз — в
//! `page_pipeline.rs::parse_and_layout`, чтобы собрать политику для этого
//! `HttpClient` перед тем, как он станет `fetch_provider`. Детали —
//! `bugs/BUG-811-FIXED.md` срез 10.
//!
//! Срезы 11/12 (тоже вне этого файла, по той же причине, что срез 10) добавили
//! `connect-src` против WebSocket/EventSource (`crates/network/src/lib.rs`'s
//! `JsWebSocketProvider`/`JsSseProvider`) и `sendBeacon` (`check_connect_src`,
//! `crates/core/src/ext.rs`) — оба делят один `HttpClient` и один
//! `connect_src_policy` с `fetch()`.
//!
//! Срез 13 добавил `worker-src` (falling back to `default-src` — `worker-src`
//! не получил своего child-src/script-src промежуточного шага CSP3 §6.4, тот
//! же однократный фолбэк на `default-src`, что и у всех директив здесь) против
//! `new Worker(url)`/`new SharedWorker(url)`: тоже вне этого файла — у
//! `_lumen_worker_fetch_script`/`_lumen_sw_fetch_script` (`crates/js/src/
//! worker.rs`/`shared_worker.rs`) нет `&Document`, гейт живёт в
//! `lumen-network::HttpClient::check_worker_src` (`with_worker_src_policy`),
//! тот же `document_csp_policy` из `page_pipeline.rs::parse_and_layout`, что
//! срез 10 уже собирает для `connect_src_policy`. Детали — `bugs/
//! BUG-811-FIXED.md` срез 13.
//!
//! Срез 14 (`crates/js/src/csp.rs`, вне этого файла — JS-only) добавил
//! доставку отчётов `report-uri`: `_lumen_dispatch_csp_violation`
//! переизвлекает директиву из уже доехавшей `originalPolicy` и шлёт
//! `fetch(..., {method:'POST'})` на каждый URI. Срез 60 (тоже JS-only, плюс
//! новая нативная привязка `_lumen_get_report_to_endpoints_json` —
//! `crates/js/src/v8_runtime/install/dom_core.rs`) добавил `report-to
//! <group>`: группа резолвится против карты `Document::report_to_endpoints`
//! (срез 59), URL'ы которой шлются той же POST-доставкой.
//!
//! Срез 15 добавил `frame-src`/`default-src` против навигации `<iframe>`/
//! `<frame>` — тот же host/scheme/`'self'` фетч-гейт, что срезы 4/6/7 дали
//! `img-src`/`script-src`/`style-src`, применённый в `frames.rs::spawn_frame`
//! перед вызовом `fetch_iframe_source` (не в этом файле — у гейта нет
//! готового `&Document`/`ResourceBase` без явной проводки, тот же повод, что
//! у срезов 10-13). Проверяются оба пути (первичная вставка и навигация,
//! включая переприсваивание `.src`); `about:blank`/пустой `src` исключены
//! заранее — CSP3 §6.5 их не ограничивает, они не долетают до сети/диска.
//!
//! Срез 18 добавил `img-src`/`default-src` против `background-image:
//! url(...)` страницы (`subresources.rs::fetch_and_decode_background_images`)
//! — переиспользует уже существующий [`img_src_blocked`] (срез 4), гейт
//! только на top-level документе; фон под-документа `<iframe>`
//! (`frames.rs::fetch_frame_background_images`) не тронут этим срезом.
//!
//! Срез 19 добавил `font-src`/`default-src` против `@font-face url()` — эта
//! директива не была даже распарсена до этого среза (`CspDirective::FontSrc`
//! — новый вариант). Тот же host/scheme/`'self'` фетч-гейт, что дают
//! [`img_src_blocked`]/[`media_src_blocked`], под именем [`font_src_blocked`];
//! вызывается не отсюда — единственный фетчер (`page_load.rs::
//! apply_loaded_page`'s `pending_web_fonts`-цикл) грузит байты на детач-потоке
//! без `&Document`, поэтому решение «фетчить или нет» принимается на главном
//! потоке до `std::thread::spawn`, той же одноразовой схемой чтения политики,
//! что срез 9 уже даёт `loading="lazy"`. Шрифты внутри `<iframe>`
//! (`frames.rs::load_frame_fonts`) не тронуты.
//!
//! Срез 20 добавил `'sha256-…'`/`'sha384-…'`/`'sha512-…'` (CSP3 §8.1) к
//! `inline_script_blocked` — до этого среза `CspSource::Hash` разбирался
//! (`crates/network/src/csp.rs`), но не участвовал в проверке: инлайновый
//! скрипт под политикой, чей единственный разрешённый источник — хэш, читался
//! как всегда заблокированный. Тело скрипта хэшируется новым
//! `HashAlgorithm::digest_base64` (`sha2`, уже в дереве зависимостей
//! `lumen-network` — TLS-цепочка сертификатов) и сравнивается со значением
//! каждого `Hash`-источника директивы; совпадение любого допускает
//! исполнение, тем же принципом «одного достаточно», что уже есть у
//! nonce/`'unsafe-inline'`. Только классические/модульные инлайновые
//! `<script>` (`scripts.rs`) — не событийные атрибуты (`onclick=…`, которых
//! `'unsafe-hashes'` касается отдельно) и не `style`-src.
//!
//! Срез 21 добавил `style-src`/`default-src` против инлайновых `<style>` —
//! [`inline_style_blocked`], тот же `'unsafe-inline'`/`'nonce-…'`/
//! `'sha256-…'`-набор, что `inline_script_blocked` уже даёт скриптам,
//! только применённый к `CspDirective::StyleSrc`. Гейт стоит в
//! `doc_extract::walk_style_blocks`, до склейки каскада: заблокированный
//! `<style>`-узел не попадает в текст, который парсит [`lumen_css_parser`],
//! вовсе — тот же принцип «не применённый CSS», что срез 7 уже даёт
//! заблокированному внешнему `<link>`. Атрибут `style=` этим не покрыт.
//!
//! Срез 22 закрыл ровно тот пробел, что срез 21 назвал не покрытым:
//! инлайновый `<style>` внутри `<iframe>` (`frames.rs::
//! fetch_frame_subresources` вызывала `extract_style_blocks(doc, None)` —
//! политика ребёнка там уже считалась для `img-src`/`style-src` внешних
//! листов (срез 8), просто не передавалась в этот вызов). Гейт — тот же
//! [`inline_style_blocked`], что срез 21 даёт top-level документу, против
//! CHILD'а собственной политики; `securitypolicyviolation` диспатчится из
//! `spawn_frame` тем же one-shot-push путём, что уже несёт
//! `blocked_by_img_src`/`blocked_by_style_src` для этого ребёнка.
//!
//! Срез 23 закрыл последний класс инлайна, названный не покрытым срезами
//! 21/22: атрибут `style=""` на произвольном элементе. Архитектурно другое
//! решение, чем срезы 21/22: точка потребления (`lumen_layout::style::
//! cascade`) — единственный choke point, единый для парсер-, скрипт- и
//! CSSOM-вставленного значения атрибута, — но `layout` не зависит от
//! `lumen-network`/`CspPolicy` (layering `dom → layout`, а не `network →
//! layout`), поэтому решение считается один раз в shell'е и travels down as
//! bare node ids (`Document::style_attr_csp_blocked`, новое поле —
//! `layout`/`dom` не знают о CSP вовсе). [`style_attribute_blocked`] — тот же
//! host/scheme-независимый гейт по духу, что [`inline_style_blocked`], но с
//! другим набором источников (CSP3 §6.4.2/§8.1): нет nonce (атрибут не может
//! нести `nonce=` для самого себя), а хэш-источник допускает совпадение
//! только вместе с `'unsafe-hashes'` — голого хэша достаточно для `<style>`
//! элемента, но никогда не для атрибута. Фолбэк на один уровень глубже
//! остальных директив этого файла: `style-src-attr` → `style-src` →
//! `default-src` (CSP3 §6.4 granular chain). Вычисляется один раз в
//! `build_page_cascade` (та же точка, что срез 21 уже даёт `<style>`), гейт
//! стоит в `cascade.rs` перед `parse_inline_style` — атрибут остаётся в DOM
//! нетронутым (`getAttribute('style')` не меняется), исключается только его
//! эффект на каскад. Покрывает только элементы дерева на момент вычисления
//! каскада (начальный парсинг + пересборка при `scripts_changed_css`) — узел,
//! получивший `style=""` другим путём после этого момента (простой
//! `setAttribute`/`style.cssText`, не трогающий `<style>`/`<link>`), гейт не
//! видит; `<iframe>` не тронут этим срезом.
//!
//! Срез 25 закрыл ровно тот пробел, что срез 18/19 назвали не покрытым:
//! `@font-face url()` и `background-image: url()` внутри `<iframe>` —
//! `load_frame_fonts`/`fetch_frame_background_images` (`frames.rs`) фетчили
//! оба ребёнком без единой проверки политики. Тот же `font_src_blocked`/
//! [`img_src_blocked`], что уже даёт top-level документу, применённый к
//! CHILD'у собственной политики (`document_csp_policy` того же `child_doc_arc`,
//! читается один раз перед обоими вызовами в `spawn_frame`); `local()`-шрифты
//! не затронуты — `font-src` гейтит только сетевой фетч.
//!
//! Срез 26 добавил разбор директивы `child-src` (`CspDirective::ChildSrc`
//! — до этого среза не распознавалась вовсе, падала в `_ => continue`) и
//! CSP3 §6.4 granular-фолбэк для `frame-src`/`worker-src`: обе раньше падали
//! напрямую на `default-src` через общий [`CspPolicy::effective_sources`],
//! минуя промежуточный `child-src`, который спека требует проверить первым.
//! Новый [`CspPolicy::fetch_directive_allows_via_child_src`] — тот же метод,
//! что срез 23 уже даёт `style-src-attr` (одним уровнем глубже общего
//! случая), применённый к `frame_src_blocked` (этот файл) и
//! `worker_src_gate` (`crates/network/src/lib.rs`, вне этого файла — та же
//! причина, что у срезов 10/13: нет `&Document` в точке принятия решения).
//!
//! Срез 28 закрыл ровно тот пробел, что этот список называл не покрытым:
//! `importScripts()` внутри уже запущенного `Worker`/`SharedWorker` теперь
//! тоже гейтится `worker-src`/`default-src` (`crates/js/src/worker.rs`'s
//! `import_scripts_csp_blocked`, разделяемая с `shared_worker.rs`) — до этого
//! среза гейт стоял только перед начальным скриптом конструктора
//! (`_lumen_worker_fetch_script`, срез 13), а последующие вызовы
//! `importScripts()` уходили прямиком в `fetch_worker_script` в обход любой
//! проверки. `ServiceWorker`'s `importScripts` (`sw_worker.rs`) не тронут —
//! срез 13 сам никогда не гейтил конструирование сервис-воркера, поэтому это
//! отдельный, более широкий пробел, а не продолжение этого. Событие
//! `securitypolicyviolation` не диспатчится: этот натив исполняется в
//! рантайме самого воркера, у которого нет ни `document`, ни CSP-шима вообще
//! (в отличие от конструктора, чей гейт живёт на родительском рантайме) —
//! заблокированный вызов виден скрипту как обычная ошибка сети (`Error:
//! importScripts: cannot load script: …`), тот же исход, что уже даёт любой
//! другой сетевой отказ этого API.
//!
//! Срез 38 (вне этого файла — `crates/shell/src/stylesheets.rs`) закрыл
//! `@import`, названный не покрытым списком ниже: `style-src`/`default-src`
//! теперь гейтит и цель `@import`, не только сам `<style>`/`<link>`, на всех
//! трёх сайтах потребления (`inline_css_imports` — общая рекурсивная
//! функция, используется внешним `<link>` для своих же `@import`,
//! top-level инлайновым `<style>` и `<style>` внутри `<iframe>`).
//!
//! Срез 39 (вне этого файла — `crates/shell/src/frames.rs::relayout_frame_content`)
//! закрыл атрибут `style=` внутри `<iframe>` после точечной DOM-мутации,
//! названный не покрытым срезом 23 и не суженный срезом 24 (тот дал только
//! спавн-времени снимок ребёнка): та же `dom_touched`-подобная пере-проверка,
//! что срез 37 дал top-level странице, только триггер — per-фреймовый
//! `frame_dirty` (`about_to_wait.rs`), а не `parse_and_layout`'s `dom_touched`.
//!
//! Ранее этот список ошибочно называл не покрытым `ServiceWorker`
//! конструирование — срезы 30/31 (`serviceWorker.register()` и его
//! `importScripts()`) закрыли это до появления списка ниже; список не был
//! обновлён тогда, чинится этим срезом.
//!
//! Срез 40 закрыл последний пункт списка «не покрыто», который держался с
//! среза 1: заголовок и каждая `<meta>` теперь проверяются как независимые
//! политики (CSP3 §3.4) — [`document_csp_policy`] парсит каждую строку
//! отдельно вместо склейки в одну через `;`, и каждая `_blocked` функция
//! этого файла принимает `&[CspPolicy]`, блокируя при нарушении ЛЮБОЙ из
//! них. Не мигрирован этим срезом: `lumen-network::HttpClient`'s
//! `connect-src`/`worker-src`/`object-src`/`media-src` (`page_pipeline.rs`'s
//! единственный вызов, который их настраивает, по-прежнему передавал им один
//! смёрженный `CspPolicy`) — у `HttpClient` нет `&Document`/парсинга по
//! месту, threading `Vec<CspPolicy>` через него — отдельная, более широкая
//! работа.
//!
//! Срез 41 закрыл ровно тот пробел, что срез 40 сам назвал не покрытым:
//! несколько ОДНОИМЁННЫХ заголовков `Content-Security-Policy` ответа теперь
//! тоже независимые политики, не только заголовок и `<meta>`. До этого среза
//! `page_source::content_security_policy_header` склеивала все вхождения
//! заголовка в одну строку через `"; "` ДО парсинга — `document_csp_policy`
//! видело результат как один готовый "part", и `CspPolicy::directives`
//! (`HashMap`) при повторе директивы (например, `script-src` в обоих
//! заголовках) хранило только последнее встреченное значение: более поздний,
//! более мягкий заголовок тихо ослаблял более строгий более ранний.
//! `content_security_policy_header` теперь возвращает `Vec<String>` — по
//! одному элементу на occurrence, — `Document::csp_header` хранит их тем же
//! списком, и `document_csp_policy` просто добавляет весь список в `parts`
//! вместо одной строки.
//!
//! Срез 42 (вне этого файла — `crates/network/src/lib.rs`) закрыл ровно
//! пробел, что срез 40 сам назвал не покрытым:
//! `HttpClient::{connect_src,worker_src,object_src,media_src}_policy`
//! сменили тип с `Option<(CspPolicy, Option<Origin>, String)>` на
//! `Option<(Vec<CspPolicy>, Option<Origin>, String)>`, и все четыре
//! `*_gate`-функции блокируют, если нарушает ХОТЯ БЫ ОДНА политика из списка —
//! тот же `policies.iter().any(...)` рисунок, что `_blocked`-функции этого
//! файла уже дают с среза 40. `page_pipeline.rs`'s единственный вызов теперь
//! зовёт [`document_csp_policy`] напрямую вместо удалённого
//! `document_csp_policy_combined`.
//!
//! Срез 43 завёл первую директиву этого файла, которая не блокирует, а
//! МЕНЯЕТ запрос: [`upgrade_insecure_url`] (`upgrade-insecure-requests`,
//! BUG-692). Она переписывает `http` → `https` до гейта `img-src` (Fetch
//! §4.1: upgrade — шаг 5, CSP-проверка — шаг 6) и подключена во всех трёх
//! producer'ах картинок ГЛАВНОГО документа: eager
//! (`subresources::fetch_and_decode_images`), streaming/dynamic
//! (`page_load::spawn_image_requests`) и отложенный `loading="lazy"`
//! (`page_load::fetch_and_register_lazy_images`). Ключ кэша/реестра картинок
//! везде остаётся сырым URL — апгрейд меняет только адрес запроса.
//!
//! Что НЕ покрыто (следующие срезы): `upgrade-insecure-requests` для всего
//! остального (картинки `<iframe>`, `background-image`, `<script src>`,
//! `<link rel=stylesheet>`/`@import`, `@font-face`, media/`<track>`,
//! `fetch()`/XHR/WebSocket, навигации и `Upgrade-Insecure-Requests: 1` на
//! навигационном запросе), остальные директивы (`manifest-src`/…
//! — распознаётся [`CspDirective::ManifestSrc`], но манифест ничем не
//! фетчится этим движком, гейтить нечего), `report-to` (Reporting API,
//! нужны группы эндпоинтов из `Report-To`, этот движок его не разбирает). См.
//! `bugs/BUG-811-FIXED.md`.
//!
//! Срез 56 закрыл дрейф, который [`document_csp_policy`]'s doc comment сам
//! называл открытым: каждая точка диспетчеризации `securitypolicyviolation`
//! несла ЕГО объединённый (`"; "`-joined) текст всех политик документа как
//! `originalPolicy`, даже когда нарушила ровно одна — CSP3 §7.8 хочет текст
//! ИМЕННО нарушенной политики. [`CspPolicy`] (`crates/network/src/csp.rs`)
//! получил поле `raw` (сырой текст, из которого распарсена именно эта
//! политика); новые `violating_*` функции этого файла ищут первую политику
//! из `&[CspPolicy]`, которая ФАКТИЧЕСКИ нарушена данной проверкой, и
//! возвращают `Some(&её.raw)` вместо `bool` — каждый call site, что диспатчит
//! событие, зовёт `violating_*` вместо `document_csp_policy`'s объединённого
//! текста.
//!
//! Срез 57 закрыл остаток, который срез 56 назвал не покрытым: инлайновые
//! `<style>` (`doc_extract::extract_style_blocks`) и `style=""` атрибуты
//! (`doc_extract::collect_style_attr_csp_blocked`) больше не сворачивают
//! список нарушений в счётчик до диспетчеризации — каждая функция теперь
//! зовёт [`violating_inline_policy`]/[`violating_style_attr_policy`] В
//! МОМЕНТ, когда тело узла ещё в скоупе, и возвращает `Vec<String>` текстов
//! нарушенных политик (одна запись на заблокированный узел, в порядке
//! документа) вместо счётчика. `blocked_inline_style_count`/
//! `blocked_style_attr_nodes`'s `usize`/длина стали
//! `blocked_inline_style_policies`/`blocked_style_attr_policies` в
//! `PageCascade`/`FrameSubresourceOutcomes`.
//!
//! Срез 58 закрыл ровно тот пробел, что срез 56 сам назвал не покрытым: CSP3
//! §7.8 хочет ОТДЕЛЬНЫЙ `securitypolicyviolation` на КАЖДУЮ нарушенную
//! политику, когда один и тот же ресурс нарушает несколько независимых
//! политик документа одновременно (CSP3 §3.4, независимые политики — срез 40).
//! Все пять `violating_*` функций этого файла ([`violating_fetch_policy`],
//! [`violating_fetch_policy_via_child_src`], [`violating_inline_policy`],
//! [`violating_base_uri_policy`], [`violating_style_attr_policy`]) сменили
//! `Option<&str>` (текст ПЕРВОЙ нарушившей политики, `.find(...)`) на
//! `Vec<&str>` (текст КАЖДОЙ нарушившей, `.filter(...)`) — пустой `Vec` то же
//! самое, что раньше `None`. Каждая точка диспетчеризации переключена с
//! `if let Some(text) = ...` на `for text in ...`; там, где у той же точки
//! есть отдельное решение «блокировать ли ресурс» (`page_pipeline.rs`'s
//! `base_uri_href_blocked`/`load_video_tracks`, `frames.rs`'s
//! `frame_src_check`, `page_load.rs`'s lazy-image/web-font пути,
//! `subresources.rs`'s `fetch_and_decode_background_images`), блокировка
//! осталась однократной — ресурс либо загружен, либо нет, независимо от
//! того, сколько политик его запрещают, — только событий теперь по одному на
//! каждую нарушенную политику. Остаток дорожки не изменился: `report-to`,
//! `manifest-src` (см. выше).

use lumen_network::csp::{CspDirective, CspPolicy, CspSource};
use lumen_network::Origin;

use crate::*;

/// Собрать текст каждой `<meta http-equiv="Content-Security-Policy">`
/// документа, в порядке документа. `Content-Security-Policy-Report-Only`
/// не поддерживается через `<meta>` — это и в спеке недопустимо (HTML LS
/// не даёт `http-equiv` репортинг-варианту).
fn collect_meta_csp(doc: &Document, id: NodeId, out: &mut Vec<String>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "meta"
    {
        let http_equiv = attrs
            .iter()
            .find(|a| a.name.local == "http-equiv")
            .map(|a| a.value.as_str())
            .unwrap_or("");
        if http_equiv.eq_ignore_ascii_case("content-security-policy")
            && let Some(content) = attrs.iter().find(|a| a.name.local == "content")
        {
            out.push(content.value.clone());
        }
        return;
    }
    for &child in &node.children {
        collect_meta_csp(doc, child, out);
    }
}

/// Действующие политики документа: заголовок `Content-Security-Policy` ответа
/// (срез 5, `Document::csp_header`) и каждая `<meta
/// http-equiv="Content-Security-Policy">` (срез 1), в порядке «заголовок,
/// затем документ».
///
/// Заголовок и каждая `<meta>` по спецификации (CSP3 §3.4) — независимые
/// политики: каждая парсится и проверяется отдельно (срез 40) — все
/// `_blocked` функции этого файла принимают `&[CspPolicy]` и блокируют, если
/// нарушена ЛЮБАЯ политика из списка — до среза 40 они упрощённо сливались в
/// одну строку через `;` перед парсингом, что для нескольких политик со
/// связанными ослаблениями (например, `'unsafe-inline'` в одной и `'self'` в
/// другой) могло дать более мягкий эффективный результат, чем спецификация.
///
/// `Content-Security-Policy-Report-Only` не учитывается ни с той, ни с другой
/// стороны: у `<meta>` репортинг-вариант недопустим по HTML LS, а заголовок
/// отфильтрован в `page_source::content_security_policy_header` — здесь
/// enforcement, а report-only по определению ничего не блокирует.
///
/// The returned `String` is the combined raw policy text (still joined with
/// `; ` for display) — used only as a fallback `originalPolicy` where a call
/// site cannot cheaply re-derive which policy actually blocked a given
/// resource; every production dispatch site prefers the specific violated
/// policy's own text via the `violating_*` functions below (срез 56), which
/// since срез 58 report every policy that independently violates the same
/// resource, not only the first (CSP3 §7.8).
pub(crate) fn document_csp_policy(doc: &Document, root: NodeId) -> Option<(Vec<CspPolicy>, String)> {
    let mut parts: Vec<String> = doc.csp_header().to_vec();
    collect_meta_csp(doc, root, &mut parts);
    if parts.is_empty() {
        return None;
    }
    let combined = parts.join("; ");
    let policies = parts.iter().map(|p| lumen_network::csp::parse_csp_header(p)).collect();
    Some((policies, combined))
}

/// Срез 56: test-only now — production callers switched to
/// [`violating_inline_policy`] so a fired `securitypolicyviolation` carries
/// the specific violated policy's text, not just a bool. Kept for the unit
/// tests below, which exercise [`inline_directive_blocked`] through this
/// name.
#[cfg(test)]
fn inline_script_blocked(policies: &[CspPolicy], nonce: Option<&str>, body: &str) -> bool {
    inline_directive_blocked(policies, &CspDirective::ScriptSrc, nonce, body)
}

/// Срез 57: test-only now — the production caller (`doc_extract::
/// extract_style_blocks`) switched to [`violating_inline_policy`] so a fired
/// `securitypolicyviolation` carries the specific violated policy's text, not
/// just a bool. Kept for the unit tests below, which exercise
/// [`inline_directive_blocked`] through this name — `true` if `style-src`
/// (or `default-src`) forbids the given inline `<style>` body.
#[cfg(test)]
fn inline_style_blocked(policies: &[CspPolicy], nonce: Option<&str>, body: &str) -> bool {
    inline_directive_blocked(policies, &CspDirective::StyleSrc, nonce, body)
}

/// Срез 57: test-only now — the production caller (`doc_extract::
/// collect_style_attr_csp_blocked`) switched to
/// [`violating_style_attr_policy`] for the same reason as
/// [`inline_style_blocked`] above. `true` if `style-src-attr`/`style-src`/
/// `default-src` forbids the value of a `style=""` attribute whose text is
/// `body` — срез 23, last inline class срезы 21/22 named as not covered.
/// Unlike [`inline_style_blocked`] (which gates `<style>` element text),
/// CSP3 §6.4.2 "inline check" treats an attribute differently on two points:
/// there is no nonce for a `style=` attribute (an element cannot carry a
/// nonce for its own attribute, only for a `<style>`/`<script>` element's own
/// `nonce=` attribute), and a hash source only matches an attribute if the
/// policy also carries `'unsafe-hashes'` (CSP3 §8.1) — a bare hash source is
/// enough for `<style>` element text but never for an attribute. Fallback
/// chain is the CSP3 §6.4 granular one (`style-src-attr` → `style-src` →
/// `default-src`), one step deeper than [`inline_directive_blocked`]'s single
/// `directive` → `default-src` step used by every other directive in this
/// file.
#[cfg(test)]
fn style_attribute_blocked(policies: &[CspPolicy], body: &str) -> bool {
    policies.iter().any(|policy| single_style_attribute_blocked(policy, body))
}

fn single_style_attribute_blocked(policy: &CspPolicy, body: &str) -> bool {
    let Some(sources) = policy
        .directives
        .get(&CspDirective::StyleSrcAttr)
        .or_else(|| policy.directives.get(&CspDirective::StyleSrc))
        .or_else(|| policy.directives.get(&CspDirective::DefaultSrc))
    else {
        return false;
    };
    let unsafe_hashes = sources.contains(&CspSource::UnsafeHashes);
    let allowed = sources.iter().any(|s| match s {
        CspSource::UnsafeInline => true,
        CspSource::Hash { algorithm, value } => {
            unsafe_hashes && algorithm.digest_base64(body.as_bytes()) == *value
        }
        _ => false,
    });
    !allowed
}

/// Общая проверка `inline_script_blocked`/[`inline_style_blocked`]: любой
/// совпавший источник (`'unsafe-inline'` ИЛИ nonce ИЛИ хэш) допускает
/// инлайн; отсутствие директивы, применимой к `directive`, — не нарушение.
/// Срез 57: test-only now, same reason as the two `#[cfg(test)]` functions
/// above that are its only remaining callers.
#[cfg(test)]
fn inline_directive_blocked(
    policies: &[CspPolicy],
    directive: &CspDirective,
    nonce: Option<&str>,
    body: &str,
) -> bool {
    policies
        .iter()
        .any(|policy| single_inline_directive_blocked(policy, directive, nonce, body))
}

fn single_inline_directive_blocked(
    policy: &CspPolicy,
    directive: &CspDirective,
    nonce: Option<&str>,
    body: &str,
) -> bool {
    let Some(sources) = policy.effective_sources(directive) else {
        return false;
    };
    let allowed = sources.iter().any(|s| match s {
        CspSource::UnsafeInline => true,
        CspSource::Nonce(n) => nonce.is_some_and(|actual| actual == n),
        CspSource::Hash { algorithm, value } => algorithm.digest_base64(body.as_bytes()) == *value,
        _ => false,
    });
    !allowed
}

/// Вызвать уже определённый JS-хук `_lumen_dispatch_csp_violation`
/// (`crates/js/src/csp.rs`) — единственная точка диспетчеризации
/// `securitypolicyviolation`, срез 1 зовёт её впервые для инлайна
/// (`blocked_uri = "inline"`); срез 6 обобщил на внешний `<script src>`
/// (`blocked_uri` = резолвленный адрес файла).
pub(crate) fn fire_script_src_violation(
    rt: &lumen_js::v8_runtime::V8JsRuntime,
    blocked_uri: &str,
    original_policy: &str,
) {
    use lumen_core::ext::JsRuntime as _;
    let _ = rt.eval(&format!(
        "_lumen_dispatch_csp_violation({}, {}, {}, 'enforce');",
        js_string_literal("script-src"),
        js_string_literal(blocked_uri),
        js_string_literal(original_policy),
    ));
}

/// Срез 56: test-only now — the one production caller
/// (`scripts.rs::resolve_script_sources`) switched to
/// [`violating_fetch_policy`] so a blocked external `<script src>` reports the
/// specific violated policy's text, not just a bool. Kept for the unit tests
/// below.
#[cfg(test)]
fn script_src_blocked(policies: &[CspPolicy], url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.fetch_directive_allows(&CspDirective::ScriptSrc, &parsed, self_origin))
}

/// Переписать `url` под `upgrade-insecure-requests` (срез 43): `Some(новый
/// URL)`, если хотя бы одна политика документа объявила директиву И схема
/// `url` — `http`; `None` — переписывать нечего (директивы нет, URL не
/// парсится, схема не `http`).
///
/// Это единственная директива CSP в этом файле, которая НЕ гейт: она ничего
/// не блокирует и не порождает `securitypolicyviolation` — она меняет сам
/// запрос ([UIR] §4.1 шаг 5: «If request's URL's scheme is "http", set
/// request's URL's scheme to "https"»). Порт при этом не трогается руками:
/// явный `:80` WHATWG-парсер уже свернул в дефолтный (`Url::port()` — `None`),
/// поэтому смена схемы сама даёт 443, а явный нестандартный порт (`:8080`)
/// спецификация сохраняет.
///
/// Апгрейд по спецификации происходит ДО проверки fetch-директив (Fetch §4.1
/// «main fetch»: upgrade — шаг 5, «should request be blocked by Content
/// Security Policy» — шаг 6), поэтому вызывающая сторона обязана гейтить уже
/// переписанный URL, а не исходный.
///
/// Исключений для loopback/IP-адресов здесь нет умышленно: «Should insecure
/// requests be upgraded for client?» смотрит только на наличие директивы в
/// политике клиента, а шаг 5 — только на схему; `http://localhost` живые
/// движки не апгрейдят по собственному решению, а не по тексту спецификации.
///
/// [UIR]: https://w3c.github.io/webappsec-upgrade-insecure-requests/
pub(crate) fn upgrade_insecure_url(policies: &[CspPolicy], url: &str) -> Option<String> {
    if !policies.iter().any(|p| p.upgrade_insecure_requests) {
        return None;
    }
    let parsed = lumen_core::url::Url::parse(url).ok()?;
    if parsed.scheme() != "http" {
        return None;
    }
    let serialized = parsed.as_str();
    let rest = serialized.strip_prefix("http:")?;
    Some(format!("https:{rest}"))
}

/// [`upgrade_insecure_url`] over an already-fetched `document_csp_policy` gate
/// tuple, falling back to `resolved` unchanged when there is no policy or
/// nothing to rewrite — GAP-CSPENF срез 52 shares this across every
/// navigation gate that already reads the same tuple for its own directive
/// (`navigate-to` in `click.rs`/`about_to_wait.rs`, `form-action` in
/// `form_submit.rs`, `frame-src` in `frames.rs`), so the upgrade (UIR §4.1
/// step 5) runs ahead of that gate (step 6) without re-deriving the order at
/// each call site.
pub(crate) fn upgrade_navigation_url(
    csp_gate: Option<&(Vec<CspPolicy>, String)>,
    resolved: &str,
) -> String {
    match csp_gate {
        Some((policy, _)) => upgrade_insecure_url(policy, resolved).unwrap_or_else(|| resolved.to_owned()),
        None => resolved.to_owned(),
    }
}

/// `true` if `csp_gate`'s policies declare `upgrade-insecure-requests` — UIR
/// §4.1 steps 1-2 ("upgrade insecure navigations set"): a navigation request
/// whose CLIENT (the initiating document, same tuple every `navigate-to`
/// gate in this module already reads) opted in carries
/// `Upgrade-Insecure-Requests: 1` on the outgoing request, independent of
/// whether [`upgrade_navigation_url`] actually rewrote the scheme — the
/// header is a hint to the server, not a record of a rewrite that happened
/// (GAP-CSPENF срез 54, `HttpClient::fetch_page`'s new `send_uir_header`).
pub(crate) fn navigation_wants_uir_header(csp_gate: Option<&(Vec<CspPolicy>, String)>) -> bool {
    csp_gate.is_some_and(|(policy, _)| policy.iter().any(|p| p.upgrade_insecure_requests))
}

/// `true` if `img-src` (or `default-src`) forbids fetching `url` — срез 4.
/// Absence of a policy is not checked here (the caller only calls this when
/// a policy exists); a `url` that fails to parse is treated as allowed — the
/// fetch proceeds and hits the normal network-failure path instead of a CSP
/// one, same "don't invent a violation" stance as the rest of this module.
pub(crate) fn img_src_blocked(policies: &[CspPolicy], url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.fetch_directive_allows(&CspDirective::ImgSrc, &parsed, self_origin))
}

/// `true` if `object-src` (or `default-src`) forbids fetching `url` as the
/// content of an `<object data>`/`<embed src>` (OBJECT-1) — same fetch-gate
/// shape as [`img_src_blocked`]. The violation itself is reported by the JS
/// shim, which gates the element's own `load`/`error` on the same directive
/// (`_lumen_check_object_src`, GAP-CSPENF срез 16); the image pipeline only
/// has to keep the bytes off the wire.
pub(crate) fn object_src_blocked(policies: &[CspPolicy], url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.fetch_directive_allows(&CspDirective::ObjectSrc, &parsed, self_origin))
}

/// `true` if `style-src` (or `default-src`) forbids fetching the external
/// `<link rel=stylesheet>` at `url` — срез 7, same fetch-gate shape as
/// [`img_src_blocked`]/`script_src_blocked`: absence of a policy is not
/// checked here (the caller only calls this when a policy exists), and a
/// `url` that fails to parse is treated as allowed.
pub(crate) fn style_src_blocked(policies: &[CspPolicy], url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.fetch_directive_allows(&CspDirective::StyleSrc, &parsed, self_origin))
}

/// Срез 56: test-only now — the one production caller
/// (`frames.rs`'s `frame_src_check`) switched to
/// [`violating_fetch_policy_via_child_src`] so a blocked `<iframe>` navigation
/// reports the specific violated policy's text, not just a bool. Kept for the
/// unit tests below.
#[cfg(test)]
fn frame_src_blocked(policies: &[CspPolicy], url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    policies.iter().any(|policy| {
        !policy.fetch_directive_allows_via_child_src(&CspDirective::FrameSrc, &parsed, self_origin)
    })
}

/// Срез 56: test-only now — the one production caller
/// (`page_pipeline.rs`'s `load_video_tracks` closure) switched to
/// [`violating_fetch_policy`] so a blocked `<track src>` reports the specific
/// violated policy's text, not just a bool. Kept for the unit tests below.
#[cfg(test)]
fn media_src_blocked(policies: &[CspPolicy], url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.fetch_directive_allows(&CspDirective::MediaSrc, &parsed, self_origin))
}

/// `true` if `font-src` (or `default-src`) forbids fetching `url` as an
/// `@font-face url()` body — срез 19, same fetch-gate shape as
/// [`img_src_blocked`]/[`media_src_blocked`]: absence of a policy is not
/// checked here (the caller only calls this when a policy exists), and a
/// `url` that fails to parse is treated as allowed.
pub(crate) fn font_src_blocked(policies: &[CspPolicy], url: &str, self_origin: Option<&Origin>) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.fetch_directive_allows(&CspDirective::FontSrc, &parsed, self_origin))
}

/// `true` if the CHILD document's own `frame-ancestors` directive refuses to
/// be embedded by a frame whose origin is `ancestor_origin` — срез 27, the
/// first navigation directive this module enforces (every directive above is
/// a fetch directive). Unlike them, `frame-ancestors` is read from the
/// PROTECTED document's own policy, not the embedder's: the caller passes
/// the child's `csp_gate` and its own origin as `self_origin`.
pub(crate) fn frame_ancestors_blocked(
    policies: &[CspPolicy],
    ancestor_origin: &Origin,
    self_origin: Option<&Origin>,
) -> bool {
    policies
        .iter()
        .any(|policy| !policy.frame_ancestor_allowed(ancestor_origin, self_origin))
}

/// `true` if `form-action` forbids submitting a `<form>` owned by this
/// document to `action_url` — срез 29, the second navigation directive this
/// module enforces (see [`frame_ancestors_blocked`] for the first): no
/// `default-src` fallback, absence of a policy is not checked here (the
/// caller only calls this when a policy exists), and a `url` that fails to
/// parse is treated as allowed, same as every fetch-gate above.
pub(crate) fn form_action_blocked(
    policies: &[CspPolicy],
    action_url: &str,
    self_origin: Option<&Origin>,
) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(action_url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.form_action_allowed(&parsed, self_origin))
}

/// Срез 56: test-only now — the one production caller
/// (`page_pipeline.rs::base_uri_href_blocked`) switched to
/// [`violating_base_uri_policy`] so a blocked `<base href>` reports the
/// specific violated policy's text, not just a bool. Kept for the unit tests
/// below.
#[cfg(test)]
fn base_uri_blocked(
    policies: &[CspPolicy],
    base_url: &str,
    self_origin: Option<&Origin>,
) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(base_url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.base_uri_allowed(&parsed, self_origin))
}

/// `true` if `navigate-to` forbids this document from navigating to
/// `target_url` — срез 33, the fourth navigation directive this module
/// enforces (see [`frame_ancestors_blocked`]/[`form_action_blocked`]/
/// `base_uri_blocked` for the first three): no `default-src` fallback,
/// absence of a policy is not checked here (the caller only calls this when a
/// policy exists), and a `target_url` that fails to parse is treated as
/// allowed, same as every fetch-gate above.
pub(crate) fn navigate_to_blocked(
    policies: &[CspPolicy],
    target_url: &str,
    self_origin: Option<&Origin>,
) -> bool {
    let Ok(parsed) = lumen_core::url::Url::parse(target_url) else {
        return false;
    };
    policies
        .iter()
        .any(|policy| !policy.navigate_to_allowed(&parsed, self_origin))
}

/// Срез 56/58: text of EVERY policy in `policies` whose `directive` (or
/// `default-src` fallback) forbids fetching `url`, in policy order — CSP3
/// §7.8/§3.4 want one `SecurityPolicyViolationEvent` per independently
/// violated policy, not one for the whole document, unlike
/// [`document_csp_policy`]'s combined text of every policy the document
/// declared. An empty `Vec` both when nothing is violated and when `url`
/// fails to parse (same "don't invent a violation" stance as every
/// `*_blocked` fetch-gate above).
pub(crate) fn violating_fetch_policy<'a>(
    policies: &'a [CspPolicy],
    directive: &CspDirective,
    url: &str,
    self_origin: Option<&Origin>,
) -> Vec<&'a str> {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return Vec::new();
    };
    policies
        .iter()
        .filter(|policy| !policy.fetch_directive_allows(directive, &parsed, self_origin))
        .map(|policy| policy.raw.as_str())
        .collect()
}

/// Same as [`violating_fetch_policy`], through the extra `child-src` fallback
/// step `frame-src`/`worker-src` get (CSP3 §6.4) — mirrors
/// `frame_src_blocked`'s own fallback chain.
pub(crate) fn violating_fetch_policy_via_child_src<'a>(
    policies: &'a [CspPolicy],
    directive: &CspDirective,
    url: &str,
    self_origin: Option<&Origin>,
) -> Vec<&'a str> {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return Vec::new();
    };
    policies
        .iter()
        .filter(|policy| !policy.fetch_directive_allows_via_child_src(directive, &parsed, self_origin))
        .map(|policy| policy.raw.as_str())
        .collect()
}

/// `<script>` element counterpart of [`violating_fetch_policy`] (BUG-1124):
/// text of every policy whose `script-src`/`default-src` pre-request check
/// (CSP3 §6.7.1.1 — nonce, integrity hashes, `'strict-dynamic'`, then the URL;
/// [`CspPolicy::script_element_fetch_allows`]) forbids this element's fetch of
/// `url`. The URL-only [`violating_fetch_policy`] read a `'nonce-…'`-only list
/// as «no source matches» and never fetched a nonced `<script src>` at all.
pub(crate) fn violating_script_element_policy<'a>(
    policies: &'a [CspPolicy],
    url: &str,
    self_origin: Option<&Origin>,
    request: &lumen_network::csp::ScriptRequestMetadata<'_>,
) -> Vec<&'a str> {
    let Ok(parsed) = lumen_core::url::Url::parse(url) else {
        return Vec::new();
    };
    policies
        .iter()
        .filter(|policy| !policy.script_element_fetch_allows(&parsed, self_origin, request))
        .map(|policy| policy.raw.as_str())
        .collect()
}

/// Inline counterpart of [`violating_fetch_policy`]: text of every policy
/// whose inline check (`'unsafe-inline'`/nonce/hash) forbids `body` for
/// `directive` — same predicate `inline_script_blocked`/
/// [`inline_style_blocked`] already share via [`single_inline_directive_blocked`].
pub(crate) fn violating_inline_policy<'a>(
    policies: &'a [CspPolicy],
    directive: &CspDirective,
    nonce: Option<&str>,
    body: &str,
) -> Vec<&'a str> {
    policies
        .iter()
        .filter(|policy| single_inline_directive_blocked(policy, directive, nonce, body))
        .map(|policy| policy.raw.as_str())
        .collect()
}

/// `base-uri` counterpart of [`violating_fetch_policy`] — text of every
/// policy whose `base-uri` forbids `base_url`, same predicate
/// `base_uri_blocked` already uses.
pub(crate) fn violating_base_uri_policy<'a>(
    policies: &'a [CspPolicy],
    base_url: &str,
    self_origin: Option<&Origin>,
) -> Vec<&'a str> {
    let Ok(parsed) = lumen_core::url::Url::parse(base_url) else {
        return Vec::new();
    };
    policies
        .iter()
        .filter(|policy| !policy.base_uri_allowed(&parsed, self_origin))
        .map(|policy| policy.raw.as_str())
        .collect()
}

/// `style=""` attribute counterpart of [`violating_inline_policy`] — text of
/// every policy whose [`single_style_attribute_blocked`] forbids `body`
/// (GAP-CSPENF срез 57/58). Kept separate rather than folded into
/// `violating_inline_policy` because the attribute form uses its own
/// fallback chain and `'unsafe-hashes'` gate, same reason
/// [`style_attribute_blocked`] is not built on [`inline_directive_blocked`].
pub(crate) fn violating_style_attr_policy<'a>(policies: &'a [CspPolicy], body: &str) -> Vec<&'a str> {
    policies
        .iter()
        .filter(|policy| single_style_attribute_blocked(policy, body))
        .map(|policy| policy.raw.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_policy_allows_inline() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!inline_script_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn script_src_none_blocks_inline() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(inline_script_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn script_src_unsafe_inline_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'self' 'unsafe-inline'");
        assert!(!inline_script_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'self'");
        assert!(inline_script_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn matching_nonce_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'nonce-abc123'");
        assert!(!inline_script_blocked(std::slice::from_ref(&p), Some("abc123"), ""));
    }

    #[test]
    fn mismatched_nonce_blocks() {
        let p = lumen_network::csp::parse_csp_header("script-src 'nonce-abc123'");
        assert!(inline_script_blocked(std::slice::from_ref(&p), Some("other"), ""));
    }

    /// GAP-CSPENF срез 20: `'sha256-…'` matching the actual inline body allows it.
    #[test]
    fn matching_sha256_hash_allows() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!inline_script_blocked(std::slice::from_ref(&p), None, "alert(1)"));
    }

    /// A hash source for a *different* body still blocks — one match is not
    /// "any hash source present".
    #[test]
    fn mismatched_hash_blocks() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(inline_script_blocked(std::slice::from_ref(&p), None, "alert(2)"));
    }

    /// `sha384`/`sha512` are matched too, not only `sha256` — CSP3 §8.1 does
    /// not privilege one algorithm.
    #[test]
    fn matching_sha384_hash_allows() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'sha384-HT2E9NfWiuQ/w1PRai+hTyqW16NIoCGA/m8VQDUopfAtcz6YQjtsMmQd5uRbVDpW'",
        );
        assert!(!inline_script_blocked(std::slice::from_ref(&p), None, "alert(1)"));
    }

    /// A policy naming both a nonce and a hash source accepts either — the
    /// match loop must not short-circuit on the first source kind it sees.
    #[test]
    fn hash_matches_even_when_nonce_source_also_present() {
        let p = lumen_network::csp::parse_csp_header(
            "script-src 'nonce-unrelated' 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!inline_script_blocked(std::slice::from_ref(&p), None, "alert(1)"));
    }

    // GAP-CSPENF срез 21: `inline_style_blocked` shares the exact match logic
    // `inline_script_blocked` already has (`inline_directive_blocked`) — these
    // only prove it is wired to `CspDirective::StyleSrc`, not `ScriptSrc`
    // (`style_src_none_blocks_inline_style` and `no_style_src_allows_inline`),
    // plus one nonce/hash spot-check that a sibling directive does not leak
    // its sources into `style-src`.

    #[test]
    fn no_style_src_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(!inline_style_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn style_src_none_blocks_inline_style() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(inline_style_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn style_src_unsafe_inline_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header("style-src 'self' 'unsafe-inline'");
        assert!(!inline_style_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn script_src_unsafe_inline_does_not_allow_inline_style() {
        let p = lumen_network::csp::parse_csp_header("script-src 'unsafe-inline'; style-src 'none'");
        assert!(inline_style_blocked(std::slice::from_ref(&p), None, ""));
    }

    #[test]
    fn style_src_matching_nonce_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header("style-src 'nonce-abc123'");
        assert!(!inline_style_blocked(std::slice::from_ref(&p), Some("abc123"), ""));
    }

    #[test]
    fn style_src_matching_sha256_hash_allows_inline_style() {
        let p = lumen_network::csp::parse_csp_header(
            "style-src 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!inline_style_blocked(std::slice::from_ref(&p), None, "alert(1)"));
    }

    #[test]
    fn no_img_src_allows() {
        let p = lumen_network::csp::parse_csp_header("script-src 'self'");
        assert!(!img_src_blocked(std::slice::from_ref(&p), "https://example.com/x.png", None));
    }

    #[test]
    fn img_src_none_blocks() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(img_src_blocked(std::slice::from_ref(&p), "https://example.com/x.png", None));
    }

    #[test]
    fn img_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("img-src cdn.example.com");
        assert!(!img_src_blocked(std::slice::from_ref(&p), "https://cdn.example.com/x.png", None));
        assert!(img_src_blocked(std::slice::from_ref(&p), "https://other.example.com/x.png", None));
    }

    /// GAP-CSPENF срез 43: без директивы ничего не переписывается.
    #[test]
    fn no_upgrade_insecure_requests_leaves_url_alone() {
        let p = lumen_network::csp::parse_csp_header("img-src 'self'");
        assert_eq!(upgrade_insecure_url(std::slice::from_ref(&p), "http://example.com/x.png"), None);
    }

    /// UIR §4.1 шаг 5: `http` → `https`, путь/запрос/фрагмент не трогаются.
    #[test]
    fn upgrade_insecure_requests_rewrites_http_scheme() {
        let p = lumen_network::csp::parse_csp_header("upgrade-insecure-requests");
        assert_eq!(
            upgrade_insecure_url(std::slice::from_ref(&p), "http://example.com/x.png?a=1#f"),
            Some("https://example.com/x.png?a=1#f".to_owned())
        );
    }

    /// Явный нестандартный порт спецификация сохраняет (переписывается только
    /// схема); дефолтный `:80` WHATWG-парсер сворачивает сам, так что после
    /// смены схемы получается 443.
    #[test]
    fn upgrade_insecure_requests_keeps_explicit_port() {
        let p = lumen_network::csp::parse_csp_header("upgrade-insecure-requests");
        assert_eq!(
            upgrade_insecure_url(std::slice::from_ref(&p), "http://example.com:8080/x.png"),
            Some("https://example.com:8080/x.png".to_owned())
        );
        assert_eq!(
            upgrade_insecure_url(std::slice::from_ref(&p), "http://example.com:80/x.png"),
            Some("https://example.com/x.png".to_owned())
        );
    }

    /// Не-`http` схемы вне действия директивы: `https` уже безопасна, `data:`/
    /// `file:` шаг 5 не называет.
    #[test]
    fn upgrade_insecure_requests_ignores_non_http_schemes() {
        let p = lumen_network::csp::parse_csp_header("upgrade-insecure-requests");
        assert_eq!(upgrade_insecure_url(std::slice::from_ref(&p), "https://example.com/x.png"), None);
        assert_eq!(upgrade_insecure_url(std::slice::from_ref(&p), "data:image/png;base64,AA"), None);
        assert_eq!(upgrade_insecure_url(std::slice::from_ref(&p), ":://не-url"), None);
    }

    /// CSP3 §3.4: директива в ЛЮБОЙ из независимых политик документа включает
    /// апгрейд — тот же `any`-рисунок, что у всех гейтов этого файла.
    #[test]
    fn upgrade_insecure_requests_in_any_policy_applies() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("img-src 'self'"),
            lumen_network::csp::parse_csp_header("upgrade-insecure-requests"),
        ];
        assert_eq!(
            upgrade_insecure_url(&policies, "http://example.com/x.png"),
            Some("https://example.com/x.png".to_owned())
        );
    }

    /// GAP-CSPENF срез 52: `upgrade_navigation_url` — обёртка над
    /// `upgrade_insecure_url`, которую разделяют все навигационные гейты
    /// (`navigate-to`/`form-action`/`frame-src`) — переписывает `http` в
    /// `https`, когда директива есть.
    #[test]
    fn upgrade_navigation_url_rewrites_when_directive_present() {
        let p = lumen_network::csp::parse_csp_header("upgrade-insecure-requests");
        let gate = (vec![p], "upgrade-insecure-requests".to_owned());
        assert_eq!(
            upgrade_navigation_url(Some(&gate), "http://example.com/next"),
            "https://example.com/next"
        );
    }

    /// Нет гейта (документ без CSP) — адрес возвращается как есть.
    #[test]
    fn upgrade_navigation_url_no_gate_leaves_url_alone() {
        assert_eq!(upgrade_navigation_url(None, "http://example.com/next"), "http://example.com/next");
    }

    /// Гейт есть, но без `upgrade-insecure-requests` — адрес не трогается,
    /// `unwrap_or_else` откатывается на `resolved` без паники.
    #[test]
    fn upgrade_navigation_url_gate_without_directive_leaves_url_alone() {
        let p = lumen_network::csp::parse_csp_header("navigate-to 'self'");
        let gate = (vec![p], "navigate-to 'self'".to_owned());
        assert_eq!(upgrade_navigation_url(Some(&gate), "http://example.com/next"), "http://example.com/next");
    }

    /// GAP-CSPENF срез 5: a document with no `<meta>` CSP still has a policy
    /// when the response carried the header.
    #[test]
    fn response_header_alone_is_a_policy() {
        let mut doc = Document::new();
        doc.set_csp_header(vec!["script-src 'none'".to_owned()]);
        let root = doc.root();
        let (policy, original) =
            document_csp_policy(&doc, root).expect("header alone must produce a policy");
        assert!(inline_script_blocked(&policy, None, ""));
        assert_eq!(original, "script-src 'none'");
    }

    /// No header and no `<meta>` — no policy at all, so nothing is blocked.
    #[test]
    fn no_header_and_no_meta_is_no_policy() {
        let doc = Document::new();
        let root = doc.root();
        assert!(document_csp_policy(&doc, root).is_none());
    }

    /// GAP-CSPENF срез 41: two occurrences of the response header are
    /// independent policies (CSP3 §3.4), same as header+`<meta>` already are
    /// (срез 40) — a later, laxer occurrence of a repeated directive must not
    /// silently override an earlier, stricter one. Before срез 41
    /// `page_source::content_security_policy_header` joined every occurrence
    /// into one string first, so `parse_csp_header` kept only the last
    /// `script-src` and the strict first header stopped blocking anything.
    #[test]
    fn repeated_response_header_stays_independent() {
        let mut doc = Document::new();
        doc.set_csp_header(vec![
            "script-src 'none'".to_owned(),
            "script-src 'unsafe-inline'".to_owned(),
        ]);
        let root = doc.root();
        let (policy, _) =
            document_csp_policy(&doc, root).expect("two headers must still produce a policy");
        assert!(
            inline_script_blocked(&policy, None, "alert(1)"),
            "the first header's own script-src must still block inline execution even though \
             the second header's occurrence allows it"
        );
    }

    /// GAP-CSPENF срез 40: a strict header and a lenient `<meta>` must both be
    /// enforced independently (CSP3 §3.4) — a document cannot loosen the
    /// header's `script-src 'self'` by declaring `'unsafe-inline'` in a
    /// `<meta>` tag of its own choosing. Before срез 40 both were merged into
    /// one string (`"script-src 'self'; script-src 'unsafe-inline'"`), and a
    /// single `CspPolicy` keeps only the last occurrence of a repeated
    /// directive — the `<meta>` value, coming second, silently overrode the
    /// header's and allowed the inline script the header alone forbids.
    #[test]
    fn strict_header_is_not_loosened_by_a_lenient_meta_policy() {
        // `document_csp_policy` itself would need a `<meta>` element in the
        // tree to exercise the header+meta path end to end (plain DOM
        // walking, already covered by `doc_extract`'s own tests) — this test
        // targets the merge logic in isolation, against two
        // independently-parsed policies standing in for "header" and "meta".
        let header_policy = lumen_network::csp::parse_csp_header("script-src 'self'");
        let meta_policy = lumen_network::csp::parse_csp_header("script-src 'unsafe-inline'");
        let policies = vec![header_policy, meta_policy];
        assert!(
            inline_script_blocked(&policies, None, "alert(1)"),
            "the header's own script-src must still block inline execution even though the meta policy allows it"
        );
    }

    #[test]
    fn img_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!img_src_blocked(std::slice::from_ref(&p), "not a url", None));
    }

    /// GAP-CSPENF срез 6: `script-src` against an external `<script src>`.
    #[test]
    fn no_script_src_allows_external() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!script_src_blocked(std::slice::from_ref(&p), "https://example.com/a.js", None));
    }

    #[test]
    fn script_src_none_blocks_external() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(script_src_blocked(std::slice::from_ref(&p), "https://example.com/a.js", None));
    }

    #[test]
    fn script_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("script-src cdn.example.com");
        assert!(!script_src_blocked(std::slice::from_ref(&p), "https://cdn.example.com/a.js", None));
        assert!(script_src_blocked(std::slice::from_ref(&p), "https://other.example.com/a.js", None));
    }

    #[test]
    fn script_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("script-src 'none'");
        assert!(!script_src_blocked(std::slice::from_ref(&p), "not a url", None));
    }

    /// GAP-CSPENF срез 7: `style-src` against an external `<link
    /// rel=stylesheet>`.
    #[test]
    fn no_style_src_allows_external() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!style_src_blocked(std::slice::from_ref(&p), "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_none_blocks_external() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(style_src_blocked(std::slice::from_ref(&p), "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("style-src cdn.example.com");
        assert!(!style_src_blocked(std::slice::from_ref(&p), "https://cdn.example.com/a.css", None));
        assert!(style_src_blocked(std::slice::from_ref(&p), "https://other.example.com/a.css", None));
    }

    #[test]
    fn style_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(style_src_blocked(std::slice::from_ref(&p), "https://example.com/a.css", None));
    }

    #[test]
    fn style_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("style-src 'none'");
        assert!(!style_src_blocked(std::slice::from_ref(&p), "not a url", None));
    }

    /// GAP-CSPENF срез 15: `frame-src` against `<iframe>`/`<frame>` navigation.
    #[test]
    fn no_frame_src_allows_navigation() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!frame_src_blocked(std::slice::from_ref(&p), "https://example.com/frame.html", None));
    }

    #[test]
    fn frame_src_none_blocks_navigation() {
        let p = lumen_network::csp::parse_csp_header("frame-src 'none'");
        assert!(frame_src_blocked(std::slice::from_ref(&p), "https://example.com/frame.html", None));
    }

    #[test]
    fn frame_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("frame-src cdn.example.com");
        assert!(!frame_src_blocked(std::slice::from_ref(&p), "https://cdn.example.com/frame.html", None));
        assert!(frame_src_blocked(std::slice::from_ref(&p), "https://other.example.com/frame.html", None));
    }

    #[test]
    fn frame_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(frame_src_blocked(std::slice::from_ref(&p), "https://example.com/frame.html", None));
    }

    /// GAP-CSPENF срез 26: `child-src` sits between `frame-src` and
    /// `default-src` in the CSP3 §6.4 fallback chain — an allowing
    /// `child-src` must win over a blocking `default-src` when `frame-src`
    /// itself is absent.
    #[test]
    fn frame_src_falls_back_to_child_src_before_default_src() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'; child-src cdn.example.com");
        assert!(!frame_src_blocked(std::slice::from_ref(&p), "https://cdn.example.com/frame.html", None));
        assert!(frame_src_blocked(std::slice::from_ref(&p), "https://other.example.com/frame.html", None));
    }

    #[test]
    fn frame_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("frame-src 'none'");
        assert!(!frame_src_blocked(std::slice::from_ref(&p), "not a url", None));
    }

    /// GAP-CSPENF срез 17: `media-src` against the shell's `<track src>` fetch.
    #[test]
    fn no_media_src_allows_track_fetch() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!media_src_blocked(std::slice::from_ref(&p), "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_none_blocks_track_fetch() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'");
        assert!(media_src_blocked(std::slice::from_ref(&p), "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("media-src cdn.example.com");
        assert!(!media_src_blocked(std::slice::from_ref(&p), "https://cdn.example.com/cap.vtt", None));
        assert!(media_src_blocked(std::slice::from_ref(&p), "https://other.example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(media_src_blocked(std::slice::from_ref(&p), "https://example.com/cap.vtt", None));
    }

    #[test]
    fn media_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'");
        assert!(!media_src_blocked(std::slice::from_ref(&p), "not a url", None));
    }

    /// A stricter sibling directive must not stand in for `media-src`: a page
    /// that locks down `img-src` only has said nothing about its media.
    #[test]
    fn img_src_none_does_not_block_media() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'; media-src *");
        assert!(!media_src_blocked(std::slice::from_ref(&p), "https://example.com/cap.vtt", None));
    }

    /// GAP-CSPENF срез 19: `font-src` against `@font-face url()`.
    #[test]
    fn no_font_src_allows_font_fetch() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!font_src_blocked(std::slice::from_ref(&p), "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_none_blocks_font_fetch() {
        let p = lumen_network::csp::parse_csp_header("font-src 'none'");
        assert!(font_src_blocked(std::slice::from_ref(&p), "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_allowed_host_passes() {
        let p = lumen_network::csp::parse_csp_header("font-src cdn.example.com");
        assert!(!font_src_blocked(std::slice::from_ref(&p), "https://cdn.example.com/font.woff2", None));
        assert!(font_src_blocked(std::slice::from_ref(&p), "https://other.example.com/font.woff2", None));
    }

    #[test]
    fn font_src_default_src_fallback_blocks() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(font_src_blocked(std::slice::from_ref(&p), "https://example.com/font.woff2", None));
    }

    #[test]
    fn font_src_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("font-src 'none'");
        assert!(!font_src_blocked(std::slice::from_ref(&p), "not a url", None));
    }

    /// A stricter sibling directive must not stand in for `font-src`.
    #[test]
    fn media_src_none_does_not_block_font() {
        let p = lumen_network::csp::parse_csp_header("media-src 'none'; font-src *");
        assert!(!font_src_blocked(std::slice::from_ref(&p), "https://example.com/font.woff2", None));
    }

    // GAP-CSPENF срез 23: `style-src-attr` against the `style=""` attribute.

    #[test]
    fn no_policy_allows_style_attribute() {
        let p = lumen_network::csp::parse_csp_header("img-src 'none'");
        assert!(!style_attribute_blocked(std::slice::from_ref(&p), "color:red"));
    }

    #[test]
    fn style_src_attr_none_blocks_attribute() {
        let p = lumen_network::csp::parse_csp_header("style-src-attr 'none'");
        assert!(style_attribute_blocked(std::slice::from_ref(&p), "color:red"));
    }

    #[test]
    fn style_src_attr_unsafe_inline_allows() {
        let p = lumen_network::csp::parse_csp_header("style-src-attr 'unsafe-inline'");
        assert!(!style_attribute_blocked(std::slice::from_ref(&p), "color:red"));
    }

    /// `style-src` (no `-attr` split) falls back for the attribute too — CSP3
    /// §6.4's granular chain, one step before `default-src`.
    #[test]
    fn style_src_fallback_allows_attribute() {
        let p = lumen_network::csp::parse_csp_header("style-src 'unsafe-inline'");
        assert!(!style_attribute_blocked(std::slice::from_ref(&p), "color:red"));
    }

    #[test]
    fn default_src_fallback_blocks_attribute() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(style_attribute_blocked(std::slice::from_ref(&p), "color:red"));
    }

    /// A hash source alone does not allow a `style=""` attribute — CSP3 §8.1
    /// requires `'unsafe-hashes'` alongside it, unlike `<style>` element text
    /// ([`inline_style_blocked`]'s `matching_sha256_hash_allows`-equivalent).
    #[test]
    fn bare_hash_does_not_allow_attribute() {
        let p = lumen_network::csp::parse_csp_header(
            "style-src-attr 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(style_attribute_blocked(std::slice::from_ref(&p), "alert(1)"));
    }

    /// `'unsafe-hashes'` plus a matching hash allows it.
    #[test]
    fn unsafe_hashes_plus_matching_hash_allows_attribute() {
        let p = lumen_network::csp::parse_csp_header(
            "style-src-attr 'unsafe-hashes' 'sha256-bhHHL3z2vDgxUt0W3dWQOrprscmda2Y5pLsLg4GF+pI='",
        );
        assert!(!style_attribute_blocked(std::slice::from_ref(&p), "alert(1)"));
        assert!(style_attribute_blocked(std::slice::from_ref(&p), "alert(2)"));
    }

    /// A nonce source never applies to a `style=""` attribute — there is no
    /// attribute to carry one, unlike `<style nonce="…">`.
    #[test]
    fn nonce_source_does_not_allow_attribute() {
        let p = lumen_network::csp::parse_csp_header("style-src-attr 'nonce-abc123'");
        assert!(style_attribute_blocked(std::slice::from_ref(&p), "color:red"));
    }

    // ── GAP-CSPENF срез 27: `frame-ancestors` enforcement ───────────────────

    #[test]
    fn frame_ancestors_blocks_unlisted_embedder() {
        let p = lumen_network::csp::parse_csp_header("frame-ancestors example.com");
        let ancestor = Origin::new("https", "other.example", 443);
        assert!(frame_ancestors_blocked(std::slice::from_ref(&p), &ancestor, None));
    }

    #[test]
    fn frame_ancestors_allows_listed_embedder() {
        let p = lumen_network::csp::parse_csp_header("frame-ancestors example.com");
        let ancestor = Origin::new("https", "example.com", 443);
        assert!(!frame_ancestors_blocked(std::slice::from_ref(&p), &ancestor, None));
    }

    #[test]
    fn no_frame_ancestors_directive_allows_any_embedder() {
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        let ancestor = Origin::new("https", "anything.example", 443);
        assert!(!frame_ancestors_blocked(std::slice::from_ref(&p), &ancestor, None));
    }

    #[test]
    fn form_action_blocks_unlisted_target() {
        let p = lumen_network::csp::parse_csp_header("form-action example.com");
        assert!(form_action_blocked(std::slice::from_ref(&p), "https://other.example/submit", None));
    }

    #[test]
    fn form_action_allows_listed_target() {
        let p = lumen_network::csp::parse_csp_header("form-action example.com");
        assert!(!form_action_blocked(std::slice::from_ref(&p), "https://example.com/submit", None));
    }

    #[test]
    fn form_action_does_not_fall_back_to_default_src() {
        // Navigation directives (CSP3 §6.4) never inherit `default-src` —
        // same rule already covered for `frame-ancestors` above.
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(!form_action_blocked(std::slice::from_ref(&p), "https://anything.example/submit", None));
    }

    #[test]
    fn form_action_self_matches_document_origin() {
        let p = lumen_network::csp::parse_csp_header("form-action 'self'");
        let origin = Origin::new("https", "example.com", 443);
        assert!(!form_action_blocked(std::slice::from_ref(&p), "https://example.com/submit", Some(&origin)));
        assert!(form_action_blocked(std::slice::from_ref(&p), "https://other.example/submit", Some(&origin)));
    }

    #[test]
    fn base_uri_blocks_unlisted_target() {
        let p = lumen_network::csp::parse_csp_header("base-uri example.com");
        assert!(base_uri_blocked(std::slice::from_ref(&p), "https://other.example/base/", None));
    }

    #[test]
    fn base_uri_allows_listed_target() {
        let p = lumen_network::csp::parse_csp_header("base-uri example.com");
        assert!(!base_uri_blocked(std::slice::from_ref(&p), "https://example.com/base/", None));
    }

    #[test]
    fn base_uri_does_not_fall_back_to_default_src() {
        // Navigation directives (CSP3 §6.4) never inherit `default-src` —
        // same rule already covered for `frame-ancestors`/`form-action` above.
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(!base_uri_blocked(std::slice::from_ref(&p), "https://anything.example/base/", None));
    }

    #[test]
    fn base_uri_self_matches_document_origin() {
        let p = lumen_network::csp::parse_csp_header("base-uri 'self'");
        let origin = Origin::new("https", "example.com", 443);
        assert!(!base_uri_blocked(std::slice::from_ref(&p), "https://example.com/base/", Some(&origin)));
        assert!(base_uri_blocked(std::slice::from_ref(&p), "https://other.example/base/", Some(&origin)));
    }

    #[test]
    fn navigate_to_blocks_unlisted_target() {
        let p = lumen_network::csp::parse_csp_header("navigate-to example.com");
        assert!(navigate_to_blocked(std::slice::from_ref(&p), "https://other.example/next", None));
    }

    #[test]
    fn navigate_to_allows_listed_target() {
        let p = lumen_network::csp::parse_csp_header("navigate-to example.com");
        assert!(!navigate_to_blocked(std::slice::from_ref(&p), "https://example.com/next", None));
    }

    #[test]
    fn navigate_to_does_not_fall_back_to_default_src() {
        // Navigation directives (CSP3 §6.4) never inherit `default-src` —
        // same rule already covered for `frame-ancestors`/`form-action`/
        // `base-uri` above.
        let p = lumen_network::csp::parse_csp_header("default-src 'none'");
        assert!(!navigate_to_blocked(std::slice::from_ref(&p), "https://anything.example/next", None));
    }

    #[test]
    fn navigate_to_self_matches_document_origin() {
        let p = lumen_network::csp::parse_csp_header("navigate-to 'self'");
        let origin = Origin::new("https", "example.com", 443);
        assert!(!navigate_to_blocked(std::slice::from_ref(&p), "https://example.com/next", Some(&origin)));
        assert!(navigate_to_blocked(std::slice::from_ref(&p), "https://other.example/next", Some(&origin)));
    }

    #[test]
    fn navigate_to_unparseable_url_not_blocked() {
        let p = lumen_network::csp::parse_csp_header("navigate-to 'none'");
        assert!(!navigate_to_blocked(std::slice::from_ref(&p), "::: not a url :::", None));
    }

    // ── GAP-CSPENF срез 58: multi-policy `violating_*` reports ─────────────
    //
    // CSP3 §7.8/§3.4 want a `securitypolicyviolation` per independently
    // violated policy when several policies of the same document forbid the
    // same resource at once, not just the first one that matches.

    /// Two independent policies both forbidding the same fetch must both show
    /// up, in policy order — not just the first.
    #[test]
    fn violating_fetch_policy_reports_every_violated_policy() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("img-src 'none'"),
            lumen_network::csp::parse_csp_header("img-src 'self'"),
        ];
        let violated = violating_fetch_policy(
            &policies,
            &CspDirective::ImgSrc,
            "https://example.com/x.png",
            None,
        );
        assert_eq!(violated, vec!["img-src 'none'", "img-src 'self'"]);
    }

    /// Only the policy that actually forbids the fetch is reported when the
    /// other one of two policies allows it.
    #[test]
    fn violating_fetch_policy_reports_only_the_violated_one() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("img-src *"),
            lumen_network::csp::parse_csp_header("img-src 'none'"),
        ];
        let violated = violating_fetch_policy(
            &policies,
            &CspDirective::ImgSrc,
            "https://example.com/x.png",
            None,
        );
        assert_eq!(violated, vec!["img-src 'none'"]);
    }

    /// Nothing violated across either policy — empty, not `None` any more.
    #[test]
    fn violating_fetch_policy_empty_when_nothing_violated() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("img-src *"),
            lumen_network::csp::parse_csp_header("script-src 'none'"),
        ];
        let violated = violating_fetch_policy(
            &policies,
            &CspDirective::ImgSrc,
            "https://example.com/x.png",
            None,
        );
        assert!(violated.is_empty());
    }

    /// Same multi-policy behaviour through the `child-src` fallback chain
    /// `frame-src`/`worker-src` get.
    #[test]
    fn violating_fetch_policy_via_child_src_reports_every_violated_policy() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("frame-src 'none'"),
            lumen_network::csp::parse_csp_header("default-src 'none'; child-src cdn.example.com"),
        ];
        let violated = violating_fetch_policy_via_child_src(
            &policies,
            &CspDirective::FrameSrc,
            "https://other.example/frame.html",
            None,
        );
        assert_eq!(violated.len(), 2);
    }

    /// Two independent policies, both forbidding the same inline body.
    #[test]
    fn violating_inline_policy_reports_every_violated_policy() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("script-src 'none'"),
            lumen_network::csp::parse_csp_header("default-src 'none'"),
        ];
        let violated = violating_inline_policy(&policies, &CspDirective::ScriptSrc, None, "alert(1)");
        assert_eq!(violated.len(), 2);
    }

    /// Only one of two policies blocks the inline body — the other allows
    /// `'unsafe-inline'`, so exactly one text comes back.
    #[test]
    fn violating_inline_policy_reports_only_the_violated_one() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("script-src 'unsafe-inline'"),
            lumen_network::csp::parse_csp_header("script-src 'none'"),
        ];
        let violated = violating_inline_policy(&policies, &CspDirective::ScriptSrc, None, "alert(1)");
        assert_eq!(violated, vec!["script-src 'none'"]);
    }

    /// Two independent policies both forbidding the same `<base href>`.
    #[test]
    fn violating_base_uri_policy_reports_every_violated_policy() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("base-uri example.com"),
            lumen_network::csp::parse_csp_header("base-uri other.example"),
        ];
        let violated =
            violating_base_uri_policy(&policies, "https://third.example/base/", None);
        assert_eq!(violated.len(), 2);
    }

    /// Only the stricter of two `base-uri` policies blocks a target the
    /// other one allows.
    #[test]
    fn violating_base_uri_policy_reports_only_the_violated_one() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("base-uri example.com"),
            lumen_network::csp::parse_csp_header("base-uri *"),
        ];
        let violated =
            violating_base_uri_policy(&policies, "https://example.com/base/", None);
        assert!(violated.is_empty());
        let violated =
            violating_base_uri_policy(&policies, "https://other.example/base/", None);
        assert_eq!(violated, vec!["base-uri example.com"]);
    }

    /// Two independent policies both forbidding the same `style=""`
    /// attribute value.
    #[test]
    fn violating_style_attr_policy_reports_every_violated_policy() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("style-src-attr 'none'"),
            lumen_network::csp::parse_csp_header("default-src 'none'"),
        ];
        let violated = violating_style_attr_policy(&policies, "color:red");
        assert_eq!(violated.len(), 2);
    }

    /// Only one of two policies blocks the attribute — the other allows
    /// `'unsafe-inline'` for `style-src-attr`.
    #[test]
    fn violating_style_attr_policy_reports_only_the_violated_one() {
        let policies = vec![
            lumen_network::csp::parse_csp_header("style-src-attr 'unsafe-inline'"),
            lumen_network::csp::parse_csp_header("style-src-attr 'none'"),
        ];
        let violated = violating_style_attr_policy(&policies, "color:red");
        assert_eq!(violated, vec!["style-src-attr 'none'"]);
    }
}
