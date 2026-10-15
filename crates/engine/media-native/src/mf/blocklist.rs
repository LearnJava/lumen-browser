//! Блок-лист адаптеров для аппаратного декодирования (D3D11VA): файл
//! `data/d3d-blocklist.txt` вшит в exe, формат строк описан в самом файле.

/// Одна строка блок-листа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rule {
    vendor: u32,
    /// `None` — любой адаптер этого производителя.
    device: Option<u32>,
    /// Заблокированы драйверы строго ниже этой версии; `None` — любые.
    below: Option<u64>,
    note: String,
}

impl Rule {
    pub(super) fn note(&self) -> &str {
        &self.note
    }
}

/// `a.b.c.d` → 4×16 бит (как в `LARGE_INTEGER` от `CheckInterfaceSupport`).
pub(super) fn parse_version(text: &str) -> Option<u64> {
    let parts: Vec<u16> = text.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let [a, b, c, d] = parts[..] else { return None };
    Some((u64::from(a) << 48) | (u64::from(b) << 32) | (u64::from(c) << 16) | u64::from(d))
}

/// Версия драйвера для сообщений: `a.b.c.d`.
pub(super) fn format_version(v: u64) -> String {
    format!("{}.{}.{}.{}", v >> 48, (v >> 32) & 0xffff, (v >> 16) & 0xffff, v & 0xffff)
}

fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u32::from_str_radix(digits, 16).ok()
}

/// Разобрать файл блок-листа; нераспознанные строки пропускаются (неверная
/// строка не должна выключать весь файл), проверяет их тест на вшитом файле.
pub(super) fn parse(text: &str) -> Vec<Rule> {
    text.lines()
        .filter_map(|line| {
            let (body, note) = line.split_once('#').unwrap_or((line, ""));
            let mut it = body.split_whitespace();
            let (vendor, device, below) = (it.next()?, it.next()?, it.next()?);
            if it.next().is_some() {
                return None;
            }
            Some(Rule {
                vendor: parse_hex(vendor)?,
                device: if device == "*" { None } else { Some(parse_hex(device)?) },
                below: if below == "*" { None } else { Some(parse_version(below)?) },
                note: note.trim().to_string(),
            })
        })
        .collect()
}

/// Правило, под которое подпадает адаптер.
pub(super) fn find(rules: &[Rule], vendor: u32, device: u32, driver: u64) -> Option<&Rule> {
    rules.iter().find(|r| {
        r.vendor == vendor && r.device.is_none_or(|d| d == device) && r.below.is_none_or(|below| driver < below)
    })
}

/// Вшитый блок-лист.
pub(super) fn builtin() -> &'static [Rule] {
    static RULES: std::sync::OnceLock<Vec<Rule>> = std::sync::OnceLock::new();
    RULES.get_or_init(|| parse(include_str!("../../data/d3d-blocklist.txt")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_roundtrip() {
        let v = parse_version("31.0.101.2140").unwrap();
        assert_eq!(format_version(v), "31.0.101.2140");
        assert!(parse_version("31.0.101").is_none());
        assert!(parse_version("31.0.101.x").is_none());
        assert!(parse_version("1.2.3.70000").is_none());
        assert!(parse_version("30.0.101.2140").unwrap() < v);
    }

    #[test]
    fn rules_match_by_vendor_device_and_driver() {
        let rules = parse(
            "# комментарий\n\
             \n\
             0x8086 * 20.19.15.4000   # старый Intel\n\
             0x10DE 0x1c82 *          # одна модель целиком\n\
             0x1002 0x73bf 30.0.0.0\n\
             мусор в строке\n\
             0x1 2 3 4 5\n",
        );
        assert_eq!(rules.len(), 3);
        let v = |s| parse_version(s).unwrap();
        // Intel: ниже порога — блок, на пороге и выше — нет.
        assert_eq!(find(&rules, 0x8086, 0x1234, v("20.19.15.3999")).map(Rule::note), Some("старый Intel"));
        assert!(find(&rules, 0x8086, 0x1234, v("20.19.15.4000")).is_none());
        assert!(find(&rules, 0x8086, 0x1234, v("31.0.101.2140")).is_none());
        // NVIDIA: только эта модель, любой драйвер.
        assert!(find(&rules, 0x10de, 0x1c82, v("99.0.0.1")).is_some());
        assert!(find(&rules, 0x10de, 0x1c83, v("0.0.0.1")).is_none());
        // Другой производитель не подпадает.
        assert!(find(&rules, 0x1002, 0x1, v("1.0.0.0")).is_none());
        assert!(find(&rules, 0x1002, 0x73bf, v("29.9.9.9")).is_some());
    }

    #[test]
    fn bundled_file_parses_completely() {
        // Каждая непустая строка вшитого файла — валидное правило.
        let text = include_str!("../../data/d3d-blocklist.txt");
        let lines = text
            .lines()
            .filter(|l| !l.split('#').next().unwrap_or("").trim().is_empty())
            .count();
        assert_eq!(builtin().len(), lines);
        // WARP заблокирован целиком.
        assert!(find(builtin(), 0x1414, 0x8c, 1).is_some());
        assert!(find(builtin(), 0x8086, 0x1, 1).is_none());
    }
}
