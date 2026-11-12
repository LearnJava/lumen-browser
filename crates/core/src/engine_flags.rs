//! Реестр именованных флагов движка (Chrome-измов) и набор включённых флагов
//! одного документа.
//!
//! Правило трёх уровней (ADR-034 §4 п.11, areas/08 CMP-08/29): если Chrome-изм
//! редкий или сомнительный — он не входит в поведение движка по умолчанию, а
//! включается записью `engine_flag` пакета совместимости на домен (как префы
//! со списком доменов у Firefox: `layout.css.fake-webkit-scrollbar.enabled-domains`).
//!
//! Словарь закрытый: запись пакета с именем вне [`KNOWN_ENGINE_FLAGS`]
//! отвергается валидацией (`CompatPack::validate`), пакет не может включить
//! поведение, которого в движке нет. Набор включённых флагов живёт на документе
//! ([`EngineFlags`]): документ хоста с совпавшей записью получает флаг записи,
//! документ другого хоста (соседняя вкладка, iframe) — свой, пустой.

/// Все имена, которые запись `engine_flag` пакета имеет право нести.
///
/// Имя попадает сюда только вместе с реализацией в движке, которая его читает:
/// запись без потребителя бессмысленна, а потребитель без записи в словаре
/// недостижим из пакета. Первый флаг — `window-chrome`: минимальная заглушка
/// `window.chrome` (`loadTimes`/`csi`, areas/08 CMP-08/29), запись появится по
/// данным корпуса и антибот-стен. `webkit-fullscreen` — семейство
/// `webkitRequestFullscreen`/`webkitFullscreenElement`: API в движке уже есть
/// (v8_compat_webkit_aliases), имя занесено, чтобы запись корпуса могла включать
/// его точечно, когда разбор корпуса покажет такой спрос.
pub const KNOWN_ENGINE_FLAGS: &[&str] = &["window-chrome", "webkit-fullscreen"];

/// Флаги, включённые для одного документа.
///
/// По умолчанию пуст; навигация заполняет его из активных записей пакета для
/// хоста документа. Флаг не наследуется и не протекает: iframe или вкладка
/// другого хоста собирает собственный набор.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EngineFlags {
    enabled: Vec<String>,
}

impl EngineFlags {
    /// `true`, если `name` есть в словаре, то есть запись пакета может его нести.
    #[must_use]
    pub fn is_known(name: &str) -> bool {
        KNOWN_ENGINE_FLAGS.contains(&name)
    }

    /// Включить флаг из словаря. Неизвестное имя и повторное включение — no-op:
    /// вызывающий (валидация пакета) уже отверг неизвестное имя.
    pub fn enable(&mut self, name: &str) {
        if Self::is_known(name) && !self.enabled.iter().any(|f| f == name) {
            self.enabled.push(name.into());
        }
    }

    /// `true`, если флаг включён для этого документа.
    #[must_use]
    pub fn is_enabled(&self, name: &str) -> bool {
        self.enabled.iter().any(|f| f == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabulary_is_nonempty_and_kebab() {
        assert!(!KNOWN_ENGINE_FLAGS.is_empty());
        for name in KNOWN_ENGINE_FLAGS {
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
                "flag name must be lowercase kebab: {name}"
            );
            assert!(EngineFlags::is_known(name));
        }
    }

    #[test]
    fn unknown_name_is_not_enabled() {
        let mut flags = EngineFlags::default();
        flags.enable("webkit-scrollbar");
        flags.enable("");
        assert!(flags.enabled.is_empty());
        assert!(!EngineFlags::is_known("webkit-scrollbar"));
    }

    #[test]
    fn enable_is_idempotent_and_checked_by_name() {
        let mut flags = EngineFlags::default();
        assert!(!flags.is_enabled("window-chrome"));
        flags.enable("window-chrome");
        flags.enable("window-chrome");
        assert!(flags.is_enabled("window-chrome"));
        assert_eq!(flags.enabled.len(), 1);
    }
}
