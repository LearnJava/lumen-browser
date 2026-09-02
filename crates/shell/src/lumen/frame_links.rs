//! Ссылки под-документа фрейма: навигация самого фрейма, его фрагментная
//! прокрутка и выход наружу через `target` (BUG-480 срез 19).
//!
//! Срез 16 довёл клик до ребёнка как СОБЫТИЕ, срез 18 — собственное поведение
//! элементов управления формы. Ссылка не работала ни там, ни там по одной и
//! той же причине: ранний возврат в [`Lumen::handle_click_at`] пропускает
//! разбор ссылки вместе с разбором формы, потому что единственный узел
//! СТРАНИЦЫ под этой точкой — сам `<iframe>`.
//!
//! Разбор адреса переиспользуется целиком ([`crate::links`]): «фрагмент или
//! загрузка», «навигабельна ли схема», «тот же ли это документ» — вопросы про
//! URL, а не про то, кто по ссылке кликнул, и расходиться ответам внутри
//! одного движка нельзя. Своё здесь только одно — КУДА идёт результат.

use crate::*;

/// Куда ведёт ссылка ребёнка — результат разбора её `target`
/// (HTML LS §4.6.3, «the rules for choosing a navigable»).
///
/// Тем же правилом выбирает адресата и отправка формы (срез 20): §4.10.21.4
/// шаг 15 ссылается ровно на §4.6.3, поэтому у `<a target>` и `<form target>`
/// не может быть двух разных ответов внутри одного движка.
pub(crate) enum LinkTarget {
    /// Фрейм с этим индексом: `_self`, отсутствующий атрибут — сам кликнутый
    /// фрейм; `_parent` у вложенного — его фрейм-родитель; именованный
    /// `target`, совпавший с `name` живого фрейма (срез 24), — тот фрейм.
    Frame(usize),
    /// Страница: `_top`, а для фрейма глубины 0 и `_parent` — его родитель и
    /// есть верхнее окно.
    Page,
    /// Новое окно (`_blank` или имя, которое здесь некому носить). Этот движок
    /// вспомогательных browsing context не создаёт вовсе ([BUG-883]).
    NewWindow,
}

impl Lumen {
    /// Разобрать клик по ссылке в под-документе фрейма `idx`.
    ///
    /// `source_node` — узел, с которого начинается поиск `<a>`: тот же
    /// `HitTestResult::source_node`, что у страницы, то есть текстовый узел
    /// внутри инлайн-элемента, а не бокс.
    ///
    /// `true` — ссылка нашлась и адресат определён (даже если навигация затем
    /// отклонена схемой или сетью): вызывающей стороне это говорит, что клик
    /// разобран.
    pub(crate) fn frame_link_click(&mut self, idx: usize, source_node: NodeId) -> bool {
        let Some(handle) = self.frames.get(idx) else { return false };
        // База ребёнка КЛОНИРУЕТСЯ, а не заимствуется: ссылку резолвит документ,
        // в котором по ней кликнули, а дальше по коду `self.frames` берётся
        // изменяемо — заём поля пережить этого не может.
        let nav_base = handle.base.clone();
        let found = {
            let Ok(doc) = handle.doc.lock() else { return false };
            links::find_link(&doc, source_node).map(|(anchor, href)| {
                let target = doc.get(anchor).get_attr("target").unwrap_or_default().to_owned();
                (href, target)
            })
        };
        let Some((href, target_attr)) = found else { return false };
        match self.link_destination(idx, &target_attr) {
            LinkTarget::NewWindow => {
                eprintln!(
                    "iframe: ссылка '{href}' с target='{target_attr}' — вспомогательные окна не поддержаны (BUG-883)"
                );
                true
            }
            LinkTarget::Page => {
                // Адрес разрешается базой РЕБЁНКА, а уходит наверх: `_top`
                // меняет документ страницы, но ссылку написал ребёнок.
                self.navigate_page_from_frame(&href, &nav_base);
                true
            }
            LinkTarget::Frame(target_idx) => {
                self.navigate_frame_from_link(target_idx, target_idx == idx, &href, &nav_base);
                true
            }
        }
    }

    /// Разобрать `target` ссылки ребёнка.
    ///
    /// `_blank` — окно, которое движок не создаёт ([BUG-883]), без исключений:
    /// спека резервирует это имя, так что живой фрейм с таким `name` (если он
    /// вообще возможен) им не адресуется. Любое другое непустое имя сперва
    /// ищется среди живых фреймов (срез 24, [`Self::find_frame_by_name`]) — и
    /// только когда совпадения нет, движок честно отказывается СОЗДАТЬ новый
    /// browsing context, а не притворяется, что умеет часть.
    pub(crate) fn link_destination(&self, idx: usize, target: &str) -> LinkTarget {
        let t = target.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("_self") {
            return LinkTarget::Frame(idx);
        }
        if t.eq_ignore_ascii_case("_top") {
            return LinkTarget::Page;
        }
        if t.eq_ignore_ascii_case("_parent") {
            let Some(parent_doc) = self.frames.get(idx).and_then(|h| h.parent_doc.clone()) else {
                // Глубина 0: родитель фрейма и есть страница.
                return LinkTarget::Page;
            };
            return self
                .frames
                .iter()
                .position(|o| Arc::ptr_eq(&o.doc, &parent_doc))
                .map_or(LinkTarget::Page, LinkTarget::Frame);
        }
        if t.eq_ignore_ascii_case("_blank") {
            return LinkTarget::NewWindow;
        }
        match self.find_frame_by_name(t) {
            Some(named) => LinkTarget::Frame(named),
            None => LinkTarget::NewWindow,
        }
    }

    /// Найти живой фрейм по имени его host-элемента — `name="..."` на
    /// `<iframe>`/`<frame>` (HTML LS §7.3.2, «the rules for choosing a
    /// navigable», ветка совпадения по browsing context name), срез 24.
    ///
    /// Имя читается с host-узла в документе, где тот стоит: у фрейма глубины 0
    /// это страница ([`Lumen::layout_source`]), у вложенного — документ его
    /// фрейма-родителя ([`crate::frames::FrameHandle::parent_doc`]) — та же
    /// пара источников, что уже различает срез 19 у `_parent`. Совпадение
    /// регистрозависимое и точное, как у `id` ([`links::find_element_by_id`]);
    /// первый найденный фрейм побеждает — повторное имя у двух живых фреймов
    /// не гарантировано движком и не встречается ни в одном сценарии среза.
    pub(crate) fn find_frame_by_name(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let page_doc = self.layout_source.as_ref().map(|s| &s.document);
        for (idx, handle) in self.frames.iter().enumerate() {
            let Some(owner) = handle.parent_doc.as_ref().or(page_doc) else { continue };
            let Ok(doc) = owner.lock() else { continue };
            if doc.get(handle.host).get_attr("name") == Some(name) {
                return Some(idx);
            }
        }
        None
    }

    /// `target=_top`/`_parent` у фрейма глубины 0: навигация СТРАНИЦЫ по
    /// ссылке, написанной ребёнком.
    ///
    /// Дальше — обычный путь страницы: те же `fragment_only`/
    /// `is_navigable_href`, что у клика по её собственной ссылке.
    fn navigate_page_from_frame(&mut self, href: &str, nav_base: &ResourceBase) {
        if let Some(frag) = links::fragment_only(href) {
            self.navigate_fragment(frag.to_owned());
            return;
        }
        if !links::is_navigable_href(href) {
            eprintln!("iframe: ссылка '{href}' с target=_top не навигабельна — пропуск");
            return;
        }
        let resolved = nav_base.resolve_str(href);
        if let Some(frag) = links::same_document_fragment(self.current_display_url(), &resolved) {
            self.navigate_fragment(frag);
            return;
        }
        self.navigate_to(PageSource::from_arg(Some(&resolved)));
    }

    /// Навигация ФРЕЙМА по ссылке ребёнка.
    ///
    /// `same_frame` — кликнули в том же под-документе, который и меняется
    /// (`_self`): только тогда `#id` и «тот же адрес с другим фрагментом»
    /// означают прокрутку, а не загрузку. Для `_parent` целевой документ —
    /// чужой, и фрагмент в нём считать от адреса кликнувшего нельзя.
    fn navigate_frame_from_link(
        &mut self,
        idx: usize,
        same_frame: bool,
        href: &str,
        nav_base: &ResourceBase,
    ) {
        if same_frame && let Some(frag) = links::fragment_only(href) {
            self.frame_navigate_fragment(idx, frag);
            return;
        }
        if !links::is_navigable_href(href) {
            eprintln!("iframe: ссылка '{href}' внутри фрейма не навигабельна — пропуск");
            return;
        }
        if same_frame {
            let resolved = nav_base.resolve_str(href);
            let current = self.frames[idx].url.clone();
            if let Some(frag) = links::same_document_fragment(&current, &resolved) {
                self.frame_navigate_fragment(idx, &frag);
                return;
            }
        }
        self.navigate_frame_to(idx, href, nav_base);
    }

    /// Заменить под-документ фрейма `idx` документом по адресу `href`.
    ///
    /// После замены индекс `idx` уже НЕ адресует этот фрейм: хэндл выброшен, а
    /// новый (вместе с хэндлами своих вложенных фреймов) добавлен в конец
    /// списка. Поэтому пересчитываются вьюпорты ВСЕХ фреймов, а не одного:
    /// «пересчитай фрейм номер N» после навигации — вопрос без адресата.
    ///
    /// FRAME-4: навигация фрейма ВЕРХНЕГО УРОВНЯ (`parent_doc.is_none()`)
    /// заводит запись в joint session history страницы — `Alt+Left` после
    /// клика по ссылке внутри такого фрейма отменяет именно эту навигацию, а
    /// не перезагружает страницу целиком. Вложенные фреймы в срез не входят:
    /// адрес до перехода не запоминается, история их не видит. Сама подмена
    /// документа вынесена в [`Self::replace_frame_document`] — тем же путём
    /// идёт и обратная навигация по истории ([`Self::traverse_frame`]),
    /// которой новый push уже не нужен: он случился здесь, при первой
    /// навигации.
    pub(crate) fn navigate_frame_to(&mut self, idx: usize, href: &str, nav_base: &ResourceBase) {
        // Снимок identity+адреса ДО замены хэндла: после неё `idx` уже не
        // адресует этот фрейм (см. doc `replace_frame_document`).
        let history_step = self
            .frames
            .get(idx)
            .filter(|h| h.parent_doc.is_none())
            .map(|h| (h.host, h.url.clone()));
        if !self.replace_frame_document(idx, href, nav_base) {
            return;
        }
        let Some((host, prev_url)) = history_step else { return };
        self.nav_back.push(NavEntry {
            source: self.source.clone(),
            scroll_x: self.scroll_x,
            scroll_y: self.scroll_y,
            display_url: self.display_url.clone(),
            same_doc_state_json: None,
            nav_key: self.current_nav_key.clone(),
            frame_target: Some((host, prev_url)),
        });
        // Новая навигация обнуляет forward — тот же контракт, что у
        // `navigate_to` для страницы (HTML LS §7.2.1).
        self.nav_fwd.clear();
        self.commit_nav_state();
    }

    /// Общее тело подмены под-документа фрейма `idx`: и для обычной
    /// навигации по ссылке/форме ([`Self::navigate_frame_to`]), и для шага
    /// истории ([`Self::traverse_frame`]) — ЧТО происходит с хэндлами и
    /// вьюпортами от источника не зависит, разнится только запись в историю
    /// (у второго её нет — сам шаг историю не расширяет).
    ///
    /// FRAME-4 срез 3: сеть+парсинг+скрипты+layout ребёнка (раньше здесь же,
    /// синхронно на UI-потоке) теперь уходят в `std::thread::spawn` —
    /// `frames::run_frame_navigation` — тем же способом, каким уже грузится
    /// сама страница (`start_streaming_load`/`render_bytes`,
    /// `LoadEvent`+`EventLoopProxy`). Возвращает `true`, если запрос
    /// ПРИНЯТ (фрейм найден, окружение есть) — не «документ уже заменён»:
    /// сама подмена приходит позже, `LoadEvent::FrameNavDone`, и её
    /// применяет [`Self::on_frame_nav_done`]. История и сброс hover/focus/
    /// active читают только адрес и identity, известные ДО сети, поэтому
    /// синхронный «принят» — всё, что им нужно.
    fn replace_frame_document(&mut self, idx: usize, href: &str, nav_base: &ResourceBase) -> bool {
        let Some(env) = self.frame_env.clone() else {
            eprintln!("iframe: навигация '{href}' без окружения загрузки страницы — пропуск");
            return false;
        };
        let Some(page_doc) = self.layout_source.as_ref().map(|s| Arc::clone(&s.document)) else {
            return false;
        };
        // НЕ `self.js_ctx`: с ADR-023 движковый поток включён по умолчанию и
        // держит хэндл у себя, оставляя это поле пустым — см.
        // [`Lumen::clone_js_ctx`]. С `self.js_ctx` родитель молча не узнавал
        // ни о новом под-документе, ни о `load` на своём `<iframe>`.
        let page_js = self.clone_js_ctx();
        let Some(prep) = frames::prepare_frame_navigation(&self.frames, idx, &page_doc, &env, page_js.as_ref())
        else {
            return false;
        };
        let generation = frames::bump_frame_nav_generation(&mut self.frame_nav_requests, &prep.host_doc, prep.host);
        let (host_doc, host) = (Arc::clone(&prep.host_doc), prep.host);
        let href = href.to_owned();
        let nav_base = nav_base.clone();
        let proxy = self.load_proxy.clone();
        std::thread::spawn(move || {
            let old_doc = Arc::clone(&prep.old_doc);
            let handles = frames::run_frame_navigation(&prep, &href, &nav_base, &page_doc, &env);
            let _ = proxy.send_event(LoadEvent::FrameNavDone { host_doc, host, old_doc, generation, handles });
        });
        true
    }

    /// Применить ответ фонового потока навигации фрейма (FRAME-4 срез 3),
    /// `LoadEvent::FrameNavDone`.
    ///
    /// Отбрасывает ответ (ничего не меняя), если `(host_doc, host)` успел
    /// навигировать ещё раз, пока этот запрос летел — быстрый второй клик по
    /// другой ссылке того же фрейма не должен откатываться медленным ответом
    /// на первый. `apply_frame_navigation` отдельно отбрасывает ответ, чей
    /// `old_doc` уже не в `self.frames` (предок навигировал сам, страница
    /// перезагрузилась целиком) — тот же исход, что раньше давал `idx`,
    /// переставший существовать к моменту (синхронного) возврата.
    pub(crate) fn on_frame_nav_done(
        &mut self,
        host_doc: &Arc<Mutex<Document>>,
        host: NodeId,
        old_doc: &Arc<Mutex<Document>>,
        generation: u64,
        handles: Vec<frames::FrameHandle>,
    ) {
        if !frames::frame_nav_generation_current(&self.frame_nav_requests, host_doc, host, generation) {
            return;
        }
        if !frames::apply_frame_navigation(&mut self.frames, old_doc, handles) {
            return;
        }
        // Индекс фрейма под курсором указывал на выброшенный хэндл (срез 16 —
        // та же причина, по которой его сбрасывает смена страницы). Срез 23:
        // фокус и нажатие адресуют узел ТОГО ЖЕ выброшенного документа, а
        // `NodeId` уникален лишь внутри него — оставленная пара нашла бы в
        // новом дереве чужой узел с совпавшим индексом.
        self.hovered_frame = None;
        self.focused_frame = None;
        self.active_frame = None;
        // FRAME-6: same staleness — an overlay anchored to a node of the
        // discarded document would, after replacement, look up whatever node
        // happens to share its index in the NEW one.
        self.frame_color_picker = None;
        self.frame_date_picker = None;
        self.frame_select_dropdown = None;
        self.refresh_frames(None);
    }

    /// FRAME-4: применить ОДИН шаг истории — вернуть фрейм верхнего уровня с
    /// host-узлом `host` к документу `target_url`.
    ///
    /// Вызывается ТОЛЬКО из `navigate_back`/`navigate_forward` (`navigation.rs`)
    /// после того, как они уже сняли со стека запись с `frame_target`: это
    /// отмена/повтор уже случившейся навигации, а не новая — сам шаг новую
    /// запись не заводит. Возвращает адрес фрейма ДО перехода (вызывающая
    /// сторона кладёт его на противоположный стек, чтобы шаг можно было
    /// отыграть назад), либо `None`, если фрейм с таким host-узлом верхнего
    /// уровня уже не существует — запись истории в этом случае просто
    /// гасится, без видимого эффекта.
    pub(crate) fn traverse_frame(&mut self, host: NodeId, target_url: &str) -> Option<String> {
        let idx = self
            .frames
            .iter()
            .position(|h| h.host == host && h.parent_doc.is_none())?;
        let prev_url = self.frames[idx].url.clone();
        let nav_base = self.frames[idx].base.clone();
        self.replace_frame_document(idx, target_url, &nav_base)
            .then_some(prev_url)
    }

    /// Фрагментная навигация ВНУТРИ под-документа: `:target`, `location` и
    /// прокрутка ребёнка — без единого запроса.
    ///
    /// Зеркало [`Lumen::navigate_fragment`] страницы, и по тем же трём шагам:
    /// сообщить JS (`location` + `hashchange`), проставить `:target` в дереве,
    /// пересчитать каскад и только потом искать бокс цели — до пересчёта
    /// правило `:target` ещё не применено, и геометрия была бы вчерашней.
    ///
    /// Вызовы уходят прямым `eval_js` по хэндлу фрейма: `route_eval_js` знает
    /// только `self.js_ctx` — контекст СТРАНИЦЫ, — а `location` принадлежит
    /// окну ребёнка (та же причина у [`Lumen::frame_mouse_event`]).
    fn frame_navigate_fragment(&mut self, idx: usize, frag: &str) {
        let Some(handle) = self.frames.get(idx) else { return };
        let new_url = links::fragment_url(&handle.url, frag);
        #[cfg(feature = "v8")]
        if let Some(js) = handle.js.clone() {
            let escaped = new_url.replace('\\', "\\\\").replace('\'', "\\'");
            js.eval_js(&format!(
                "_lumen_dispatch_navigate('fragment', '{escaped}', true, true)"
            ));
            js.eval_js(&format!("_lumen_navigate_or_fragment('{escaped}', false)"));
        }
        {
            let Ok(mut doc) = self.frames[idx].doc.lock() else { return };
            if frag.is_empty() {
                doc.set_target::<String>(None);
            } else {
                doc.set_target(Some(frag.to_owned()));
            }
        }
        // Адрес хэндла — то, от чего следующий клик отмеряет «тот же документ».
        self.frames[idx].url = new_url;
        self.refresh_frames(Some(idx));
        if frag.is_empty() {
            self.apply_frame_scroll(idx, 0.0);
            return;
        }
        let node = {
            let Ok(doc) = self.frames[idx].doc.lock() else { return };
            links::find_element_by_id(&doc, frag)
        };
        let target_y = node.and_then(|n| {
            self.frames[idx]
                .layout
                .as_ref()
                .and_then(|lb| forms::find_box_rect(lb, n))
                .map(|r| r.y)
        });
        if let Some(y) = target_y {
            self.apply_frame_scroll(idx, y);
        }
    }
}
