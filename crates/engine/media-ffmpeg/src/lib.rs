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
    use lumen_core::ext::VideoDecoder;

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
}
