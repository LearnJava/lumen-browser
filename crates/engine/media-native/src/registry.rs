//! Реестр декодеров кодеков (ADR-035 п.1).
//!
//! Для каждого кодека реестр выбирает первый бэкенд, который *на этой машине*
//! умеет его открыть. Порядок в списке — приоритет: аппаратный декодер ОС →
//! программный декодер ОС → встроенный свободный. Бэкенды ОС
//! (`COMPAT-MEDIA-OSDEC-*`) добавляются в начало [`Registry::builtin`]; сейчас
//! в нём только встроенные: AV1 (`rav1d`, [`lumen_av1`]) и Vorbis (`lewton`).
//!
//! От реестра зависят три вещи, которые не должны расходиться: что открывает
//! [`crate::NativeVideoDecoder`], что отвечает `canPlayType` и что вернёт
//! `mediaCapabilities.decodingInfo` — «feature detection никогда не лжёт»
//! (ADR-034 п.2).

use std::sync::OnceLock;

use lumen_media_demux::{Codec, Packet, TrackInfo};

use crate::{av1, vorbis};

/// Декодированный видеокадр в порядке показа. Преобразование в RGBA отложено:
/// кадры, пропускаемые при перемотке к моменту между ключевыми, в него не нужны.
pub trait DecodedFrame: Send {
    /// Момент показа, мкс (метка пакета, породившего кадр).
    fn pts_us(&self) -> i64;
    /// Ширина кадра в пикселях.
    fn width(&self) -> u32;
    /// Высота кадра в пикселях.
    fn height(&self) -> u32;
    /// Кадр как RGBA8 (row-major, `width * height * 4` байт).
    ///
    /// # Errors
    /// Диагностика преобразования (плоскости не совпадают с размером кадра).
    fn to_rgba8(&self) -> Result<Vec<u8>, String>;
}

/// Покадровый декодер одной видеодорожки, созданный бэкендом.
pub trait VideoFrameDecoder: Send {
    /// Отправить пакет дорожки в декодер. `restart` — первый пакет после
    /// открытия или перемотки (декодер начинает с чистого состояния).
    ///
    /// # Errors
    /// Диагностика: пакет повреждён или не поддерживается.
    fn send(&mut self, packet: &Packet, restart: bool) -> Result<(), String>;
    /// Следующий готовый кадр в порядке показа; `None` — нужны новые пакеты.
    fn next_frame(&mut self) -> Option<Box<dyn DecodedFrame>>;
    /// Конец потока: вытолкнуть кадры, застрявшие в конвейере декодера.
    ///
    /// # Errors
    /// Диагностика декодера.
    fn drain(&mut self) -> Result<(), String>;
    /// Перемотка: отбросить всё накопленное состояние.
    fn flush(&mut self);
}

/// Декодер PCM одной аудиодорожки.
pub trait AudioPcmDecoder: Send {
    /// Декодировать пакет в интерливленные 16-битные сэмплы.
    ///
    /// # Errors
    /// Диагностика: пакет повреждён.
    fn decode(&mut self, packet: &Packet) -> Result<Vec<i16>, String>;
    /// Перемотка: забыть перекрытие окон предыдущего пакета.
    fn reset(&mut self);
}

/// Бэкенд видеокодеков.
pub trait VideoBackend: Send + Sync {
    /// Имя для журнала и диагностики.
    fn name(&self) -> &'static str;
    /// Умеет ли бэкенд открыть такой кодек (с профилем из `codecs=`) на этой машине.
    fn supports(&self, codec: Codec, codec_string: &str) -> bool;
    /// Создать декодер дорожки.
    ///
    /// # Errors
    /// Диагностика: неверная конфигурация кодека, нет ресурсов.
    fn create(&self, track: &TrackInfo) -> Result<Box<dyn VideoFrameDecoder>, String>;
}

/// Бэкенд аудиокодеков.
pub trait AudioBackend: Send + Sync {
    /// Имя для журнала и диагностики.
    fn name(&self) -> &'static str;
    /// Умеет ли бэкенд открыть такой кодек на этой машине.
    fn supports(&self, codec: Codec, codec_string: &str) -> bool;
    /// Создать декодер дорожки; аудиоформат (частота, каналы) берётся из `track`.
    ///
    /// # Errors
    /// Диагностика: неверная конфигурация кодека (заголовки Vorbis).
    fn create(&self, track: &TrackInfo) -> Result<Box<dyn AudioPcmDecoder>, String>;
}

/// Упорядоченный список бэкендов.
pub struct Registry {
    video: Vec<Box<dyn VideoBackend>>,
    audio: Vec<Box<dyn AudioBackend>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let video: Vec<_> = self.video.iter().map(|b| b.name()).collect();
        let audio: Vec<_> = self.audio.iter().map(|b| b.name()).collect();
        f.debug_struct("Registry").field("video", &video).field("audio", &audio).finish()
    }
}

impl Registry {
    /// Реестр штатной сборки.
    #[must_use]
    pub fn builtin() -> Self {
        Self {
            // Сюда, перед встроенными, встают бэкенды ОС: аппаратные, затем программные.
            video: vec![Box::new(av1::Av1Backend)],
            audio: vec![Box::new(vorbis::VorbisBackend)],
        }
    }

    /// Пустой реестр (тесты).
    #[must_use]
    pub fn empty() -> Self {
        Self { video: Vec::new(), audio: Vec::new() }
    }

    /// Добавить видеобэкенд в конец списка (наименьший приоритет).
    pub fn push_video(&mut self, backend: Box<dyn VideoBackend>) {
        self.video.push(backend);
    }

    /// Добавить аудиобэкенд в конец списка (наименьший приоритет).
    pub fn push_audio(&mut self, backend: Box<dyn AudioBackend>) {
        self.audio.push(backend);
    }

    /// Первый видеобэкенд, открывающий кодек.
    #[must_use]
    pub fn video_for(&self, codec: Codec, codec_string: &str) -> Option<&dyn VideoBackend> {
        self.video.iter().map(AsRef::as_ref).find(|b| b.supports(codec, codec_string))
    }

    /// Первый аудиобэкенд, открывающий кодек.
    #[must_use]
    pub fn audio_for(&self, codec: Codec, codec_string: &str) -> Option<&dyn AudioBackend> {
        self.audio.iter().map(AsRef::as_ref).find(|b| b.supports(codec, codec_string))
    }

    /// Открывается ли кодек вообще (видео или аудио).
    #[must_use]
    pub fn supports(&self, codec: Codec, codec_string: &str) -> bool {
        self.video_for(codec, codec_string).is_some() || self.audio_for(codec, codec_string).is_some()
    }

    /// Ответ `HTMLMediaElement.canPlayType()` (HTML LS §4.8.11.7): `"probably"` —
    /// контейнер известен и каждый кодек из `codecs=` открывается; `"maybe"` —
    /// кодеки не названы, но контейнер несёт хотя бы один открываемый кодек;
    /// `""` — иначе.
    #[must_use]
    pub fn can_play_type(&self, mime: &str) -> &'static str {
        let parsed = parse_mime(mime);
        let Some(container) = parsed.container else { return "" };
        match parsed.codecs {
            None => {
                let any = container
                    .codecs()
                    .iter()
                    .any(|&c| self.supports(c, probe_codec_string(c)));
                if any { "maybe" } else { "" }
            }
            Some(list) => {
                if list.is_empty() {
                    return "";
                }
                let all = list.iter().all(|token| {
                    let (codec, string) = classify_codec(token);
                    container.codecs().contains(&codec) && self.supports(codec, &string)
                });
                if all { "probably" } else { "" }
            }
        }
    }
}

/// Реестр штатной сборки, созданный один раз на процесс.
#[must_use]
pub fn builtin() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::builtin)
}

/// Контейнеры, которые умеет разбирать `lumen-media-demux`, и кодеки, допустимые
/// в каждом из них для `canPlayType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MimeContainer {
    Mp4,
    WebM,
    Ogg,
}

impl MimeContainer {
    fn codecs(self) -> &'static [Codec] {
        match self {
            Self::Mp4 => &[
                Codec::Av1,
                Codec::H264,
                Codec::Hevc,
                Codec::Vp9,
                Codec::Aac,
                Codec::Mp3,
                Codec::Opus,
                Codec::Flac,
            ],
            Self::WebM => &[Codec::Av1, Codec::Vp8, Codec::Vp9, Codec::Opus, Codec::Vorbis],
            Self::Ogg => &[Codec::Theora, Codec::Vorbis, Codec::Opus, Codec::Flac],
        }
    }
}

struct ParsedMime {
    container: Option<MimeContainer>,
    /// `None` — параметра `codecs` нет; `Some(vec![])` — он пуст.
    codecs: Option<Vec<String>>,
}

fn parse_mime(mime: &str) -> ParsedMime {
    let lower = mime.trim().to_ascii_lowercase();
    let mut parts = lower.split(';');
    let base = parts.next().unwrap_or("").trim();
    let container = match base {
        "video/mp4" | "audio/mp4" | "audio/x-m4a" => Some(MimeContainer::Mp4),
        "video/webm" | "audio/webm" => Some(MimeContainer::WebM),
        "video/ogg" | "audio/ogg" | "application/ogg" => Some(MimeContainer::Ogg),
        _ => None,
    };
    let mut codecs = None;
    for param in parts {
        let Some((name, value)) = param.split_once('=') else { continue };
        if name.trim() == "codecs" {
            let value = value.trim().trim_matches(['"', '\'']);
            codecs = Some(
                value
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect(),
            );
        }
    }
    ParsedMime { container, codecs }
}

/// Кодек и его каноническая строка по токену из `codecs=` (RFC 6381).
fn classify_codec(token: &str) -> (Codec, String) {
    let t = token.trim().to_ascii_lowercase();
    let head = t.split('.').next().unwrap_or("");
    let codec = match head {
        "av01" => Codec::Av1,
        "avc1" | "avc3" => Codec::H264,
        "hvc1" | "hev1" => Codec::Hevc,
        "vp8" => Codec::Vp8,
        "vp9" | "vp09" => Codec::Vp9,
        "theora" => Codec::Theora,
        "opus" => Codec::Opus,
        "vorbis" => Codec::Vorbis,
        "flac" => Codec::Flac,
        "mp3" => Codec::Mp3,
        "mp4a" => match t.as_str() {
            "mp4a.69" | "mp4a.6b" | "mp4a.40.34" => Codec::Mp3,
            _ if t.starts_with("mp4a.40.") => Codec::Aac,
            _ => Codec::Other,
        },
        _ => Codec::Other,
    };
    (codec, t)
}

/// Строка кодека «без параметров» для вопроса «открывается ли кодек вообще».
fn probe_codec_string(codec: Codec) -> &'static str {
    match codec {
        Codec::Av1 => "av01.0.05M.08",
        Codec::H264 => "avc1.42e01e",
        Codec::Hevc => "hvc1.1.6.L93.B0",
        Codec::Vp8 => "vp8",
        Codec::Vp9 => "vp09.00.10.08",
        Codec::Theora => "theora",
        Codec::Aac => "mp4a.40.2",
        Codec::Mp3 => "mp3",
        Codec::Opus => "opus",
        Codec::Vorbis => "vorbis",
        Codec::Flac => "flac",
        Codec::Other => "",
    }
}
