//! Индикатор применённых правок совместимости (COMPAT-PACK-IND, ADR-034 п.4).
//!
//! Чистая часть: какие записи пакета применены к хосту, строка `console.info` на запись и
//! список id для «Выключить и перезагрузить». Строку «К сайту применено N исправлений» рисует
//! `lumen_chrome` (`compat_fixes_label`), чтение настроек и перезагрузка — `crate::lumen::compat`.

use lumen_update_manifest::compat_pack::{CompatPack, CompatPolicy};

/// Применённая к странице запись: то, что нужно индикатору и консоли.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedFix {
    /// Id записи в пакете.
    pub id: String,
    /// Класс первопричины (`engine-css`, `gate-loop`, …).
    pub root_cause: String,
    /// `BUG-NNN` или `KB-NNN`, на который опирается запись.
    pub bug: String,
}

/// Записи пакета, применённые к `host` с учётом выбора пользователя, в порядке применения.
#[must_use]
pub fn applied_fixes(pack: &CompatPack, host: &str, policy: &CompatPolicy) -> Vec<AppliedFix> {
    pack.active_entries_for_host(host, policy)
        .into_iter()
        .map(|e| AppliedFix { id: e.id.clone(), root_cause: e.root_cause.clone(), bug: e.bug.clone() })
        .collect()
}

/// Одно сообщение `console.info` на применённую запись: что за запись, почему и где подробности.
#[must_use]
pub fn console_line(fix: &AppliedFix) -> String {
    format!(
        "[Lumen] К странице применена правка совместимости {} ({}): {}. Подробности и выключатель — about:compat",
        fix.id, fix.root_cause, fix.bug
    )
}

/// Сообщения для консоли за одну загрузку: ровно по одному на запись.
#[must_use]
pub fn console_batch(fixes: &[AppliedFix]) -> Vec<(u8, String)> {
    // Уровень 0 — `console.log`/`info`/`debug` (devtools::console_panel::ConsoleLevel::Log).
    fixes.iter().map(|f| (0, console_line(f))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(json: &str) -> CompatPack {
        CompatPack::parse_unsigned(json.as_bytes(), &lumen_storage::PslProvider::new()).expect("валидный пакет")
    }

    const TWO_ENTRIES: &str = r#"{"format":1,"revision":1,"entries":[
        {"id":"a-css","root_cause":"engine-css","matches":["example.com"],
         "action":{"kind":"css","css":"body{margin:0}"},"bug":"BUG-1","review_by":"2099-01-01",
         "test_exemption":"тест","engine_max":"99.0.0"},
        {"id":"b-issue","root_cause":"site-bug","matches":["example.com"],
         "action":{"kind":"known_issue","message":"сайт ломается"},"bug":"KB-2","review_by":"2099-01-01",
         "test_exemption":"тест"}
    ]}"#;

    #[test]
    fn two_entries_give_two_fixes_and_two_console_lines() {
        let pack = pack(TWO_ENTRIES);
        let fixes = applied_fixes(&pack, "www.example.com", &CompatPolicy::default());
        assert_eq!(fixes.len(), 2);
        let batch = console_batch(&fixes);
        assert_eq!(batch.len(), 2);
        assert!(batch[0].1.contains("a-css") && batch[0].1.contains("BUG-1"));
        assert!(batch[1].1.contains("b-issue") && batch[1].1.contains("KB-2"));
        assert!(batch.iter().all(|(level, _)| *level == 0));
    }

    #[test]
    fn disabling_every_applied_entry_leaves_zero() {
        let pack = pack(TWO_ENTRIES);
        let fixes = applied_fixes(&pack, "example.com", &CompatPolicy::default());
        let policy = CompatPolicy { pack_off: false, disabled: fixes.iter().map(|f| f.id.clone()).collect() };
        assert!(applied_fixes(&pack, "example.com", &policy).is_empty());
    }

    #[test]
    fn other_host_and_pack_off_give_zero() {
        let pack = pack(TWO_ENTRIES);
        assert!(applied_fixes(&pack, "other.org", &CompatPolicy::default()).is_empty());
        let off = CompatPolicy { pack_off: true, disabled: Vec::new() };
        assert!(applied_fixes(&pack, "example.com", &off).is_empty());
    }
}
