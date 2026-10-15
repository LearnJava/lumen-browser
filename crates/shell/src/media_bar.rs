//! `#mediaBar` — подсказка «для этого видео нужен компонент Windows»
//! (COMPAT-MEDIA-OSDEC-WIN-BAR).
//!
//! `<video src>` с H.264/HEVC открывается декодерами ОС
//! (`lumen-media-native`, `mf/`). На Windows N/KN их нет, и страница получает
//! только `MEDIA_ERR_*`: пользователь не узнаёт, что чинится установкой
//! Media Feature Pack / «HEVC Video Extensions». Шелл замечает сбой там, где
//! открывает контейнер (`tick_video_sessions`), спрашивает у
//! `lumen_media_native::missing_os_codec`, чего не хватает, и показывает
//! инфобар. «Скрыть» действует до конца сессии; смена страницы убирает
//! панель, но не отменяет «Скрыть».

use lumen_chrome::ChromeMediaBarModel;
use lumen_media_native::MissingOsCodec;

/// Состояние `#mediaBar`.
#[derive(Debug, Default)]
pub(crate) struct MediaBar {
    /// Чего не хватает открытой сейчас странице; `None` — панель скрыта.
    missing: Option<MissingOsCodec>,
    /// Скрытое пользователем до конца сессии.
    hidden: Vec<MissingOsCodec>,
}

impl MediaBar {
    /// Страница не смогла открыть видео из-за `what`. `true`, если панель
    /// изменилась (нужна перерисовка хрома).
    pub(crate) fn note_missing(&mut self, what: MissingOsCodec) -> bool {
        if self.hidden.contains(&what) || self.missing == Some(what) {
            return false;
        }
        self.missing = Some(what);
        true
    }

    /// «Скрыть»: панель не возвращается до конца сессии.
    pub(crate) fn dismiss(&mut self) {
        if let Some(what) = self.missing.take()
            && !self.hidden.contains(&what)
        {
            self.hidden.push(what);
        }
    }

    /// Страница сменилась: подсказка относилась к прежней.
    pub(crate) fn reset_page(&mut self) {
        self.missing = None;
    }

    /// Адрес установки для показанной панели.
    pub(crate) fn install_uri(&self) -> Option<&'static str> {
        self.missing.map(MissingOsCodec::install_uri)
    }

    /// Данные для `#mediaBar`.
    pub(crate) fn model(&self) -> ChromeMediaBarModel {
        let Some(what) = self.missing else { return ChromeMediaBarModel::default() };
        let (title, meta, install_label) = match what {
            MissingOsCodec::MediaFeaturePack => (
                "Для воспроизведения этого видео нужен пакет мультимедиа Windows",
                "В этой редакции Windows нет Media Feature Pack: он ставится в «Дополнительных компонентах».",
                "Открыть параметры",
            ),
            MissingOsCodec::HevcExtension => (
                "Для воспроизведения этого видео нужно расширение HEVC",
                "Установите «HEVC Video Extensions» из Microsoft Store.",
                "Открыть Microsoft Store",
            ),
        };
        ChromeMediaBarModel {
            open: true,
            title: title.to_owned(),
            meta: meta.to_owned(),
            install_label: install_label.to_owned(),
        }
    }
}

/// Открыть `ms-settings:` / `ms-windows-store:` обработчиком ОС. Вне Windows
/// таких адресов нет, панель там не показывается: возвращает `false`.
pub(crate) fn open_install_uri(uri: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(uri).spawn().is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = uri;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_is_closed_until_a_video_fails() {
        let bar = MediaBar::default();
        assert!(!bar.model().open);
        assert_eq!(bar.install_uri(), None);
    }

    #[test]
    fn missing_codec_opens_the_bar_with_its_install_target() {
        let mut bar = MediaBar::default();
        assert!(bar.note_missing(MissingOsCodec::MediaFeaturePack));
        let m = bar.model();
        assert!(m.open);
        assert!(m.title.contains("пакет мультимедиа Windows"));
        assert_eq!(bar.install_uri(), Some("ms-settings:optionalfeatures"));
        // Второй сбой того же рода панель не меняет.
        assert!(!bar.note_missing(MissingOsCodec::MediaFeaturePack));
        assert!(bar.note_missing(MissingOsCodec::HevcExtension));
        assert!(bar.install_uri().is_some_and(|u| u.starts_with("ms-windows-store:")));
    }

    #[test]
    fn dismiss_lasts_for_the_session_but_page_change_only_closes() {
        let mut bar = MediaBar::default();
        bar.note_missing(MissingOsCodec::MediaFeaturePack);
        bar.dismiss();
        assert!(!bar.model().open);
        // Новая страница с тем же сбоем панель не возвращает.
        bar.reset_page();
        assert!(!bar.note_missing(MissingOsCodec::MediaFeaturePack));
        // Другой компонент — отдельное «Скрыть».
        assert!(bar.note_missing(MissingOsCodec::HevcExtension));
        bar.reset_page();
        assert!(!bar.model().open);
        assert!(bar.note_missing(MissingOsCodec::HevcExtension));
    }
}
