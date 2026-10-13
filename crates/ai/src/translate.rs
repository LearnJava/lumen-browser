//! Локальный перевод текстовых узлов страницы (UX-TRANSLATE).
//!
//! Работает поверх [`GenerationBackend`] (локальная модель, ADR-019): сеть не
//! используется, текст страницы не покидает машину. Модуль не знает про DOM —
//! получает плоский список строк (текстовые узлы в порядке документа) и
//! возвращает перевод по тем же индексам. Разметка сохраняется тем, что
//! вызывающий подменяет только `data` текстовых узлов; оригиналы он хранит у
//! себя, это и есть откат.
//!
//! Протокол с моделью: сегменты нумеруются `[i] текст`, ответ ожидается в том же
//! формате, по строке на сегмент. Сегмент, которого нет в ответе, остаётся
//! `None` — вызывающий оставляет оригинал, а не подставляет мусор.

use crate::generation::{GenerationBackend, GenerationError};

/// Максимум символов исходного текста в одном запросе к модели.
const MAX_BATCH_CHARS: usize = 2000;

/// Результат перевода: `None` — сегмент не переведён (оставить оригинал).
pub type Translated = Vec<Option<String>>;

/// Сегмент без букв (числа, пунктуация, пробелы) переводить незачем.
fn is_translatable(s: &str) -> bool {
    s.chars().any(char::is_alphabetic)
}

/// Схлопывает переводы строк и пробельные серии: протокол строчный.
fn flatten(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Делит индексы переводимых сегментов на пакеты не больше `MAX_BATCH_CHARS`
/// (один слишком длинный сегмент занимает пакет в одиночку).
fn plan_batches(segments: &[String]) -> Vec<Vec<usize>> {
    let mut batches: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut size = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        if !is_translatable(seg) {
            continue;
        }
        let len = seg.chars().count();
        if !current.is_empty() && size + len > MAX_BATCH_CHARS {
            batches.push(std::mem::take(&mut current));
            size = 0;
        }
        current.push(i);
        size += len;
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

fn build_prompt(segments: &[String], batch: &[usize], target: &str, source: Option<&str>) -> String {
    let from = source.map_or(String::new(), |s| format!(" from {s}"));
    let mut p = format!(
        "Translate each numbered line{from} to {target}. Keep the `[n]` prefixes, \
         output exactly one line per input line, do not merge, skip or comment. \
         Output only the translation.\n\n"
    );
    for &i in batch {
        p.push_str(&format!("[{i}] {}\n", flatten(&segments[i])));
    }
    p
}

/// Разбирает ответ модели: строки `[i] текст` → `(i, текст)`. Строки без
/// префикса и индексы вне `allowed` отбрасываются.
fn parse_response(response: &str, allowed: &[usize]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for line in response.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some((num, text)) = rest.split_once(']') else { continue };
        let Ok(i) = num.trim().parse::<usize>() else { continue };
        let text = text.trim();
        if text.is_empty() || !allowed.contains(&i) {
            continue;
        }
        out.push((i, text.to_owned()));
    }
    out
}

/// Переводит `segments` на `target` (например `"Russian"`). Ошибка бэкенда в
/// одном пакете не рвёт остальные: его сегменты остаются `None`; если же не
/// удался ни один пакет — возвращается ошибка (модель недоступна).
pub fn translate_segments(
    backend: &dyn GenerationBackend,
    segments: &[String],
    target: &str,
    source: Option<&str>,
) -> Result<Translated, GenerationError> {
    let mut result: Translated = vec![None; segments.len()];
    let batches = plan_batches(segments);
    let mut first_err = None;
    let mut ok_batches = 0usize;
    for batch in &batches {
        match backend.generate(&build_prompt(segments, batch, target, source), "") {
            Ok(resp) => {
                ok_batches += 1;
                for (i, text) in parse_response(&resp, batch) {
                    result[i] = Some(text);
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) if ok_batches == 0 => Err(e),
        _ => Ok(result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Отвечает по заготовленному списку; запоминает промпты.
    struct Canned {
        replies: Mutex<Vec<Result<String, GenerationError>>>,
        prompts: Mutex<Vec<String>>,
    }

    impl Canned {
        fn new(replies: Vec<Result<String, GenerationError>>) -> Self {
            Self { replies: Mutex::new(replies), prompts: Mutex::new(Vec::new()) }
        }
    }

    impl GenerationBackend for Canned {
        fn generate(&self, prompt: &str, _context: &str) -> Result<String, GenerationError> {
            self.prompts.lock().unwrap().push(prompt.to_owned());
            self.replies.lock().unwrap().remove(0)
        }
    }

    fn segs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn translates_by_index_and_skips_non_text() {
        let backend = Canned::new(vec![Ok("[0] Привет\n[2] Мир\n".to_owned())]);
        let out = translate_segments(&backend, &segs(&["Hello", "123 !", "World"]), "Russian", None)
            .unwrap();
        assert_eq!(out, vec![Some("Привет".into()), None, Some("Мир".into())]);
        let prompt = backend.prompts.lock().unwrap()[0].clone();
        assert!(prompt.contains("[0] Hello") && prompt.contains("[2] World"));
        assert!(!prompt.contains("[1]"));
    }

    #[test]
    fn missing_and_foreign_indices_stay_none() {
        let backend = Canned::new(vec![Ok("noise\n[0] Да\n[7] чужой\n[1]\n".to_owned())]);
        let out = translate_segments(&backend, &segs(&["Yes", "No"]), "Russian", Some("English"))
            .unwrap();
        assert_eq!(out, vec![Some("Да".into()), None]);
        assert!(backend.prompts.lock().unwrap()[0].contains("from English"));
    }

    #[test]
    fn long_input_is_split_into_batches() {
        let long = "word ".repeat(300); // 1500 символов
        let backend = Canned::new(vec![Ok("[0] а".to_owned()), Ok("[1] б".to_owned())]);
        let out =
            translate_segments(&backend, &[long.clone(), long], "Russian", None).unwrap();
        assert_eq!(out, vec![Some("а".into()), Some("б".into())]);
        assert_eq!(backend.prompts.lock().unwrap().len(), 2);
    }

    #[test]
    fn one_failed_batch_keeps_others() {
        let long = "word ".repeat(300);
        let backend = Canned::new(vec![
            Err(GenerationError::InvalidResponse("x".into())),
            Ok("[1] б".to_owned()),
        ]);
        let out = translate_segments(&backend, &[long.clone(), long], "Russian", None).unwrap();
        assert_eq!(out, vec![None, Some("б".into())]);
    }

    #[test]
    fn all_batches_failing_is_an_error() {
        let backend = Canned::new(vec![Err(GenerationError::InvalidResponse("x".into()))]);
        assert!(translate_segments(&backend, &segs(&["Hi"]), "Russian", None).is_err());
    }

    #[test]
    fn newlines_in_segment_are_flattened() {
        let backend = Canned::new(vec![Ok("[0] ок".to_owned())]);
        translate_segments(&backend, &segs(&["a\n  b"]), "Russian", None).unwrap();
        assert!(backend.prompts.lock().unwrap()[0].contains("[0] a b\n"));
    }
}
