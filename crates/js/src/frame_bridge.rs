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
use std::collections::HashMap;
#[cfg(feature = "v8-backend")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "v8-backend")]
use std::sync::{Arc, Mutex, OnceLock, Weak};

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

/// Псевдо-bid «это окно самого читающего контекста»: `_lumen_f_sibling_binding`
/// отдаёт его, когда iframe предка, прочитанный через фасад предка, и есть
/// хост текущего фрейма (`frameElement.contentWindow === window`, BUG-1643).
/// Реестр такого слота не имеет — шим подставляет настоящий `window`.
#[cfg(feature = "v8-backend")]
pub(crate) const SELF_BID: u32 = u32::MAX - 2;

/// Начало диапазона bid-ов [`FrameDocSlots::siblings`]: `SIBLING_BASE + i` —
/// i-й биндинг соседнего фрейма (BUG-1643). Обычные индексы `frames` сюда не
/// доходят (нужны 2^31 фреймов).
#[cfg(feature = "v8-backend")]
pub(crate) const SIBLING_BASE: u32 = 0x8000_0000;

/// Один зарегистрированный под-документ `<iframe>` в реестре рантайма.
///
/// Живёт в [`FrameDocSlots`] не дольше контекста страницы. Навигация фрейма
/// его ЗАМЕЩАЕТ, сохраняя индекс, а биндинг хоста, вынутого из документа,
/// становится надгробием ([`DETACHED_HOST`]) при следующей регистрации, — см.
/// [`upsert_binding`].
#[cfg(feature = "v8-backend")]
#[derive(Clone)]
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
    /// Значение атрибута `name` хоста, снятое СНИМКОМ при регистрации
    /// биндинга (не живое чтение). У биндингов дочерних фреймов (`frames`) —
    /// ключ именованного доступа `window[name]` родителя (срез 3). У
    /// биндинга-предка (`parent`, [`PARENT_BID`]) — имя СОБСТВЕННОГО
    /// контекста, которое читает `window.name` ребёнка (BUG-921).
    pub(crate) name: Option<String>,
    /// `false` — cross-origin или opaque sandbox: нативы чтения отдают пустые
    /// результаты, `.document` фасада окна — `null`.
    pub(crate) accessible: bool,
    /// BUG-1198: `Some(id)` — у документа непрозрачное происхождение
    /// (`sandbox` без `allow-same-origin`, HTML LS §7.1.1 «sandboxed origin
    /// browsing context flag»): `event.origin` его сообщений — `"null"`, а
    /// `id` — идентичность этого происхождения для `Origin.from(event)`.
    /// Одна на документ (все сообщения одного документа same-origin друг с
    /// другом) и новая у каждого нового документа того же хоста — перезагрузка
    /// выдаёт другое непрозрачное происхождение ([`next_opaque_origin_id`],
    /// перенос при повторной регистрации — [`upsert_binding`]). `None` —
    /// tuple-origin по `url`.
    pub(crate) opaque_id: Option<u64>,
    /// BUG-979: хэндл для синхронного кросс-изолятного чтения/вызова
    /// РЕАЛЬНЫХ глобалов этого под-документа (не только фиксированный
    /// IDL-набор `winFacade`) — `None` у тестовых биндингов без рантайма и
    /// когда `accessible == false` (глобалы читаются только same-origin,
    /// натив дополнительно гейтит это явно, поле не единственная защита).
    ///
    /// BUG-1682: слабая ссылка. Сильная делала родителя и ребёнка циклом
    /// `Arc` (у ребёнка в слоте `parent` — рантайм родителя, у родителя в
    /// `frames` — рантайм ребёнка), и `Drop for V8JsRuntime` не срабатывал
    /// никогда: поток и изолят каждого фрейма жили до конца процесса. Рантайм
    /// фрейма держит shell (`FrameHandle::js`); реестр лишь заглядывает в него,
    /// пока тот жив.
    pub(crate) peer: Option<Weak<dyn crate::frame_peer_bridge::FramePeerBridge>>,
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
    /// BUG-1643: соседние фреймы (дети того же предка), на которые ребёнок
    /// вышел через фасад предка — `parent.document.getElementById(id)
    /// .contentWindow`. Копии биндингов из реестра предка, заводятся лениво
    /// ([`sibling_slot`]); bid = [`SIBLING_BASE`] + индекс.
    pub(crate) siblings: Vec<SiblingSlot>,
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

/// Копия биндинга соседнего фрейма в реестре ребёнка (BUG-1643).
#[cfg(feature = "v8-backend")]
pub(crate) struct SiblingSlot {
    /// Слот предка, в чьём `frames` лежит хост соседа ([`PARENT_BID`] /
    /// [`TOP_BID`]): документ-владелец хоста нужен `frameElement` и навигации.
    pub(crate) owner: u32,
    pub(crate) binding: FrameDocBinding,
}

/// Реестры всех рантаймов по ключу `Arc::as_ptr(&doc)` собственного документа
/// (тот же ключ, что `self_key`/[`frame_outbox`]). Ребёнок по документу
/// предка из своего слота `parent`/`top` находит реестр предка и читает из его
/// `frames` соседей (BUG-1643). `Weak` — запись умершего рантайма не держит
/// реестр; `register_frame_registry` вычищает мёртвые при вставке.
#[cfg(feature = "v8-backend")]
fn frame_registries() -> &'static Mutex<HashMap<usize, Weak<Mutex<FrameDocSlots>>>> {
    static R: OnceLock<Mutex<HashMap<usize, Weak<Mutex<FrameDocSlots>>>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Записать реестр рантайма под ключом его собственного документа.
#[cfg(feature = "v8-backend")]
pub(crate) fn register_frame_registry(key: usize, registry: &FrameDocRegistry) {
    let mut map = frame_registries().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|_, w| w.strong_count() > 0);
    map.insert(key, Arc::downgrade(registry));
}

#[cfg(feature = "v8-backend")]
fn registry_of_doc(key: usize) -> Option<FrameDocRegistry> {
    frame_registries().lock().unwrap_or_else(|e| e.into_inner()).get(&key)?.upgrade()
}

/// Положить копию соседнего биндинга в `siblings` и вернуть его bid: запись
/// того же хоста того же владельца ЗАМЕЩАЕТСЯ на месте (навигация соседа
/// сохраняет bid, на который уже ссылаются кэшированные фасады).
#[cfg(feature = "v8-backend")]
fn sibling_slot(slots: &mut FrameDocSlots, owner: u32, binding: FrameDocBinding) -> u32 {
    let i = match slots
        .siblings
        .iter()
        .position(|s| s.owner == owner && s.binding.host_nid == binding.host_nid)
    {
        Some(i) => {
            slots.siblings[i].binding = binding;
            i
        }
        None => {
            slots.siblings.push(SiblingSlot { owner, binding });
            slots.siblings.len() - 1
        }
    };
    SIBLING_BASE + i as u32
}

/// Найти среди детей предка `owner` (по `host_nid` или по ключу документа) и
/// вернуть bid в реестре `slots`: сам читающий — [`SELF_BID`], его родитель —
/// [`PARENT_BID`], иначе сосед ([`sibling_slot`]). Замок реестра предка
/// берётся только после замка своего и всегда «вверх» по цепочке предков,
/// поэтому циклов ожидания нет (реестр предка замков потомков не берёт).
#[cfg(feature = "v8-backend")]
fn resolve_owner_child(
    slots: &mut FrameDocSlots,
    owner: u32,
    host_nid: Option<u32>,
    doc_key: Option<usize>,
) -> Option<u32> {
    let (owner_key, owner_accessible) = {
        let o = resolve_slot(slots, owner)?;
        (Arc::as_ptr(&o.doc) as usize, o.accessible)
    };
    let owner_reg = registry_of_doc(owner_key)?;
    let mut found = {
        let o = owner_reg.lock().unwrap_or_else(|e| e.into_inner());
        o.frames
            .iter()
            .find(|b| {
                b.host_nid != DETACHED_HOST
                    && host_nid.is_none_or(|h| b.host_nid == h)
                    && doc_key.is_none_or(|k| Arc::as_ptr(&b.doc) as usize == k)
            })
            .cloned()?
    };
    let key = Arc::as_ptr(&found.doc) as usize;
    if Some(key) == slots.self_key {
        return Some(SELF_BID);
    }
    if slots.parent.as_ref().is_some_and(|p| Arc::as_ptr(&p.doc) as usize == key) {
        return Some(PARENT_BID);
    }
    // Достижимость пары: ребёнок и сосед оба same-origin с владельцем.
    found.accessible &= owner_accessible;
    Some(sibling_slot(slots, owner, found))
}

/// Положить биндинг под-документа в реестр и вернуть его `bid`: ЗАМЕСТИТЬ
/// запись того же хоста, если она есть, иначе добавить в конец
/// (BUG-480 срез 19).
///
/// Замещение с сохранением индекса — содержательная часть, а не экономия:
/// вложенный browsing context при навигации остаётся тем же, меняется лишь его
/// документ. Простой `push` давал бы второй биндинг того же хоста, а поиск по
/// `host_nid` берёт ПЕРВОЕ совпадение (`_lumen_frame_binding`), так что
/// `contentDocument`/`contentWindow` родителя вечно отдавали бы выброшенный
/// документ; заодно `window.length` рос бы на каждую навигацию, а `window[i]`
/// разъезжался бы с порядком документа.
///
/// BUG-1198: shell регистрирует один и тот же документ дважды — до его
/// скриптов (без `peer`) и после; непрозрачное происхождение принадлежит
/// документу, поэтому повторная регистрация ТОГО ЖЕ документа сохраняет
/// прежний `opaque_id`, а новый документ хоста (навигация, перезагрузка)
/// остаётся со свежим.
///
/// BUG-1682: добавление НОВОГО хоста сначала хоронит биндинги хостов, которых
/// в собственном документе больше нет ([`bury_detached_bindings`]), — иначе
/// страница, создающая и удаляющая `<iframe>`, копила бы под-документы без
/// предела.
#[cfg(feature = "v8-backend")]
pub(crate) fn upsert_binding(slots: &mut FrameDocSlots, mut binding: FrameDocBinding) -> usize {
    if !slots.frames.iter().any(|b| b.host_nid == binding.host_nid) {
        bury_detached_bindings(slots);
    }
    match slots.frames.iter().position(|b| b.host_nid == binding.host_nid) {
        Some(i) => {
            let old = &slots.frames[i];
            if binding.opaque_id.is_some() && Arc::ptr_eq(&old.doc, &binding.doc) {
                binding.opaque_id = old.opaque_id.or(binding.opaque_id);
            }
            slots.frames[i] = binding;
            i
        }
        None => {
            slots.frames.push(binding);
            slots.frames.len() - 1
        }
    }
}

/// `host_nid` надгробия — биндинга хоста, вынутого из документа (BUG-1682).
///
/// Запись остаётся на своём месте, а не удаляется из `frames`: индекс — это
/// `bid`, который фасады, построенные раньше, держат в замыканиях, и сдвиг
/// перевёл бы их на чужой документ. Надгробие не находит ни один host-nid
/// (`contentWindow` вынутого хоста — `null`, HTML LS §4.8.5), не входит в
/// `window.length` и не держит ни документ ребёнка, ни его рантайм.
#[cfg(feature = "v8-backend")]
pub(crate) const DETACHED_HOST: u32 = u32::MAX;

/// Превратить в надгробия биндинги, чей хост больше не подключён к
/// собственному документу контекста ([`FrameDocSlots::self_doc`]).
///
/// Документ занят другим потоком — уборка пропускается до следующей
/// регистрации: ждать его здесь нельзя, вызывающий держит замок реестра.
#[cfg(feature = "v8-backend")]
fn bury_detached_bindings(slots: &mut FrameDocSlots) {
    let Some(self_doc) = slots.self_doc.as_ref() else { return };
    let doc = match self_doc.try_lock() {
        Ok(d) => d,
        Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return,
    };
    for b in slots.frames.iter_mut().filter(|b| b.host_nid != DETACHED_HOST) {
        // `host_nid` — индекс слота без поколения (`frames.rs` регистрирует
        // `node.index()`): слот живого хоста не переиспользуется, так что
        // ошибиться можно лишь в сторону лишней жизни переиспользованного.
        if !doc.is_connected(lumen_dom::NodeId::from_index(b.host_nid as usize)) {
            *b = FrameDocBinding {
                host_nid: DETACHED_HOST,
                doc: Arc::new(Mutex::new(lumen_dom::Document::new())),
                url: String::new(),
                name: None,
                accessible: false,
                opaque_id: None,
                peer: None,
            };
        }
    }
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
    /// BUG-1643: отправитель — соседний фрейм (ребёнок того же предка);
    /// получатель находит его через реестр предка ([`resolve_owner_child`]).
    Sibling(usize),
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

/// BUG-1198: выдать идентичность нового непрозрачного происхождения —
/// монотонный счётчик процесса, а не адрес документа: адрес освобождённого
/// документа может достаться его же перезагруженной копии, и два разных
/// непрозрачных происхождения совпали бы.
#[cfg(feature = "v8-backend")]
pub(crate) fn next_opaque_origin_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
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
pub(crate) fn resolve_slot(slots: &FrameDocSlots, bid: u32) -> Option<&FrameDocBinding> {
    match bid {
        PARENT_BID => slots.parent.as_ref(),
        TOP_BID => slots.top.as_ref(),
        i if i >= SIBLING_BASE => slots.siblings.get((i - SIBLING_BASE) as usize).map(|s| &s.binding),
        i => slots.frames.get(i as usize),
    }
}

// ── Срез 25: relayout ребёнка после мутации ЧЕРЕЗ МОСТ ───────────────────────

/// Глобальный сигнал «документ с этим ключом мутирован мостом» (BUG-480
/// срез 25). Ключ — тот же `Arc::as_ptr(&doc) as usize`, что у
/// [`FrameDocSlots::self_key`]/[`frame_outbox`]: родитель мутирует ребёнка в
/// СВОЁМ изоляте ([`with_accessible_doc_mut`]), у ребёнка своя, отдельная
/// `V8JsRuntime` с собственным `dom_dirty` — дёрнуть его напрямую неоткуда, и
/// этот реестр единственный канал сигнала для кросс-изолятной записи. Мутация
/// ребёнка ЕГО СОБСТВЕННЫМ скриптом идёт обычными нативами `dom.rs` в его же
/// рантайме и уже поднимает штатный `dom_dirty` этого рантайма — реестр ей не
/// нужен, шелл дренирует оба источника рядом (`about_to_wait.rs`).
///
/// BUG-1110: значение — `Weak` документа. Живой `Weak` удерживает выделение
/// `Arc`, поэтому аллокатор не может выдать тот же адрес новому документу,
/// пока запись в реестре есть; запись умершего документа `take` отбрасывает
/// (флаг «наследует» только живой документ), а `mark` вычищает такие записи.
#[cfg(feature = "v8-backend")]
fn frame_dom_dirty() -> &'static Mutex<HashMap<usize, Weak<Mutex<lumen_dom::Document>>>> {
    static DIRTY: OnceLock<Mutex<HashMap<usize, Weak<Mutex<lumen_dom::Document>>>>> =
        OnceLock::new();
    DIRTY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Пометить документ мутированным мостом (см. [`frame_dom_dirty`]).
#[cfg(feature = "v8-backend")]
fn mark_frame_dom_dirty(doc: &Arc<Mutex<lumen_dom::Document>>) {
    let mut map = frame_dom_dirty().lock().unwrap_or_else(|e| e.into_inner());
    map.retain(|_, w| w.strong_count() > 0);
    map.insert(Arc::as_ptr(doc) as usize, Arc::downgrade(doc));
}

/// Забрать (и сбросить) флаг «документ с этим ключом мутирован мостом».
/// Единственный вызывающий — [`crate::v8_runtime::runtime::V8JsRuntime::take_frame_dom_dirty`]
/// со своим `self_doc_key`; `false`, если мутаций не было.
#[cfg(feature = "v8-backend")]
pub(crate) fn take_frame_dom_dirty(key: usize) -> bool {
    let entry = frame_dom_dirty().lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
    entry.is_some_and(|w| w.strong_count() > 0)
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
///
/// Срез 25: помечает целевой документ грязным ([`frame_dom_dirty`])
/// безусловно, как только биндинг разрешён и доступен — до вызова `f`, а не
/// по его результату. Все восемь нативов, идущих через эту точку, либо сами
/// меняют дерево (append/insert/remove/set_text) либо готовят узел к
/// вставке (create_element/create_text, где мутация видимого дерева ещё не
/// произошла) — в обоих случаях лишний пересчёт фрейма стоит дёшево
/// (`relayout_frame_content` гейтов не имеет само по себе, но вызывается
/// самим шеллом не чаще одного раза за тик), а пропущенный испортил бы
/// видимый результат.
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
    let shared = Arc::clone(&binding.doc);
    let mut doc = shared.lock().unwrap_or_else(|e| e.into_inner());
    let result = f(&mut doc);
    drop(doc);
    mark_frame_dom_dirty(&shared);
    result
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
    Some(lumen_dom::NodeId::from_raw(nid))
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
    // BUG-1643: iframe предка, прочитанный через фасад предка
    // (`parent.document.getElementById(id)`), — bid его окна в реестре
    // читающего ребёнка. Хост лежит в `frames` реестра предка, которого у
    // ребёнка нет, поэтому биндинг копируется оттуда лениво
    // ([`resolve_owner_child`]); сам ребёнок даёт [`SELF_BID`].
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_sibling_binding",
            into_v8_fn2(move |owner: u32, host_nid: u32| -> Option<u32> {
                if owner != PARENT_BID && owner != TOP_BID {
                    return None;
                }
                let mut reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                resolve_owner_child(&mut reg, owner, Some(host_nid), None)
            }),
        )?;
    }
    // BUG-1643: слот предка-владельца хоста для bid соседа (frameElement и
    // навигация соседа идут через документ владельца); для остальных bid —
    // null.
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_owner_bid",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                (SIBLING_BASE..SELF_BID).contains(&bid)
                    .then(|| reg.siblings.get((bid - SIBLING_BASE) as usize).map(|s| s.owner))
                    .flatten()
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
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                reg.frames.iter().filter(|b| b.host_nid != DETACHED_HOST).count() as u32
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
        // BUG-921: снимок имени контекста, сделанный при регистрации биндинга
        // (`register_parent_document`/`register_frame_document`), а не живой
        // атрибут хоста — не гейтится `accessible`, потому что это СВОЁ имя
        // контекста, а не чтение чужого документа.
        rt.register_native(
            "_lumen_f_name",
            into_v8_fn1(move |bid: u32| -> Option<String> {
                let reg = reg.lock().unwrap_or_else(|e| e.into_inner());
                resolve_slot(&reg, bid).and_then(|b| b.name.clone())
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
                // BUG-1198: непрозрачное происхождение адресата не равно
                // никакому сериализованному — доставляет только '*'.
                let matches = match target_origin.as_str() {
                    "*" => true,
                    _ if binding.opaque_id.is_some() => false,
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
                    b if b >= SIBLING_BASE => SourceKind::Sibling(reg.self_key.unwrap_or(0)),
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
                let mut reg = reg.lock().unwrap_or_else(|e| e.into_inner());
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
                //
                // BUG-1198: отправитель с непрозрачным происхождением даёт
                // `origin === "null"` и идентичность этого происхождения
                // (`opaque`) — по ней шим строит `Origin.from(event)`.
                // BUG-1643: сосед-отправитель ищется в реестре предка, а это
                // мутация слотов — до того, как `sender_origin` займёт `reg`.
                let sibling_bids: Vec<Option<u32>> = taken
                    .iter()
                    .map(|m| match m.source {
                        SourceKind::Sibling(k) => [PARENT_BID, TOP_BID]
                            .into_iter()
                            .find_map(|owner| resolve_owner_child(&mut reg, owner, None, Some(k))),
                        _ => None,
                    })
                    .collect();
                let sender_origin = |b: &FrameDocBinding| match b.opaque_id {
                    Some(id) => ("null".to_owned(), Some(id)),
                    None => (binding_origin(&b.url, &reg.self_origin), None),
                };
                let items: Vec<serde_json::Value> = taken
                    .into_iter()
                    .zip(sibling_bids)
                    .map(|(m, sibling_bid)| {
                        let (source_bid, (origin, opaque)) = match m.source {
                            SourceKind::Parent => (
                                reg.parent.as_ref().map(|_| PARENT_BID),
                                reg.parent
                                    .as_ref()
                                    .map(sender_origin)
                                    .unwrap_or_default(),
                            ),
                            SourceKind::ChildDoc(doc_key) => {
                                match reg
                                    .frames
                                    .iter()
                                    .position(|b| Arc::as_ptr(&b.doc) as usize == doc_key)
                                {
                                    Some(j) => (Some(j as u32), sender_origin(&reg.frames[j])),
                                    // Отправителя нет в прямых слотах получателя
                                    // (внук → top): source = null, origin пустой.
                                    None => (None, (String::new(), None)),
                                }
                            }
                            // BUG-1643: сосед; SELF_BID (сообщение себе через
                            // чужой фасад) разрешает шим в настоящий window.
                            SourceKind::Sibling(_) => match sibling_bid {
                                Some(SELF_BID) => (Some(SELF_BID), (reg.self_origin.clone(), None)),
                                Some(b) => (
                                    Some(b),
                                    resolve_slot(&reg, b).map(sender_origin).unwrap_or_default(),
                                ),
                                None => (None, (String::new(), None)),
                            },
                        };
                        serde_json::json!({
                            "bid": source_bid,
                            "origin": origin,
                            "opaque": opaque,
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
                    find_element_by_tag(d, "body").map(|n| n.raw())
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
                    find_element_by_tag(d, "head").map(|n| n.raw())
                }, None)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_document_element",
            into_v8_fn1(move |bid: u32| -> Option<u32> {
                with_accessible_doc(&reg, bid, |d| d.document_element().map(|n| n.raw()), None)
            }),
        )?;
    }
    // OBJECT-1 срез 5: тип под-документа — по нему `getSVGDocument()`
    // отличает SVG-документ (`image/svg+xml`) от HTML.
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_content_type",
            into_v8_fn1(move |bid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| d.content_type().to_owned(), String::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_referrer",
            into_v8_fn1(move |bid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    d.document_referrer().unwrap_or_default().to_owned()
                }, String::new())
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
                    .map(|n| n.raw())
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
                    lumen_layout::query_all(d, &sel).into_iter().next().map(|n| n.raw())
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
                        .map(|n| n.raw())
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
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_raw(nid), &sel)
                        .into_iter()
                        .next()
                        .map(|n| n.raw())
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
                    lumen_layout::query_all_scoped(d, lumen_dom::NodeId::from_raw(nid), &sel)
                        .into_iter()
                        .map(|n| n.raw())
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
                    d.get(lumen_dom::NodeId::from_raw(nid))
                        .children
                        .iter()
                        .filter(|&&c| matches!(&d.get(c).data, lumen_dom::NodeData::Element { .. }))
                        .map(|&c| c.raw())
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
                    let id = lumen_dom::NodeId::from_raw(nid);
                    d.get(id).parent.and_then(|pid| {
                        matches!(&d.get(pid).data, lumen_dom::NodeData::Element { .. })
                            .then(|| pid.raw())
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
                    d.get(lumen_dom::NodeId::from_raw(nid))
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
                    d.get(lumen_dom::NodeId::from_raw(nid))
                        .get_attr(&name)
                        .is_some()
                }, false)
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_attr_names",
            into_v8_fn2(move |bid: u32, nid: u32| -> Vec<String> {
                with_accessible_doc(&reg, bid, |d| {
                    match &d.get(lumen_dom::NodeId::from_raw(nid)).data {
                        lumen_dom::NodeData::Element { attrs, .. } => {
                            attrs.iter().map(|a| a.name.local.to_string()).collect()
                        }
                        _ => Vec::new(),
                    }
                }, Vec::new())
            }),
        )?;
    }
    {
        let reg = Arc::clone(&registry);
        rt.register_native(
            "_lumen_f_tag",
            into_v8_fn2(move |bid: u32, nid: u32| -> String {
                with_accessible_doc(&reg, bid, |d| {
                    match &d.get(lumen_dom::NodeId::from_raw(nid)).data {
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
                        &d.get(lumen_dom::NodeId::from_raw(nid)).data,
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
                        &d.get(lumen_dom::NodeId::from_raw(nid)).data,
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
                    collect_text_content(d, lumen_dom::NodeId::from_raw(nid))
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
  // BUG-1643: окно самого читающего контекста / диапазон bid-ов соседей
  // (зеркало frame_bridge::SELF_BID/SIBLING_BASE).
  var SELF_BID = 0xFFFFFFFD;
  var SIBLING_BASE = 0x80000000;

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
  function isSiblingBid(bid) { return bid >= SIBLING_BASE && bid < SELF_BID; }

  // Слот, чьим документом владеет хост этого окна, если хост лежит не в
  // документе текущего контекста: предок для фасада предка, предок-владелец
  // для фасада соседа (BUG-1643); null — хост в собственном документе.
  function hostBid(bid) {
    if (isAncestorBid(bid)) return bid;
    if (isSiblingBid(bid)) {
      var o = _lumen_f_owner_bid(bid);
      return (o === null || o === undefined) ? null : o;
    }
    return null;
  }

  // BUG-1643: окно/документ iframe, прочитанного через фасад предка
  // (`parent.document.getElementById(id)`). Хоста этого iframe в реестре
  // ребёнка нет — натив достаёт биндинг из реестра предка (frame_bridge.rs,
  // resolve_owner_child). Не iframe, фасад не предка и нет биндинга — null.
  function foreignFrameBid(bid, nid) {
    if (!isAncestorBid(bid) || _lumen_f_tag(bid, nid) !== 'iframe') return null;
    var r = _lumen_f_sibling_binding(bid, nid);
    return (r === null || r === undefined) ? null : r;
  }
  function foreignFrameWindow(bid, nid) {
    var r = foreignFrameBid(bid, nid);
    if (r === null) return null;
    return r === SELF_BID ? window : winFacade(r);
  }
  function foreignFrameDocument(bid, nid) {
    var r = foreignFrameBid(bid, nid);
    if (r === null) return null;
    if (r === SELF_BID) return window.document;
    return _lumen_f_accessible(r) ? docFacade(r) : null;
  }

  // GAP-NAVCTX срез 8: `fr.contentWindow.location.href = url` (и .assign/
  // .replace) — навигация ЧЕРЕЗ фасад, не через `iframe.src=` родителя. Пишем
  // прямо в атрибут `src` хоста: тот же `delta.changed`, который уже разбирает
  // `frame_dynamic.rs::poll_dynamic_frames` (срез 6/7) — второй `frame-load` их
  // силами, без нового пути навигации. hostNid лежит в документе родителя
  // ВСЕГДА (тег `<iframe>` не бывает внутри собственного под-документа); для
  // обычного bid это документ ТЕКУЩЕГО контекста (доступен напрямую), для
  // bid-предка — документ, за которым закреплён биндинг (доступен только
  // через бридж-натив).
  //
  // GAP-NAVCTX срез 9: срез 8 писал `url` дословно в `src` хоста, и хост
  // резолвил его относительно СВОЕЙ базы (документа-ВЛАДЕЛЬЦА, как обычный
  // `src=`) — расходится со спекой, которая резолвит навигацию против базы
  // НАВИГИРУЮЩЕГО (читающего фасад) документа. Этот скрипт целиком выполняется
  // в JS-реалме читателя (`winFacade`/`navigateFrameHost` — функции того же
  // замыкания, что и весь шим документа), поэтому `_lumen_document_base_url()`
  // здесь — база именно читателя. Резолвим `url` против неё ДО записи в `src`
  // хоста: получив уже абсолютный URL, хост его не переразрешает, и правило
  // "база хоста" из среза 8 больше ни на что не влияет.
  function navigateFrameHost(bid, hostNid, url) {
    if (hostNid === null || hostNid === undefined) return;
    var resolved = String(url);
    if (typeof _url_resolve === 'function' && typeof _lumen_document_base_url === 'function') {
      var r = _url_resolve(resolved, _lumen_document_base_url());
      if (r) resolved = r;
    }
    var hb = hostBid(bid);
    if (hb !== null) {
      _lumen_f_set_attr(hb, hostNid, 'src', resolved);
    } else if (typeof _lumen_make_element === 'function') {
      var el = _lumen_make_element(hostNid);
      if (el) el.setAttribute('src', resolved);
    }
  }

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

  // BUG-970: `element.attributes` (DOM §4.9.1 NamedNodeMap) over a
  // cross-frame facade node. Mirrors `_lumen_make_named_node_map` in
  // WEB_API_SHIM (BUG-732) but reads/writes through the `_lumen_f_*` bridge
  // natives instead of the local-document ones, and its `Attr`-like entries
  // are plain objects rather than instances of the shim's global `Attr` —
  // the bridge has no cross-frame identity for attribute nodes, only for
  // elements. Live: re-reads `_lumen_f_attr_names` on every access, so it
  // tracks the child's own `setAttribute`/`removeAttribute` without being
  // rebuilt, same as the main document's map.
  function frameAttr(bid, nid, name) {
    function value() { var v = _lumen_f_attr(bid, nid, name); return v !== null && v !== undefined ? v : ''; }
    function setValue(v) { _lumen_f_set_attr(bid, nid, name, String(v)); }
    return {
      name: name,
      nodeName: name,
      localName: name,
      prefix: null,
      namespaceURI: null,
      nodeType: 2,
      specified: true,
      get value()        { return value(); },
      set value(v)       { setValue(v); },
      get nodeValue()     { return value(); },
      set nodeValue(v)    { setValue(v); },
      get textContent()   { return value(); },
      set textContent(v)  { setValue(v); },
      get ownerElement() { return frameElem(bid, nid); },
    };
  }

  function frameNamedNodeMap(bid, nid) {
    function names() { return _lumen_f_attr_names(bid, nid); }
    var methods = {
      item: function(i) { var list = names(); i = i >>> 0; return i < list.length ? frameAttr(bid, nid, list[i]) : null; },
      getNamedItem: function(n) {
        var name = String(n);
        return _lumen_f_has_attr(bid, nid, name) ? frameAttr(bid, nid, name) : null;
      },
      getNamedItemNS: function(ns, n) { return methods.getNamedItem(n); },
      setNamedItem: function(attr) {
        if (!attr || typeof attr.name !== 'string') {
          throw new TypeError('setNamedItem: argument is not an Attr');
        }
        var prev = methods.getNamedItem(attr.name);
        _lumen_f_set_attr(bid, nid, attr.name, String(attr.value));
        return prev;
      },
      setNamedItemNS: function(attr) { return methods.setNamedItem(attr); },
      // The returned node must keep reporting the removed value — `frameAttr`
      // is a live view keyed on `name` still being present, so it would read
      // back '' the instant the attribute is gone. Snapshot the value first.
      removeNamedItem: function(n) {
        var name = String(n);
        if (!_lumen_f_has_attr(bid, nid, name)) {
          throw (typeof DOMException === 'function')
            ? new DOMException('No attribute named ' + name, 'NotFoundError')
            : new Error('No attribute named ' + name);
        }
        var value = _lumen_f_attr(bid, nid, name);
        _lumen_f_remove_attr(bid, nid, name);
        return {
          name: name, nodeName: name, localName: name, prefix: null,
          namespaceURI: null, nodeType: 2, specified: true,
          value: value, nodeValue: value, textContent: value,
          ownerElement: null,
        };
      },
      removeNamedItemNS: function(ns, n) { return methods.removeNamedItem(n); },
    };
    return new Proxy({}, {
      get: function(target, prop) {
        if (prop === 'length') return names().length;
        if (typeof prop === 'string' && Object.prototype.hasOwnProperty.call(methods, prop)) {
          return methods[prop];
        }
        if (typeof prop === 'string' && /^\d+$/.test(prop)) return methods.item(Number(prop));
        if (typeof prop === 'string') return methods.getNamedItem(prop) || undefined;
        if (prop === Symbol.iterator) {
          return function() {
            var list = names();
            var i = 0;
            return { next: function() {
              return i < list.length ? { value: frameAttr(bid, nid, list[i++]), done: false } : { value: undefined, done: true };
            } };
          };
        }
        return undefined;
      },
      has: function(target, prop) {
        if (prop === 'length' || prop === Symbol.iterator) return true;
        if (typeof prop === 'string' && Object.prototype.hasOwnProperty.call(methods, prop)) return true;
        if (typeof prop === 'string' && /^\d+$/.test(prop)) return Number(prop) < names().length;
        return typeof prop === 'string' && _lumen_f_has_attr(bid, nid, prop);
      },
    });
  }

  function frameElem(bid, nid) {
    if (nid === null || nid === undefined || nid < 0) return null;
    var cache = elems[bid];
    if (!cache) { cache = {}; elems[bid] = cache; }
    var cached = cache[nid];
    if (cached) return cached;
    // BUG-1147: views over this node's `style`/`class`/`data-*` attributes,
    // built on first read and kept for the facade's lifetime
    // (`el.style === el.style`), like `_lumen_wrapper_slot` of the main shim.
    var views = {};
    function view(key, make) {
      if (views[key] === undefined) {
        views[key] = typeof make === 'function' ? make(nid, bid) : undefined;
      }
      return views[key];
    }
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
      // BUG-970: DOM §4.9.1 `NamedNodeMap` over this node's attributes — not
      // cached (unlike the main document's `attributes`, BUG-732), the map
      // itself is a live Proxy so a fresh one is cheap and always current.
      get attributes() { return frameNamedNodeMap(bid, nid); },
      // BUG-1147: the main shim's own `CSSStyleDeclaration`/`DOMTokenList`/
      // `DOMStringMap`, bound to this sub-document through their `fbid`
      // argument — every read and write goes through the `_lumen_f_*` attribute
      // natives, so the child's cascade sees `style.cssText = …` and the
      // parent sees the child's own later `setAttribute('style', …)`. The
      // `typeof` guard in `view` covers minimal test isolates without the shim.
      get style() {
        return view('style', typeof _lumen_make_style === 'function' ? _lumen_make_style : undefined);
      },
      // Web IDL [PutForwards=cssText], same as the main shim (BUG-494).
      set style(v) { var st = this.style; if (st) st.cssText = String(v); },
      get classList() {
        return view('classList', typeof _lumen_make_class_list === 'function' ? _lumen_make_class_list : undefined);
      },
      get dataset() {
        return view('dataset', typeof _lumen_make_dataset === 'function' ? _lumen_make_dataset : undefined);
      },
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
      // BUG-1643: `<iframe>` предка, прочитанный через его фасад; у остальных
      // элементов и фасадов свойства нет, как и раньше.
      get contentWindow() {
        return _lumen_f_tag(bid, nid) === 'iframe' ? foreignFrameWindow(bid, nid) : undefined;
      },
      get contentDocument() {
        return _lumen_f_tag(bid, nid) === 'iframe' ? foreignFrameDocument(bid, nid) : undefined;
      },
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

  // OBJECT-1 срез 4: компонент URL под-документа для фасада Location.
  // Нераспознаваемый URL (пустой — документа ещё нет) даёт пустые компоненты
  // и origin 'null', как у непрозрачного происхождения.
  function locationPart(bid, part) {
    if (!_lumen_f_accessible(bid)) {
      throw (typeof DOMException === 'function')
        ? new DOMException('Blocked a frame from accessing a cross-origin frame.', 'SecurityError')
        : new TypeError('cross-origin frame location');
    }
    var u = null;
    try { u = new URL(String(_lumen_f_url(bid) || '')); } catch (e) {}
    if (u === null) return part === 'origin' ? 'null' : '';
    return u[part];
  }

  function docFacade(bid) {
    var cached = docs[bid];
    if (cached) return cached;
    var d = {};
    // Same binding marker the element facades carry (frameElem): lets
    // same-realm APIs that take a Document — IntersectionObserver's `root`
    // (BUG-626) — recognise this facade. Non-enumerable, so the facade's
    // visible key set is unchanged.
    Object.defineProperty(d, '__bid__', { value: bid, enumerable: false });
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
    Object.defineProperty(d, 'contentType',       { get: function() { return _lumen_f_content_type(bid); }, configurable: true });
    // BUG-1121: строка, а не undefined. BUG-1231: значение — реферер запроса
    // `<iframe src>`, выставленный shell'ом на документ ребёнка.
    Object.defineProperty(d, 'referrer',          { get: function() { return _lumen_f_referrer(bid); }, configurable: true });
    // Ребёнок получил window load ещё в срезе 1 — readyState к моменту доступа
    // всегда «complete»; отдельного трекинга переходов срез 2 не ведёт.
    Object.defineProperty(d, 'readyState',        { get: function() { return 'complete'; }, configurable: true });
    Object.defineProperty(d, 'defaultView',       { get: function() { return winFacade(bid); }, configurable: true });
    // OBJECT-1 срез 4: `document.location` — тот же Location, что у окна
    // (HTML LS §3.1.1; документ под фасадом всегда активен в своём контексте).
    Object.defineProperty(d, 'location',          { get: function() { return winFacade(bid).location; }, configurable: true });
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
    // BUG-979: self-references resolve to `wins[bid]` (the PROXY this
    // function returns), not the raw `w` object being built here — a plain
    // `w.window = w` would make `contentWindow.window !== contentWindow`
    // once `winFacade` starts wrapping `w` in a Proxy below.  Safe as a
    // getter: nothing reads these before `wins[bid]` is populated at the end
    // of this same synchronous call.
    Object.defineProperty(w, 'window', { get: function() { return wins[bid]; }, configurable: true });
    Object.defineProperty(w, 'self', { get: function() { return wins[bid]; }, configurable: true });
    Object.defineProperty(w, 'frames', { get: function() { return wins[bid]; }, configurable: true });
    // parent/top зависят от того, ЧЕЙ фасад читают и откуда. Фасад предка
    // (PARENT_BID/TOP_BID в изоляте ребёнка) ведёт вверх только к top
    // (`parent.top === top`, `top.parent === top`): промежуточных предков
    // изолят ребёнка не знает, поэтому у фрейма глубины ≥ 3 `parent.parent`
    // — сразу top. Раньше фасад родителя был сам себе parent, и цикл поиска
    // TCF-локатора `while (w !== top) w = w.parent` во внуке (pubmatic
    // user_sync.html) крутился вечно. Фасад дочернего фрейма, читаемый из
    // родителя, отсылает к настоящему окну читающего контекста (срез 2).
    Object.defineProperty(w, 'parent', {
      get: function() {
        if (isAncestorBid(bid)) return topOfContext();
        if (isSiblingBid(bid)) return winFacade(_lumen_f_owner_bid(bid));
        return typeof window !== 'undefined' ? window : null;
      },
      configurable: true,
    });
    Object.defineProperty(w, 'top', {
      get: function() { return topOfContext(); },
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
        var hb = hostBid(bid);
        if (hb !== null) return frameElem(hb, _lumen_f_host(bid));
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
        // Для дочернего фрейма — имя, запомненное при регистрации: правка
        // атрибута хоста после создания контекста его не переименовывает
        // (HTML LS §7.2.3; та же семантика, что у window.name ребёнка, BUG-921).
        var hb = hostBid(bid);
        var a = hb !== null
          ? _lumen_f_attr(hb, hostNid, 'name')
          : _lumen_f_name(bid);
        return (a === null || a === undefined) ? '' : a;
      },
      configurable: true,
    });
    // OBJECT-1 срез 4: один и тот же объект Location на окно (HTML LS §7.10.1:
    // `w.location === w.location`, и `document.location` фасада документа —
    // он же) с компонентами URL (origin/protocol/host/…). Компоненты читаются
    // только same-origin: чужой фасад отвечает SecurityError, как настоящий
    // cross-origin Location (§7.2.3.3); href на запись и assign/replace
    // остаются доступны — навигация чужого фрейма разрешена.
    var loc = {};
    Object.defineProperty(loc, 'href', {
      get: function() { return _lumen_f_url(bid); },
      set: function(v) { navigateFrameHost(bid, hostNid, String(v)); },
      configurable: true,
    });
    ['origin', 'protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash']
      .forEach(function(part) {
        Object.defineProperty(loc, part, {
          get: function() { return locationPart(bid, part); },
          configurable: true,
        });
      });
    loc.toString = function() { return _lumen_f_url(bid); };
    loc.assign = function(v) { navigateFrameHost(bid, hostNid, String(v)); };
    loc.replace = function(v) { navigateFrameHost(bid, hostNid, String(v)); };
    loc.reload = function() {};
    Object.defineProperty(w, 'location', {
      get: function() { return loc; },
      set: function(v) { navigateFrameHost(bid, hostNid, String(v)); },
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
    var proxied = wrapWinFacadeGlobals(w, bid);
    // GAP-ORIGIN: `Origin.from(contentWindow)` — only a same-origin window
    // hands out its origin; a cross-origin or opaque-sandboxed one has none
    // the caller may see (`TypeError`). An `about:` child inherits the origin
    // of the context that reads it, which `accessible` already vouched for.
    if (typeof _lumen_origin_register_source === 'function') {
      _lumen_origin_register_source(proxied, function() {
        if (!_lumen_f_accessible(bid)) return null;
        var u = _lumen_f_url(bid);
        return u.slice(0, 6) === 'about:' ? globalThis : u;
      });
    }
    wins[bid] = proxied;
    return proxied;
  }

  // BUG-979: `w` above only ever answers the fixed IDL-property set it was
  // built with — same-origin script of the framed document can declare ANY
  // other global (a function, a plain variable), and until this wrapper
  // those were unreachable through `contentWindow.foo`/named-window access,
  // even though HTML LS grants unrestricted same-origin access. `get`
  // defers to the real own properties first (so `document`/`parent`/`top`/…
  // above are untouched and no native round-trip happens for them, or for
  // anything already on `Object.prototype` like `toString`), and only for a
  // genuine miss asks the Rust bridge for the peer's real `globalThis[prop]`
  // (`_lumen_f_global_get`/`_lumen_f_global_call` — `frame_bridge_globals.rs`).
  // Symbols always fall through untouched (well-known symbols like
  // `Symbol.toPrimitive`, WeakMap keys, …) — cross-isolate lookup only
  // makes sense for string property names.
  function wrapWinFacadeGlobals(w, bid) {
    return new Proxy(w, {
      get: function(target, prop, receiver) {
        if (typeof prop === 'symbol' || Reflect.has(target, prop)) {
          return Reflect.get(target, prop, receiver);
        }
        var r = _lumen_f_global_get(bid, String(prop));
        if (r.kind === 'value') { return r.value; }
        if (r.kind === 'function') {
          return function() {
            var args = Array.prototype.slice.call(arguments);
            // BUG-1099: `new w.Ctor(...)` — объект пира через границу
            // изолятов не передать (возвращается только сериализованное
            // значение), поэтому конструкция идёт в этом контексте тем же
            // именем (Worker/SharedWorker/… есть в каждом контексте шима).
            if (new.target !== undefined) {
              var local = globalThis[String(prop)];
              if (typeof local === 'function') {
                return Reflect.construct(local, args);
              }
            }
            var cr = _lumen_f_global_call(bid, String(prop), args);
            if (cr.kind === 'error') { throw new Error(cr.message); }
            return cr.value;
          };
        }
        return undefined;
      },
    });
  }

  __lumen_C._lumen_frame_content_document = function(hostNid) {
    var bid = bidOrNull(hostNid);
    if (bid === null || !_lumen_f_accessible(bid)) return null;
    return docFacade(bid);
  };
  __lumen_C._lumen_frame_content_window = function(hostNid) {
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
    // BUG-1198: в полном шиме `top` — unforgeable (BUG-587), переопределить
    // его нельзя; его геттер читает этот хук. Прямое определение остаётся для
    // минимальных изолятов без шима, где `top` ещё настраиваемый.
    __lumen_C._lumen_frame_top = topOfContext;
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
    // window.name фрейма — имя контекста, запомненное ОДИН РАЗ при его
    // создании из атрибута name хоста (HTML LS §7.2.3, BUG-921): дальнейшие
    // правки атрибута контекст не переименовывают, только явное присваивание
    // `window.name =` или новая навигация фрейма.
    var __customName = null;
    try {
      Object.defineProperty(window, 'name', {
        get: function() {
          if (__customName !== null) return __customName;
          var p = _lumen_parent_binding();
          if (p === null || p === undefined) {
            return __prevName === undefined ? '' : __prevName;
          }
          var a = _lumen_f_name(p);
          return (a === null || a === undefined) ? '' : a;
        },
        set: function(v) { __customName = String(v == null ? '' : v); },
        configurable: true,
      });
    } catch (e) {}
  }

  // Родитель зарегистрирован: включить parent/top/frameElement/name.
  __lumen_C._lumen_frame_install_hierarchy = function() {
    if (typeof window === 'undefined') return;
    installHierarchyAccessors();
  };

  // Фрейм зарегистрирован: индексный (`window[idx]`) + именованный
  // (`window[имя]`) доступники окна фрейма; живой length. Вызывается из
  // V8JsRuntime::register_frame_document с индексом биндинга; порядок
  // регистрации = порядок документа (спечный tree order). Именованный
  // доступ покрывает ТОЛЬКО iframe (embed/form/img/object — не бриджевая
  // территория).
  __lumen_C._lumen_frame_install_index = function(idx) {
    if (typeof window === 'undefined') return;
    installLengthAccessor();
    try {
      var host = _lumen_frame_host_at(idx);
      if (host === null || host === undefined) return;
      var mk = function(h) {
        return function() { return _lumen_frame_content_window(h); };
      };
      // BUG-589: `window`'s indexed [[DefineOwnProperty]] now rejects every
      // array-index define (WebIDL: no indexed property setter -> always
      // false, even for a supported index) — this flag is this one call's
      // carve-out to still install the getter itself (see
      // `named_access.rs::indexed_define_trusted`).
      __lumen_C._lumen_indexed_define_trusted = true;
      try {
        Object.defineProperty(window, String(idx), { get: mk(host), configurable: true });
      } finally {
        __lumen_C._lumen_indexed_define_trusted = false;
      }
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
  __lumen_C._lumen_frame_pump_messages = function() {
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
            _lumen_deliver_frame_message(m.data, m.origin, source, m.opaque);
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
  __lumen_C._lumen_frame_mirror_resource = function(nid, type) {
    if (typeof nid !== 'number' || nid < 0) return false;
    if (typeof type !== 'string' || !type || type.length > 128) return false;
    if (typeof _lumen_f_queue_parent_resource !== 'function') return false;
    try { return !!_lumen_f_queue_parent_resource(nid, type); } catch (e) { return false; }
  };
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests;
