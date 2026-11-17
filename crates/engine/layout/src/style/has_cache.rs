//! BUG-1800 срез 17 — кэш результатов `:has()` между флашами.
//!
//! Зачем: has-reach корни ([`super::restyle::NodeRestyleIndex::has_reach_roots`])
//! ставят глубокий корень на кандидата по структурному «could flip». Перепись
//! срез 17 (живой github, `LUMEN_S17_PROBE`/`LUMEN_S17_DROP`): 247 would-корней
//! при 16/16 `LUMEN_VERIFY_INCR_CASCADE` equal — реальный флип `:has()` не
//! случился ни разу. Диф заменяет структурный вопрос точным: совпало ли
//! значение аргумента со значением на конец прошлого флаша; совпало — корень
//! не нужен, расхождение или промах — широкая дорога (как раньше).
//!
//! Звучность (уроки S31/S32 BUG-341):
//! - ключ значения — (ревизия листа, raw-id узла, адрес листового
//!   `RelativeSelector`): ревизия глобально уникальна по листу
//!   (`StylesheetRevision::fresh`), raw-id поколенческий (слот не
//!   переиспользуется), адрес стабилен, пока жив лист. Значение — чистая
//!   функция от (DOM, селектор); «скрытой» зависимости у ключа нет.
//! - запись (`record_fresh`) делает только сам диф в root-set времени,
//!   штампуя МУТАЦИОННУЮ ЭПОХУ этого флаша (`touched.epoch`); чтение
//!   (`cached_prior`) отвечает только на записи ПРЕДЫДУЩЕЙ эпохи (=水位
//!   basis текущего флаша): интервал «запись → чтение» покрыт ровно теми
//!   мутациями, которые дельта этого флаша и анализирует. Анализ того же
//!   DOM повторно (гипотетическая запись без мутаций — контрактные тесты
//!   root-set, state-флаши без тачей) эпоху не меняет → широкая дорога.
//! - короткое замыкание `any()` в матчере кэш не трогает вовсе: значения
//!   кэша — только собственные оценки дифа; неоценённый аргумент = промах.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI8, Ordering};
use std::sync::Mutex;

use lumen_css_parser::{RelativeSelector, StylesheetRevision};
use lumen_dom::NodeId;

/// Живых ревизий в кэше не больше этой величины: прод живёт в одной, тесты в
/// одном процессе могут гонять несколько листов параллельно — их записи не
/// должны затирать друг друга (значения по-ревизионно верны по построению).
const MAX_REVISIONS: usize = 4;

/// Ревизия листа → значения `(raw-id узла, адрес листового rel)` →
/// `(значение на конец флаша этой мутационной эпохи, эпоха)`.
type RevMap = HashMap<StylesheetRevision, HashMap<(u32, usize), (bool, u64)>>;

static CACHE: std::sync::LazyLock<Mutex<RevMap>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
/// 0 — не читано (env), 1 — диф включён, −1 — выключен (сеттер тестов/стендов).
static STATE: AtomicI8 = AtomicI8::new(0);

fn env_off() -> bool {
    std::env::var_os("LUMEN_NO_HASREACH_DIFF").is_some_and(|v| v != "0")
}

/// `LUMEN_NO_HASREACH_DIFF=1` — диф выключен, has-reach корни ставятся по
/// структурному could-flip как до среза 17. Читается один раз на процесс.
pub fn diff_enabled() -> bool {
    match STATE.load(Ordering::Relaxed) {
        0 => {
            let off = env_off();
            STATE.store(if off { -1 } else { 1 }, Ordering::Relaxed);
            !off
        }
        v => v > 0,
    }
}

/// Переключатель для стендов/тестов: плечи в одном процессе (env читается
/// один раз, поэтому A/B внутри процесса идёт через сеттер).
#[cfg(test)]
pub fn set_diff_enabled_for_test(on: bool) {
    STATE.store(if on { 1 } else { -1 }, Ordering::Relaxed);
}

/// Записать значение, оценённое дифом в root-set времени флаша мутационной
/// эпохи `epoch`: это «до»-значение следующего флаша (его basis-эпоха).
/// Кэш прогревается с первого флаша для любого кандидата — включая правила,
/// которые каскад не матчил (субъектов нет на странице), где ленивой записи
/// из матчера не случается никогда.
pub fn record_fresh(node: NodeId, rs: &RelativeSelector, matched: bool, rev: StylesheetRevision, epoch: u64) {
    let key = (node.raw(), std::ptr::from_ref(rs) as usize);
    let mut set = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if set.len() >= MAX_REVISIONS && !set.contains_key(&rev) {
        // Актуальная дорожка пересобирается следующим проходом; вытеснение
        // возвращает широкую дорогу, не неверный ответ.
        set.clear();
    }
    set.entry(rev).or_default().insert(key, (matched, epoch));
}

/// Значение с прошлого флаша: запись той же мутационной эпохи, что `basis_epoch`
/// (水位 `basis.touch_epoch` этого флаша). `None` — промах (не оценивалось,
/// другая ревизия, вытесненная дорожка) ИЛИ запись текущей эпохи (анализ
/// того же DOM повторно — узнать из него «до»-значение нельзя): для дифа и то,
/// и другое — «флип», широкая дорога.
pub fn cached_prior(node: NodeId, rs: &RelativeSelector, rev: StylesheetRevision, basis_epoch: u64) -> Option<bool> {
    let set = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let (matched, epoch) = set.get(&rev)?.get(&(node.raw(), std::ptr::from_ref(rs) as usize))?;
    (*epoch == basis_epoch).then_some(*matched)
}
