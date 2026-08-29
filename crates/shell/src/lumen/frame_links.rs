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
    /// фрейм; `_parent` у вложенного — его фрейм-родитель.
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
    /// Именованный контекст (`target="content"`) и `_blank` попадают в одну
    /// ветку намеренно: спека для имени, которого нет, предписывает СОЗДАТЬ
    /// окно, а частичная поддержка — «это имя понимаю, то нет» — была бы хуже
    /// честного отказа с логом.
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
        LinkTarget::NewWindow
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
    pub(crate) fn navigate_frame_to(&mut self, idx: usize, href: &str, nav_base: &ResourceBase) {
        let Some(env) = self.frame_env.clone() else {
            eprintln!("iframe: навигация '{href}' без окружения загрузки страницы — пропуск");
            return;
        };
        let Some(page_doc) = self.layout_source.as_ref().map(|s| Arc::clone(&s.document)) else {
            return;
        };
        // НЕ `self.js_ctx`: с ADR-023 движковый поток включён по умолчанию и
        // держит хэндл у себя, оставляя это поле пустым — см.
        // [`Lumen::clone_js_ctx`]. С `self.js_ctx` родитель молча не узнавал
        // ни о новом под-документе, ни о `load` на своём `<iframe>`.
        let page_js = self.clone_js_ctx();
        if !frames::navigate_frame(
            &mut self.frames,
            idx,
            href,
            nav_base,
            &page_doc,
            &env,
            page_js.as_ref(),
        ) {
            return;
        }
        // Индекс фрейма под курсором указывал на выброшенный хэндл (срез 16 —
        // та же причина, по которой его сбрасывает смена страницы).
        self.hovered_frame = None;
        self.refresh_frames(None);
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
