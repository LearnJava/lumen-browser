//! Удалённая доставка пакета совместимости (COMPAT-PACK-REMOTE, ADR-034 §4):
//! фоновый опрос основного хоста и зеркала на старте и каждые
//! [`CHECK_INTERVAL_SECS`]; применяется только подписанный пакет со строго
//! большей ревизией, чем последняя проверенная (откат и подмена байта
//! отвергаются `CompatPack::parse_signed` и порогом ревизии). Без сети —
//! последняя проверенная копия (`<data>/compat-pack/pack.json`), совсем
//! ничего — вшитая базовая копия ([`lumen_update_manifest::compat_pack::CompatPack::base`]).
//!
//! Потребители (`crate::lumen::compat`) берут пакет через [`current_pack`] в
//! момент загрузки страницы, поэтому новая запись доходит до вкладки на
//! следующей навигации после применения — без релиза. Выключение записи
//! пользователем (`compat_disabled_ids`) работает одинаково для базовой и
//! привезённой копии — через `CompatPolicy`.
//!
//! `PACK_TRUSTED_KEYS` пока пуст (ключ ещё не отчеканен,
//! `docs/release-signing.md`), так что продакшн-копирование отвергает любой
//! ответ с сетью; механизм проверяется тестами с одноразовой парой ключей.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lumen_core::ext::NetworkTransport;
use lumen_core::url::Url;
use lumen_update_manifest::compat_pack::{CompatPack, PACK_TRUSTED_KEYS};
use serde::{Deserialize, Serialize};

/// Основной хост доставки — релиз-ассет того же репозитория, что и `latest.json`
/// (тот же домен, что `update::MANIFEST_URL`).
pub const PACK_URL: &str =
    "https://github.com/LearnJava/lumen-browser/releases/latest/download/compat_pack.json";

/// Зеркало доставки. Пока владелец не подтвердит хостинг (Р7), зеркало — CDN
/// поверх того же репозитория; недоступность зеркала не ошибка, если ответил
/// основной хост.
pub const PACK_MIRROR_URL: &str =
    "https://cdn.jsdelivr.net/gh/LearnJava/lumen-browser@compat-pack/compat_pack.json";

/// Минимум между двумя опросами (постановка COMPAT-PACK-REMOTE: 6 ч).
pub const CHECK_INTERVAL_SECS: i64 = 6 * 3600;

/// Гранулярность фонового цикла: как часто поток просыпается проверить порог.
const POLL_GRANULARITY_SECS: u64 = 15 * 60;

/// Локальное состояние доставки: ревизия последней проверенной копии и порог
/// опроса. Терпимо к отсутствующим полям — тот же принцип, что `update::UpdateState`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PackState {
    /// Ревизия последней проверенной копии; `0` — привезённого пакета нет.
    #[serde(default)]
    pub revision: u32,
    /// Unix-секунды последнего опроса (даже неудачного), `0` — ни разу.
    #[serde(default)]
    pub last_checked_at: i64,
}

/// `<data>/compat-pack/` — файлы подсистемы доставки.
#[must_use]
pub fn pack_dir() -> PathBuf {
    crate::adblock::browser_data_dir().join("compat-pack")
}

/// Последняя проверенная копия пакета (байты в том виде, в каком их подписали).
#[must_use]
pub fn pack_path() -> PathBuf {
    pack_dir().join("pack.json")
}

/// Порог опроса и ревизия.
#[must_use]
pub fn state_path() -> PathBuf {
    pack_dir().join("state.json")
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Опрос не нужен, пока не прошло [`CHECK_INTERVAL_SECS`] с последнего.
#[must_use]
pub fn is_check_due(last_checked_at: i64, now: i64) -> bool {
    now - last_checked_at >= CHECK_INTERVAL_SECS
}

/// Загрузить состояние с диска или дефолт (некорректный файл — не фатально).
#[must_use]
pub fn load_state() -> PackState {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Сохранить состояние; сбой записи — best-effort, как в `update::save_state`.
pub fn save_state(state: &PackState) {
    if std::fs::create_dir_all(pack_dir()).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(state_path(), json);
    }
}

/// Итог одного опроса (чистая функция [`apply_fetch_result`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchOutcome {
    /// Применён: подписан, валиден и строго новее хранимой ревизии.
    Applied(CompatPack),
    /// Ответ получен, но ревизия не новее хранимой (в т.ч. откат) — ничего не меняем.
    NotNewer,
    /// Подпись, формат или валидация отвергли байты; содержимое не раскрываем.
    Rejected(String),
}

/// Проверить ответ сети и решить, применять ли его. Ревизия должна быть строго
/// больше хранимой — монотонность доставок, откат отвергнут до проверки записи.
/// `keys` — параметр, чтобы тесты подставляли одноразовую пару вместо пустых
/// [`PACK_TRUSTED_KEYS`] (тот же приём, что `update::apply_check_result_with_keys`).
#[must_use]
pub fn apply_fetch_result(
    bytes: &[u8],
    stored: &PackState,
    keys: &[(&str, [u8; 32])],
    psl: &dyn lumen_core::ext::PublicSuffixList,
) -> (PackState, FetchOutcome) {
    let mut state = stored.clone();
    state.last_checked_at = now_unix();
    match CompatPack::parse_signed(bytes, keys, psl) {
        Ok(pack) if pack.revision > stored.revision => {
            state.revision = pack.revision;
            (state, FetchOutcome::Applied(pack))
        }
        Ok(_) => (state, FetchOutcome::NotNewer),
        Err(e) => (state, FetchOutcome::Rejected(e.to_string())),
    }
}

/// Прочитать и проверить локальную копию, привезённую прошлым опросом. Подпись
/// перепроверяется: файл на диске — сетевой ввод, как и ответ HTTP. Ошибка —
/// `None`, потребители откатываются на базовую копию.
#[must_use]
pub fn load_verified_pack(dir: &std::path::Path) -> Option<CompatPack> {
    let bytes = std::fs::read(dir.join("pack.json")).ok()?;
    CompatPack::parse_signed_production(&bytes, &lumen_storage::PslProvider::new()).ok()
}

/// Текущий пакет для всех потребителей в шелле: последняя проверенная
/// привезённая копия (в памяти или с диска), иначе базовая. Кэш в памяти
/// обновляет только фоновый поток после [`FetchOutcome::Applied`].
#[must_use]
pub fn current_pack() -> CompatPack {
    let mut cache = match CURRENT.write() {
        Ok(g) => g,
        Err(_) => return CompatPack::base(&lumen_storage::PslProvider::new()),
    };
    if cache.is_none() {
        *cache = load_verified_pack(&pack_dir());
    }
    cache
        .clone()
        .unwrap_or_else(|| CompatPack::base(&lumen_storage::PslProvider::new()))
}

static CURRENT: RwLock<Option<CompatPack>> = RwLock::new(None);

/// Один опрос: основной хост, при неудаче — зеркало; порог и ревизию ведёт
/// `state.json`. Сетевые ошибки — не фатально: остаётся прошлая копия.
fn poll_once() {
    let state = load_state();
    if !is_check_due(state.last_checked_at, now_unix()) {
        return;
    }
    let client = crate::update::update_http_client();
    for url in [PACK_URL, PACK_MIRROR_URL] {
        let Some(parsed) = Url::parse(url).ok() else { continue };
        let Ok(body) = client.fetch(&parsed) else { continue };
        let (state, outcome) =
            apply_fetch_result(&body, &state, PACK_TRUSTED_KEYS, &lumen_storage::PslProvider::new());
        match outcome {
            FetchOutcome::Applied(pack) => {
                if std::fs::create_dir_all(pack_dir()).is_ok()
                    && std::fs::write(pack_path(), &body).is_ok()
                    && let Ok(mut cache) = CURRENT.write()
                {
                    *cache = Some(pack);
                }
                save_state(&state);
                return;
            }
            FetchOutcome::NotNewer => {
                save_state(&state);
                return;
            }
            FetchOutcome::Rejected(reason) => {
                eprintln!("compat-pack: {url} отклонён: {reason}");
                save_state(&state);
                return;
            }
        }
    }
    // Оба адреса недоступны: порог всё равно сдвигаем, чтобы не стучаться
    // чаще интервала.
    save_state(&PackState { revision: state.revision, last_checked_at: now_unix() });
}

/// Фоновый цикл доставки: первый опрос сразу (старт), дальше — каждые
/// [`CHECK_INTERVAL_SECS`]. Никогда не паникует в цикл событий: все ошибки
/// внутри [`poll_once`] уже сведены к «осталась прошлая копия».
pub fn spawn_poll_thread() {
    std::thread::Builder::new()
        .name("compat-pack-remote".to_owned())
        .spawn(|| loop {
            poll_once();
            std::thread::sleep(Duration::from_secs(POLL_GRANULARITY_SECS));
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use lumen_update_manifest::compat_pack::{CompatEntry, CompatPolicy, PACK_FORMAT};

    fn psl() -> lumen_storage::PslProvider {
        lumen_storage::PslProvider::new()
    }

    fn entry(id: &str, host: &str) -> CompatEntry {
        // Минимальная запись `css`: валидация требует причину, баг, пересмотр
        // и парный тест (или обоснование его отсутствия).
        serde_json::from_str::<CompatEntry>(&format!(
            r#"{{"id":"{id}","matches":["{host}"],"root_cause":"gate-loop","bug":"BUG-0000","review_by":"2099-01-01","tests":["tests/pair/{id}"],"action":{{"kind":"css","css":"body{{}}"}}}}"#
        ))
        .unwrap()
    }

    fn pack(revision: u32, entries: Vec<CompatEntry>) -> CompatPack {
        CompatPack { format: PACK_FORMAT, revision, entries, key_id: String::new(), signature: String::new() }
    }

    fn signing_key() -> ed25519_dalek::SigningKey {
        ed25519_dalek::SigningKey::from_bytes(&[11u8; 32])
    }

    fn signed_bytes(pack: &mut CompatPack, key: &ed25519_dalek::SigningKey) -> Vec<u8> {
        pack.sign("test-key", key);
        serde_json::to_vec(&pack).unwrap()
    }

    #[test]
    fn applied_newer_revision_is_recorded() {
        let key = signing_key();
        let mut fetched = pack(3, vec![entry("r-css", "example.com")]);
        let bytes = signed_bytes(&mut fetched, &key);
        let keys = [("test-key", key.verifying_key().to_bytes())];
        let (state, outcome) = apply_fetch_result(&bytes, &PackState::default(), &keys, &psl());
        let FetchOutcome::Applied(applied) = outcome else {
            panic!("expected Applied, got {outcome:?}");
        };
        assert_eq!(applied.revision, 3);
        assert_eq!(state.revision, 3, "порог ревизии поднят до применённой");
    }

    #[test]
    fn rollback_is_not_applied() {
        let key = signing_key();
        let keys = [("test-key", key.verifying_key().to_bytes())];
        let stored = PackState { revision: 5, last_checked_at: 0 };
        let mut older = pack(4, vec![entry("old", "example.com")]);
        let (state, outcome) =
            apply_fetch_result(&signed_bytes(&mut older, &key), &stored, &keys, &psl());
        assert_eq!(outcome, FetchOutcome::NotNewer, "откат ревизии отвергнут");
        assert_eq!(state.revision, 5, "хранимая ревизия не понизилась");
    }

    #[test]
    fn tampered_bytes_are_rejected() {
        let key = signing_key();
        let keys = [("test-key", key.verifying_key().to_bytes())];
        let mut fetched = pack(2, vec![entry("t-css", "example.com")]);
        let mut bytes = signed_bytes(&mut fetched, &key);
        let pos = bytes.len() - 2;
        bytes[pos] ^= 0xFF; // правим подписанный байт после подписи
        let (_, outcome) = apply_fetch_result(&bytes, &PackState::default(), &keys, &psl());
        assert!(
            matches!(outcome, FetchOutcome::Rejected(_)),
            "подменённый байт не применяется: {outcome:?}"
        );
    }

    #[test]
    fn unsigned_key_is_rejected() {
        // Пустой набор доверенных ключей — сегодняшний продакшн: любой ответ
        // сети отвергается, пока владелец не отчеканит ключ.
        let key = signing_key();
        let mut fetched = pack(2, vec![entry("u-css", "example.com")]);
        let bytes = signed_bytes(&mut fetched, &key);
        let (_, outcome) = apply_fetch_result(&bytes, &PackState::default(), &[], &psl());
        assert!(matches!(outcome, FetchOutcome::Rejected(_)));
    }

    #[test]
    fn disabled_id_removes_entry_from_fetched_pack() {
        let key = signing_key();
        let keys = [("test-key", key.verifying_key().to_bytes())];
        let mut fetched = pack(7, vec![entry("d-css", "example.com")]);
        let bytes = signed_bytes(&mut fetched, &key);
        let (_, FetchOutcome::Applied(pack)) =
            apply_fetch_result(&bytes, &PackState::default(), &keys, &psl())
        else {
            panic!("expected Applied");
        };
        let policy_on = lumen_update_manifest::compat_pack::CompatPolicy::default();
        assert_eq!(pack.active_entries_for_host("example.com", &policy_on).len(), 1);
        let policy_off = lumen_update_manifest::compat_pack::CompatPolicy {
            pack_off: false,
            disabled: vec!["d-css".to_owned()],
        };
        assert!(
            pack.active_entries_for_host("example.com", &policy_off).is_empty(),
            "disabled_id выключает запись привезённого пакета без релиза"
        );
    }

    #[test]
    fn current_pack_reflects_applied_copy_and_falls_back_to_base() {
        // Приёмка «новая запись доходит до вкладки»: потребители читают
        // current_pack() в момент загрузки страницы, применённая копия видна
        // сразу после установки кэша.
        let key = signing_key();
        let keys = [("test-key", key.verifying_key().to_bytes())];
        let mut fetched = pack(9, vec![entry("cur-css", "example.org")]);
        let bytes = signed_bytes(&mut fetched, &key);
        let (_, FetchOutcome::Applied(applied)) =
            apply_fetch_result(&bytes, &PackState::default(), &keys, &psl())
        else {
            panic!("expected Applied");
        };
        let before = current_pack();
        assert!(
            before.active_entries_for_host("example.org", &CompatPolicy::default()).is_empty(),
            "до доставки записи нет (базовая копия)"
        );
        *CURRENT.write().unwrap() = Some(applied);
        let after = current_pack();
        assert_eq!(
            after.active_entries_for_host("example.org", &CompatPolicy::default()).len(),
            1,
            "после применения запись видна потребителю"
        );
        *CURRENT.write().unwrap() = None; // не оставлять глобальное состояние тесту
    }

    #[test]
    fn load_verified_pack_missing_or_tampered_is_none() {
        let dir = std::env::temp_dir().join(format!("lumen_test_cpr_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load_verified_pack(&dir), None, "нет файла — базовая копия");
        let key = ed25519_dalek::SigningKey::from_bytes(&[11u8; 32]); // не продакшн-ключ
        let mut fetched = pack(2, vec![entry("x-css", "example.com")]);
        let mut bytes = signed_bytes(&mut fetched, &key);
        std::fs::write(dir.join("pack.json"), &bytes).unwrap();
        assert_eq!(
            load_verified_pack(&dir),
            None,
            "непродакшн-подпись не доверяется с диска"
        );
        bytes.clear();
        std::fs::write(dir.join("pack.json"), &bytes).unwrap();
        assert_eq!(load_verified_pack(&dir), None, "битый файл — не фатально");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_check_due_throttles() {
        assert!(is_check_due(0, 1_700_000_000));
        assert!(!is_check_due(1000, 1000 + CHECK_INTERVAL_SECS - 1));
        assert!(is_check_due(1000, 1000 + CHECK_INTERVAL_SECS));
    }
}
