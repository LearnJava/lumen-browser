//! FRAME-8: фреймы, которыми распоряжается скрипт ПОСЛЕ первичного прохода
//! [`frames::load_frame_sub_documents`] — вставленные из обработчика
//! события/таймера/rAF, или уже вставленные, чей `src` скрипт поменял позже.
//! Симптом и разбор — `bugs/BUG-885-OPEN.md`.
//!
//! HTML LS §4.8.5 «process the iframe attributes» запускается при вставке
//! host-элемента в документ и при каждом изменении его `src`, а не один раз
//! при разборе документа. `load_frame_sub_documents` — тот единственный
//! разовый проход (`page_pipeline.rs`, до первого `window.load`); этот модуль
//! добирает то, что он пропустил, тем же сигналом, на котором уже держится
//! restyle после JS-мутации (`dom_dirty`/`take_frame_dom_dirty`) — каждый
//! вызов [`Lumen::poll_dynamic_frames`] стоит рядом с местом, где этот флаг
//! уже потреблён для relayout, а не заводит отдельное потребление.
//!
//! Загрузка НОВОГО фрейма уходит на фоновый поток тем же приёмом, что и
//! навигация по ссылке (FRAME-4 срез 3, `frame_links.rs`) — сеть и парсинг не
//! должны блокировать UI-поток. [`Lumen::pending_new_frames`] бронирует
//! `(host_doc, host)` на время полёта, чтобы повторный скан до ответа не
//! запустил вторую загрузку того же host-элемента.
//!
//! Смена `src` УЖЕ загруженного фрейма переиспользует [`Lumen::navigate_frame_to`]
//! целиком (включая запись в историю — по спеке навигация фрейма через `src`
//! ничем не отличается от навигации по ссылке, кликнутой ребёнком), только
//! адрес резолвится базой ХОЗЯИНА, а не базой документа, где кликнули (клика
//! здесь не было вовсе).
//!
//! Скан ([`Lumen::poll_dynamic_frames`]) стоит рядом с обоими существующими
//! потребителями `dom_dirty` — синхронной веткой (`LUMEN_NO_ENGINE_THREAD=1`)
//! и движковым потоком, включённым по умолчанию с ADR-023
//! (`relayout.rs::pump_raf_engine_thread`). Сам спавн фонового потока для
//! НОВОГО фрейма намеренно отделён от скана на один полный проход event loop
//! — см. doc-comment [`Lumen::dispatch_pending_frame_loads`].

use crate::*;

/// Один снятый, но ещё не отправленный на фоновый поток запрос загрузки
/// нового фрейма (FRAME-8) — см. doc-comment [`Lumen::dispatch_pending_frame_loads`].
pub(crate) struct PendingFrameLoad {
    prep: frame_dynamic_load::FrameNewLoadPrep,
    is_top: bool,
}

impl Lumen {
    /// Просканировать СТРАНИЦУ и каждый живой фрейм на новые/изменившиеся
    /// дочерние `<iframe>`/`<frame>` и запустить их загрузку (FRAME-8).
    ///
    /// Вызывающая сторона уже держит булев `dom_dirty`, добытый для restyle —
    /// зови отсюда, а не заводи отдельный опрос: `collect_iframes` внутри
    /// [`frames::scan_dynamic_frames`] — полный проход дерева, и его цена
    /// оправдана только когда что-то в дереве действительно тронуто.
    pub(crate) fn poll_dynamic_frames(&mut self) {
        let Some(env) = self.frame_env.clone() else { return };
        let Some(page_doc) = self.layout_source.as_ref().map(|s| Arc::clone(&s.document)) else {
            return;
        };
        let page_base = env.page_base.clone();
        let page_js = self.clone_js_ctx();
        self.poll_dynamic_frames_in(&page_doc, true, 0, &page_base, page_js.as_ref());

        // Снимок индексов: `self.frames` уже плоский список по всем уровням
        // вложенности, поэтому один проход по нему покрывает и глубокие
        // фреймы — у любого из них может появиться собственный ребёнок.
        for idx in 0..self.frames.len() {
            let Some((doc, base, depth, js)) = self
                .frames
                .get(idx)
                .map(|h| (Arc::clone(&h.doc), h.base.clone(), h.depth, h.js.clone()))
            else {
                continue;
            };
            self.poll_dynamic_frames_in(&doc, false, depth + 1, &base, js.as_ref());
        }
    }

    /// Один уровень скана — страница ИЛИ один живой фрейм. `depth` — глубина,
    /// на которой встанут НОВЫЕ дети (`FrameHandle::depth`); дальше
    /// `MAX_FRAME_DEPTH` рекурсия сама не уходит — тот же предел, что у
    /// `spawn_frame` внутри `load_frame_sub_documents`. Только детектирует и
    /// ставит в очередь [`Self::pending_frame_load_dispatch`] — сам фоновый
    /// поток спавнит [`Self::dispatch_pending_frame_loads`] на СЛЕДУЮЩЕМ тике.
    fn poll_dynamic_frames_in(
        &mut self,
        doc: &Arc<Mutex<Document>>,
        is_top: bool,
        depth: usize,
        base: &ResourceBase,
        js: Option<&Arc<dyn PersistentJs>>,
    ) {
        let delta = frame_dynamic_load::scan_dynamic_frames(doc, &self.frames, is_top);
        for info in delta.new {
            let prep = frame_dynamic_load::prepare_new_frame_load(info, doc, depth, base, js);
            let key = frame_dynamic_load::new_frame_load_key(&prep);
            if self
                .pending_new_frames
                .iter()
                .any(|(d, h)| *h == key.1 && Arc::ptr_eq(d, &key.0))
            {
                continue;
            }
            self.pending_new_frames.push((Arc::clone(&key.0), key.1));
            // FRAME-8: спавн потока НЕ здесь — см. doc-comment
            // `Self::dispatch_pending_frame_loads`.
            self.pending_frame_load_dispatch.push(PendingFrameLoad { prep, is_top });
        }
        for (idx, new_src) in delta.changed {
            self.navigate_frame_to(idx, &new_src, base);
        }
    }

    /// Реально запустить фоновые потоки для запросов, снятых
    /// [`Self::poll_dynamic_frames`] на ПРЕДЫДУЩЕМ тике (FRAME-8).
    ///
    /// Спавн намеренно отделён от скана на один полный проход event loop: скан
    /// стоит внутри `pump_raf_engine_thread`, вызванного сразу по завершении
    /// движкового rAF/task-хода — создание НОВОГО V8-изолята (`spawn_frame` →
    /// `run_scripts_with_dom`) ad-hoc потоком в этот самый момент наблюдаемо
    /// вешало весь процесс (движковый поток переставал тикать таймеры
    /// страницы без единого паник-сообщения — не доказанный до конца V8/
    /// движковый-поток race, а не архитектурная необходимость отсрочки).
    /// Тот же спавн из обработчика клика (`frame_links.rs::replace_frame_document`)
    /// такой гонки не ловит — там нет соседства с только что завершившимся
    /// движковым ходом. Вызывается из начала `on_about_to_wait`, до какой-либо
    /// накачки JS в этом тике.
    pub(crate) fn dispatch_pending_frame_loads(&mut self) {
        if self.pending_frame_load_dispatch.is_empty() {
            return;
        }
        let Some(env) = self.frame_env.clone() else {
            self.pending_frame_load_dispatch.clear();
            return;
        };
        let Some(page_doc) = self.layout_source.as_ref().map(|s| Arc::clone(&s.document)) else {
            self.pending_frame_load_dispatch.clear();
            return;
        };
        for pending in self.pending_frame_load_dispatch.drain(..) {
            let PendingFrameLoad { prep, is_top } = pending;
            let (host_doc, host) = frame_dynamic_load::new_frame_load_key(&prep);
            let page_doc = Arc::clone(&page_doc);
            let env = env.clone();
            let proxy = self.load_proxy.clone();
            std::thread::spawn(move || {
                let handles = frame_dynamic_load::run_new_frame_load(&prep, &page_doc, &env);
                let _ = proxy.send_event(LoadEvent::FrameNewLoadDone { host_doc, host, is_top, handles });
            });
        }
    }

    /// Применить ответ фонового потока [`Self::poll_dynamic_frames`]
    /// (`LoadEvent::FrameNewLoadDone`).
    pub(crate) fn on_frame_new_load_done(
        &mut self,
        host_doc: &Arc<Mutex<Document>>,
        host: NodeId,
        is_top: bool,
        handles: Vec<frames::FrameHandle>,
    ) {
        self.pending_new_frames
            .retain(|(d, h)| !(*h == host && Arc::ptr_eq(d, host_doc)));
        if !frame_dynamic_load::apply_new_frame_load(&mut self.frames, host_doc, host, is_top, handles) {
            return;
        }
        self.refresh_frames(None);
    }
}
