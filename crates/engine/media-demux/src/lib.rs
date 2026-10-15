//! Демуксеры контейнеров без FFmpeg (UX-VIDEO-DEMUX, [ADR-035](../../../docs/decisions/ADR-035-media-decoding-without-ffmpeg.md) п.2).
//!
//! Три контейнера, один результат — дорожки с описанием кодека и поток
//! пакетов с метками времени в общем формате ([`Packet`]):
//!
//! | Контейнер | Разбор |
//! |---|---|
//! | MP4 / ISOBMFF (в том числе fMP4 с `moof`) | `mp4parse` (таблицы `moov`, аудио-конфиги) + собственный разбор `moof/traf/trun` и `stsd` (HEVC, `vpcC`, `av1C`) |
//! | WebM / Matroska | `matroska-demuxer` |
//! | Ogg (Vorbis, Opus, Theora, FLAC) | `ogg` + `lewton` (длительность Vorbis-пакетов) |
//!
//! Крейт ничего не декодирует: пакеты уходят бэкендам кодеков реестра
//! (`lumen-native`, UX-VIDEO-NATIVE). Метки времени — микросекунды (`i64`,
//! могут быть отрицательными: правка `elst`, пре-ролл).
//!
//! ```
//! # fn main() -> Result<(), lumen_media_demux::DemuxError> {
//! # let bytes: &[u8] = include_bytes!("../tests/data/opus.ogg");
//! let mut d = lumen_media_demux::Demuxer::open(bytes)?;
//! assert_eq!(d.tracks()[0].codec_string, "opus");
//! while let Some(p) = d.next_packet()? {
//!     let _ = (p.track, p.pts_us, p.data.len());
//! }
//! # Ok(()) }
//! ```

mod bytes;
mod codec;
mod matroska;
mod mp4;
mod ogg_demux;

use std::fmt;

/// Контейнер файла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// MP4 / QuickTime / ISOBMFF (в том числе фрагментированный).
    Mp4,
    /// Matroska (`DocType = matroska`).
    Matroska,
    /// WebM (`DocType = webm`).
    WebM,
    /// Ogg.
    Ogg,
}

/// Вид дорожки; субтитры, метаданные и прочее демуксер пропускает.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    /// Видео.
    Video,
    /// Аудио.
    Audio,
}

/// Известные кодеки. Всё остальное — [`Codec::Other`] (строка кодека в
/// [`TrackInfo::codec_string`] сохраняет исходный идентификатор).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    /// H.264 / AVC.
    H264,
    /// H.265 / HEVC.
    Hevc,
    /// AV1.
    Av1,
    /// VP8.
    Vp8,
    /// VP9.
    Vp9,
    /// Theora.
    Theora,
    /// AAC.
    Aac,
    /// MPEG audio layer III.
    Mp3,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// FLAC.
    Flac,
    /// Неизвестный или не поддерживаемый кодек.
    Other,
}

/// Описание дорожки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    /// Идентификатор дорожки, на который ссылается [`Packet::track`]:
    /// `track_ID` (MP4), номер дорожки (Matroska), серийный номер потока (Ogg).
    pub id: u32,
    /// Видео или аудио.
    pub kind: TrackKind,
    /// Кодек.
    pub codec: Codec,
    /// Строка кодека в духе RFC 6381 — то, что пишут в `codecs="…"`:
    /// `avc1.64001f`, `hev1.1.6.L93.B0`, `av01.0.05M.08`, `vp09.00.10.08`,
    /// `mp4a.40.2`, `opus`, `vorbis`.
    pub codec_string: String,
    /// Конфигурация кодека для инициализации декодера: `avcC` / `hvcC` /
    /// `av1C` / `vpcC` (содержимое бокса), `AudioSpecificConfig` для AAC,
    /// `OpusHead`, три заголовка Vorbis в упаковке Xiph (как в Matroska
    /// `CodecPrivate`), `STREAMINFO` для FLAC. Пусто, если у кодека её нет.
    pub extradata: Vec<u8>,
    /// Ширина кадра в пикселях (0 для аудио).
    pub width: u32,
    /// Высота кадра в пикселях (0 для аудио).
    pub height: u32,
    /// Частота дискретизации в Гц (0 для видео).
    pub sample_rate: u32,
    /// Число каналов (0 для видео).
    pub channels: u16,
}

/// Пакет кодека в общем виде.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    /// [`TrackInfo::id`] дорожки.
    pub track: u32,
    /// Момент показа, мкс.
    pub pts_us: i64,
    /// Момент декодирования, мкс (у Matroska и Ogg равен `pts_us`).
    pub dts_us: i64,
    /// Длительность, мкс; 0 — неизвестна.
    pub duration_us: i64,
    /// Точка входа: с неё можно начинать декодирование.
    pub keyframe: bool,
    /// Данные пакета в том виде, как лежат в контейнере (для H.264 в MP4 —
    /// с префиксами длины, не Annex B).
    pub data: Vec<u8>,
}

/// Ошибка демуксера.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DemuxError {
    /// Сигнатура не похожа ни на MP4, ни на Matroska/WebM, ни на Ogg.
    UnknownContainer,
    /// Контейнер повреждён или обрезан.
    Malformed(String),
    /// Контейнер корректен, но в нём нет ничего, что мы умеем отдать
    /// (шифрованные дорожки, только субтитры и т. п.).
    Unsupported(String),
}

impl fmt::Display for DemuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownContainer => f.write_str("неизвестный контейнер"),
            Self::Malformed(m) => write!(f, "повреждённый контейнер: {m}"),
            Self::Unsupported(m) => write!(f, "не поддерживается: {m}"),
        }
    }
}

impl std::error::Error for DemuxError {}

/// Результат демуксера.
pub type Result<T> = std::result::Result<T, DemuxError>;

enum Inner<'a> {
    Mp4(mp4::Mp4<'a>),
    Matroska(matroska::Mkv<'a>),
    Ogg(ogg_demux::OggDemux<'a>),
}

/// Открытый контейнер. Данные не копируются до [`Demuxer::next_packet`].
pub struct Demuxer<'a> {
    container: Container,
    inner: Inner<'a>,
}

impl<'a> Demuxer<'a> {
    /// Определить контейнер по сигнатуре и разобрать его заголовки.
    ///
    /// # Errors
    /// [`DemuxError`] — неизвестная сигнатура, повреждённый заголовок или
    /// отсутствие видео/аудио дорожек.
    pub fn open(bytes: &'a [u8]) -> Result<Self> {
        let (container, inner) = match sniff(bytes) {
            Some(Container::Mp4) => (Container::Mp4, Inner::Mp4(mp4::Mp4::open(bytes)?)),
            Some(Container::Ogg) => (Container::Ogg, Inner::Ogg(ogg_demux::OggDemux::open(bytes)?)),
            Some(_) => {
                let m = matroska::Mkv::open(bytes)?;
                (m.container(), Inner::Matroska(m))
            }
            None => return Err(DemuxError::UnknownContainer),
        };
        Ok(Self { container, inner })
    }

    /// Контейнер файла.
    #[must_use]
    pub fn container(&self) -> Container {
        self.container
    }

    /// Видео- и аудиодорожки (остальные пропущены).
    #[must_use]
    pub fn tracks(&self) -> &[TrackInfo] {
        match &self.inner {
            Inner::Mp4(d) => d.tracks(),
            Inner::Matroska(d) => d.tracks(),
            Inner::Ogg(d) => d.tracks(),
        }
    }

    /// Длительность, мкс; `None`, если контейнер её не сообщает (Matroska
    /// без `Duration`, поток).
    #[must_use]
    pub fn duration_us(&self) -> Option<i64> {
        match &self.inner {
            Inner::Mp4(d) => d.duration_us(),
            Inner::Matroska(d) => d.duration_us(),
            Inner::Ogg(d) => d.duration_us(),
        }
    }

    /// Следующий пакет в порядке следования в файле (дорожки перемешаны, как
    /// записаны); `None` — конец.
    ///
    /// # Errors
    /// [`DemuxError::Malformed`] — пакет вышел за пределы данных.
    pub fn next_packet(&mut self) -> Result<Option<Packet>> {
        match &mut self.inner {
            Inner::Mp4(d) => d.next_packet(),
            Inner::Matroska(d) => d.next_packet(),
            Inner::Ogg(d) => d.next_packet(),
        }
    }

    /// Перемотка: следующие пакеты каждой дорожки начинаются с ближайшей
    /// точки входа не позже `target_us` (аудио — с пакета, покрывающего
    /// момент). Для Matroska точность — до кластера/ключевого кадра по Cues.
    ///
    /// # Errors
    /// [`DemuxError`] — повреждённый индекс контейнера.
    pub fn seek_us(&mut self, target_us: i64) -> Result<()> {
        match &mut self.inner {
            Inner::Mp4(d) => d.seek_us(target_us),
            Inner::Matroska(d) => d.seek_us(target_us),
            Inner::Ogg(d) => d.seek_us(target_us),
        }
    }
}

/// Контейнер по первым байтам; Matroska и WebM различимы только после
/// разбора EBML-заголовка, здесь оба дают `Matroska`.
fn sniff(b: &[u8]) -> Option<Container> {
    if b.starts_with(b"OggS") {
        return Some(Container::Ogg);
    }
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(Container::Matroska);
    }
    let kind = b.get(4..8)?;
    [
        &b"ftyp"[..],
        b"moov",
        b"mdat",
        b"moof",
        b"styp",
        b"free",
        b"skip",
        b"wide",
    ]
    .contains(&kind)
    .then_some(Container::Mp4)
}

/// Микросекунды из тиков шкалы `ts` (тиков в секунду), с округлением.
pub(crate) fn ticks_to_us(ticks: i64, ts: u64) -> i64 {
    if ts == 0 {
        return 0;
    }
    let ts = i128::from(ts);
    let v = (i128::from(ticks) * 2_000_000 + ts).div_euclid(2 * ts);
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

#[cfg(test)]
mod tests;
