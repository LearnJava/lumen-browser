//! UX-IMPORT: пароли Firefox (`key4.db` + `logins.json`).
//!
//! **Неподтверждено против настоящего профиля Firefox.** В песочнице нет
//! установленного Firefox с сохранённым паролем, поэтому ниже — наилучшая
//! реализация по открытым описаниям формата (`key4.db` — SQLite, стандартный
//! PKCS#5 PBES2/RFC 8018, в отличие от бинарного `key3.db`); проверено только
//! то, что сама реализация самосогласованна (тесты шифруют синтетические
//! данные тем же алгоритмом и читают их обратно через `pkcs5`). Перед тем как
//! снять эту пометку, нужен прогон на реальном профиле с известным паролем.
//!
//! Схема: `key4.db.metaData` (`id='password'`) хранит `item1` — `globalSalt`.
//! `key4.db.nssPrivate.a11` — мастер-ключ (32 байта AES-256), обёрнутый
//! PBES2(PBKDF2-HMAC-SHA256, AES-256-CBC) с паролем `SHA1(globalSalt ++
//! masterPassword)` (мастер-пароль профиля; здесь всегда пустой — ввод
//! мастер-пароля в UI не реализован). `logins.json` хранит
//! `encryptedUsername`/`encryptedPassword` — ASN.1 DER
//! (AlgorithmIdentifier(AES-256-CBC, iv) + шифртекст), прямым AES-256-CBC
//! мастер-ключом, без второй KDF. Записи другим шифром (3DES, старые
//! профили/формат `key3.db`) не поддержаны — считаются пропущенными.

use std::path::Path;

use lumen_core::{Error, Result};
use sha1::{Digest, Sha1};

use crate::import::{importable, open_copy};
use crate::import_logins::{base64_decode, ImportedLogin};

/// Результат чтения профиля Firefox.
#[derive(Debug, Default)]
pub struct FirefoxLoginsRead {
    /// Успешно расшифрованные записи.
    pub logins: Vec<ImportedLogin>,
    /// Не расшифровано: другой шифр (3DES), мастер-пароль профиля не пуст,
    /// или запись повреждена.
    pub skipped: usize,
}

fn err(what: impl std::fmt::Display) -> Error {
    Error::Storage(format!("import firefox logins: {what}"))
}

/// Одна TLV (DER, definite-length): тег, длина заголовка, срез значения.
fn read_tlv(buf: &[u8]) -> Option<(u8, usize, &[u8])> {
    let &tag = buf.first()?;
    let len_byte = *buf.get(1)?;
    let (len, header_len) = if len_byte & 0x80 == 0 {
        (len_byte as usize, 2)
    } else {
        let n = (len_byte & 0x7f) as usize;
        if n == 0 || n > 4 {
            return None;
        }
        let bytes = buf.get(2..2 + n)?;
        (bytes.iter().fold(0usize, |acc, &b| (acc << 8) | b as usize), 2 + n)
    };
    Some((tag, header_len, buf.get(header_len..header_len + len)?))
}

/// Первый `OBJECT IDENTIFIER`, встреченный в прямом порядке (включая вложенные
/// `SEQUENCE`/`SET`).
fn find_first_oid(buf: &[u8]) -> Option<&[u8]> {
    let mut rest = buf;
    while !rest.is_empty() {
        let (tag, header_len, value) = read_tlv(rest)?;
        if tag == 0x06 {
            return Some(value);
        }
        if (tag == 0x30 || tag == 0x31) && let Some(oid) = find_first_oid(value) {
            return Some(oid);
        }
        rest = &rest[header_len + value.len()..];
    }
    None
}

/// Все `OCTET STRING`, в порядке встречи (прямой обход, включая вложенные).
fn collect_octet_strings<'a>(buf: &'a [u8], out: &mut Vec<&'a [u8]>) {
    let mut rest = buf;
    while let Some((tag, header_len, value)) = read_tlv(rest) {
        if tag == 0x04 {
            out.push(value);
        }
        if tag == 0x30 || tag == 0x31 {
            collect_octet_strings(value, out);
        }
        rest = &rest[header_len + value.len()..];
    }
}

const OID_AES256_CBC: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2A];

/// `a11` — `SEQUENCE { AlgorithmIdentifier, OCTET STRING шифртекст }`.
/// Алгоритм (PBES1/PBES2) парсит и расшифровывает `pkcs5`; здесь нужно
/// только отделить его DER-байты от шифртекста.
fn decrypt_master_key(a11: &[u8], password: &[u8]) -> Option<Vec<u8>> {
    let (tag, _, outer) = read_tlv(a11)?;
    if tag != 0x30 {
        return None;
    }
    let (alg_tag, alg_header, alg_value) = read_tlv(outer)?;
    if alg_tag != 0x30 {
        return None;
    }
    let alg_der = &outer[..alg_header + alg_value.len()];
    let (ct_tag, _, ciphertext) = read_tlv(&outer[alg_header + alg_value.len()..])?;
    if ct_tag != 0x04 {
        return None;
    }
    let scheme = pkcs5::EncryptionScheme::try_from(alg_der).ok()?;
    let key = scheme.decrypt(password, ciphertext).ok()?;
    (key.len() == 32).then_some(key)
}

/// Поле `encryptedUsername`/`encryptedPassword` (base64 DER). Только
/// AES-256-CBC прямым мастер-ключом (без KDF); другой алгоритм — `None`.
fn decrypt_entry_field(master_key: &[u8], der_b64: &str) -> Option<String> {
    use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    debug_assert_eq!(master_key.len(), 32);
    let raw = base64_decode(der_b64)?;
    let (tag, _, outer) = read_tlv(&raw)?;
    if tag != 0x30 {
        return None;
    }
    if find_first_oid(outer)? != OID_AES256_CBC {
        return None; // 3DES/легаси — не поддержано (см. doc-comment модуля).
    }
    let mut octets = Vec::new();
    collect_octet_strings(outer, &mut octets);
    let (ciphertext, rest) = octets.split_last()?;
    let iv = rest.iter().rev().find(|o| o.len() == 16)?;
    let plain = cbc::Decryptor::<aes::Aes256>::new_from_slices(master_key, iv)
        .ok()?
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .ok()?;
    String::from_utf8(plain).ok()
}

/// Прочитать и расшифровать пароли профиля (`profile_dir` — каталог с
/// `key4.db`/`logins.json`, master-пароль профиля предполагается пустым).
pub fn read_firefox_logins(profile_dir: &Path) -> Result<FirefoxLoginsRead> {
    let logins_json = std::fs::read_to_string(profile_dir.join("logins.json")).map_err(err)?;
    let parsed: serde_json::Value = serde_json::from_str(&logins_json).map_err(err)?;
    let entries = parsed.get("logins").and_then(|l| l.as_array()).cloned().unwrap_or_default();
    let mut out = FirefoxLoginsRead::default();
    if entries.is_empty() {
        return Ok(out);
    }

    let (conn, _guard) = open_copy(&profile_dir.join("key4.db"))?;
    let global_salt: Vec<u8> = conn
        .query_row("SELECT item1 FROM metaData WHERE id = 'password'", [], |r| r.get(0))
        .map_err(|e| err(format!("metaData: {e}")))?;
    // Эффективный пароль PBE = SHA1(globalSalt ++ masterPassword); пустой
    // мастер-пароль — единственный поддержанный случай сейчас.
    let effective_password = Sha1::digest(&global_salt);

    let mut stmt = conn.prepare("SELECT a11 FROM nssPrivate").map_err(err)?;
    let mut rows = stmt.query([]).map_err(err)?;
    let mut master_key = None;
    while let Some(row) = rows.next().map_err(err)? {
        let a11: Vec<u8> = row.get(0).map_err(err)?;
        if let Some(key) = decrypt_master_key(&a11, &effective_password) {
            master_key = Some(key);
            break;
        }
    }
    let Some(master_key) = master_key else {
        return Err(err(
            "ключ не получен (мастер-пароль профиля не пуст, либо нестандартный key4.db)",
        ));
    };

    for entry in entries {
        let host = entry.get("hostname").and_then(|h| h.as_str()).unwrap_or("");
        let fields = (
            entry.get("encryptedUsername").and_then(|u| u.as_str()),
            entry.get("encryptedPassword").and_then(|p| p.as_str()),
        );
        let decrypted = match fields {
            (Some(u), Some(p)) => {
                decrypt_entry_field(&master_key, u).zip(decrypt_entry_field(&master_key, p))
            }
            _ => None,
        };
        match decrypted {
            Some((username, password))
                if importable(host) && !username.is_empty() && !password.is_empty() =>
            {
                out.logins.push(ImportedLogin { origin: host.to_string(), username, password });
            }
            _ => out.skipped += 1,
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    use rusqlite::Connection;

    const OID_PBES2: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x05, 0x0D];
    const OID_PBKDF2: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x05, 0x0C];
    const OID_HMAC_SHA256: &[u8] = &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x02, 0x09];

    fn der_tlv(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        if value.len() < 128 {
            out.push(value.len() as u8);
        } else {
            let be = (value.len() as u32).to_be_bytes();
            let trimmed: Vec<u8> = be.iter().copied().skip_while(|&b| b == 0).collect();
            out.push(0x80 | trimmed.len() as u8);
            out.extend_from_slice(&trimmed);
        }
        out.extend_from_slice(value);
        out
    }
    fn der_seq(parts: &[Vec<u8>]) -> Vec<u8> {
        der_tlv(0x30, &parts.concat())
    }
    fn der_oid(b: &[u8]) -> Vec<u8> {
        der_tlv(0x06, b)
    }
    fn der_octets(b: &[u8]) -> Vec<u8> {
        der_tlv(0x04, b)
    }
    fn der_small_int(n: u8) -> Vec<u8> {
        der_tlv(0x02, &[n])
    }

    /// Собрать DER `a11` (PBES2/PBKDF2-SHA256/AES-256-CBC) ровно в той форме,
    /// которую разбирает [`decrypt_master_key`], шифруя `plain` ключом,
    /// выведенным PBKDF2 из `password`/`salt`/`iterations` — независимо от
    /// продакшен-кода (там расшифровка идёт через `pkcs5`).
    fn build_pbes2_blob(password: &[u8], salt: &[u8], iterations: u8, iv: &[u8; 16], plain: &[u8]) -> Vec<u8> {
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password, salt, iterations as u32, &mut key);
        let ciphertext = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key, iv)
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(plain);

        let prf = der_seq(&[der_oid(OID_HMAC_SHA256), vec![0x05, 0x00]]);
        let pbkdf2_params = der_seq(&[der_octets(salt), der_small_int(iterations), prf]);
        let kdf = der_seq(&[der_oid(OID_PBKDF2), pbkdf2_params]);
        let enc = der_seq(&[der_oid(OID_AES256_CBC), der_octets(iv)]);
        let pbes2_params = der_seq(&[kdf, enc]);
        let alg = der_seq(&[der_oid(OID_PBES2), pbes2_params]);
        der_seq(&[alg, der_octets(&ciphertext)])
    }

    /// Та же форма, что реальный `logins.json`: AES-256-CBC прямым ключом
    /// (без PBE), для [`decrypt_entry_field`].
    fn build_entry_blob(master_key: &[u8; 32], iv: &[u8; 16], plain: &str) -> String {
        let ciphertext = cbc::Encryptor::<aes::Aes256>::new_from_slices(master_key, iv)
            .unwrap()
            .encrypt_padded_vec_mut::<Pkcs7>(plain.as_bytes());
        let alg = der_seq(&[der_oid(OID_AES256_CBC), der_octets(iv)]);
        let der = der_seq(&[alg, der_octets(&ciphertext)]);
        base64_encode(&der)
    }

    fn base64_encode(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for chunk in data.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |a, (i, b)| a | u32::from(*b) << (16 - 8 * i));
            for i in 0..4 {
                s.push(if i <= chunk.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        s
    }

    #[test]
    fn master_key_roundtrip_via_pkcs5() {
        let password = Sha1::digest(b"global-salt-test");
        let plain = [7u8; 32];
        let blob = build_pbes2_blob(&password, b"entry-salt", 5, &[1u8; 16], &plain);
        let key = decrypt_master_key(&blob, &password).unwrap();
        assert_eq!(key, plain);
        assert!(decrypt_master_key(&blob, b"wrong").is_none());
    }

    #[test]
    fn entry_field_roundtrip() {
        let key = [9u8; 32];
        let b64 = build_entry_blob(&key, &[2u8; 16], "secret-пароль");
        assert_eq!(decrypt_entry_field(&key, &b64).unwrap(), "secret-пароль");
        assert!(decrypt_entry_field(&[1u8; 32], &b64).is_none(), "чужой ключ");
    }

    #[test]
    fn full_profile_read() {
        let dir = std::env::temp_dir()
            .join(format!("lumen-firefox-logins-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let global_salt = b"gs".to_vec();
        let password = Sha1::digest(&global_salt);
        let master_key = [5u8; 32];
        let a11 = build_pbes2_blob(&password, b"salt2", 3, &[3u8; 16], &master_key);

        let db = dir.join("key4.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE metaData(id TEXT PRIMARY KEY, item1 BLOB, item2 BLOB);
             CREATE TABLE nssPrivate(a11 BLOB);",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metaData(id, item1, item2) VALUES('password', ?1, x'00')",
            rusqlite::params![global_salt],
        )
        .unwrap();
        conn.execute("INSERT INTO nssPrivate(a11) VALUES(?1)", rusqlite::params![a11]).unwrap();
        drop(conn);

        let good_user = build_entry_blob(&master_key, &[4u8; 16], "ann");
        let good_pass = build_entry_blob(&master_key, &[5u8; 16], "pw-ff");
        let logins = serde_json::json!({"logins": [
            {"hostname": "https://a.test", "encryptedUsername": good_user, "encryptedPassword": good_pass},
            {"hostname": "not-a-url", "encryptedUsername": good_user, "encryptedPassword": good_pass},
            {"hostname": "https://b.test", "encryptedUsername": "AAAA", "encryptedPassword": "AAAA"},
        ]});
        std::fs::write(dir.join("logins.json"), logins.to_string()).unwrap();

        let read = read_firefox_logins(&dir).unwrap();
        assert_eq!(read.logins.len(), 1);
        assert_eq!(read.logins[0].origin, "https://a.test");
        assert_eq!(read.logins[0].username, "ann");
        assert_eq!(read.logins[0].password, "pw-ff");
        assert_eq!(read.skipped, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
