//! Декодер video/audio-контейнеров (`video/mp4`, `video/webm`,
//! `video/ogg`) через hand-rolled FFI к FFmpeg — GAP-MEDIADECODE,
//! [ADR-030](../../../docs/decisions/ADR-030-media-codec-strategy-ffmpeg.md).
//!
//! Реализует [`lumen_core::ext::VideoDecoder`]/`VideoDecodeSession`, объявленные
//! до подключения FFmpeg как trait-anchor (срез 2).
//!
//! **Требует feature `ffmpeg`** (выключена по умолчанию) — без неё крейт
//! компилируется, но не экспортирует ничего, и `cargo build --workspace`
//! не трогает FFmpeg ни на одной машине. С feature `ffmpeg` дополнительно
//! требуется `FFMPEG_DIR`, указывающий на FFmpeg shared-dev дистрибутив
//! (headers + import libs + DLL), и те же DLL на `PATH` в рантайме — см.
//! `build.rs` и ADR-030.
//!
//! Пока не подключено (следующий срез): вызов из
//! `crates/js/src/video_bindings.rs`'s resource-selection алгоритма —
//! этот крейт сегодня самодостаточен и ничем в workspace не используется.

#[cfg(feature = "ffmpeg")]
mod decoder;
#[cfg(feature = "ffmpeg")]
mod ffi;

#[cfg(feature = "ffmpeg")]
pub use decoder::FfmpegVideoDecoder;

#[cfg(all(test, feature = "ffmpeg"))]
mod tests {
    use lumen_core::ext::{VideoDecodeSession, VideoDecoder};

    use super::decoder::FfmpegSession;
    use super::FfmpegVideoDecoder;

    /// `2x2-green.webm` — тот же файл, на котором ADR-030 (срез 3)
    /// подтвердил декодирующий цикл в scratch-PoC; здесь то же самое
    /// проверяется через реальный тип этого крейта и через custom-IO
    /// путь (`&[u8]`, не путь на диске).
    #[test]
    fn decodes_first_frame_of_2x2_green_webm() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/css/css-sizing/aspect-ratio/support/2x2-green.webm"
        );
        let bytes = std::fs::read(path).expect("тестовый .webm должен быть на диске");

        let decoder = FfmpegVideoDecoder;
        let mut session = decoder.open(&bytes).expect("open() должен декодировать 2x2-green.webm");
        assert_eq!(session.dimensions(), (2, 2));

        let rgba = session.frame_at(0.0).expect("frame_at(0.0) должен вернуть первый кадр");
        assert_eq!(rgba.len(), 2 * 2 * 4);
        // Тот же зелёный пиксель, что и в ADR-030 срез 3: rgba[0..4] == [0, 127, 0, 255].
        assert_eq!(&rgba[0..4], &[0, 127, 0, 255]);
    }

    /// `test.mp4` (H.264) декодируется через тот же путь, что и VP8/9 —
    /// подтверждает, что custom-IO слой не завязан на конкретный кодек.
    #[test]
    fn decodes_seek_in_test_mp4() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/css/css-ui/support/test.mp4"
        );
        let bytes = std::fs::read(path).expect("тестовый .mp4 должен быть на диске");

        let decoder = FfmpegVideoDecoder;
        let mut session = decoder.open(&bytes).expect("open() должен декодировать test.mp4");
        assert_eq!(session.dimensions(), (400, 300));
        assert!(session.duration_secs().unwrap_or(0.0) > 6.0);

        let rgba = session.frame_at(1.0).expect("frame_at(1.0) должен сработать после seek");
        assert_eq!(rgba.len(), 400 * 300 * 4);
    }

    /// GAP-MEDIADECODE срез 12: `test.mp4` несёт AAC-дорожку 22050 Hz
    /// стерео (подтверждено `ffprobe` и живым прогоном в scratch-PoC) —
    /// `audio_track()` должен вернуть эти значения без декодирования PCM.
    #[test]
    fn audio_track_detects_aac_stereo_in_test_mp4() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/css/css-ui/support/test.mp4"
        );
        let bytes = std::fs::read(path).expect("тестовый .mp4 должен быть на диске");

        let decoder = FfmpegVideoDecoder;
        let session = decoder.open(&bytes).expect("open() должен декодировать test.mp4");

        let audio = session.audio_track().expect("test.mp4 несёт AAC-дорожку");
        assert_eq!(audio.sample_rate, 22050);
        assert_eq!(audio.channels, 2);
    }

    /// `2x2-green.webm` не несёт аудиодорожки вовсе — `audio_track()`
    /// должен вернуть `None`, а не ошибку (немой контейнер — не дефект).
    #[test]
    fn audio_track_is_none_for_silent_webm() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/css/css-sizing/aspect-ratio/support/2x2-green.webm"
        );
        let bytes = std::fs::read(path).expect("тестовый .webm должен быть на диске");

        let decoder = FfmpegVideoDecoder;
        let session = decoder.open(&bytes).expect("open() должен декодировать 2x2-green.webm");

        assert_eq!(session.audio_track(), None);
    }

    /// GAP-MEDIADECODE срез 13: AAC-дорожка действительно декодируется в
    /// PCM, не только детектируется (срез 12). `test.mp4` для этого не
    /// подходит, хоть и несёт AAC 22050 Hz стерео — независимая проверка
    /// (`ffmpeg -af volumedetect` и сырой PCM-дамп через `ffmpeg`) вскрыла,
    /// что его аудиодорожка бит-в-бит тишина на всём протяжении файла (сама
    /// фикстура такая, не дефект декодера — срез 12 знал только
    /// `sample_rate`/`channels`, не содержимое). Используется другая
    /// WPT-фикстура с реальным (не тихим) звуком —
    /// `fetch/api/request/destination/resources/dummy_video.mp4` (h264 +
    /// AAC mono 44100 Hz, `mean_volume=-3.5dB` по независимому промеру).
    #[test]
    fn decode_audio_pcm_returns_nonsilent_samples_for_dummy_video_mp4() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/fetch/api/request/destination/resources/dummy_video.mp4"
        );
        let bytes = std::fs::read(path).expect("тестовый .mp4 должен быть на диске");

        let decoder = FfmpegVideoDecoder;
        let mut session = decoder.open(&bytes).expect("open() должен декодировать dummy_video.mp4");

        let pcm = session
            .decode_audio_pcm(4096)
            .expect("dummy_video.mp4 несёт декодируемую AAC-дорожку");
        assert!(!pcm.is_empty(), "декодер не вернул ни одного PCM-сэмпла");
        assert!(
            pcm.iter().any(|&s| s != 0),
            "аудиодорожка dummy_video.mp4 не должна декодироваться как тишина"
        );
    }

    /// `2x2-green.webm` не несёт аудиодорожки — `decode_audio_pcm` должен
    /// вернуть диагностируемую ошибку, а не панику/пустой `Ok`.
    #[test]
    fn decode_audio_pcm_errors_for_silent_webm() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/css/css-sizing/aspect-ratio/support/2x2-green.webm"
        );
        let bytes = std::fs::read(path).expect("тестовый .webm должен быть на диске");

        let decoder = FfmpegVideoDecoder;
        let mut session = decoder.open(&bytes).expect("open() должен декодировать 2x2-green.webm");

        assert!(session.decode_audio_pcm(1024).is_err());
    }

    /// GAP-MEDIADECODE срез 19: `frame_at` (видео-seek) до этого среза
    /// сбрасывал `avcodec_flush_buffers` только для видео-кодека — аудио-
    /// кодек-контекст после перемотки демуксера оставался с внутренним
    /// буфером декодера от позиции ДО seek (bit-reservoir/переупорядочивание
    /// кадров AAC), что могло подмешать в `decode_audio_pcm` пару кадров с
    /// прежней позиции раньше настоящих пост-seek пакетов. Регрессия на сам
    /// факт восстановления: после перемотки НАЗАД (`frame_at(0.0)` после
    /// `frame_at(1.0)`, т. е. против направления декодирования) следующий
    /// `decode_audio_pcm` должен успешно вернуть непустой PCM снова, а не
    /// упасть в EOF-ошибку демуксера (симптом стейлового состояния декодера,
    /// наблюдавшийся до фикса при последовательных seek).
    #[test]
    fn decode_audio_pcm_recovers_after_backward_seek() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/fetch/api/request/destination/resources/dummy_video.mp4"
        );
        let bytes = std::fs::read(path).expect("тестовый .mp4 должен быть на диске");

        let decoder = FfmpegVideoDecoder;
        let mut session = decoder.open(&bytes).expect("open() должен декодировать dummy_video.mp4");

        session.frame_at(1.0).expect("frame_at(1.0) должен сработать");
        let first = session
            .decode_audio_pcm(2048)
            .expect("PCM после первого seek должен декодироваться");
        assert!(!first.is_empty());

        session.frame_at(0.0).expect("frame_at(0.0) (seek назад) должен сработать");
        let second = session
            .decode_audio_pcm(2048)
            .expect("PCM после seek назад должен декодироваться заново, а не падать в EOF");
        assert!(
            second.iter().any(|&s| s != 0),
            "аудио после seek назад не должно декодироваться как тишина"
        );
    }

    /// GAP-MEDIADECODE срез 21: непрерывное воспроизведение вперёд (каждый
    /// следующий `frame_at` чуть дальше предыдущего декодированного кадра,
    /// как реальный тик `tick_video_ffmpegs`) не должно делать ни одного
    /// `av_seek_frame` — демуксер уже стоит там, где нужно продолжать
    /// чтение. Регрессия на срез 20's остаток («per-tick троттлинг не
    /// пересмотрен») — до этого среза каждый из этих вызовов делал
    /// seek+flush и передекодировал GOP заново.
    #[test]
    fn frame_at_sequential_forward_ticks_do_not_reseek() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/css/css-ui/support/test.mp4"
        );
        let bytes = std::fs::read(path).expect("тестовый .mp4 должен быть на диске");

        let mut session = FfmpegSession::open(bytes).expect("open() должен декодировать test.mp4");
        assert_eq!(session.seek_count(), 0, "open() сам по себе не должен seek'ать");

        // Имитация ~30fps троттлинга tick_video_ffmpegs: каждый тик чуть
        // дальше предыдущего декодированного кадра. Первые несколько тиков
        // догоняют B-frame reorder-delay `open()`'s первого кадра (его
        // реальный pts — не ровно 0.0, а несколько кадров вперёд — этот тест
        // не про эту границу, только про установившееся воспроизведение),
        // поэтому счётчик seek снимается ПОСЛЕ разгона, не с самого начала.
        for tick in 1..10 {
            let secs = f64::from(tick) * (1.0 / 30.0);
            session
                .frame_at(secs)
                .unwrap_or_else(|e| panic!("frame_at({secs}) должен сработать: {e}"));
        }

        let steady_state_seeks_before = session.seek_count();
        for tick in 10..30 {
            let secs = f64::from(tick) * (1.0 / 30.0);
            session
                .frame_at(secs)
                .unwrap_or_else(|e| panic!("frame_at({secs}) должен сработать без seek: {e}"));
        }

        assert_eq!(
            session.seek_count(),
            steady_state_seeks_before,
            "монотонное воспроизведение вперёд в установившемся режиме не должно вызывать av_seek_frame"
        );
    }

    /// Симметричный случай: перемотка НАЗАД (типичный `<video loop>`/JS
    /// `currentTime = 0`) по-прежнему должна идти через настоящий
    /// `av_seek_frame` — срез 21 не должен молча пропускать реальные seek'и.
    #[test]
    fn frame_at_backward_jump_still_reseeks() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/wpt/css/css-ui/support/test.mp4"
        );
        let bytes = std::fs::read(path).expect("тестовый .mp4 должен быть на диске");

        let mut session = FfmpegSession::open(bytes).expect("open() должен декодировать test.mp4");
        // > MAX_FORWARD_SCAN_SECS (2.0) от pts~0 сразу после open() —
        // намеренный большой прыжок вперёд, не троттлинг-тик.
        session.frame_at(5.0).expect("frame_at(5.0) должен сработать");
        assert_eq!(session.seek_count(), 1, "прыжок далеко вперёд от pts=0 должен seek'ать");

        // `frame_at(0.0)` попал бы в кэш `first_frame` и вернулся бы без
        // единого вызова декодера — перемотка назад проверяется на секунду,
        // которая не совпадает с кэшированным первым кадром.
        session.frame_at(1.0).expect("frame_at(1.0) (перемотка назад) должен сработать");
        assert_eq!(session.seek_count(), 2, "перемотка назад должна была сделать настоящий seek");
    }
}
