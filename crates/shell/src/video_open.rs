//! BUG-1838: открытие медиа-сессии `<video>` (fetch контейнера → `VideoDecoder::open` →
//! первый кадр) — в фоновом потоке, а не в обработчике кадра.
//!
//! Раньше [`Lumen::tick_video_sessions`] на каждый отложенный запрос загрузки синхронно
//! качал весь файл, поднимал Media Foundation/D3D11VA и декодировал кадр 0 — на okko.tv
//! это давало кадры по 3–11 с, каждый сразу после `video: загружен`, то есть страница на
//! секунды переставала отвечать после `window.load`. Здесь та же работа уезжает в
//! отсоединённый поток: сеть — как у прочих подресурсов, декодер — там же, где качали;
//! UI-поток по [`VideoOpenMsg`] только регистрирует готовый кадр, пишет intrinsic-размеры
//! и заводит сессию.
//!
//! Разделение шагов (п.1 бага) печатается в строке `video: загружен` — `fetch` / `open` /
//! `кадр` / `очередь`, чтобы следующий аудит видел цену каждого шага, а не только сумму.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lumen_core::ext::{EventSink, VideoDecodeSession, VideoDecoder};
use lumen_network::ReferrerPolicy;

use crate::DocCookieJar;
use crate::resource_base::ResourceBase;

/// Всё, что нужно воркеру, чтобы открыть контейнер `<video>`.
///
/// От документа здесь — только снятые на UI-потоке данные: база подресурса
/// (`<base href>` уже применён), перезаписанный `upgrade-insecure-requests`-URL,
/// referrer-политика и cookie-jar. Сам `Document` воркеру не нужен и не передаётся:
/// единственное чтение `&Document` (CSP/база) делает вызывающая сторона.
pub(crate) struct VideoOpenJob {
    /// Индекс узла в арене документа-хозяина (BUG-1796: не обязательно верхнего).
    pub(crate) nid: u32,
    /// Сырой `src` — ключ `load_failures` и диагностики, как до BUG-1838.
    pub(crate) src: String,
    /// URL запроса после `upgrade-insecure-requests` (иначе — `src`).
    pub(crate) fetch_url: String,
    pub(crate) base: ResourceBase,
    pub(crate) sink: Arc<dyn EventSink>,
    pub(crate) cookie_jar: Option<DocCookieJar>,
    pub(crate) referrer_policy: ReferrerPolicy,
    pub(crate) decoder: Arc<dyn VideoDecoder>,
}

/// Раскладка времени открытия по шагам (BUG-1838 п.1) — то, чего у бага не было.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct VideoOpenMarks {
    /// Сеть: весь файл контейнера.
    pub(crate) fetch_ms: u64,
    /// `VideoDecoder::open` — демультиплексор + инициализация декодера ОС
    /// (на Windows поднимает Media Foundation и D3D11VA).
    pub(crate) open_ms: u64,
    /// Первый кадр (`frame_at(0.0)`) — первое реальное обращение к декодеру.
    pub(crate) frame_ms: u64,
}

impl VideoOpenMarks {
    /// Сумма шагов воркера, мс.
    fn total_ms(self) -> u64 {
        self.fetch_ms + self.open_ms + self.frame_ms
    }

    /// Поля строки `video: загружен`: `fetch 4200, open 5900, кадр 60, очередь 6`.
    /// `total_ms` — всё время от отправки воркера до применения на UI-потоке;
    /// `очередь` — разница, то есть ожидание цикла событий (обычно единицы мс).
    pub(crate) fn log_fields(self, total_ms: u64) -> String {
        let queue_ms = total_ms.saturating_sub(self.total_ms());
        format!(
            "fetch {}, open {}, кадр {}, очередь {}",
            self.fetch_ms, self.open_ms, self.frame_ms, queue_ms
        )
    }
}

/// Открытая сессия и её первый кадр — результат, который принимает UI-поток.
pub(crate) struct OpenedVideo {
    pub(crate) nid: u32,
    /// Готовая сессия декодирования: trait объявлен `Send`, поэтому сессия,
    /// открытая воркером, переезжает в `Lumen::video_sessions` как есть.
    pub(crate) session: Box<dyn VideoDecodeSession>,
    /// Кадр 0 в RGBA8 — регистрируется под `video:{nid}`, если у узла нет `poster`.
    pub(crate) image: lumen_image::Image,
    /// Длительность контейнера в мс (`0` — неизвестна), как `cycle_ms` в playback.
    pub(crate) duration_ms: u64,
    pub(crate) marks: VideoOpenMarks,
}

/// На каком шаге открытие встало — по этому признаку UI-поток выбирает прежнюю
/// строку лога (`пропуск` / `ошибка декодирования` / `не декодирован первый кадр`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VideoOpenFailure {
    /// Сеть: файл контейнера не скачался.
    Fetch,
    /// `VideoDecoder::open` не открыл контейнер.
    Decoder,
    /// Контейнер открылся, но кадр 0 не декодировался.
    FirstFrame,
}

/// Ответ воркера: открыл или объяснил, почему нет.
pub(crate) enum VideoOpenMsg {
    Opened(OpenedVideo),
    Failed {
        nid: u32,
        /// Сырой `src` — для строки лога.
        src: String,
        /// Диагностика для `load_failures` (её же читает JS).
        reason: String,
        failure: VideoOpenFailure,
        /// COMPAT-MEDIA-OSDEC-WIN-BAR: не хватило компонента ОС для кодека
        /// контейнера — заполняет только неудача `open`, где байты уже есть.
        /// Решение о плашке принимает UI-поток (`media_bar` + релейаут хрома).
        os_codec: Option<lumen_media_native::MissingOsCodec>,
    },
}

impl VideoOpenMsg {
    pub(crate) fn nid(&self) -> u32 {
        match self {
            Self::Opened(o) => o.nid,
            Self::Failed { nid, .. } => *nid,
        }
    }
}

/// Событие цикла: воркер закончил, результат можно применять на UI-потоке.
///
/// `tab_id`/`generation` — снятые при отправке (BUG-1214): результат, приехавший
/// после навигации или ухода вкладки в фон, выбрасывается. `doc`/`frame` — домен nid
/// (BUG-1796): по ним принимающая сторона ещё раз проверяет, что документ-хозяин
/// всё ещё жив, прежде чем писать в него intrinsic-размеры.
pub(crate) struct VideoOpenedEvent {
    pub(crate) tab_id: usize,
    pub(crate) generation: u64,
    pub(crate) doc: Option<Arc<Mutex<lumen_dom::Document>>>,
    pub(crate) frame: Option<usize>,
    /// Момент отправки — от него считается полное время до применения.
    pub(crate) spawned_at: Instant,
    pub(crate) msg: VideoOpenMsg,
}

/// Бронь «открытие этого узла уже в полёте» — второй поток на тот же nid не нужен.
pub(crate) struct VideoOpenReservation {
    pub(crate) tab_id: usize,
    pub(crate) nid: u32,
}

/// Занять бронь открытия. `false` — открытие этого `(вкладка, nid)` уже в полёте:
/// второй `load()` того же узла второй поток не запускает, первый всё равно
/// перезапишет сессию.
pub(crate) fn reserve_video_open(
    opens: &mut Vec<VideoOpenReservation>,
    tab_id: usize,
    nid: u32,
) -> bool {
    if opens.iter().any(|r| r.tab_id == tab_id && r.nid == nid) {
        return false;
    }
    opens.push(VideoOpenReservation { tab_id, nid });
    true
}

/// Снять бронь — по приходу результата либо когда поток не удалось запустить.
pub(crate) fn release_video_open(opens: &mut Vec<VideoOpenReservation>, tab_id: usize, nid: u32) {
    opens.retain(|r| !(r.tab_id == tab_id && r.nid == nid));
}

/// BUG-1838: открыть контейнер в отсоединённом потоке; `done` получает результат
/// ровно один раз (или не получает, если поток не удалось запустить).
///
/// # Errors
/// `std::io::Error` от `std::thread::Builder::spawn` — ОС не дала поток; вызывающая
/// сторона обязана снять бронь и записать неудачу сама.
pub(crate) fn spawn_video_open(
    job: VideoOpenJob,
    done: impl FnOnce(VideoOpenMsg) + Send + 'static,
) -> std::io::Result<()> {
    spawn_video_open_with(job, fetch_job, done)
}

/// Тело production-фетча — та же функция, что до BUG-1838 вызывалась синхронно.
fn fetch_job(job: &VideoOpenJob) -> Result<Vec<u8>, String> {
    crate::subresources::fetch_video_bytes(
        &job.fetch_url,
        &job.base,
        &job.sink,
        job.cookie_jar.clone(),
        job.referrer_policy,
    )
    .map_err(|e| e.to_string())
}

/// [`spawn_video_open`] с подменяемым фетчем — вход для тестов (реальная сеть им не
/// нужна; проверяется именно то, что открытие ушло с вызывающего потока).
fn spawn_video_open_with<F>(
    job: VideoOpenJob,
    fetch: F,
    done: impl FnOnce(VideoOpenMsg) + Send + 'static,
) -> std::io::Result<()>
where
    F: FnOnce(&VideoOpenJob) -> Result<Vec<u8>, String> + Send + 'static,
{
    std::thread::Builder::new()
        .name(format!("lumen-video-open-{}", job.nid))
        .spawn(move || done(open_video(job, fetch)))?;
    Ok(())
}

/// Скачать, открыть, декодировать кадр 0 — всё, что раньше делал обработчик кадра.
fn open_video(
    job: VideoOpenJob,
    fetch: impl FnOnce(&VideoOpenJob) -> Result<Vec<u8>, String>,
) -> VideoOpenMsg {
    let t0 = Instant::now();
    let bytes = match fetch(&job) {
        Ok(b) => b,
        Err(reason) => {
            return VideoOpenMsg::Failed {
                nid: job.nid,
                src: job.src,
                reason,
                failure: VideoOpenFailure::Fetch,
                os_codec: None,
            };
        }
    };
    let t_fetch = t0.elapsed();
    let mut session = match job.decoder.open(&bytes) {
        Ok(s) => s,
        Err(reason) => {
            return VideoOpenMsg::Failed {
                nid: job.nid,
                src: job.src,
                reason,
                failure: VideoOpenFailure::Decoder,
                os_codec: lumen_media_native::missing_os_codec(&bytes),
            };
        }
    };
    let t_open = t0.elapsed();
    let (width, height) = session.dimensions();
    let rgba = match session.frame_at(0.0) {
        Ok(r) => r,
        Err(reason) => {
            return VideoOpenMsg::Failed {
                nid: job.nid,
                src: job.src,
                reason,
                failure: VideoOpenFailure::FirstFrame,
                os_codec: None,
            };
        }
    };
    let t_frame = t0.elapsed();
    let duration_ms = session.duration_secs().map_or(0, |s| (s * 1000.0) as u64);
    VideoOpenMsg::Opened(OpenedVideo {
        nid: job.nid,
        session,
        image: lumen_image::Image {
            width,
            height,
            format: lumen_image::PixelFormat::Rgba8,
            data: rgba,
            icc_profile: None,
            vector_source: None,
        },
        duration_ms,
        marks: VideoOpenMarks {
            fetch_ms: ms(t_fetch),
            open_ms: ms(t_open - t_fetch),
            frame_ms: ms(t_frame - t_open),
        },
    })
}

/// `Duration` → мс.
fn ms(d: Duration) -> u64 {
    d.as_millis() as u64
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::{OpenedVideo, VideoOpenJob, VideoOpenMsg, spawn_video_open_with};
    use lumen_core::ext::{AudioTrackInfo, VideoDecodeSession, VideoDecoder};

    struct FakeSession {
        width: u32,
        height: u32,
    }

    impl VideoDecodeSession for FakeSession {
        fn dimensions(&self) -> (u32, u32) {
            (self.width, self.height)
        }

        fn duration_secs(&self) -> Option<f64> {
            Some(1.5)
        }

        fn frame_at(&mut self, _secs: f64) -> Result<Vec<u8>, String> {
            Ok(vec![0u8; 4 * 2 * 2])
        }

        fn audio_track(&self) -> Option<AudioTrackInfo> {
            None
        }
    }

    struct FakeDecoder {
        /// Сколько «открывается» — тест на неблокирующий спавн держит его в сети.
        fail: Option<String>,
    }

    impl VideoDecoder for FakeDecoder {
        fn decoder_name(&self) -> &'static str {
            "fake"
        }

        fn mime_types(&self) -> &'static [&'static str] {
            &["video/mp4"]
        }

        fn open(&self, _bytes: &[u8]) -> Result<Box<dyn VideoDecodeSession>, String> {
            match &self.fail {
                Some(e) => Err(e.clone()),
                None => Ok(Box::new(FakeSession { width: 2, height: 2 })),
            }
        }
    }

    fn job(decoder: FakeDecoder) -> VideoOpenJob {
        VideoOpenJob {
            nid: 7,
            src: "clip.mp4".to_string(),
            fetch_url: "https://example.test/clip.mp4".to_string(),
            base: crate::resource_base::ResourceBase::url_base(
                "https://example.test/".to_string(),
            ),
            sink: std::sync::Arc::new(crate::event_sink::StdoutEventSink),
            cookie_jar: None,
            referrer_policy: lumen_network::ReferrerPolicy::default_policy(),
            decoder: std::sync::Arc::new(decoder),
        }
    }

    /// Гвоздь бага: `spawn_video_open_with` возвращается, пока фетч ещё висит, —
    /// вызывающий (UI) поток больше не платит секунды за `open`/первый кадр.
    #[test]
    fn opening_does_not_block_the_caller() {
        let (release, wait) = mpsc::channel::<()>();
        let (tx, rx) = mpsc::channel();
        let started = std::time::Instant::now();
        spawn_video_open_with(
            job(FakeDecoder { fail: None }),
            move |_job| {
                // «Сеть» висит, пока тест не отпустит.
                let _ = wait.recv();
                Ok(vec![1, 2, 3])
            },
            move |msg| {
                let _ = tx.send(msg);
            },
        )
        .expect("thread spawned");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "спавн вернулся только через {:?}",
            started.elapsed()
        );
        assert!(rx.try_recv().is_err(), "результат не мог приехать до отпускания фетча");
        release.send(()).expect("release fetch");
        match rx.recv_timeout(Duration::from_secs(5)).expect("worker replied") {
            VideoOpenMsg::Opened(OpenedVideo { nid, image, duration_ms, .. }) => {
                assert_eq!(nid, 7);
                assert_eq!((image.width, image.height), (2, 2));
                assert_eq!(image.data.len(), 4 * 2 * 2);
                assert_eq!(duration_ms, 1_500);
            }
            VideoOpenMsg::Failed { reason, .. } => panic!("неожиданная неудача: {reason}"),
        }
    }

    /// Неудача открытия приезжает с причиной и nid — UI-поток кладёт её в
    /// `load_failures` (JS читает оттуда `error`).
    #[test]
    fn decoder_failure_carries_reason_and_nid() {
        let (tx, rx) = mpsc::channel();
        spawn_video_open_with(
            job(FakeDecoder { fail: Some("нет декодера H.264".to_string()) }),
            |_job| Ok(vec![0u8; 8]),
            move |msg| {
                let _ = tx.send(msg);
            },
        )
        .expect("thread spawned");
        match rx.recv_timeout(Duration::from_secs(5)).expect("worker replied") {
            VideoOpenMsg::Failed { nid, reason, src, .. } => {
                assert_eq!(nid, 7);
                assert_eq!(src, "clip.mp4");
                assert_eq!(reason, "нет декодера H.264");
            }
            VideoOpenMsg::Opened(_) => panic!("декодер обязан был провалиться"),
        }
    }

    /// Сетевой отказ не доходит до декодера и не теряет сообщение.
    #[test]
    fn fetch_failure_is_reported_as_is() {
        let (tx, rx) = mpsc::channel();
        spawn_video_open_with(
            job(FakeDecoder { fail: None }),
            |_job| Err("HTTP 404".to_string()),
            move |msg| {
                let _ = tx.send(msg);
            },
        )
        .expect("thread spawned");
        match rx.recv_timeout(Duration::from_secs(5)).expect("worker replied") {
            VideoOpenMsg::Failed { reason, .. } => assert_eq!(reason, "HTTP 404"),
            VideoOpenMsg::Opened(_) => panic!("фетч обязан был провалиться"),
        }
    }

    /// Бронь не даёт второму `load()` того же узла запустить второй поток; nid
    /// соседней вкладки — не помеха, а снятая бронь снова занимается.
    #[test]
    fn reservation_is_per_tab_and_node() {
        use super::{release_video_open, reserve_video_open};
        let mut opens = Vec::new();
        assert!(reserve_video_open(&mut opens, 1, 42));
        assert!(!reserve_video_open(&mut opens, 1, 42), "тот же узел — уже в полёте");
        assert!(reserve_video_open(&mut opens, 2, 42), "nid другой вкладки — не тот же узел");
        release_video_open(&mut opens, 1, 42);
        assert!(reserve_video_open(&mut opens, 1, 42), "снятая бронь снова свободна");
        assert_eq!(opens.len(), 2);
    }
}
