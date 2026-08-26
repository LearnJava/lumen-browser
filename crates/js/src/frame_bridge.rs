//! BUG-480 срез 2 — мост `contentWindow`/`contentDocument` из JS родителя в
//! под-документ `<iframe>` (HTML LS §4.8.5 content navigable).
//!
//! Срез 1 (`load_frame_sub_documents`, shell) загружает каждый статический
//! `<iframe>` в отдельный `Document` с собственным V8-контекстом. Изолят у
//! каждого контекста свой, поэтому прямой передачи объектов между окнами нет
//! и не будет (см. commit-message среза 1); вместо этого этот модуль держит в
//! **родительском** изоляте реестр биндингов «хост-элемент → под-документ» и
//! семейство нативов `_lumen_f_*`, читающих под-документ через его
//! `Arc<Mutex<Document>>`. JS-фасады (Window/Document/Element) строятся шимом
//! поверх этих нативов и интернируются, поэтому `iframe.contentWindow ===`
//! `iframe.contentWindow`, а `contentDocument.defaultView === contentWindow`.
//!
//! Поток данных: shell после загрузки ребёнка вызывает
//! [`V8JsRuntime::register_frame_document`] (до диспатча trusted `load` на
//! хосте), биндинг ложится в реестр — геттеры из `iframe_element.rs` начинают
//! видеть фасады сразу, патчить враппер хоста не нужно.
//!
//! BUG-480 срез 3 — иерархия окон поверх того же реестра:
//! - у **ребёнка** слоты `parent`/`top` заполняются биндингами на документы
//!   предков (`register_parent_document`/`register_top_document`) — шим
//!   переопределяет `window.parent`/`window.top`/`window.frameElement`/
//!   `window.name` геттерами с честным fallback на прежнее поведение, пока
//!   слотов нет;
//! - в **каждом** контексте `window.length` становится живым счётчиком
//!   дочерних фреймов, а `register_frame_document` ставит на `window`
//!   индексный (`window[0]`…) и именованный (`window[имя]`) доступники;
//!   `window.frames === window` остаётся самоссылкой (спека), индексный
//!   доступ идёт через неё.
//!
//! Границы среза 3:
//! - фасады не пробрасывают вызов функций между изолятами: `parent.foo()`,
//!   где `foo` объявлена скриптом родителя, не работает (читать свойства
//!   документа родителя можно);
//! - `length`/индексные доступники фасадов остаются нулём/пустыми: счёт
//!   фреймов чужого изолята недоступен, динамический `length` есть только у
//!   настоящего `window` контекста;
//! - доступ только чтение; мутации из родителя и события — будущие срезы;
//! - cross-origin / opaque-sandbox (`sandbox` без `allow-same-origin`)
//!   биндинги регистрируются с `accessible: false`: `contentWindow` отдаёт
//!   фасад без `.document` (спека: WindowProxy доступен всегда),
//!   `contentDocument` — `null`;
//! - фреймы без загруженного под-документа (динамически созданные `<iframe>`,
//!   неудавшийся fetch) биндинга не имеют — оба геттера дают `null`;
//! - геометрия (`offsetWidth`, `getBoundingClientRect`) — честные нули:
//!   layout содержимого фрейма — отдельный срез.
//!
//! BUG-480 срез 4 — `postMessage` через границу изолятов (HTML LS §9.2.9):
//! - у каждого фасада окна есть `postMessage(message, targetOrigin)`; сообщение
//!   уходит в глобальный исходящий ящик ([`FRAME_OUTBOX`], ключ адресата —
//!   указатель его `Arc<Mutex<Document>>`, один и тот же инстанс у shell,
//!   реестра родителя и JS-контекста ребёнка — см. срез 1);
//! - получатель разбирает свой ящик в `_lumen_frame_pump_messages()`, которую
//!   shell вызывает на каждом тике рядом с pump_websockets и т.д. (и для
//!   страницы, и для хэндлов фреймов); доставка асинхронная, как task;
//! - `targetOrigin`: `'*'` — всегда; `'/'` (и опущенный аргумент) — только
//!   same-origin по уже вычисленному shell'ом флагу `accessible`; явная строка
//!   — точное совпадение с нормализованным origin URL биндинга;
//! - `event.source` — фасад окна отправителя в реестре получателя,
//!   `event.origin` — origin отправителя (для `about:srcdoc`/`about:blank`
//!   детей — origin родителя-получателя, как по спеке про наследование);
//! - сериализация данных — JSON-круготрип: примитивы, массивы и plain object'ы
//!   ходят честно; функции/символы бросают `DataCloneError` на отправке,
//!   вложенные функции/узлы DOM деградируют до `null`/`{}` (подмножество
//!   structured clone — отклонение задокументировано в баг-файле);
//! - срез 4 покрывает рёбра «предок ↔ непосредственный потомок»: постинг
//!   внком на `window.top` доставляется, но `event.source` у верха для внука
//!   — `null` (внук не лежит в прямых слотах top), sibling↔sibling — будущий
//!   срез.
//!
//! BUG-480 срез 5 — мутации под-документа из JS родителя (HTML LS §7.5
//! «API for accessible to the contentDocument»):
//! - фасады Element/Document получают запись через новое мутабельное
//!   семейство нативов: `createElement`/`createTextNode` на документе,
//!   `setAttribute`/`removeAttribute`/`appendChild`/`insertBefore`/
//!   `removeChild`/`remove`/сеттер `textContent` на элементе, сеттер `title`
//!   на документе; все операции идут в общий `Arc<Mutex<Document>>`, поэтому
//!   видны контексту ребёнка немедленно (его врапперы читают живое дерево);
//! - аргументы-узлы проверяются на принадлежность тому же биндингу
//!   (`__bid__` на фасаде): чужой фасад → тихий no-op; нативы дополнительно
//!   проверяют границы арены и цикл («потомок под собственного предка» —
//!   отклонение от спеки: HierarchyRequestError заменён тихим no-op, как и
//!   везде в бридже «невалидно = пусто»);
//! - отклонения задокументированы: вставленные `<script>` не исполняются
//!   (исполнение скриптов ребёнка происходит один раз при загрузке, срез 1),
//!   подресурсы вставленных `<img>`/`<link>` не запрашиваются (загрузка
//!   подресурсов фреймов — будущий срез), restyle/layout/paint фрейма не
//!   запускается (фреймы ещё не рендерятся вовсе); события через границу
//!   изолятов (`facade.click()` → слушатели ребёнка) — следующий срез.
//!
//! BUG-480 срез 6 — события через границу изолятов: `facade.click()` →
//! слушатели ребёнка:
//! - у фасада Element появился `click()`; вызов кладёт конверт в глобальный
//!   ящик синтетических событий ([`frame_event_outbox`], тот же ключ адресата,
//!   что у postMessage — указатель `Arc<Mutex<Document>>`);
//! - получатель разбирает свой ящик на том же тике пумпы, что и сообщения
//!   (`_lumen_frame_pump_messages`, shell вызывает её и у страницы, и у
//!   хэндлов фреймов), и доставляет через хук WEB_API_SHIM
//!   `_lumen_deliver_frame_click(nid)`;
//! - хук исполняет СОБСТВЕННУЮ семантику click ребёнка — общую функцию
//!   `_lumen_perform_click` шима (`dom.rs`), ту же, что
//!   `HTMLElement.prototype.click`: disabled-гейт, re-entrancy guard,
//!   activation target до диспатча, MouseEvent + `_lumen_dispatch_rich`
//!   (пузырьки, слушатели, on-атрибуты) и активационное поведение после;
//! - доставка асинхронная на тике пумпы (отклонение от синхронного по спеке
//!   dispatch — то же, что у postMessage среза 4); очередь ограничена тем же
//!   [`FRAME_OUTBOX_CAP`], переполнение теряет конверт молча;
//! - только элементы: клик по фасаду текста/комментария и чужой/вышедший за
//!   границы арены nid отклоняются нативом («невалидно = пусто»).
//!
//! BUG-480 срез 7 — focus()/blur() и произвольный dispatchEvent через фасад:
//! - ящик событий обобщён: [`PendingFrameEvent`] несёт [`FrameEventKind`]
//!   (`click` | `focus` | `blur` | `dom`) вместо неявного «всегда клик»,
//!   постановка идёт через единый натив `_lumen_f_queue_event(bid, nid,
//!   spec_json)`; конверты всех видов разбираются одной пумпой и доставляются
//!   хуками WEB_API_SHIM `_lumen_deliver_frame_focus` /
//!   `_lumen_deliver_frame_blur` / `_lumen_deliver_frame_dom_event` (клик —
//!   прежний `_lumen_deliver_frame_click`);
//! - `focus(options)` / `blur()` исполняют СОБСТВЕННУЮ семантику ребёнка
//!   (`_lumen_is_focusable`, `_lumen_focus_update`: blur/focusout на старом,
//!   focus/focusin на новом), но БЕЗ уведомления шелла: очередь фокус-запросов
//!   рантайма фрейма шеллом пока не дренируется (фреймы не рендерятся),
//!   запрос там только копился бы; `preventScroll` переносится в конверте,
//!   но пока игнорируется доставкой — layout у фреймов нулевой;
//! - `dispatchEvent(event)` сериализует type/bubbles/cancelable/composed/detail
//!   (JSON-круготрип detail) и доставляет в ребёнке ту же недоверенную
//!   последовательность, что его собственный `el.dispatchEvent`:
//!   `_lumen_dispatch` + активационное поведение для script-dispatched 'click'
//!   (BUG-439); возвращаемое значение спеки (!defaultPrevented) при
//!   асинхронной доставке недостижимо — метод ничего не возвращает.
//!
//! BUG-480 срез 8 — исполнение `<script>`, вставленных в под-документ из
//! родителя (срез 5 вставлял их молча):
//! - фасадные `appendChild`/`insertBefore` после успешной мутации проверяют тег
//!   вставленного узла и для `<script>` ставят в тот же ящик событий конверт
//!   [`FrameEventKind::RunScript`] (в момент вставки код НЕ исполняется —
//!   доставка через границу изолятов у моста всегда асинхронная);
//! - ребёнок на своём тике отдаёт элемент своей ШТАТНОЙ
//!   `_lumen_script_prepare` (HTML LS §4.12.1): гейт типа (data-блоки не
//!   исполняются), пустой `src` → `error`, внешний `src` → fetch силами
//!   провайдеров самого фрейма, инлайн-классика синхронно с честным
//!   `document.currentScript`; повторная вставка уже исполненного скрипта не
//!   перезапускает (per-element «already started» ведёт получатель);
//! - конверт, адресат которого успел отсоединиться до доставки, теряется без
//!   пометки «started» — повторная вставка исполнится, как у главного
//!   документа.
//!
//! BUG-480 срез 9 — URL-рефлексии фасадов и поздний `src` скрипта (внешний
//! `<script src>` из родителя: первый клиент подресурсной доставки фрейма):
//! - фасадный элемент получил `src`/`href` (HTML LS §2.6.2): сеттер хранит
//!   строку дословно, геттер разрешает против базы под-документа (href первого
//!   `<base>` против URL документа, иначе сам URL — §4.2.3); до среза запись
//!   `s.src = url` заводила свойство JS-обёртки в изоляте родителя, атрибут в
//!   дереве ребёнка не появлялся, и `_lumen_script_prepare` молча уходил в
//!   инлайн-ветку пустого тела;
//! - натив записи атрибута (`_lumen_f_set_attr`) после успешного `src` на
//!   `<script>` ставит тот же конверт RunScript, что и вставка (HTML LS
//!   §4.12.1: изменение src перезапускает prepare) — каноничное
//!   `s.src = …` ПОСЛЕ appendChild и до него теперь равнозначны; уже
//!   начавшийся скрипт no-op-ается у получателя;
//! - «already started» ведётся по спеке: флаг ставится, только когда
//!   подготовка реально началась (fetch запущен / тело исполнено / пустой src
//!   отстрелил error) — дата-блок и элемент без src и без тела остаются
//!   непомеченными, и поздний `src` обязан получить вторую доставку.
//!   Предикат старта — `_lumen_frame_script_will_start` (dom.rs), зеркальный
//!   ранним выходам `_lumen_script_prepare`.
//!
//! BUG-480 срез 10 — обратная доставка ресурсных событий через границу
//! изолятов (обработчики ФАСАДА):
//! - ресурсное событие (`load`/`error` от `_lumen_resource_fire`: внешний
//!   скрипт, динамическая таблица стилей, блок `<style>`, …) в контексте,
//!   у которого ЕСТЬ слот родителя, зеркалится конвертом
//!   [`FrameEventKind::Resource`] в документ родителя — обратное направление
//!   всех прежних ящиков (родитель → ребёнок);
//! - фасадный элемент получил `addEventListener`/`removeEventListener`
//!   (карта слушателей в WeakMap замыкания шима); доставка на тике пумпы
//!   родителя строит Event в его изоляте, ставит `target`/`currentTarget` на
//!   интернированный фасад nid'а (bid отправителя разрешает реестр
//!   ПОЛУЧАТЕЛЯ, как `event.source` у postMessage) и вызывает сначала
//!   слушателей, затем свойство `on<type>` — тем же порядком, что локальный
//!   диспатч; этим закрыт хвост среза 9: `s.onload`, назначенный родителем на
//!   фасад внешнего скрипта, больше не no-op;
//! - гейты: постановка требует живого слота родителя и элемента собственного
//!   документа (новое поле [`FrameDocSlots::self_doc`] даёт нативу доступ к
//!   собственной арене); фильтр доступности стоит при разборе ящика — биндинг
//!   отправителя, которого у получателя нет или который `accessible: false`,
//!   конверт отбрасывает (у cross-origin/opaque детей фасадов элементов нет);
//! - отклонения задокументированы: ребро только «ребёнок → непосредственный
//!   родитель» (внук зеркалится среднему фрейму, как у postMessage среза 4);
//!   доставка асинхронная на тике пумпы; событие в изоляте получателя —
//!   новый объект Event, а не тот же инстанс, что у ребёнка.

#[cfg(feature = "v8-backend")]
use std::sync::{Arc, Mutex, OnceLock};

/// Псевдо-bid слота «окно родителя» в реестре ([`FrameDocSlots::parent`]).
///
/// Обычные биндинги адресуются индексом в `frames`; специальные значения
/// сверху диапазона `u32` позволяют всему семейству нативов `_lumen_f_*`
/// работать со документом предка без второго семейства функций.
#[cfg(feature = "v8-backend")]
pub(crate) const PARENT_BID: u32 = u32::MAX;

/// Псевдо-bid слота «верхнее окно» в реестре ([`FrameDocSlots::top`]).
/// Заполняется только для фреймов глубины ≥ 2: у фрейма первого уровня
/// `parent === top`, и обе геттер-цепочки ведут через [`PARENT_BID`].
#[cfg(feature = "v8-backend")]
pub(crate) const TOP_BID: u32 = u32::MAX - 1;

/// Один зарегистрированный под-документ `<iframe>` в реестре рантайма.
///
/// Живёт в [`FrameDocSlots`] столько же, сколько контекст страницы:
/// биндинги никогда не удаляются по одному — замена страницы уносит весь
/// рантайм вместе с реестром (тот же lifecycle, что у [`crate::img_bitmap_store`]).
#[cfg(feature = "v8-backend")]
pub(crate) struct FrameDocBinding {
    /// `NodeId` элемента-хоста `<iframe>` в документе родителя.
    ///
    /// Для биндингов-предков ([`PARENT_BID`]) — nid хоста в документе родителя:
    /// по нему ребёнок строит `window.frameElement`. У топового слота хоста
    /// нет — поле заполнено условным значением и не читается.
    pub(crate) host_nid: u32,
    /// Под-документ. Отдельный `Arc` — его же держит shell (`FrameHandle.doc`)
    /// и JS-контекст самого ребёнка.
    pub(crate) doc: Arc<Mutex<lumen_dom::Document>>,
    /// Разрешённый адрес под-документа (`about:srcdoc`/`about:blank`/URL).
    pub(crate) url: String,
    /// Значение атрибута `name` хоста, если задан — ключ именованного доступа
    /// `window[name]` (срез 3).
    pub(crate) name: Option<String>,
    /// `false` — cross-origin или opaque sandbox: нативы чтения отдают пустые
    /// результаты, `.document` фасада окна — `null`.
    pub(crate) accessible: bool,
}

/// Реестр биндингов одного V8-изолята: дочерние фреймы + ссылки на предков.
///
/// `frames` — под-документы этого контекста (индекс = стабильный `bid` для
/// нативов `_lumen_f_*`, порядок регистрации = порядок документа).
/// `parent`/`top` — документы предков для контекста самого фрейма (срез 3):
/// заполняются shell-ом через `register_parent_document`/`register_top_document`.
#[cfg(feature = "v8-backend")]
#[derive(Default)]
pub(crate) struct FrameDocSlots {
    pub(crate) frames: Vec<FrameDocBinding>,
    pub(crate) parent: Option<FrameDocBinding>,
    pub(crate) top: Option<FrameDocBinding>,
    /// Срез 4: указатель `Arc` собственного документа этого контекста —
    /// ключ получателя в [`FRAME_OUTBOX`]. Заполняется `install_dom`
    /// (`Arc::as_ptr(&doc)`); у минимальных тестовых изолятов без
    /// `install_dom` остаётся `None`, и `_lumen_frame_take_messages`
    /// ничего не отдаёт.
    pub(crate) self_key: Option<usize>,
    /// Срез 10: клон `Arc` собственного документа — по нему натив
    /// `_lumen_f_queue_parent_resource` проверяет nid (элемент собственной
    /// арены) и берёт `source_doc` для конверта [`FrameDocSlots::self_key`].
    /// Заполняется тем же блоком `install_dom`, что и [`FrameDocSlots::self_key`].
    pub(crate) self_doc: Option<Arc<Mutex<lumen_dom::Document>>>,
    /// Срез 4: нормализованный origin собственной страницы — им заменяется
    /// origin `about:`-биндинга при вычислении `event.origin` (наследование
    /// origin у srcdoc/about:blank детей).
    pub(crate) self_origin: String,
}

/// Общий `Arc` между `V8JsRuntime`, нативами этого модуля и вызовами
/// `register_*_document`.
#[cfg(feature = "v8-backend")]
pub(crate) type FrameDocRegistry = Arc<Mutex<FrameDocSlots>>;

// ── Срез 4: исходящий ящик кросс-фреймовых postMessage ───────────────────────

/// Отправитель сообщения в [`PendingFrameMessage`] — как получатель строит
/// `event.source` из СВОЕГО реестра.
#[cfg(feature = "v8-backend")]
#[derive(Clone, Copy)]
pub(crate) enum SourceKind {
    /// Отправитель — родитель получателя: source = фасад слота parent.
    Parent,
    /// Отправитель — потомок; ключ — указатель `Arc` документа отправителя,
    /// по которому получатель ищет свой слот `frames[j]`.
    ChildDoc(usize),
}

/// Одно сообщение в [`FRAME_OUTBOX`] до разбора получателем.
#[cfg(feature = "v8-backend")]
pub(crate) struct PendingFrameMessage {
    /// Клон `Arc<Mutex<Document>>` адресата. Держит документ живым и указатель
    /// стабильным до доставки — сообщение не попадёт чужому контексту, занявшему
    /// освобождённый адрес.
    pub(crate) target_doc: Arc<Mutex<lumen_dom::Document>>,
    /// Кто отправил ([`SourceKind`]).
    pub(crate) source: SourceKind,
    /// Данные сообщения, уже сериализованные отправителем (`JSON.stringify`).
    pub(crate) data_json: String,
}

/// Глобальный ящик «кто-то вызвал `facade.postMessage(...)`».
///
/// Пишут нативы любого изолята, читает только `_lumen_frame_take_messages`
/// на JS-потоке получателя. Ёмкость ограничена: переполнение молча теряет
/// сообщение (отправка postMessage не имеет отчётности о доставке и по спеке),
/// чтобы зомби-страница не растила память бесконечно.
#[cfg(feature = "v8-backend")]
pub(crate) fn frame_outbox() -> &'static Mutex<Vec<PendingFrameMessage>> {
    static OUTBOX: OnceLock<Mutex<Vec<PendingFrameMessage>>> = OnceLock::new();
    OUTBOX.get_or_init(|| Mutex::new(Vec::new()))
}

/// Верхняя граница неотправленных сообщений во всём процессе.
#[cfg(feature = "v8-backend")]
const FRAME_OUTBOX_CAP: usize = 256;

// ── Срезы 6–7: ящик синтетических событий через границу изолятов ─────────────

/// Что именно вызвали на фасаде (срез 6 — `click`, срез 7 — остальные).
///
/// Получатель исполняет каждое значение СВОЕЙ семантикой: конверт переносит
/// только намерение и его аргументы, никаких ссылок на объекты изолята
/// отправителя через границу не идёт.
#[cfg(feature = "v8-backend")]
pub(crate) enum FrameEventKind {
    /// Активация элемента: ребёнок исполняет свою семантику `click()`
    /// (`_lumen_deliver_frame_click` → `_lumen_perform_click`).
    Click,
    /// Фокус на элементе под-документа: `_lumen_is_focusable` +
    /// `_lumen_focus_update(nid)` без уведомления шелла (см. доки модуля).
    Focus {
        /// Флаг `options.preventScroll`; доставкой пока игнорируется —
        /// layout у фреймов нулевой, перенесён ради стабильности протокола.
        prevent_scroll: bool,
    },
    /// Снятие фокуса: no-op, если сфокусировано не это; иначе
    /// `_lumen_focus_update(-1)`.
    Blur,
    /// Произвольное синтетическое событие: та же последовательность, что у
    /// `el.dispatchEvent` самого ребёнка. `detail_json` — уже сериализованный
    /// отправителем `JSON.stringify(detail)`; при разборе вкладывается в
    /// `CustomEvent` как есть (JSON-подмножество structured clone).
    Dom {
        ev_type: String,
        bubbles: bool,
        cancelable: bool,
        composed: bool,
        detail_json: Option<String>,
    },
    /// Срез 8: в под-документ вставлен `<script>` (фасадный `appendChild`/
    /// `insertBefore`). Получатель готовит и исполняет элемент своей штатной
    /// `_lumen_script_prepare`; повторная вставка уже исполненного не
    /// перезапускает (per-element «already started» на стороне получателя).
    RunScript,
    /// Срез 10: ресурсное событие (`load`/`error`) на элементе ОТПРАВИТЕЛЯ
    /// (ребёнка) — зеркальное направление ко всем прежним видам: получатель —
    /// РОДИТЕЛЬ, и доставляет оно в обработчики фасада (`addEventListener`
    /// фасада, затем свойство `on<type>`), а не в семантику ребёнка.
    Resource {
        /// Тип события (`'load'`/`'error'`/…).
        ev_type: String,
    },
}

/// Разобрать спецификацию конверта, построенную шимом фасада.
///
/// Формат: `{"kind":"click"}` | `{"kind":"blur"}`
/// | `{"kind":"focus","preventScroll":bool}`
/// | `{"kind":"dom","type":str,"bubbles":bool,"cancelable":bool,
///    "composed":bool,"detail":<любой JSON|null>}`.
/// Всё остальное (чужой kind, пустой/слишком длинный type, невалидный JSON)
/// — `None`: вызывающий JS получает тихий «нет», неотличимый от отсутствия
/// биндинга.
#[cfg(feature = "v8-backend")]
fn parse_frame_event_spec(spec: &str) -> Option<FrameEventKind> {
    let v: serde_json::Value = serde_json::from_str(spec).ok()?;
    match v.get("kind").and_then(serde_json::Value::as_str)? {
        "click" => Some(FrameEventKind::Click),
        "focus" => Some(FrameEventKind::Focus {
            prevent_scroll: v
                .get("preventScroll")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
        }),
        "blur" => Some(FrameEventKind::Blur),
        "runscript" => Some(FrameEventKind::RunScript),
        "dom" => {
            let ev_type = v.get("type")?.as_str()?.to_owned();
            // Верхняя граница разумная, а не спечная: тип события — имя
            // слушателя в карте `_lumen_listeners`, гигантская строка там
            // ничего осмысленного не адресует.
            if ev_type.is_empty() || ev_type.len() > 128 {
                return None;
            }
            let flag = |key: &str| {
                v.get(key)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            };
            Some(FrameEventKind::Dom {
                ev_type,
                bubbles: flag("bubbles"),
                cancelable: flag("cancelable"),
                composed: flag("composed"),
                detail_json: v
                    .get("detail")
                    .filter(|d| !d.is_null())
                    .map(serde_json::Value::to_string),
            })
        }
        _ => None,
    }
}

/// Один конверт «фасад вызвал click()/focus()/blur()/dispatchEvent()» до
/// разбора получателем.
#[cfg(feature = "v8-backend")]
pub(crate) struct PendingFrameEvent {
    /// Клон `Arc<Mutex<Document>>` адресата — тот же ключ получателя, что у
    /// [`PendingFrameMessage`]: держит документ живым, указатель стабильным.
    pub(crate) target_doc: Arc<Mutex<lumen_dom::Document>>,
    /// Индекс целевого узла в арене документа-адресата.
    pub(crate) nid: u32,
    /// Что вызвано на фасаде (семантику исполняет получатель).
    pub(crate) kind: FrameEventKind,
    /// Срез 10: ключ документа ОТПРАВИТЕЛЯ (указатель его `Arc`). Для
    /// [`FrameEventKind::Resource`] получатель разрешает по нему bid биндинга
    /// отправителя (как `event.source` у postMessage); прежние виды значение
    /// не читают. `0` — отправитель без установленного `install_dom`
    /// (минимальный тестовый изолят): bid не резолвится, Resource-конверт
    /// отбрасывается.
    pub(crate) source_doc: usize,
}

/// Глобальный ящик «кто-то вызвал `facade.click()`/`focus()`/`blur()`/
/// `dispatchEvent()`». Пишут нативы любого изолята, читает только
/// `_lumen_frame_take_events` на JS-потоке получателя (на том же тике пумпы,
/// что и postMessage). Ёмкость ограничена так же, как у [`frame_outbox`]:
/// переполнение молча теряет конверт — зомби-страница не должна расти память
/// бесконечно.
#[cfg(feature = "v8-backend")]
fn frame_event_outbox() -> &'static Mutex<Vec<PendingFrameEvent>> {
    static OUTBOX: OnceLock<Mutex<Vec<PendingFrameEvent>>> = OnceLock::new();
    OUTBOX.get_or_init(|| Mutex::new(Vec::new()))
}

/// Срез 8: есть ли у контекста с ключом документа `key` неразобранные конверты
/// в любом из ящиков моста. Шелл будит спящий event-loop, пока это так:
/// без этого конверт, поставленный после затихания страницы (обработчик load,
/// чужой таймер), лежит до случайного пробуждения. Конверты мёртвых адресатов
/// (ключ не совпал ни с одним живым контекстом) опрос не держат — латентная
/// утечка ящика остаётся ограниченной капой, а не превращается в горячий
/// цикл.
#[cfg(feature = "v8-backend")]
pub(crate) fn frame_transport_has_for(key: Option<usize>) -> bool {
    let Some(key) = key else {
        return false;
    };
    let msgs = frame_outbox().lock().unwrap_or_else(|e| e.into_inner());
    if msgs.iter().any(|m| Arc::as_ptr(&m.target_doc) as usize == key) {
        return true;
    }
    drop(msgs);
    let evs = frame_event_outbox()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    evs.iter().any(|e| Arc::as_ptr(&e.target_doc) as usize == key)
}

/// Нормализованный origin URL биндинга для `event.origin`/валидации
/// `targetOrigin`. `about:*` наследует origin контекста-родителя — здесь это
/// выражено тем, что вызывающая сторона подставляет `self_origin` получателя.
#[cfg(feature = "v8-backend")]
fn binding_origin(url: &str, fallback: &str) -> String {
    if url.starts_with("about:") {
        return fallback.to_owned();
    }
    crate::file_input::origin_for_url(url)
}

/// Разрешить `bid` (индекс или псевдо-bid предка) в слот реестра.
#[cfg(feature = "v8-backend")]
fn resolve_slot(slots: &FrameDocSlots, bid: u32) -> Option<&FrameDocBinding> {
    match bid {
        PARENT_BID => slots.parent.as_ref(),
        TOP_BID => slots.top.as_ref(),
        i => slots.frames.get(i as usize),
    }
}

/// Захватить биндинг `bid` на чтение, если он существует и разрешён.
///
/// Все нативы чтения проходят через эту точку: несуществующий bid и
/// cross-origin/opaque bid неотличимы для вызывающего JS — оба дают «пусто».
#[cfg(feature = "v8-backend")]
fn with_accessible_doc<R>(
    registry: &FrameDocRegistry,
    bid: u32,
    f: impl FnOnce(&lumen_dom::Document) -> R,
    empty: R,
) -> R {
    let reg = registry.lock().unwrap_or_else(|e| e.into_inner());
    let Some(binding) = resolve_slot(&reg, bid) else {
        return empty;
    };
    if !binding.accessible {
        return empty;
    }
    let doc = binding.doc.lock().unwrap_or_else(|e| e.into_inner());
    f(&doc)
}

/// Мутабельный вариант [`with_accessible_doc`] для среза 5 (запись в
/// под-документ). Те же правила: нет биндинга или `accessible: false` —
/// вызывающий JS получает «пусто».
#[cfg(feature = "v8-backend")]
fn with_accessible_doc_mut<R>(
    registry: &FrameDocRegistry,
    bid: u32,
    f: impl FnOnce(&mut lumen_dom::Document) -> R,
    empty: R,
) -> R {
    let reg = registry.lock().unwrap_or_else(|e| e.into_inner());
    let Some(binding) = resolve_slot(&reg, bid) else {
        return empty;
    };
    if !binding.accessible {
        return empty;
    }
    let mut doc = binding.doc.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut doc)
}

/// Границы арены для нативов записи: `Document::get` индексирует `Vec` без
/// проверок, а JS передаёт сырые `u32`. В отличие от нативов чтения
/// (исторически доверяющих фасадным `nid`) мутации обязаны отвергать чужой
/// индекс — иначе фасад одного документа испортил бы дерево другого.
#[cfg(feature = "v8-backend")]
fn checked_node(doc: &lumen_dom::Document, nid: u32) -> Option<lumen_dom::NodeId> {
    if nid as usize >= doc.node_count() {
        return None;
    }
    Some(lumen_dom::NodeId::from_index(nid as usize))
}

/// DEVX-8a-аналог (`lumen_dom::Document::is_self_or_ancestor` — приватный):
/// true, если `candidate` — сам `node` или его предок по цепочке `parent`.
#[cfg(feature = "v8-backend")]
fn is_self_or_ancestor(
    doc: &lumen_dom::Document,
    candidate: lumen_dom::NodeId,
    node: lumen_dom::NodeId,
) -> bool {
    let mut cur = Some(node);
    while let Some(n) = cur {
        if n == candidate {
            return true;
        }
        cur = doc.get(n).parent;
    }
    false
}

/// Mirrors `v8_runtime::set_attribute` (приватен там; третья копия — рядом
/// уже живут `dom.rs` и `v8_runtime.rs`).
#[cfg(feature = "v8-backend")]
fn bridge_set_attr(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, name: &str, value: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        if let Some(attr) = attrs
            .iter_mut()
            .find(|a| a.name.local.eq_ignore_ascii_case(name))
        {
            attr.value = value.to_string();
        } else {
            attrs.push(lumen_dom::Attribute {
                name: lumen_dom::QualName::html(name.to_ascii_lowercase()),
                value: value.to_string(),
            });
        }
    }
}

/// Mirrors `v8_runtime::remove_attribute`.
#[cfg(feature = "v8-backend")]
fn bridge_remove_attr(doc: &mut lumen_dom::Document, id: lumen_dom::NodeId, name: &str) {
    if let lumen_dom::NodeData::Element { attrs, .. } = &mut doc.get_mut(id).data {
        attrs.retain(|a| !a.name.local.eq_ignore_ascii_case(name));
    }
}

/// Mirrors `v8_runtime::set_text_content`: Text/Comment перезаписывают свою
/// строку на месте, остальные узлы заменяют детей одним текстовым узлом.
#[cfg(feature = "v8-backend")]
fn bridge_set_text_content(
    doc: &mut lumen_dom::Document,
    id: lumen_dom::NodeId,
    text: &str,
) -> bool {
    match &mut doc.get_mut(id).data {
        lumen_dom::NodeData::Text(s) | lumen_dom::NodeData::Comment(s) => {
            *s = text.to_string();
            return true;
        }
        _ => {}
    }
    let children: Vec<lumen_dom::NodeId> = doc.get(id).children.clone();
    for child in children {
        doc.detach(child);
    }
    if !text.is_empty()
        && let Ok(text_node) = doc.try_create_text(text)
    {
        doc.append_child(id, text_node);
    }
    true
}

/// Срез 8: после успешной фасадной вставки — конверт [`FrameEventKind::RunScript`],
/// если в под-документ попал `<script>`. Ребёнок исполняет его на своём тике;
/// здесь только факт вставки фиксируется. Второй короткий захват реестра после
/// мутации: биндинги только растут (слот стабилен), документ — тот же `Arc`.
/// Переполненный ящик теряет конверт молча, как у всех событий моста.
#[cfg(feature = "v8-backend")]
fn queue_run_script_if_script(registry: &FrameDocRegistry, bid: u32, nid: u32) {
    let (target_doc, source_doc) = {
        let reg = registry.lock().unwrap_or_else(|e| e.into_inner());
        let Some(binding) = resolve_slot(&reg, bid) else {
            return;
        };
        if !binding.accessible {
            return;
        }
        let is_script = {
            let doc = binding.doc.lock().unwrap_or_else(|e| e.into_inner());
            checked_node(&doc, nid).is_some_and(|id| {
                matches!(
                    &doc.get(id).data,
                    lumen_dom::NodeData::Element { name, .. }
                        if name.local.eq_ignore_ascii_case("script")
                )
            })
        };
        if !is_script {
            return;
        }
        (
            Arc::clone(&binding.doc),
            reg.self_key.unwrap_or(0),
        )
    };
    let outbox = frame_event_outbox();
    let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
    if outbox.len() >= FRAME_OUTBOX_CAP {
        return;
    }
    outbox.push(PendingFrameEvent {
        target_doc,
        nid,
        kind: FrameEventKind::RunScript,
        source_doc,
    });
}

/// Срез 10: зеркальное направление ящика событий — ресурсное событие
/// (`load`/`error` от `_lumen_resource_fire`) контекста-фрейма ставится
/// конвертом в документ РОДИТЕЛЯ. Гейты постановки: у отправителя живой слот
/// родителя и собственный документ ([`FrameDocSlots::self_doc`], ставится
/// `install_dom` — минимальные тестовые изоляты молча не ставят ничего), а
/// nid — элемент собственной арены. Доступность пары проверяет получатель при
/// разборе ящика: фасады элементов существуют только у доступных биндингов.
#[cfg(feature = "v8-backend")]
fn queue_parent_resource(
    registry: &FrameDocRegistry,
    nid: u32,
    ev_type: String,
) -> bool {
    let (target_doc, source_doc) = {
        let reg = registry.lock().unwrap_or_else(|e| e.into_inner());
        let Some(self_doc) = reg.self_doc.as_ref() else {
            return false;
        };
        let is_element = {
            let doc = self_doc.lock().unwrap_or_else(|e| e.into_inner());
            checked_node(&doc, nid).is_some_and(|id| {
                matches!(&doc.get(id).data, lumen_dom::NodeData::Element { .. })
            })
        };
        if !is_element {
            return false;
        }
        match reg.parent.as_ref() {
            Some(parent) => (Arc::clone(&parent.doc), reg.self_key.unwrap_or(0)),
            None => return false,
        }
    };
    let outbox = frame_event_outbox();
    let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
    if outbox.len() >= FRAME_OUTBOX_CAP {
        return false;
    }
    outbox.push(PendingFrameEvent {
        target_doc,
        nid,
        kind: FrameEventKind::Resource { ev_type },
        source_doc,
    });
    true
}

/// Первый элемент с тегом `tag` (ASCII case-insensitive) в document order.
/// Mirrors `v8_runtime::find_element_by_tag`.
#[cfg(feature = "v8-backend")]
fn find_element_by_tag(doc: &lumen_dom::Document, tag: &str) -> Option<lumen_dom::NodeId> {
    find_first_matching(doc, doc.root(), &|node| {
        node.element_name()
            .map(|n| n.local.eq_ignore_ascii_case(tag))
            .unwrap_or(false)
    })
}

/// Предзаказный обход поддерева с предикатом. Mirrors `v8_runtime::find_first_matching`.
#[cfg(feature = "v8-backend")]
fn find_first_matching(
    doc: &lumen_dom::Document,
    start: lumen_dom::NodeId,
    pred: &dyn Fn(&lumen_dom::Node) -> bool,
) -> Option<lumen_dom::NodeId> {
    let node = doc.get(start);
    if pred(node) {
        return Some(start);
    }
    for &child in &node.children.clone() {
        if let Some(found) = find_first_matching(doc, child, pred) {
            return Some(found);
        }
    }
    None
}

/// Конкатенация текстовых узлов поддерева. Mirrors `v8_runtime::collect_text_content`.
#[cfg(feature = "v8-backend")]
fn collect_text_content(doc: &lumen_dom::Document, id: lumen_dom::NodeId) -> String {
    if let lumen_dom::NodeData::Comment(s) = &doc.get(id).data {
        return s.clone();
    }
    fn inner(doc: &lumen_dom::Document, id: lumen_dom::NodeId, out: &mut String) {
        let node = doc.get(id);
        if let lumen_dom::NodeData::Text(s) = &node.data {
            out.push_str(s);
        }
        for &child in &node.children.clone() {
            inner(doc, child, out);
        }
    }
    let mut out = String::new();
    inner(doc, id, &mut out);
    out
}

/// Зарегистрировать нативы `_lumen_f_*` + оценить JS-шим фасадов.
///
/// Вызывается из `install_dom` (список `install_v8!`) с клоном реестра
/// рантайма — тем же, куда пишет [`V8JsRuntime::register_frame_document`].
#[cfg(feature = "v8-backend")]
pub(crate) fn install_frame_bridge_v8(
    rt: &crate::v8_runtime::V8JsRuntime,
    registry: FrameDocRegistry,
) -> lumen_core::JsResult<()> {
    use crate::v8_compat::{into_v8_fn0, into_v8_fn1, into_v8_fn2, into_v8_fn3, into_v8_fn4};
    use lumen_core::ext::JsRuntime as _;

    // bid → есть ли биндинг вообще (для contentWindow, который существует
    // даже при accessible=false).
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_binding",
            into_v8_fn1(move |host_nid: u32| -> Option<u32> {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                reg.frames
                    .iter()
                    .position(|b| b.host_nid == host_nid)
                    .map(|i| i as u32)
            }),
        )?;
    }
    // Срез 3: bid слотов предков текущего контекста (или null, если контекст
    // сам верхний). Геттеры window.parent/top/frameElement читают их лениво.
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_parent_binding",
            into_v8_fn0(move || -> Option<u32> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .parent
                    .is_some()
                    .then_some(PARENT_BID)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_top_binding",
            into_v8_fn0(move || -> Option<u32> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .top
                    .is_some()
                    .then_some(TOP_BID)
            }),
        )?;
    }
    // Срез 3: число дочерних фреймов этого контекста (window.length) и доступ
    // к биндингу по индексу регистрации (постановка индексных/именованных
    // доступников на window после каждого register_frame_document).
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_count",
            into_v8_fn0(move || -> u32 {
                reg.lock().unwrap_or_else(|e| e.into_inner()).frames.len() as u32
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_host_at",
            into_v8_fn1(move |idx: u32| -> Option<u32> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .frames
                    .get(idx as usize)
                    .map(|b| b.host_nid)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_name_at",
            into_v8_fn1(move |idx: u32| -> Option<String> {
                reg.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .frames
                    .get(idx as usize)?
                    .name
                    .clone()
                    .filter(|n| !n.is_empty())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_accessible",
            into_v8_fn1(move |bid: u32| -> bool {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                resolve_slot(&reg, bid).is_some_and(|b| b.accessible)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_host",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                resolve_slot(&reg, bid)
                    .filter(|b| b.accessible)
                    .map(|b| b.host_nid)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_url",
            into_v8_fn1(move |bid: u32| -> String {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                match resolve_slot(&reg, bid) {
                    Some(b) if b.accessible => b.url.clone(),
                    _ => String::new(),
                }
            }),
        )?;
    }

    // ── Срез 4: кросс-фреймовый postMessage ──────────────────────────────────
    // Валидация targetOrigin + постановка в глобальный ящик. Выполняется на
    // JS-потоке ОТПРАВИТЕЛЯ; получатель разбирает ящик у себя на тике
    // (_lumen_frame_pump_messages), поэтому никаких перекрёстных вызовов
    // между изолятами нет.
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_post_message",
            into_v8_fn3(move |bid: u32, data_json: String, target_origin: String| -> bool {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                let Some(binding) = resolve_slot(&reg, bid) else {
                    return false;
                };
                // HTML LS §9.2.9 шаг 3: '*' доставляет всегда, '/' — только
                // same-origin (у нас это уже вычисленный shell'ом accessible),
                // явная строка — совпадение с origin адресата.
                let matches = match target_origin.as_str() {
                    "*" => true,
                    "/" | "" => binding.accessible,
                    o => o.eq_ignore_ascii_case(&binding_origin(
                        &binding.url,
                        &reg.self_origin,
                    )),
                };
                if !matches {
                    return false;
                }
                // Кто отправитель для получателя: постинг в фасад потомка —
                // сам отправитель его родитель; постинг в фасад предка
                // (PARENT_BID/TOP_BID) — отправитель потомок со своим ключом.
                // bid фасада отправителя разрешит получатель у себя (take):
                // в момент постановки реестр получателя недоступен.
                let source = match bid {
                    PARENT_BID | TOP_BID => SourceKind::ChildDoc(reg.self_key.unwrap_or(0)),
                    _ => SourceKind::Parent,
                };
                let target_doc = Arc::clone(&binding.doc);
                let outbox = frame_outbox();
                let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
                if outbox.len() >= FRAME_OUTBOX_CAP {
                    return false;
                }
                outbox.push(PendingFrameMessage {
                    target_doc,
                    source,
                    data_json,
                });
                true
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_take_messages",
            into_v8_fn0(move || -> String {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                let key = match reg.self_key {
                    Some(k) => k,
                    None => return String::new(),
                };
                let outbox = frame_outbox();
                let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
                let mut taken = Vec::new();
                let mut rest = Vec::new();
                for msg in outbox.drain(..) {
                    if Arc::as_ptr(&msg.target_doc) as usize == key {
                        taken.push(msg);
                    } else {
                        rest.push(msg);
                    }
                }
                *outbox = rest;
                if taken.is_empty() {
                    return String::new();
                }
                // event.origin и bid фасада отправителя считаются ЗДЕСЬ:
                // только реестр получателя знает свои слоты (source) и чем
                // наследуется origin about:-детей (srcdoc/about:blank →
                // origin получателя-родителя).
                let items: Vec<serde_json::Value> = taken
                    .into_iter()
                    .map(|m| {
                        let (source_bid, origin) = match m.source {
                            SourceKind::Parent => (
                                reg.parent.as_ref().map(|_| PARENT_BID),
                                reg.parent
                                    .as_ref()
                                    .map(|b| binding_origin(&b.url, &reg.self_origin))
                                    .unwrap_or_default(),
                            ),
                            SourceKind::ChildDoc(doc_key) => {
                                match reg
                                    .frames
                                    .iter()
                                    .position(|b| Arc::as_ptr(&b.doc) as usize == doc_key)
                                {
                                    Some(j) => {
                                        let b = &reg.frames[j];
                                        (
                                            Some(j as u32),
                                            binding_origin(&b.url, &reg.self_origin),
                                        )
                                    }
                                    // Отправителя нет в прямых слотах получателя
                                    // (внук → top): source = null, origin пустой.
                                    None => (None, String::new()),
                                }
                            }
                        };
                        serde_json::json!({
                            "bid": source_bid,
                            "origin": origin,
                            "data": serde_json::from_str::<serde_json::Value>(&m.data_json)
                                .unwrap_or(serde_json::Value::Null),
                        })
                    })
                    .collect();
                serde_json::to_string(&items).unwrap_or_else(|_| String::new())
            }),
        )?;
    }

    // ── Срезы 6–7: синтетические события через границу изолятов ──────────────
    // Родительский фасад вызвал click()/focus()/blur()/dispatchEvent():
    // постановка конверта в глобальный ящик событий. Выполняется на JS-потоке
    // ОТПРАВИТЕЛЯ; получатель разбирает ящик у себя на тике
    // (_lumen_frame_take_events рядом с take_messages). Правила те же, что у
    // нативов чтения/записи: нет биндинга, cross-origin / opaque (accessible:
    // false), не-элемент или nid за границей арены — тихий «нет» (false),
    // неотличимый для вызывающего JS; туда же попадает неразбираемая
    // спецификация конверта.
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_queue_event",
            into_v8_fn3(move |bid: u32, nid: u32, spec: String| -> bool {
                let kind = match parse_frame_event_spec(&spec) {
                    Some(k) => k,
                    None => return false,
                };
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                let Some(binding) = resolve_slot(&reg, bid) else {
                    return false;
                };
                if !binding.accessible {
                    return false;
                }
                let is_element = {
                    let doc = binding.doc.lock().unwrap_or_else(|e| e.into_inner());
                    checked_node(&doc, nid).is_some_and(|id| {
                        matches!(&doc.get(id).data, lumen_dom::NodeData::Element { .. })
                    })
                };
                if !is_element {
                    return false;
                }
                let outbox = frame_event_outbox();
                let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
                if outbox.len() >= FRAME_OUTBOX_CAP {
                    return false;
                }
                outbox.push(PendingFrameEvent {
                    target_doc: Arc::clone(&binding.doc),
                    nid,
                    kind,
                    source_doc: reg.self_key.unwrap_or(0),
                });
                true
            }),
        )?;
    }
    // ── Срез 10: обратная доставка ресурсных событий ─────────────────────────
    // Рёбро «ребёнок → родитель»: `_lumen_resource_fire` шима фрейма зовёт
    // глобальное зеркало (`_lumen_frame_mirror_resource`), оно — этот натив.
    // Постановка без bid: получатель сам разрешит биндинг отправителя по
    // `source_doc` (как `event.source` у postMessage).
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_queue_parent_resource",
            into_v8_fn2(move |nid: u32, ev_type: String| -> bool {
                queue_parent_resource(&reg, nid, ev_type)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_frame_take_events",
            into_v8_fn0(move || -> String {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                let key = match reg.self_key {
                    Some(k) => k,
                    None => return String::new(),
                };
                let outbox = frame_event_outbox();
                let mut outbox = outbox.lock().unwrap_or_else(|e| e.into_inner());
                let mut taken = Vec::new();
                let mut rest = Vec::new();
                for ev in outbox.drain(..) {
                    if Arc::as_ptr(&ev.target_doc) as usize == key {
                        taken.push(ev);
                    } else {
                        rest.push(ev);
                    }
                }
                *outbox = rest;
                if taken.is_empty() {
                    return String::new();
                }
                let items: Vec<serde_json::Value> = taken
                    .into_iter()
                    .filter_map(|ev| match ev.kind {
                        FrameEventKind::Click => Some(serde_json::json!({
                            "kind": "click", "nid": ev.nid,
                        })),
                        FrameEventKind::Focus { prevent_scroll } => Some(serde_json::json!({
                            "kind": "focus", "nid": ev.nid, "preventScroll": prevent_scroll,
                        })),
                        FrameEventKind::Blur => Some(serde_json::json!({
                            "kind": "blur", "nid": ev.nid,
                        })),
                        FrameEventKind::RunScript => Some(serde_json::json!({
                            "kind": "runscript", "nid": ev.nid,
                        })),
                        FrameEventKind::Resource { ev_type } => {
                            // Срез 10: bid отправителя разрешает реестр
                            // ПОЛУЧАТЕЛЯ. Нет биндинга или `accessible: false` —
                            // фасадов элементов у получателя нет, конверт
                            // отбрасывается здесь (фильтр доступности пары).
                            let sender_bid = reg.frames.iter().position(|b| {
                                Arc::as_ptr(&b.doc) as usize == ev.source_doc && b.accessible
                            })?;
                            Some(serde_json::json!({
                                "kind": "resource", "nid": ev.nid, "type": ev_type,
                                "bid": sender_bid as u32,
                            }))
                        }
                        FrameEventKind::Dom {
                            ev_type,
                            bubbles,
                            cancelable,
                            composed,
                            detail_json,
                        } => {
                            let detail = detail_json
                                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                                .unwrap_or(serde_json::Value::Null);
                            Some(serde_json::json!({
                                "kind": "dom", "nid": ev.nid,
                                "type": ev_type, "bubbles": bubbles,
                                "cancelable": cancelable, "composed": composed,
                                "detail": detail,
                            }))
                        }
                    })
                    .collect();
                serde_json::to_string(&items).unwrap_or_else(|_| String::new())
            }),
        )?;
    }

    // ── Document-level чтение ────────────────────────────────────────────────
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_body",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "body").map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_head",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "head").map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_document_element",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| d.document_element().map(|n| n.index() as u32), None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_title",
            into_v8_fn1(move |bid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    find_element_by_tag(d, "title")
                        .map(|nid| collect_text_content(d, nid))
                        .unwrap_or_default()
                }, String::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_by_id",
            into_v8_fn2(move |bid: u32, id: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    find_first_matching(d, d.root(), &|node| {
                        matches!(&node.data, lumen_dom::NodeData::Element { .. })
                            && node.get_attr("id") == Some(id.as_str())
                    })
                    .map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query",
            into_v8_fn2(move |bid: u32, sel: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all(d, &sel).into_iter().next().map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_all",
            into_v8_fn2(move |bid: u32, sel: String| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all(d, &sel)
                        .into_iter()
                        .map(|n| n.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }

    // ── Element-level чтение ─────────────────────────────────────────────────
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_scoped",
            into_v8_fn3(move |bid: u32, nid: u32, sel: String| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_index(nid as usize), &sel)
                        .into_iter()
                        .next()
                        .map(|n| n.index() as u32)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_query_all_scoped",
            into_v8_fn3(move |bid: u32, nid: u32, sel: String| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_index(nid as usize), &sel)
                        .into_iter()
                        .map(|n| n.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_children",
            into_v8_fn2(move |bid: u32, nid: u32| -> Vec<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .children
                        .iter()
                        .filter(|&&c| matches!(&d.get(c).data, lumen_dom::NodeData::Element { .. }))
                        .map(|&c| c.index() as u32)
                        .collect()
                }, Vec::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_parent_element",
            into_v8_fn2(move |bid: u32, nid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| {
                    let id = lumen_dom::NodeId::from_index(nid as usize);
                    d.get(id).parent.and_then(|pid| {
                        matches!(&d.get(pid).data, lumen_dom::NodeData::Element { .. })
                            .then(|| pid.index() as u32)
                    })
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_attr",
            into_v8_fn3(move |bid: u32, nid: u32, name: String| -> Option<String> {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .get_attr(&name)
                        .map(str::to_owned)
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_has_attr",
            into_v8_fn3(move |bid: u32, nid: u32, name: String| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    d.get(lumen_dom::NodeId::from_index(nid as usize))
                        .get_attr(&name)
                        .is_some()
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_tag",
            into_v8_fn2(move |bid: u32, nid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    match &d.get(lumen_dom::NodeId::from_index(nid as usize)).data {
                        lumen_dom::NodeData::Element { name, .. } => name.local.clone(),
                        _ => String::new(),
                    }
                }, String::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_is_text",
            into_v8_fn2(move |bid: u32, nid: u32| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    matches!(
                        &d.get(lumen_dom::NodeId::from_index(nid as usize)).data,
                        lumen_dom::NodeData::Text(_)
                    )
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_is_comment",
            into_v8_fn2(move |bid: u32, nid: u32| -> bool {
                with_accessible_doc(&reg, bid, |d| {
                    matches!(
                        &d.get(lumen_dom::NodeId::from_index(nid as usize)).data,
                        lumen_dom::NodeData::Comment(_)
                    )
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_text",
            into_v8_fn2(move |bid: u32, nid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    collect_text_content(d, lumen_dom::NodeId::from_index(nid as usize))
                }, String::new())
            }),
        )?;
    }

    // ── Срез 5: мутации под-документа ─────────────────────────────────────────
    // Все нативы записи проходят через with_accessible_doc_mut и проверяют
    // границы арены (checked_node): чужой/вышедший за пределы nid — тихий
    // no-op. Возврат «неудача» (false/-1/null) неотличим для JS от
    // отсутствия биндинга — конвенция бриджа «невалидно = пусто».
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_create_element",
            into_v8_fn2(move |bid: u32, tag: String| -> i32 {
                with_accessible_doc_mut(&reg, bid, |d| {
                    // -1 при переполнении арены (MAX_DOM_NODES); шим отдаёт null.
                    d.try_create_element(lumen_dom::QualName::html(tag.to_ascii_lowercase()))
                        .map(|n| n.index() as i32)
                        .unwrap_or(-1)
                }, -1)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_create_text",
            into_v8_fn2(move |bid: u32, data: String| -> i32 {
                with_accessible_doc_mut(&reg, bid, |d| {
                    d.try_create_text(data)
                        .map(|n| n.index() as i32)
                        .unwrap_or(-1)
                }, -1)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_set_attr",
            into_v8_fn4(move |bid: u32, nid: u32, name: String, value: String| -> bool {
                let ok = with_accessible_doc_mut(&reg, bid, |d| {
                    checked_node(d, nid).is_some_and(|id| {
                        let is_element = matches!(&d.get(id).data, lumen_dom::NodeData::Element { .. });
                        if is_element {
                            bridge_set_attr(d, id, &name, &value);
                        }
                        is_element
                    })
                }, false);
                // Срез 9: поздний src на `<script>` — тот же триггер подготовки
                // (HTML LS §4.12.1: изменение src перезапускает prepare), что и
                // вставка. Конверт идёт через тот же helper: он сам проверяет
                // доступность биндинга и тег; доставка no-op-ается у получателя
                // для уже начавшегося скрипта («already started»).
                if ok && name.eq_ignore_ascii_case("src") {
                    queue_run_script_if_script(&reg, bid, nid);
                }
                ok
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_remove_attr",
            into_v8_fn3(move |bid: u32, nid: u32, name: String| -> bool {
                with_accessible_doc_mut(&reg, bid, |d| {
                    checked_node(d, nid).is_some_and(|id| {
                        let is_element = matches!(&d.get(id).data, lumen_dom::NodeData::Element { .. });
                        if is_element {
                            bridge_remove_attr(d, id, &name);
                        }
                        is_element
                    })
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_append_child",
            into_v8_fn3(move |bid: u32, parent_nid: u32, child_nid: u32| -> bool {
                let ok = with_accessible_doc_mut(&reg, bid, |d| {
                    match (checked_node(d, parent_nid), checked_node(d, child_nid)) {
                        (Some(parent), Some(child)) => {
                            // DEVX-8a: потомок под собственного предка создаёт
                            // цикл в арене (в release у append_child нет
                            // защиты) — отклоняем до вызова.
                            if is_self_or_ancestor(d, child, parent) {
                                return false;
                            }
                            d.append_child(parent, child);
                            true
                        }
                        _ => false,
                    }
                }, false);
                // Срез 8: вставленный `<script>` исполняется ребёнком на его
                // тике (конверт RunScript), не в момент вставки — доставка
                // через границу изолятов у моста всегда асинхронная.
                if ok {
                    queue_run_script_if_script(&reg, bid, child_nid);
                }
                ok
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_insert_before",
            into_v8_fn3(move |bid: u32, node_nid: u32, ref_nid: u32| -> bool {
                let ok = with_accessible_doc_mut(&reg, bid, |d| {
                    match (checked_node(d, node_nid), checked_node(d, ref_nid)) {
                        (Some(node), Some(reference)) => {
                            // Спека: reference без родителя → pre-insert
                            // невалиден; цикл — тот же запрет DEVX-8a.
                            let parent = d.get(reference).parent;
                            match parent {
                                Some(p) if !is_self_or_ancestor(d, node, p) => {
                                    d.insert_before(node, reference);
                                    true
                                }
                                _ => false,
                            }
                        }
                        _ => false,
                    }
                }, false);
                if ok {
                    queue_run_script_if_script(&reg, bid, node_nid);
                }
                ok
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_remove_node",
            into_v8_fn2(move |bid: u32, nid: u32| -> bool {
                with_accessible_doc_mut(&reg, bid, |d| {
                    match checked_node(d, nid) {
                        Some(id) => {
                            d.detach(id);
                            true
                        }
                        None => false,
                    }
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_set_text",
            into_v8_fn3(move |bid: u32, nid: u32, text: String| -> bool {
                with_accessible_doc_mut(&reg, bid, |d| {
                    checked_node(d, nid)
                        .is_some_and(|id| bridge_set_text_content(d, id, &text))
                }, false)
            }),
        )?;
    }

    rt.eval(FRAME_BRIDGE_SHIM)?;
    Ok(())
}

/// JavaScript shim: фасады Window/Document/Element над нативами `_lumen_f_*`.
///
/// Точка входа для геттеров `iframe_element.rs` — две глобальные функции,
/// принимающие `__nid__` хоста; всё остальное спрятано в замыкании модуля.
///
/// Срез 3 добавляет: (1) переопределение `window.parent/top/frameElement/
/// length` геттерами, которые читают слоты предков реестра и пока тех нет
/// ведут себя как прежние константы из WEB_API_SHIM; (2) глобальный
/// `_lumen_frame_install_index(idx)`, который `register_frame_document`
/// вызывает после каждой регистрации — ставит на `window` индексный и
/// именованный доступники к окну фрейма. Исполняется строго после
/// WEB_API_SHIM (порядок в `install_dom`), поэтому `window` уже существует
/// в проде; в минимальных тестовых изолятах блок пропускается через typeof.
#[cfg(feature = "v8-backend")]
const FRAME_BRIDGE_SHIM: &str = r#"(function() {
  'use strict';

  // Псевдо-bid слотов предков (зеркало frame_bridge::PARENT_BID/TOP_BID).
  var PARENT_BID = 0xFFFFFFFF;
  var TOP_BID = 0xFFFFFFFE;

  // Интерны фасадов, ключ — bid (стабильный индекс биндинга). Живут столько
  // же, сколько контекст страницы: identity фасадов обязана быть постоянной.
  var wins = {};
  var docs = {};
  var elems = {};

  // Срез 10: слушатели фасадных элементов (WeakMap фасад → {тип: [fn]}).
  // Фасады не настоящие EventTarget, поэтому карта своя; доставку в неё делает
  // ветка 'resource' пумпы (обратные ресурсные события ребёнка). Ключ — сам
  // интернированный объект фасада, так что identity-зависимость та же, что у
  // самих фасадов.
  var elemListeners = new WeakMap();

  function facadeOn(fel, type, fn, add) {
    var map = elemListeners.get(fel);
    if (!map) { map = {}; elemListeners.set(fel, map); }
    var list = map[type];
    if (!list) { list = []; map[type] = list; }
    if (add) {
      if (list.indexOf(fn) === -1) list.push(fn);
    } else {
      var i = list.indexOf(fn);
      if (i !== -1) list.splice(i, 1);
    }
  }

  function bidOrNull(hostNid) {
    if (hostNid === null || hostNid === undefined) return null;
    var bid = _lumen_frame_binding(hostNid);
    return (bid === null || bid === undefined) ? null : bid;
  }

  function isAncestorBid(bid) { return bid === PARENT_BID || bid === TOP_BID; }

  // Срез 5: аргумент-узел мутации — фасад ТОГО ЖЕ биндинга с числовым __nid__.
  function isBridgeNode(bid, n) {
    return n !== null && n !== undefined && typeof n === 'object'
      && n.__bid__ === bid && typeof n.__nid__ === 'number';
  }

  // appendChild/insertBefore над нативами записи. ref === null/undefined —
  // спечный синоним appendChild (insertBefore(node, null)); чужой фасад или
  // отклонённая нативом вставка — тихий no-op (undefined вместо узла).
  function bridgeInsert(bid, parentNid, child, ref) {
    if (!isBridgeNode(bid, child)) return undefined;
    var ok;
    if (ref === null || ref === undefined) {
      ok = _lumen_f_append_child(bid, parentNid, child.__nid__);
    } else if (isBridgeNode(bid, ref)) {
      ok = _lumen_f_insert_before(bid, child.__nid__, ref.__nid__);
    } else {
      ok = false;
    }
    return ok ? child : undefined;
  }

  // Разрешение top-окна текущего КОНТЕКСТА (не фасада): отдельный слот top,
  // иначе слот parent (фрейм 1-го уровня), иначе сам window.
  function topOfContext() {
    if (typeof window === 'undefined') return null;
    var t = _lumen_top_binding();
    if (t !== null && t !== undefined) return winFacade(t);
    var p = _lumen_parent_binding();
    if (p !== null && p !== undefined) return winFacade(p);
    return window;
  }

  // Срез 9: база под-документа для URL-рефлексий фасадов — href первого
  // <base>, разрешённый против URL документа, иначе сам URL (HTML LS §4.2.3,
  // тот же алгоритм, что _lumen_document_base_url главного шима, но над
  // деревом ЧУЖОГО документа через нативы бриджа). `_url_resolve` живёт в
  // WEB_API_SHIM и есть во всех прод-контекстах; минимальные тестовые изоляты
  // без шима получают сырые значения атрибутов.
  function frameBase(bid) {
    var url = '';
    try { url = String(_lumen_f_url(bid) || ''); } catch (e) {}
    try {
      var b = _lumen_f_query(bid, 'base');
      if (b !== null && b !== undefined && typeof _url_resolve === 'function') {
        var h = _lumen_f_attr(bid, b, 'href');
        if (h) {
          var r = _url_resolve(String(h), url);
          if (r) return r;
        }
      }
    } catch (e) {}
    return url;
  }

  // Срез 9: URL-рефлексия фасада (HTML LS §2.6.2 «Reflecting content
  // attributes»): геттер отдаёт '' при отсутствующем атрибуте, иначе значение,
  // разрешённое против базы под-документа; сеттер хранит строку дословно.
  // Запись идёт через натив записи атрибута, поэтому поздний `script.src = …`
  // сам ставит конверт RunScript (натив _lumen_f_set_attr, срез 9).
  function frameUrlReflected(bid, nid, attr) {
    var v = _lumen_f_attr(bid, nid, attr);
    if (v === null || v === undefined || String(v) === '') return '';
    if (typeof _url_resolve !== 'function') return String(v);
    return _url_resolve(String(v), frameBase(bid)) || String(v);
  }

  function frameElem(bid, nid) {
    if (nid === null || nid === undefined || nid < 0) return null;
    var cache = elems[bid];
    if (!cache) { cache = {}; elems[bid] = cache; }
    var cached = cache[nid];
    if (cached) return cached;
    var el = {
      __bid__: bid,
      __nid__: nid,
      get nodeType() {
        if (_lumen_f_is_text(bid, nid)) return 3;
        if (_lumen_f_is_comment(bid, nid)) return 8;
        return 1;
      },
      get localName()  { return _lumen_f_tag(bid, nid); },
      get tagName()    { var t = _lumen_f_tag(bid, nid); return t ? t.toUpperCase() : t; },
      get nodeName()   { var t = _lumen_f_tag(bid, nid); return t ? t.toUpperCase() : t; },
      get id()         { var v = _lumen_f_attr(bid, nid, 'id'); return v !== null && v !== undefined ? v : ''; },
      set id(v)        { _lumen_f_set_attr(bid, nid, 'id', String(v)); },
      get className()  { var v = _lumen_f_attr(bid, nid, 'class'); return v !== null && v !== undefined ? v : ''; },
      set className(v) { _lumen_f_set_attr(bid, nid, 'class', String(v)); },
      // Срез 9: URL-рефлексии src/href (script/img/iframe/a/link/…). Ставятся
      // на каждый элемент безусловно — как в главном шиме (BUG-450 про
      // безусловные canvas-члены там отдельная тема); сеттер хранит дословно,
      // геттер разрешает против базы под-документа.
      get src()        { return frameUrlReflected(bid, nid, 'src'); },
      set src(v)       { _lumen_f_set_attr(bid, nid, 'src', String(v)); },
      get href()       { return frameUrlReflected(bid, nid, 'href'); },
      set href(v)      { _lumen_f_set_attr(bid, nid, 'href', String(v)); },
      getAttribute: function(n) { return _lumen_f_attr(bid, nid, String(n)); },
      hasAttribute: function(n) { return _lumen_f_has_attr(bid, nid, String(n)); },
      get children() { return _lumen_f_children(bid, nid).map(function(c) { return frameElem(bid, c); }); },
      get childElementCount() { return _lumen_f_children(bid, nid).length; },
      get firstElementChild() { return frameElem(bid, _lumen_f_children(bid, nid)[0]); },
      get lastElementChild() {
        var ch = _lumen_f_children(bid, nid);
        return frameElem(bid, ch[ch.length - 1]);
      },
      get parentElement() { return frameElem(bid, _lumen_f_parent_element(bid, nid)); },
      querySelector: function(sel) { return frameElem(bid, _lumen_f_query_scoped(bid, nid, String(sel))); },
      querySelectorAll: function(sel) {
        return _lumen_f_query_all_scoped(bid, nid, String(sel)).map(function(c) { return frameElem(bid, c); });
      },
      // BUG-480 срез 5: запись через фасад. Аргументы-узлы обязаны быть
      // фасадами ТОГО ЖЕ биндинга (чужой документ — тихий no-op); removeChild
      // снимает ребёнка с фактического родителя в дереве (как у главного
      // документа), без проверки иерархии — отклонение задокументировано.
      setAttribute: function(n, v) { _lumen_f_set_attr(bid, nid, String(n), String(v)); },
      removeAttribute: function(n) { _lumen_f_remove_attr(bid, nid, String(n)); },
      appendChild: function(c) { return bridgeInsert(bid, nid, c, null); },
      insertBefore: function(c, ref) { return bridgeInsert(bid, nid, c, ref); },
      removeChild: function(c) {
        if (!isBridgeNode(bid, c)) return undefined;
        _lumen_f_remove_node(bid, c.__nid__);
        return c;
      },
      remove: function() { _lumen_f_remove_node(bid, nid); },
      get textContent() { return _lumen_f_text(bid, nid); },
      set textContent(v) { _lumen_f_set_text(bid, nid, String(v)); },
      // BUG-480 срезы 6–7: действия над элементом под-документа. Все они
      // асинхронные: конверт уходит в ящик событий, ребёнок на своём тике
      // исполняет СОБСТВЕННУЮ семантику вызова (хуки _lumen_deliver_frame_*
      // из WEB_API_SHIM). Не-элемент, чужой/недоступный биндинг, неразбираемая
      // спецификация и переполненный ящик — тихий no-op (конвенция бриджа
      // «невалидно = пусто»).
      click: function() {
        var t = _lumen_f_tag(bid, nid);
        if (t) _lumen_f_queue_event(bid, nid, '{"kind":"click"}');
      },
      focus: function(options) {
        var t = _lumen_f_tag(bid, nid);
        if (!t) return;
        _lumen_f_queue_event(bid, nid, JSON.stringify({
          kind: 'focus',
          preventScroll: !!(options && options.preventScroll),
        }));
      },
      blur: function() {
        var t = _lumen_f_tag(bid, nid);
        if (t) _lumen_f_queue_event(bid, nid, '{"kind":"blur"}');
      },
      // Срез 7: произвольное событие через границу. Аргумент — Event-подобный
      // объект ИЗОЛЯТА ОТПРАВИТЕЛЯ: переносится только его снимок
      // type/bubbles/cancelable/composed + detail (JSON-круготрип), живых
      // ссылок через границу не идёт. Возвращаемое значение спеки
      // (!defaultPrevented) при асинхронной доставке недостижимо — метод
      // ничего не возвращает.
      dispatchEvent: function(evt) {
        var t = _lumen_f_tag(bid, nid);
        if (!t || !evt || typeof evt !== 'object') return;
        var type = typeof evt.type === 'string' ? evt.type : String(evt.type);
        if (!type) return;
        var spec = {
          kind: 'dom',
          type: type,
          bubbles: !!evt.bubbles,
          cancelable: !!evt.cancelable,
          composed: !!evt.composed,
        };
        if (evt.detail !== undefined && evt.detail !== null) spec.detail = evt.detail;
        _lumen_f_queue_event(bid, nid, JSON.stringify(spec));
      },
      // Срез 10: слушатели фасада. Доставку в них делает ветка 'resource'
      // пумпы (ресурсные события ребёнка: load/error внешнего скрипта и т.п.);
      // порядок вызова — сначала слушатели, затем свойство on<type>, как у
      // локального диспатча шима.
      addEventListener: function(type, fn) {
        if (typeof type === 'string' && type && typeof fn === 'function') {
          facadeOn(el, type, fn, true);
        }
      },
      removeEventListener: function(type, fn) {
        if (typeof type === 'string' && type && typeof fn === 'function') {
          facadeOn(el, type, fn, false);
        }
      },
      // BUG-480 срез 2: содержимое фрейма не layout'ится — честные нули вместо
      // выдуманных размеров (layout фреймов — будущий срез).
      get offsetWidth()  { return 0; },
      get offsetHeight() { return 0; },
      get clientWidth()  { return 0; },
      get clientHeight() { return 0; },
      get scrollWidth()  { return 0; },
      get scrollHeight() { return 0; },
      getBoundingClientRect: function() {
        return { x: 0, y: 0, width: 0, height: 0, top: 0, left: 0, right: 0, bottom: 0 };
      },
    };
    cache[nid] = el;
    return el;
  }

  function docFacade(bid) {
    var cached = docs[bid];
    if (cached) return cached;
    var d = {};
    function el(nid) { return frameElem(bid, nid); }
    Object.defineProperty(d, 'body',              { get: function() { return el(_lumen_f_body(bid)); }, configurable: true });
    Object.defineProperty(d, 'head',              { get: function() { return el(_lumen_f_head(bid)); }, configurable: true });
    Object.defineProperty(d, 'documentElement',   { get: function() { return el(_lumen_f_document_element(bid)); }, configurable: true });
    Object.defineProperty(d, 'title',             { get: function() { return _lumen_f_title(bid); }, configurable: true });
    // Срез 5: title записывается в существующий <title>, а без него —
    // создаётся и вставляется в <head> (HTML LS §4.2.2); совсем без head —
    // тихий no-op.
    Object.defineProperty(d, 'title', {
      set: function(v) {
        var t = _lumen_f_query(bid, 'title');
        if (t === null || t === undefined) {
          var h = el(_lumen_f_head(bid));
          if (h === null || h === undefined) return;
          var i = _lumen_f_create_element(bid, 'title');
          if (i === null || i === undefined || i < 0) return;
          h.appendChild(frameElem(bid, i));
        }
        d.querySelector('title').textContent = String(v);
      },
      configurable: true,
    });
    Object.defineProperty(d, 'URL',               { get: function() { return _lumen_f_url(bid); }, configurable: true });
    Object.defineProperty(d, 'documentURI',       { get: function() { return _lumen_f_url(bid); }, configurable: true });
    // Ребёнок получил window load ещё в срезе 1 — readyState к моменту доступа
    // всегда «complete»; отдельного трекинга переходов срез 2 не ведёт.
    Object.defineProperty(d, 'readyState',        { get: function() { return 'complete'; }, configurable: true });
    Object.defineProperty(d, 'defaultView',       { get: function() { return winFacade(bid); }, configurable: true });
    d.getElementById = function(id) { return el(_lumen_f_by_id(bid, String(id))); };
    d.querySelector = function(sel) { return el(_lumen_f_query(bid, String(sel))); };
    d.querySelectorAll = function(sel) {
      return _lumen_f_query_all(bid, String(sel)).map(function(n) { return frameElem(bid, n); });
    };
    // Срез 5: фабрики узлов под-документа. Отрицательный ответ натива
    // (переполнение арены) → null, как у главного документа.
    d.createElement = function(tag) {
      var i = _lumen_f_create_element(bid, String(tag));
      return (i !== null && i !== undefined && i >= 0) ? frameElem(bid, i) : null;
    };
    d.createTextNode = function(data) {
      var i = _lumen_f_create_text(bid, String(data));
      return (i !== null && i !== undefined && i >= 0) ? frameElem(bid, i) : null;
    };
    docs[bid] = d;
    return d;
  }

  function winFacade(bid) {
    var cached = wins[bid];
    if (cached) return cached;
    var w = {};
    var hostNid = _lumen_f_accessible(bid) ? _lumen_f_host(bid) : null;
    Object.defineProperty(w, 'document', {
      get: function() { return _lumen_f_accessible(bid) ? docFacade(bid) : null; },
      configurable: true,
    });
    w.window = w;
    w.self = w;
    w.frames = w;
    // parent/top зависят от того, ЧЕЙ фасад читают и откуда. Фасад предка
    // (PARENT_BID/TOP_BID в изоляте ребёнка) сам себе parent/top: контекст,
    // в котором он построен, — его потомок. Фасад дочернего фрейма, читаемый
    // из родителя, отсылает к настоящему окну читающего контекста (срез 2).
    Object.defineProperty(w, 'parent', {
      get: function() {
        if (isAncestorBid(bid)) return w;
        return typeof window !== 'undefined' ? window : null;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'top', {
      get: function() {
        if (isAncestorBid(bid)) return w;
        return topOfContext();
      },
      configurable: true,
    });
    Object.defineProperty(w, 'closed', { get: function() { return false; }, configurable: true });
    // Счётчик фреймов чужого изолята недоступен — у фасадов честный ноль;
    // живой length есть только у настоящего window (ниже).
    w.length = 0;
    Object.defineProperty(w, 'frameElement', {
      get: function() {
        if (!_lumen_f_accessible(bid)) return null;
        // Хост фрейма лежит в документе родителя; для фасада ПРЕДКА
        // (читается из изолята ребёнка) элемент строится нативами бриджа
        // над документом родителя, для обычного фасада — обычным враппером
        // текущего документа (хост и читающий код живут в одном дереве).
        if (isAncestorBid(bid)) return frameElem(bid, _lumen_f_host(bid));
        return (hostNid !== null && typeof _lumen_make_element === 'function')
          ? _lumen_make_element(hostNid)
          : null;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'name', {
      get: function() {
        if (hostNid === null) return '';
        // Аналогично frameElement: атрибут хоста предка читаем через бридж.
        var a = isAncestorBid(bid)
          ? _lumen_f_attr(bid, hostNid, 'name')
          : _lumen_get_attr(hostNid, 'name');
        return (a === null || a === undefined) ? '' : a;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'location', {
      get: function() {
        var href = _lumen_f_url(bid);
        return { href: href, toString: function() { return href; } };
      },
      configurable: true,
    });
    w.close = function() {};
    // Срез 4: postMessage на WindowProxy. Данные уходят JSON-круготрипом;
    // функции/символы клонировать нельзя — DataCloneError (TypeError там,
    // где DOMException ещё не установлен). Опущенный targetOrigin = '/'
    // (спечный дефолт), т.е. доставка только same-origin.
    w.postMessage = function(message, targetOrigin) {
      if (typeof message === 'function' || typeof message === 'symbol') {
        throw (typeof DOMException === 'function')
          ? new DOMException('object could not be cloned', 'DataCloneError')
          : new TypeError('object could not be cloned');
      }
      var json = (message === undefined) ? 'null' : JSON.stringify(message);
      if (typeof json !== 'string') {
        throw (typeof DOMException === 'function')
          ? new DOMException('object could not be cloned', 'DataCloneError')
          : new TypeError('object could not be cloned');
      }
      var to = (targetOrigin === undefined || targetOrigin === null) ? '/' : String(targetOrigin);
      _lumen_f_post_message(bid, json, to);
    };
    wins[bid] = w;
    return w;
  }

  globalThis._lumen_frame_content_document = function(hostNid) {
    var bid = bidOrNull(hostNid);
    if (bid === null || !_lumen_f_accessible(bid)) return null;
    return docFacade(bid);
  };
  globalThis._lumen_frame_content_window = function(hostNid) {
    var bid = bidOrNull(hostNid);
    if (bid === null) return null;
    return winFacade(bid);
  };

  // ── Срез 3: иерархия окон настоящего window этого контекста ──────────────
  // Установщики вызываются ЛЕНИВО из register_parent_document/
  // register_frame_document: пока фреймов нет, глобальные свойства контекста
  // остаются ровно теми, что поставил WEB_API_SHIM (`window.parent = window`,
  // `length = 0`). Это критично: инсталлтайм-акцессор на parent/top/length
  // ломает топ-левел `var parent = …` страниц и тестов (V8 не подменяет
  // существующий акцессор var-объявлением). Первый же вызов регистрации
  // меняет свойства; попытка поверх НЕконфигурируемого var-биндинга
  // пользователя молча пропускается try/catch — деградация до прежнего
  // значения.

  var __hierarchyInstalled = false;
  var __lengthInstalled = false;

  function installLengthAccessor() {
    if (__lengthInstalled) return;
    __lengthInstalled = true;
    try {
      Object.defineProperty(window, 'length', {
        get: function() { return _lumen_frame_count(); },
        configurable: true,
      });
    } catch (e) {}
  }

  function installHierarchyAccessors() {
    if (__hierarchyInstalled) return;
    __hierarchyInstalled = true;
    var __prevName = window.name;
    try {
      Object.defineProperty(window, 'parent', {
        get: function() {
          var p = _lumen_parent_binding();
          return (p !== null && p !== undefined) ? winFacade(p) : window;
        },
        configurable: true,
      });
    } catch (e) {}
    try {
      Object.defineProperty(window, 'top', {
        get: function() { return topOfContext(); },
        configurable: true,
      });
    } catch (e) {}
    try {
      Object.defineProperty(window, 'frameElement', {
        get: function() {
          var p = _lumen_parent_binding();
          return (p !== null && p !== undefined && _lumen_f_accessible(p))
            ? winFacade(p).frameElement
            : null;
        },
        configurable: true,
      });
    } catch (e) {}
    // window.name фрейма — атрибут name хоста (HTML LS §7.2.3); явное
    // присваивание перекрывает атрибут до замены документа.
    var __customName = null;
    try {
      Object.defineProperty(window, 'name', {
        get: function() {
          if (__customName !== null) return __customName;
          var p = _lumen_parent_binding();
          if (p === null || p === undefined || !_lumen_f_accessible(p)) {
            return __prevName === undefined ? '' : __prevName;
          }
          var host = _lumen_f_host(p);
          if (host === null || host === undefined) return '';
          var a = _lumen_f_attr(p, host, 'name');
          return (a === null || a === undefined) ? '' : a;
        },
        set: function(v) { __customName = String(v == null ? '' : v); },
        configurable: true,
      });
    } catch (e) {}
  }

  // Родитель зарегистрирован: включить parent/top/frameElement/name.
  globalThis._lumen_frame_install_hierarchy = function() {
    if (typeof window === 'undefined') return;
    installHierarchyAccessors();
  };

  // Фрейм зарегистрирован: индексный (`window[idx]`) + именованный
  // (`window[имя]`) доступники окна фрейма; живой length. Вызывается из
  // V8JsRuntime::register_frame_document с индексом биндинга; порядок
  // регистрации = порядок документа (спечный tree order). Именованный
  // доступ покрывает ТОЛЬКО iframe (embed/form/img/object — не бриджевая
  // территория).
  globalThis._lumen_frame_install_index = function(idx) {
    if (typeof window === 'undefined') return;
    installLengthAccessor();
    try {
      var host = _lumen_frame_host_at(idx);
      if (host === null || host === undefined) return;
      var mk = function(h) {
        return function() { return _lumen_frame_content_window(h); };
      };
      Object.defineProperty(window, String(idx), { get: mk(host), configurable: true });
      var nm = _lumen_frame_name_at(idx);
      if (nm) {
        Object.defineProperty(window, nm, { get: mk(host), configurable: true });
      }
    } catch (e) {}
  };

  // ── Срез 4: разбор ящика кросс-фреймовых postMessage ──────────────────────
  // Shell вызывает на каждом тике (pump_frame_messages) и у страницы, и у
  // каждого фрейма. Натив отдаёт JSON-массив сообщений, адресованных ЭТОМУ
  // контексту; каждое разворачивается в MessageEvent и доставляется через
  // хук из WEB_API_SHIM (window.onmessage + addEventListener('message')).
  globalThis._lumen_frame_pump_messages = function() {
    if (typeof window === 'undefined') return;
    if (typeof _lumen_deliver_frame_message === 'function') {
      var raw = _lumen_frame_take_messages();
      if (raw) {
        var msgs;
        try { msgs = JSON.parse(raw); } catch (e) { msgs = null; }
        if (msgs && msgs.length) {
          for (var i = 0; i < msgs.length; i++) {
            var m = msgs[i];
            var source = null;
            if (m.bid !== null && m.bid !== undefined) {
              source = winFacade(m.bid);
            }
            _lumen_deliver_frame_message(m.data, m.origin, source);
          }
        }
      }
    }
    // ── Срезы 6–7: синтетические события из чужих фасадов ───────────────────
    // Доставка ТОЛЬКО при наличии хука из WEB_API_SHIM: в минимальных
    // тестовых изолятах хука нет, транспорт проверяется переопределением
    // хуков в тестах. Ошибка одного конверта не отменяет разбор остальных.
    var rawEv = _lumen_frame_take_events();
    if (rawEv) {
      var evs;
      try { evs = JSON.parse(rawEv); } catch (e) { evs = null; }
      if (evs) {
        for (var j = 0; j < evs.length; j++) {
          var env = evs[j];
          if (!env || typeof env.nid !== 'number') continue;
          try {
            if (env.kind === 'click') {
              if (typeof _lumen_deliver_frame_click === 'function') {
                _lumen_deliver_frame_click(env.nid);
              }
            } else if (env.kind === 'focus' && typeof _lumen_deliver_frame_focus === 'function') {
              _lumen_deliver_frame_focus(env.nid, !!env.preventScroll);
            } else if (env.kind === 'blur' && typeof _lumen_deliver_frame_blur === 'function') {
              _lumen_deliver_frame_blur(env.nid);
            } else if (env.kind === 'dom' && typeof _lumen_deliver_frame_dom_event === 'function') {
              _lumen_deliver_frame_dom_event(env.nid, env);
            } else if (env.kind === 'runscript' &&
                       typeof _lumen_deliver_frame_run_script === 'function') {
              _lumen_deliver_frame_run_script(env.nid);
            } else if (env.kind === 'resource') {
              // ── Срез 10: ресурсное событие ребёнка в обработчики фасада ────
              // Разбирается ЗДЕСЬ, а не хуком WEB_API_SHIM: получатели —
              // интернированные фасады и их карта слушателей, они приватны для
              // этого замыкания. bid отправителя ставит натив разбора (реестр
              // ПОЛУЧАТЕЛЯ, фильтр доступности прошёл уже там). Event строится
              // заново в этом изоляте; порядок вызова — слушатели фасада,
              // затем свойство on<type>. В минимальных изолятах без Event
              // деградирует до plain-object снимка.
              if ((env.bid !== null && env.bid !== undefined) &&
                  typeof env.type === 'string' && env.type) {
                var fel = frameElem(env.bid, env.nid);
                if (fel) {
                  var rev = null;
                  try {
                    rev = new Event(env.type, { bubbles: false, cancelable: false });
                  } catch (e2) {}
                  if (!rev) rev = { type: env.type, bubbles: false, cancelable: false };
                  rev.target = fel;
                  rev.currentTarget = fel;
                  try { rev.isTrusted = true; } catch (e3) {}
                  var map = elemListeners.get(fel);
                  var list = map ? map[env.type] : null;
                  if (list && list.length) {
                    var snap = list.slice();
                    for (var k = 0; k < snap.length; k++) {
                      try { snap[k].call(fel, rev); } catch (err) {
                        if (typeof _lumen_report_exception === 'function') {
                          try { _lumen_report_exception(err); } catch (e4) {}
                        }
                      }
                    }
                  }
                  var oh = fel['on' + env.type];
                  if (typeof oh === 'function') {
                    try { oh.call(fel, rev); } catch (err2) {
                      if (typeof _lumen_report_exception === 'function') {
                        try { _lumen_report_exception(err2); } catch (e5) {}
                      }
                    }
                  }
                }
              }
            }
          } catch (e) {}
        }
      }
    }
  };

  // ── Срез 10: зеркало ресурсных событий в родительский изолят ──────────────
  // Вызывается из `_lumen_resource_fire` (WEB_API_SHIM) после локального
  // диспатча. Гейт «у контекста есть родитель» стоит внутри натива
  // (`_lumen_f_queue_parent_resource` читает слот parent): топ-страница
  // платит один пустой вызов натива на событие и не ставит конвертов.
  // Возврат — поставлен ли конверт (для тестов).
  globalThis._lumen_frame_mirror_resource = function(nid, type) {
    if (typeof nid !== 'number' || nid < 0) return false;
    if (typeof type !== 'string' || !type || type.length > 128) return false;
    if (typeof _lumen_f_queue_parent_resource !== 'function') return false;
    try { return !!_lumen_f_queue_parent_resource(nid, type); } catch (e) { return false; }
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    /// Рантайм с установленным бриджем и одним биндингом.
    ///
    /// `html` парсится как полный под-документ; `accessible=false` моделирует
    /// cross-origin/opaque-sandbox фрейм.
    fn with_frame(html: &str, accessible: bool, f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        // Прод-контекст всегда имеет window (WEB_API_SHIM исполняется раньше
        // бриджа); тестовый изолят объявляет его ДО установки, чтобы блок
        // переопределения window.parent/top/… в шиме отработал так же.
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let doc = lumen_html_parser::parse(html);
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::new(Mutex::new(doc)),
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible,
        });
        f(&rt);
    }

    /// Рантайм с бриджем и пустым реестром — моделирует страницу без
    /// загруженных фреймов.
    fn with_empty_registry(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, registry).unwrap();
        f(&rt);
    }

    /// Контекст фрейма: реестр со слотом родителя (и опционально верха),
    /// моделирующий JS-изолят загруженного `<iframe>`.
    ///
    /// `parent_html` — документ родителя (читается ребёнком при доступном
    /// мосте); `top_html` — отдельный документ верха для глубины 2. При
    /// `top_html == None` слот top не заполняется: у фрейма первого уровня
    /// `top === parent` разрешается через [`PARENT_BID`].
    ///
    /// После заполнения слотов вызывается `_lumen_frame_install_hierarchy` —
    /// тот же шаг, что делает `register_parent_document` в проде (шим
    /// ставит геттеры parent/top/frameElement/name лениво, см. шим).
    fn with_child_context(
        parent_html: &str,
        top_html: Option<&str>,
        host_nid: u32,
        accessible: bool,
        f: impl FnOnce(&V8JsRuntime),
    ) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        {
            let mut reg = registry.lock().unwrap();
            reg.parent = Some(FrameDocBinding {
                host_nid,
                doc: Arc::new(Mutex::new(lumen_html_parser::parse(parent_html))),
                url: "https://parent.example/".to_owned(),
                name: Some("hostframe".to_owned()),
                accessible,
            });
            if let Some(top) = top_html {
                reg.top = Some(FrameDocBinding {
                    host_nid: 0,
                    doc: Arc::new(Mutex::new(lumen_html_parser::parse(top))),
                    url: "https://top.example/".to_owned(),
                    name: None,
                    accessible,
                });
            }
        }
        rt.eval(
            "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
        )
        .unwrap();
        f(&rt);
    }

    fn eval_bool(rt: &V8JsRuntime, expr: &str) -> bool {
        matches!(rt.eval(expr).unwrap(), JsValue::Bool(true))
    }

    #[test]
    fn unbound_iframe_gives_null_for_both_getters() {
        with_empty_registry(|rt| {
            assert!(eval_bool(
                rt,
                "_lumen_frame_content_window(7) === null && _lumen_frame_content_document(7) === null"
            ));
        });
    }

    #[test]
    fn content_document_facade_reads_child_tree() {
        with_frame(
            "<html><body><div id='a' class='x'>hello</div></body></html>",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d !== null && d.body.tagName === 'BODY' && d.body.nodeType === 1"
                ));
                assert!(eval_bool(rt, "_lumen_frame_content_document(7).title === ''"));
                assert!(eval_bool(
                    rt,
                    "var e = _lumen_frame_content_document(7).getElementById('a'); \
                     e.localName === 'div' && e.id === 'a' && e.className === 'x' \
                     && e.textContent === 'hello' && e.getAttribute('class') === 'x'"
                ));
                assert!(eval_bool(
                    rt,
                    "var all = _lumen_frame_content_document(7).querySelectorAll('div'); \
                     all.length === 1 && all[0].id === 'a'"
                ));
                assert!(eval_bool(
                    rt,
                    "_lumen_frame_content_document(7).querySelector('body').tagName === 'BODY' \
                     && _lumen_frame_content_document(7).querySelector('nothing') === null"
                ));
            },
        );
    }

    #[test]
    fn facades_are_interned_and_cross_linked() {
        with_frame("<html><body></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "var w1 = _lumen_frame_content_window(7), w2 = _lumen_frame_content_window(7); \
                 w1 === w2"
            ));
            assert!(eval_bool(
                rt,
                "var d1 = _lumen_frame_content_document(7), d2 = _lumen_frame_content_document(7); \
                 d1 === d2 && w1.document === d1 && d1.defaultView === w1"
            ));
            assert!(eval_bool(
                rt,
                "w1.window === w1 && w1.self === w1 && w1.frames === w1 \
                 && w1.parent === window && w1.top === window && w1.closed === false"
            ));
            assert!(eval_bool(
                rt,
                "var b1 = d1.body, b2 = d1.body; b1 === b2 && b1.parentElement === d1.documentElement"
            ));
        });
    }

    #[test]
    fn inaccessible_frame_hides_document_but_keeps_window() {
        with_frame("<html><body><p>secret</p></body></html>", false, |rt| {
            assert!(eval_bool(rt, "_lumen_frame_content_document(7) === null"));
            assert!(eval_bool(
                rt,
                "var w = _lumen_frame_content_window(7); \
                 w !== null && w.document === null && w.location.href === ''"
            ));
        });
    }

    #[test]
    fn unknown_host_returns_null_without_touching_registry() {
        with_frame("<html><body></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "_lumen_frame_content_window(99) === null && _lumen_frame_content_document(99) === null"
            ));
        });
    }

    #[test]
    fn element_children_and_parent_walk_child_tree_only() {
        with_frame(
            "<html><body><ul><li>one</li><li>two</li></ul></body></html>",
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var ul = _lumen_frame_content_document(7).querySelector('ul'); \
                     ul.children.length === 2 && ul.children[0].textContent === 'one' \
                     && ul.firstElementChild.textContent === 'one' \
                     && ul.lastElementChild.textContent === 'two' \
                     && ul.children[0].parentElement === ul \
                     && ul.parentElement.tagName === 'BODY'"
                ));
            },
        );
    }

    #[test]
    fn geometry_is_zero_until_frame_layout_lands() {
        with_frame("<html><body><p>x</p></body></html>", true, |rt| {
            assert!(eval_bool(
                rt,
                "var b = _lumen_frame_content_document(7).body; \
                 b.offsetWidth === 0 && b.getBoundingClientRect().width === 0"
            ));
        });
    }

    // ── Срез 3: иерархия окон ─────────────────────────────────────────────────

    #[test]
    fn top_level_context_keeps_self_parent_and_top() {
        // Иерархия включена (как после первой регистрации в любом контексте),
        // но слотов предков нет: геттеры обязаны воспроизводить прежние
        // константы WEB_API_SHIM — parent/top = self, frameElement = null,
        // length = число фреймов (0).
        with_empty_registry(|rt| {
            rt.eval(
                "typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()",
            )
            .unwrap();
            assert!(eval_bool(rt, "typeof _lumen_frame_install_hierarchy === 'function'"));
            assert!(eval_bool(rt, "window.parent === window"), "parent fallback");
            assert!(eval_bool(rt, "window.top === window"), "top fallback");
            assert!(eval_bool(rt, "window.frameElement === null"), "frameElement fallback");
            // length ставит только регистрация фрейма (_lumen_frame_install_index):
            // здесь фреймов нет — геттера ещё нет, в проде статический 0 из
            // WEB_API_SHIM, в минимальном изоляте undefined.
            assert!(eval_bool(rt, "window.length === undefined"), "length not yet installed");
        });
    }

    #[test]
    fn child_window_parent_reads_parent_document() {
        with_child_context(
            "<html><body><div id='p'>parent</div></body></html>",
            None,
            42,
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "window.parent !== window \
                     && window.top === window.parent \
                     && window.parent.document !== null \
                     && window.parent.document.getElementById('p').textContent === 'parent' \
                     && window.parent.location.href === 'https://parent.example/'"
                ));
            },
        );
    }

    #[test]
    fn child_parent_facade_is_interned_and_self_referential() {
        with_child_context("<html><body></body></html>", None, 42, true, |rt| {
            assert!(eval_bool(
                rt,
                "var p1 = window.parent, p2 = window.parent; \
                 p1 === p2 && p1.parent === p1 && p1.top === p1 \
                 && p1.self === p1 && p1.window === p1"
            ));
        });
    }

    #[test]
    fn grandchild_top_resolves_root_not_direct_parent() {
        // Слот parent указывает на документ промежуточного фрейма, top — на
        // корень: window.top должен вести в корень, отличаясь от parent.
        with_child_context(
            "<html><body><b>mid</b></body></html>",
            Some("<html><body><i>root</i></body></html>"),
            9,
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "window.parent !== window && window.top !== window.parent \
                     && window.top.document.querySelector('i').textContent === 'root' \
                     && window.parent.document.querySelector('b').textContent === 'mid' \
                     && window.top.parent === window.top"
                ));
            },
        );
    }

    #[test]
    fn cross_origin_child_gets_window_but_no_documents() {
        with_child_context(
            "<html><body><div>secret</div></body></html>",
            None,
            42,
            false,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var p = window.parent; \
                     p !== null && typeof p === 'object' \
                     && p.document === null \
                     && window.top.document === null \
                     && window.frameElement === null \
                     && window.name === ''"
                ));
            },
        );
    }

    #[test]
    fn child_frame_element_is_host_facade_from_parent_tree() {
        // host_nid = 4 — индекс <iframe> в дереве родителя
        // [root, html, head, body, iframe]; нативы читают nid без проверок.
        with_child_context(
            "<html><body><iframe id='host' name='hostframe'></iframe></body></html>",
            None,
            4,
            true,
            |rt| {
                assert!(eval_bool(
                    rt,
                    "var fe = window.frameElement; \
                     fe !== null && fe.localName === 'iframe' && fe.id === 'host' \
                     && fe.getAttribute('name') === 'hostframe'"
                ));
                assert!(eval_bool(rt, "window.name === 'hostframe'"));
            },
        );
    }

    #[test]
    fn frame_length_and_accessors_track_registrations() {
        // Регистрация двух фреймов + постановка доступников тем же шагом,
        // что делает register_frame_document в проде (push + install_index).
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        for (i, (nid, name)) in [(0u32, (11u32, None)), (1u32, (12u32, Some("second".to_owned())))] {
            registry.lock().unwrap().frames.push(FrameDocBinding {
                host_nid: nid,
                doc: Arc::new(Mutex::new(lumen_html_parser::parse(
                    "<html><body><p>x</p></body></html>",
                ))),
                url: "about:blank".to_owned(),
                name,
                accessible: true,
            });
            rt.eval(&format!("_lumen_frame_install_index({i})")).unwrap();
        }
        assert!(eval_bool(&rt, "window.length === 2"));
        assert!(eval_bool(
            &rt,
            "window[0] !== null && window[0] !== window \
             && window[0].document.body.tagName === 'BODY'"
        ));
        assert!(eval_bool(
            &rt,
            "window.second !== undefined && window.second === window[1] \
             && window[0] === window[0]"
        ));
        assert!(eval_bool(&rt, "window[5] === undefined"));
    }

    // ── Срез 4: кросс-фреймовый postMessage ───────────────────────────────────

    /// Пара изолятов «родитель ↔ ребёнок» с общим документом — та же топология,
    /// что строит shell (срезы 1–3): Arc документа ребёнка один и тот же в
    /// реестре родителя и в self_key ребёнка, и наоборот для родителя.
    ///
    /// Вместо WEB_API_SHIM (в минимальном изоляте его нет) каждый контекст
    /// получает мини-хуки приёма, складывающие доставки в `__msgs` (срез 4),
    /// клики в `__clicks` (срез 6) и фокус/dom-события в `__acts` (срез 7).
    fn with_parent_child_pair(
        parent_accessible_to_child: bool,
        child_accessible_to_parent: bool,
    ) -> (V8JsRuntime, V8JsRuntime) {
        let (rt_parent, rt_child, _parent_doc, _child_doc) =
            with_parent_child_pair_docs(parent_accessible_to_child, child_accessible_to_parent);
        (rt_parent, rt_child)
    }

    /// Вариант [`with_parent_child_pair`], дополнительно отдающий оба общих
    /// `Arc<Mutex<Document>>` — для Rust-стороны ожидаемых nid (срез 6).
    fn with_parent_child_pair_docs(
        parent_accessible_to_child: bool,
        child_accessible_to_parent: bool,
    ) -> (
        V8JsRuntime,
        V8JsRuntime,
        Arc<Mutex<lumen_dom::Document>>,
        Arc<Mutex<lumen_dom::Document>>,
    ) {
        let parent_doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><div id='p'>parent</div></body></html>",
        )));
        let child_doc = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><b>child</b></body></html>",
        )));

        let rt_parent = V8JsRuntime::new().unwrap();
        let reg_p: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt_parent.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt_parent, Arc::clone(&reg_p)).unwrap();
        {
            let mut p = reg_p.lock().unwrap();
            p.self_key = Some(Arc::as_ptr(&parent_doc) as usize);
            p.self_origin = "https://parent.example".to_owned();
            p.frames.push(FrameDocBinding {
                host_nid: 7,
                doc: Arc::clone(&child_doc),
                url: "about:srcdoc".to_owned(),
                name: None,
                accessible: child_accessible_to_parent,
            });
        }
        rt_parent
            .eval("typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()")
            .unwrap();

        let rt_child = V8JsRuntime::new().unwrap();
        let reg_c: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt_child.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt_child, Arc::clone(&reg_c)).unwrap();
        {
            let mut c = reg_c.lock().unwrap();
            c.self_key = Some(Arc::as_ptr(&child_doc) as usize);
            c.self_origin = "about:srcdoc".to_owned();
            c.parent = Some(FrameDocBinding {
                host_nid: 7,
                doc: Arc::clone(&parent_doc),
                url: "https://parent.example/".to_owned(),
                name: Some("hostframe".to_owned()),
                accessible: parent_accessible_to_child,
            });
        }
        rt_child
            .eval("typeof _lumen_frame_install_hierarchy === 'function' && _lumen_frame_install_hierarchy()")
            .unwrap();

        for rt in [&rt_parent, &rt_child] {
            rt.eval(
                "globalThis.__msgs = []; \
                 globalThis.__clicks = []; \
                 globalThis.__acts = []; \
                 globalThis._lumen_deliver_frame_message = function(d, o, s) { \
                     __msgs.push({ d: d, o: o, s: s }); \
                 }; \
                 globalThis._lumen_deliver_frame_click = function(nid) { \
                     __clicks.push(nid); \
                 }; \
                 globalThis._lumen_deliver_frame_focus = function(nid, ps) { \
                     __acts.push(['focus', nid, !!ps]); \
                 }; \
                 globalThis._lumen_deliver_frame_blur = function(nid) { \
                     __acts.push(['blur', nid]); \
                 }; \
                 globalThis._lumen_deliver_frame_dom_event = function(nid, env) { \
                     __acts.push(['dom', nid, env.type, !!env.bubbles, \
                                  (env.detail === undefined) ? null : env.detail]); \
                 };",
            )
            .unwrap();
        }
        (rt_parent, rt_child, parent_doc, child_doc)
    }

    fn msg_count(rt: &V8JsRuntime) -> usize {
        match rt.eval("__msgs.length") {
            Ok(JsValue::Number(n)) => n as usize,
            _ => 0,
        }
    }

    #[test]
    fn post_message_from_child_reaches_parent_with_inherited_origin() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        // Ребёнок постит через настоящий фасад window.parent.
        rt_child
            .eval("window.parent.postMessage({a: 1, b: ['x', 2]}, '*')")
            .unwrap();
        assert_eq!(msg_count(&rt_parent), 0, "до пумпы ничего не доставлено");
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 1);
        // origin srcdoc-ребёнка наследуется от родителя-получателя;
        // source — интернированный фасад окна ребёнка.
        assert!(eval_bool(
            &rt_parent,
            "__msgs[0].d.a === 1 && __msgs[0].d.b.length === 2 && __msgs[0].d.b[1] === 2 \
             && __msgs[0].o === 'https://parent.example' \
             && __msgs[0].s !== null && __msgs[0].s === _lumen_frame_content_window(7)"
        ));
        // Повторная пумпа ничего не добавляет — ящик разобран.
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 1);
    }

    #[test]
    fn post_message_from_parent_reaches_child() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_parent
            .eval("_lumen_frame_content_window(7).postMessage('hello', '*')")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_child), 1);
        assert!(eval_bool(
            &rt_child,
            "__msgs[0].d === 'hello' \
             && __msgs[0].o === 'https://parent.example' \
             && __msgs[0].s === window.parent"
        ));
    }

    #[test]
    fn top_level_undefined_becomes_null_like_json() {
        // Отклонение от structured clone, задокументированное в баг-файле:
        // JSON не переносит верхнеуровневый undefined — уходит null.
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_parent
            .eval("_lumen_frame_content_window(7).postMessage(undefined, '*')")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(eval_bool(&rt_child, "__msgs[0].d === null"));
    }

    #[test]
    fn explicit_origin_mismatch_is_silently_dropped() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("window.parent.postMessage('x', 'https://other.example')")
            .unwrap();
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 0);
    }

    #[test]
    fn slash_target_origin_follows_accessibility_flag() {
        // same-origin пара: '/' доставляет…
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("window.parent.postMessage('same', '/')")
            .unwrap();
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent), 1);
        drop((rt_parent, rt_child));

        // …cross-origin пара (accessible=false у слота родителя): '/' режется.
        let (rt_parent2, rt_child2) = with_parent_child_pair(false, true);
        rt_child2
            .eval("window.parent.postMessage('cross', '/')")
            .unwrap();
        rt_parent2.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_parent2), 0);
    }

    #[test]
    fn unbound_bid_posts_nothing() {
        let (_rt_parent, rt_child) = with_parent_child_pair(true, true);
        assert!(eval_bool(
            &rt_child,
            "_lumen_f_post_message(99, '1', '*') === false"
        ));
    }

    #[test]
    fn functions_throw_data_clone_error() {
        let (_rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("globalThis.DOMException = function(m, n) { this.message = m; this.name = n; };")
            .unwrap();
        assert!(eval_bool(
            &rt_child,
            "(function() { \
                 try { window.parent.postMessage(function(){}, '*'); return false; } \
                 catch (e) { return e.name === 'DataCloneError'; } \
             })()"
        ));
    }

    #[test]
    fn foreign_messages_survive_until_their_receiver_pumps() {
        // Сообщение адресовано родителю: разбор ящика ребёнком его не трогает.
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_child
            .eval("window.parent.postMessage('keep', '*')")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(msg_count(&rt_child), 0);
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(eval_bool(&rt_parent, "__msgs[0].d === 'keep'"));
    }

    // ── Срез 5: мутации под-документа из родителя ─────────────────────────────

    /// Рантайм + реестр + общий `Arc` документа биндинга `frames[0]` —
    /// для проверок мутаций и со стороны JS, и со стороны Rust.
    fn with_shared_frame(
        html: &str,
        accessible: bool,
        f: impl FnOnce(&V8JsRuntime, &Arc<Mutex<lumen_dom::Document>>),
    ) {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        let doc = Arc::new(Mutex::new(lumen_html_parser::parse(html)));
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::clone(&doc),
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible,
        });
        f(&rt, &doc);
    }

    /// Второй изолят над ТЕМ ЖЕ `Arc<Mutex<Document>>` — модель «контекст
    /// ребёнка видит мутации родителя»: у shell документ фрейма один и тот же
    /// инстанс в реестре родителя и в контексте ребёнка (срез 1).
    fn sibling_runtime_over(doc: &Arc<Mutex<lumen_dom::Document>>) -> V8JsRuntime {
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        registry.lock().unwrap().frames.push(FrameDocBinding {
            host_nid: 7,
            doc: Arc::clone(doc),
            url: "about:srcdoc".to_owned(),
            name: None,
            accessible: true,
        });
        rt
    }

    #[test]
    fn mutations_land_in_the_real_child_tree() {
        with_shared_frame(
            "<html><body><p>keep</p></body></html>",
            true,
            |rt, doc| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     var p = d.createElement('p'); \
                     p.id = 'injected'; \
                     p.setAttribute('data-from', 'parent'); \
                     p.textContent = 'from-parent'; \
                     d.body.appendChild(p) === p && \
                     d.getElementById('injected').textContent === 'from-parent' && \
                     d.getElementById('injected').getAttribute('data-from') === 'parent'"
                ));
                // Мутация попала в настоящее дерево, а не в копию фасада.
                let injected = {
                    let d = doc.lock().unwrap();
                    lumen_dom::Document::find_by_id(&d, "injected")
                };
                let injected = injected.expect("узел должен существовать в общем дереве");
                let d = doc.lock().unwrap();
                assert_eq!(
                    d.get(injected).get_attr("data-from"),
                    Some("parent"),
                    "атрибут выставлен нативом в общий Document"
                );
                let body = d.body().expect("body на месте");
                assert_eq!(d.get(body).children.len(), 2, "p добавлен последним ребёнком body");
            },
        );
    }

    #[test]
    fn mutations_are_visible_to_another_isolate_sharing_the_arc() {
        let shared = Arc::new(Mutex::new(lumen_html_parser::parse(
            "<html><body><b>old</b></body></html>",
        )));
        let writer = sibling_runtime_over(&shared);
        let reader = sibling_runtime_over(&shared);
        assert!(eval_bool(
            &writer,
            "var d = _lumen_frame_content_document(7); \
             var s = d.createElement('span'); s.textContent = 'fresh'; \
             d.body.insertBefore(s, d.body.firstElementChild) !== undefined"
        ));
        assert!(eval_bool(
            &reader,
            "var b = _lumen_frame_content_document(7).querySelector('body'); \
             b.children.length === 2 && b.children[0].textContent === 'fresh' \
             && b.children[0].tagName === 'SPAN'"
        ));
    }

    #[test]
    fn text_content_setter_replaces_children_of_element() {
        with_shared_frame(
            "<html><body><div id='a'><b>one</b>two<i>three</i></div></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var a = _lumen_frame_content_document(7).getElementById('a'); \
                     a.textContent = 'flat'; \
                     a.children.length === 0 && a.textContent === 'flat'"
                ));
            },
        );
    }

    #[test]
    fn remove_and_insert_before_reorder_children() {
        with_shared_frame(
            "<html><body><ul id='u'><li id='x'>x</li><li id='z'>z</li></ul></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     var y = d.createElement('li'); y.id = 'y'; y.textContent = 'y'; \
                     d.getElementById('u').insertBefore(y, d.getElementById('z')) !== undefined && \
                     d.getElementById('u').children[1].id === 'y'"
                ));
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.getElementById('y').remove(); \
                     var ids = d.getElementById('u').children.map(function(c) { return c.id; }); \
                     ids.join(',') === 'x,z'"
                ));
            },
        );
    }

    #[test]
    fn remove_child_takes_node_off_its_actual_parent() {
        with_shared_frame(
            "<html><body><div id='box'><b id='inner'>t</b></div></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     var inner = d.getElementById('inner'); \
                     d.getElementById('box').removeChild(inner) === inner && \
                     d.getElementById('box').children.length === 0 && \
                     inner.parentElement === null"
                ));
            },
        );
    }

    #[test]
    fn cross_binding_facade_argument_is_silently_ignored() {
        // Два биндинга в одном изоляте (разные host_nid): фасад документа
        // первого не принимает узел второго — проверка __bid__ режет аргумент
        // до натива. Владение nid'ом на нативном уровне обеспечивает именно
        // эта JS-граница: нативы модуля приватны и доверяют своим фасадам,
        // как нативы главного документа доверяют его врапперам.
        let rt = V8JsRuntime::new().unwrap();
        let registry: FrameDocRegistry = Arc::new(Mutex::new(FrameDocSlots::default()));
        rt.eval("var window = globalThis;").unwrap();
        install_frame_bridge_v8(&rt, Arc::clone(&registry)).unwrap();
        for (host, html) in [
            (7u32, "<html><body><p id='first'>a</p></body></html>"),
            (8u32, "<html><body><p id='second'>b</p></body></html>"),
        ] {
            registry.lock().unwrap().frames.push(FrameDocBinding {
                host_nid: host,
                doc: Arc::new(Mutex::new(lumen_html_parser::parse(html))),
                url: "about:srcdoc".to_owned(),
                name: None,
                accessible: true,
            });
        }
        assert!(eval_bool(
            &rt,
            "var d0 = _lumen_frame_content_document(7); \
             var d1 = _lumen_frame_content_document(8); \
             var alien = d1.createElement('div'); \
             alien.__bid__ !== d0.body.__bid__ && \
             d0.body.appendChild(alien) === undefined && \
             d0.body.children.length === 1 && \
             d1.body.children.length === 1"
        ));
        // Чужой фасад и в removeChild/insertBefore игнорируется.
        assert!(eval_bool(
            &rt,
            "var d0 = _lumen_frame_content_document(7); \
             var d1 = _lumen_frame_content_document(8); \
             d0.body.removeChild(d1.createElement('span')) === undefined && \
             d0.body.insertBefore(d1.createElement('i'), null) === undefined && \
             d0.getElementById('first').textContent === 'a'"
        ));
    }

    #[test]
    fn inaccessible_frame_mutations_are_no_ops() {
        with_shared_frame(
            "<html><body><p>x</p></body></html>",
            false,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d === null && \
                     _lumen_f_create_element(7, 'div') === -1 && \
                     _lumen_f_append_child(7, 3, 4) === false && \
                     _lumen_f_set_attr(7, 3, 'id', 'hax') === false"
                ));
            },
        );
    }

    #[test]
    fn cycle_append_is_rejected_without_corrupting_tree() {
        with_shared_frame(
            "<html><body><div id='deep'>t</div></body></html>",
            true,
            |rt, _| {
                // documentElement нельзя вставить под собственного потомка.
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.body.appendChild(d.documentElement) === undefined && \
                     d.documentElement.parentElement === null"
                ));
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.getElementById('deep').textContent === 't' && \
                     d.body.children.length === 1"
                ));
            },
        );
    }

    #[test]
    fn title_setter_creates_title_inside_head() {
        with_shared_frame("<html><head></head><body>x</body></html>", true, |rt, _| {
            assert!(eval_bool(
                rt,
                "var d = _lumen_frame_content_document(7); \
                 d.title === '' && (d.title = 'framed') || true; \
                 d.title === 'framed'"
            ));
        });
        // Повторная запись идёт в существующий <title>.
        with_shared_frame(
            "<html><head><title>old</title></head><body></body></html>",
            true,
            |rt, _| {
                assert!(eval_bool(
                    rt,
                    "var d = _lumen_frame_content_document(7); \
                     d.title === 'old' && ((d.title = 'new') || true) && d.title === 'new'"
                ));
            },
        );
    }

    #[test]
    fn created_text_node_round_trips_through_facade() {
        with_shared_frame("<html><body></body></html>", true, |rt, _| {
            assert!(eval_bool(
                rt,
                "var d = _lumen_frame_content_document(7); \
                 var t = d.createTextNode('plain'); \
                 t.nodeType === 3 && t.textContent === 'plain' && \
                 d.body.appendChild(t) === t && \
                 d.body.textContent === 'plain'"
            ));
        });
    }

    // ── Срез 6: события через границу изолятов ────────────────────────────────

    /// Индекс первого элемента с тегом `tag` в дереве `doc`.
    fn element_index(doc: &lumen_dom::Document, tag: &str) -> Option<u32> {
        find_first_matching(doc, doc.root(), &|node| {
            node.element_name()
                .map(|n| n.local.eq_ignore_ascii_case(tag))
                .unwrap_or(false)
        })
        .map(|n| n.index() as u32)
    }

    fn clicks(rt: &V8JsRuntime) -> Vec<u32> {
        match rt.eval("JSON.stringify(__clicks)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Доставки среза 7: `[вид, nid, ...аргументы]` на каждый конверт.
    fn acts(rt: &V8JsRuntime) -> Vec<serde_json::Value> {
        match rt.eval("JSON.stringify(__acts)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn facade_click_from_parent_delivers_on_child_pump() {
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, true);
        rt_parent
            .eval(
                "var b = _lumen_frame_content_document(7).querySelector('b'); \
                 globalThis.__bnid = b.__nid__; \
                 b.click(); b.click();",
            )
            .unwrap();
        // До пумпы доставки нет — конверты лежат в ящике.
        assert!(clicks(&rt_child).is_empty(), "до пумпы ничего не доставлено");
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let expected = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        // nid фасада — индекс в арене ребёнка; оба клика доставлены по адресу.
        let got = clicks(&rt_child);
        assert_eq!(got, vec![expected, expected]);
        match rt_parent.eval("__bnid") {
            Ok(JsValue::Number(n)) => assert_eq!(n as u32, expected),
            other => panic!("__bnid не число: {other:?}"),
        }
        // Ящик разобран: повторная пумпа ничего не добавляет.
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(clicks(&rt_child).len(), 2);
    }

    #[test]
    fn child_facade_click_waits_for_the_parent_pump() {
        let (rt_parent, rt_child, pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_child
            .eval("window.parent.document.body.click();")
            .unwrap();
        // Пумпа ребёнка не трогает чужой ящик — конверт адресован родителю.
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(clicks(&rt_child).is_empty());
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        let expected = {
            let d = pd.lock().unwrap();
            element_index(&d, "body").expect("body в дереве родителя")
        };
        assert_eq!(clicks(&rt_parent), vec![expected]);
    }

    #[test]
    fn click_into_inaccessible_child_is_dropped() {
        // Второй флаг false: слот frames родителя cross-origin/opaque — фасад
        // документа не выдаётся и натив постановки режет конверт до ящика.
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, false);
        let b_nid = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        assert!(eval_bool(
            &rt_parent,
            &format!(
                "_lumen_frame_content_document(7) === null && \
                 _lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"click\"}}') === false"
            )
        ));
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(clicks(&rt_child).is_empty());
    }

    #[test]
    fn click_queue_rejects_non_elements_and_bad_args() {
        with_shared_frame("<html><body><b>x</b></body></html>", true, |rt, doc| {
            let text_nid = {
                let d = doc.lock().unwrap();
                let b = find_first_matching(&d, d.root(), &|node| {
                    node.element_name()
                        .map(|n| n.local.eq_ignore_ascii_case("b"))
                        .unwrap_or(false)
                })
                .expect("<b> существует");
                d.get(b).children[0].index() as u32
            };
            // Текстовый узел, nid за границей арены и несуществующий bid —
            // все три дают «нет» и ничего не ставят в ящик.
            assert!(eval_bool(
                rt,
                &format!(
                    "_lumen_f_queue_event(0, {text_nid}, '{{\"kind\":\"click\"}}') === false && \
                     _lumen_f_queue_event(0, 4294967295, '{{\"kind\":\"click\"}}') === false && \
                     _lumen_f_queue_event(5, 1, '{{\"kind\":\"click\"}}') === false"
                )
            ));
        });
    }

    // ── Срез 7: focus()/blur()/dispatchEvent через фасад ──────────────────────

    #[test]
    fn facade_focus_and_blur_deliver_on_child_pump_in_order() {
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, true);
        rt_parent
            .eval(
                "var b = _lumen_frame_content_document(7).querySelector('b'); \
                 b.focus({ preventScroll: true }); \
                 b.blur();",
            )
            .unwrap();
        assert!(acts(&rt_child).is_empty(), "до пумпы ничего не доставлено");
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let expected = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        let a = acts(&rt_child);
        assert_eq!(
            a,
            vec![
                serde_json::json!(["focus", expected, true]),
                serde_json::json!(["blur", expected]),
            ],
            "порядок focus → blur сохранён, preventScroll перенесён"
        );
    }

    #[test]
    fn facade_dispatch_event_round_trips_flags_and_detail() {
        let (rt_parent, rt_child) = with_parent_child_pair(true, true);
        rt_parent
            .eval(
                "_lumen_frame_content_document(7).querySelector('b') \
                 .dispatchEvent({ type: 'ping', bubbles: true, cancelable: true, \
                                  detail: { n: 7, tag: 'x' } });",
            )
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let a = acts(&rt_child);
        assert_eq!(a.len(), 1);
        assert_eq!(
            a[0],
            serde_json::json!(["dom", a[0][1], "ping", true, {"n": 7, "tag": "x"}]),
            "type/bubbles/detail дошли до хука получателя"
        );
        // detail — не мусор от сериализации, а исходное значение.
        assert!(eval_bool(
            &rt_child,
            "__acts[0][4].n === 7 && __acts[0][4].tag === 'x'"
        ));
    }

    #[test]
    fn child_facade_actions_address_the_parent_doc() {
        // Симметрия с кликом среза 6: конверт адресован документу-владельцу
        // элемента, пумпа отправителя его не трогает.
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_child
            .eval("window.parent.document.getElementById('p').focus();")
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(acts(&rt_child).is_empty());
        rt_parent.eval("_lumen_frame_pump_messages()").unwrap();
        let a = acts(&rt_parent);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0][0], serde_json::json!("focus"));
    }

    #[test]
    fn actions_into_inaccessible_child_are_dropped() {
        let (rt_parent, rt_child, _pd, cd) = with_parent_child_pair_docs(true, false);
        let b_nid = {
            let d = cd.lock().unwrap();
            element_index(&d, "b").expect("<b> в дереве ребёнка")
        };
        assert!(eval_bool(
            &rt_parent,
            &format!(
                "_lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"focus\"}}') === false && \
                 _lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"blur\"}}') === false && \
                 _lumen_f_queue_event(0, {b_nid}, '{{\"kind\":\"dom\",\"type\":\"x\"}}') === false"
            )
        ));
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert!(acts(&rt_child).is_empty());
    }

    #[test]
    fn queue_event_rejects_malformed_specs_without_touching_outbox() {
        with_shared_frame("<html><body><b>x</b></body></html>", true, |rt, _| {
            // Невалидный JSON, чужой kind, пустой/нестроковый/слишком длинный
            // type — все отклоняются; после этого валидный blur по-прежнему
            // проходит (ящик жив, мусор его не заблокировал).
            assert!(eval_bool(
                rt,
                "_lumen_f_queue_event(0, 2, 'not json') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"zap\"}') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"dom\",\"type\":\"\"}') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"dom\",\"type\":5}') === false && \
                 _lumen_f_queue_event(0, 2, '{\"kind\":\"dom\"}') === false"
            ));
            let long_type = "x".repeat(129);
            assert!(eval_bool(
                rt,
                &format!(
                    "_lumen_f_queue_event(0, 2, '{}') === false",
                    serde_json::json!({"kind": "dom", "type": long_type})
                )
            ));
            assert!(eval_bool(
                rt,
                "_lumen_f_queue_event(0, 2, '{\"kind\":\"blur\"}') === true"
            ));
        });
    }

    // ── Срез 8: исполнение вставленных из родителя <script> ──────────────────

    #[test]
    fn facade_appended_script_queues_run_script_with_its_nid() {
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        // Хук среза 8 поверх пары: складываем nid пришедших RunScript.
        rt_child
            .eval(
                "globalThis.__runs = []; \
                 globalThis._lumen_deliver_frame_run_script = function(nid) { \
                     __runs.push(nid); \
                 };",
            )
            .unwrap();
        rt_parent
            .eval(
                "var d = _lumen_frame_content_document(7); \
                 var div = d.createElement('div'); \
                 d.body.appendChild(div); \
                 var s = d.createElement('script'); \
                 s.textContent = 'void 0;'; \
                 d.body.appendChild(s); \
                 var s2 = d.createElement('script'); \
                 s2.textContent = 'void 1;'; \
                 d.body.insertBefore(s2, div); \
                 globalThis.__snid = [s.__nid__, s2.__nid__];",
            )
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        // nid скриптов читаем из JS родителя (индексы арены согласованы по
        // определению — фасады строятся над тем же деревом, что читает ребёнок).
        let expected: Vec<u32> = match rt_parent.eval("JSON.stringify(__snid)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        let got: Vec<u32> = match rt_child.eval("JSON.stringify(__runs)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        assert_eq!(
            got, expected,
            "оба скрипта доставлены со своими nid; <div> конверт не ставил"
        );
    }

    #[test]
    fn run_script_envelope_requires_accessible_binding() {
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, false);
        rt_child
            .eval(
                "globalThis.__runs = []; \
                 globalThis._lumen_deliver_frame_run_script = function(nid) { \
                     __runs.push(nid); \
                 };",
            )
            .unwrap();
        // cross-origin: фасада документа нет вовсе — вставки не было.
        assert!(eval_bool(
            &rt_parent,
            "_lumen_frame_content_document(7) === null"
        ));
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let got: Vec<u32> = match rt_child.eval("JSON.stringify(__runs)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        assert!(got.is_empty());
        // Прямая постановка в cross-origin биндинг тоже режется (native
        // мутации вернул false ещё до queue_run_script_if_script).
        assert!(eval_bool(
            &rt_parent,
            "_lumen_f_append_child(0, 3, 4) === false"
        ));
    }

    // ── Срез 9: URL-рефлексии фасадов и поздний src скрипта ───────────────────

    /// Поздний `setAttribute('src', …)` на `<script>`-фасаде ставит ВТОРОЙ
    /// конверт RunScript — тот же триггер подготовки, что и вставка (HTML LS
    /// §4.12.1). До среза конверт ставился только фактом вставки, поэтому
    /// скрипт, вставленный пустым и получивший src после первой доставки,
    /// не готовился никогда.
    #[test]
    fn facade_late_src_on_script_queues_second_run_script() {
        let (rt_parent, rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_child
            .eval(
                "globalThis.__runs = []; \
                 globalThis._lumen_deliver_frame_run_script = function(nid) { \
                     __runs.push(nid); \
                 };",
            )
            .unwrap();
        rt_parent
            .eval(
                "var d = _lumen_frame_content_document(7); \
                 var s = d.createElement('script'); \
                 d.body.appendChild(s); \
                 globalThis.__snid = s.__nid__;",
            )
            .unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        assert_eq!(
            match rt_child.eval("JSON.stringify(__runs)") {
                Ok(JsValue::String(s)) => serde_json::from_str::<Vec<u32>>(&s).unwrap_or_default(),
                _ => Vec::new(),
            }
            .len(),
            1,
            "первая доставка — от вставки"
        );
        // Каноничная форма `s.src = url` идёт через ту же запись атрибута.
        rt_parent.eval("s.src = 'https://child.example/x.js';").unwrap();
        rt_child.eval("_lumen_frame_pump_messages()").unwrap();
        let got: Vec<u32> = match rt_child.eval("JSON.stringify(__runs)") {
            Ok(JsValue::String(s)) => serde_json::from_str(&s).unwrap_or_default(),
            _ => Vec::new(),
        };
        let snid = u32::try_from(
            match rt_parent.eval("__snid") {
                Ok(JsValue::Number(n)) => n as i64,
                _ => -1,
            },
        )
        .unwrap_or(u32::MAX);
        assert_eq!(got, vec![snid, snid], "поздний src доставил второй конверт с тем же nid");
    }

    /// Рефлексии src/href фасада: сеттер хранит строку ДОСЛОВНО в атрибуте,
    /// геттер отдаёт '' для отсутствующего атрибута и разрешает значение
    /// против базы под-документа (href первого `<base>` против URL документа).
    /// `_url_resolve` в минимальном изоляте отсутствует — тест ставит детерминированный
    /// stub, делающий видимость обоих уровней разрешения.
    #[test]
    fn facade_src_href_reflections_store_verbatim_and_resolve_against_base() {
        let (rt_parent, _rt_child, _pd, _cd) = with_parent_child_pair_docs(true, true);
        rt_parent
            .eval(
                "globalThis._url_resolve = function(rel, base) { \
                     return 'resolved:' + base + '|' + rel; \
                 };",
            )
            .unwrap();
        rt_parent
            .eval(
                "var d = _lumen_frame_content_document(7); \
                 var img = d.createElement('img'); \
                 globalThis.__img = img; \
                 globalThis.__absentSrc = img.src; \
                 globalThis.__absentHref = img.href; \
                 img.src = 'pic.png'; \
                 globalThis.__storedRaw = _lumen_f_attr(0, img.__nid__, 'src'); \
                 globalThis.__resolvedSrc = img.src; \
                 var b = d.createElement('base'); \
                 b.setAttribute('href', '/dir/'); \
                 d.head.appendChild(b); \
                 globalThis.__afterBase = img.src;",
            )
            .unwrap();
        // Геттер без атрибута — '' (не null, не URL документа).
        assert_eq!(
            rt_parent.eval("__absentSrc").ok(),
            Some(JsValue::String(String::new()))
        );
        assert_eq!(
            rt_parent.eval("__absentHref").ok(),
            Some(JsValue::String(String::new()))
        );
        // Сеттер сохранил атрибут дословно.
        assert_eq!(
            rt_parent.eval("__storedRaw").ok(),
            Some(JsValue::String("pic.png".to_owned()))
        );
        // Геттер разрешил против базы; после вставки <base href="/dir/"> базой
        // стало разрешённое значение base-href (двухуровневая композиция §4.2.3).
        assert_eq!(
            rt_parent.eval("__resolvedSrc").ok(),
            Some(JsValue::String(
                "resolved:about:srcdoc|pic.png".to_owned()
            ))
        );
        assert_eq!(
            rt_parent.eval("__afterBase").ok(),
            Some(JsValue::String(
                "resolved:resolved:about:srcdoc|/dir/|pic.png".to_owned()
            ))
        );
        // href — та же пара аксессоров над другим атрибутом (base уже
        // вставлен выше, поэтому разрешение идёт через двухуровневую базу).
        rt_parent.eval("img.href = 'a.html';").unwrap();
        assert_eq!(
            rt_parent.eval("_lumen_f_attr(0, img.__nid__, 'href')").ok(),
            Some(JsValue::String("a.html".to_owned()))
        );
        assert_eq!(
            rt_parent.eval("img.href").ok(),
            Some(JsValue::String(
                "resolved:resolved:about:srcdoc|/dir/|a.html".to_owned()
            ))
        );
    }
}
