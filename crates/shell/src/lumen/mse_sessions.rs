//! `<video src=blob:MediaSource>` (COMPAT-MSE): сессии воспроизведения поверх буферов `MediaSource`.
//!
//! У обычного `<video>` оболочка скачивает контейнер и открывает его целиком
//! (`tick_video_sessions`). У MSE байтов для скачивания нет: кадры приходят из
//! `appendBuffer` страницы в общий реестр `lumen_media_mse`, а нативы
//! `__lumen_mse_attach` / `__lumen_mse_detach` лишь сообщают, какой узел к какому
//! `MediaSource` привязан (`VideoGifStore::mse_attached`). Каждый тик оболочка
//! сверяет эту карту со своими сессиями:
//!
//! 1. появились дорожки → создаётся `playback` (пауза, размеры видеодорожки), страница
//!    видит `__lumen_video_ready` и отдаёт `loadedmetadata`;
//! 2. появились видеокадры → `lumen_media_native::open_mse` открывает сессию, дальше кадры
//!    и звук выдаёт общий цикл `tick_video_sessions`;
//! 3. каждый тик `cycle_ms` подтягивается к длительности источника (она растёт с буфером);
//! 4. узел отвязан → сессия, `playback` и аудиовыход снимаются.

use crate::*;

impl Lumen {
    /// Сверить привязки `MediaSource` с открытыми сессиями. Вызывается из
    /// [`Lumen::tick_video_sessions`] до цикла выдачи кадров.
    #[allow(clippy::unwrap_used)] // Mutex хранилища не отравляется: держится только на время копирования
    pub(crate) fn tick_mse_sessions(&mut self) {
        let attached: HashMap<u32, u64> = self.video_gif_store.mse_attached.lock().unwrap().clone();
        if attached.is_empty() && self.mse_sessions.is_empty() {
            return;
        }

        // Отвязанные (или перепривязанные к другому источнику) узлы.
        let stale: Vec<u32> = self
            .mse_sessions
            .iter()
            .filter(|(nid, id)| attached.get(nid) != Some(id))
            .map(|(nid, _)| *nid)
            .collect();
        for nid in stale {
            self.mse_sessions.remove(&nid);
            self.video_gif_store.playback.lock().unwrap().remove(&nid);
            self.video_gif_store.load_failures.lock().unwrap().remove(&nid);
            self.video_sessions.remove(&nid);
            self.video_last_ms.remove(&nid);
            self.video_audio_sinks.remove(&nid);
            self.video_last_audio_ms.remove(&nid);
            self.request_redraw();
        }

        let mut waiting = false;
        for (nid, id) in attached {
            let Some(shared) = lumen_media_mse::source(id) else { continue };
            let (tracks, has_frames, duration_us) = {
                let core = shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                (core.tracks(), core.has_video_frames(), core.effective_duration_us())
            };
            let cycle_ms = u64::try_from(duration_us / 1000).unwrap_or(0);

            if self.mse_sessions.get(&nid) != Some(&id) {
                if tracks.is_empty() {
                    waiting = true;
                    continue;
                }
                let (width, height) = tracks
                    .iter()
                    .find(|t| t.width > 0)
                    .map_or((0, 0), |t| (t.width, t.height));
                self.video_gif_store.load_failures.lock().unwrap().remove(&nid);
                self.video_gif_store.playback.lock().unwrap().insert(
                    nid,
                    lumen_js::video_gif_store::VideoPlaybackState {
                        paused: true,
                        position_ms: 0,
                        play_epoch_ms: None,
                        cycle_ms,
                        loop_count: 1,
                        width,
                        height,
                    },
                );
                self.mse_sessions.insert(nid, id);
                if width > 0
                    && let Some(src_ref) = self.layout_source.as_ref()
                {
                    let mut doc = src_ref.document.lock().unwrap();
                    let node_id = lumen_dom::NodeId::from_index(nid as usize);
                    if apply_intrinsic_size(
                        &mut doc,
                        node_id,
                        width,
                        height,
                        self.relayout_viewport().unwrap_or(Size::new(0.0, 0.0)),
                    ) {
                        self.note_shell_attr_writes(vec![nid]);
                    }
                }
                eprintln!("mse: привязан nid={nid} источник={id} ({width}×{height})");
                self.request_redraw();
            }

            if let Some(state) = self.video_gif_store.playback.lock().unwrap().get_mut(&nid) {
                state.cycle_ms = cycle_ms;
            }

            if self.video_sessions.contains_key(&nid) {
                continue;
            }
            // Ошибка открытия (нет декодера под кодек) не повторяется каждый тик: о ней
            // узнаёт страница через `__lumen_video_failed`.
            if self.video_gif_store.load_failures.lock().unwrap().contains_key(&nid) {
                continue;
            }
            if !has_frames {
                waiting = true;
                continue;
            }
            match lumen_media_native::open_mse(shared) {
                Ok(session) => {
                    eprintln!("mse: открыта сессия nid={nid}");
                    self.video_sessions.insert(nid, session);
                    self.video_last_ms.remove(&nid);
                    self.video_audio_sinks.remove(&nid);
                    self.video_last_audio_ms.remove(&nid);
                    self.request_redraw();
                }
                Err(e) => {
                    eprintln!("mse: не открыта сессия nid={nid}: {e}");
                    self.video_gif_store.load_failures.lock().unwrap().insert(nid, e);
                    self.request_redraw();
                }
            }
        }
        if waiting {
            // Кадры придут из задач страницы, которые сами перерисовку не просят.
            self.request_redraw();
        }
    }

    /// Снять все привязки MSE (навигация, смена вкладки).
    #[allow(clippy::unwrap_used)] // см. выше
    pub(crate) fn clear_mse_sessions(&mut self) {
        self.video_gif_store.mse_attached.lock().unwrap().clear();
        self.mse_sessions.clear();
    }
}
