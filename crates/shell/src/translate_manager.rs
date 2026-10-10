//! UX-TRANSLATE: перевод текущей страницы локальной моделью.
//!
//! Форма та же, что у [`crate::import_manager::ImportManager`]: медленная часть
//! (запросы к модели) идёт в `std::thread`, результат приходит по `mpsc`, а
//! подмена текстовых узлов выполняется на UI-потоке в [`TranslateManager::poll`]
//! — документ принадлежит `Lumen`, поток видит только копии строк.
//!
//! Сессия привязана к `Arc<Mutex<Document>>` страницы, на которой запущена:
//! после навигации документ другой, и `NodeId` старой сессии не имеют смысла,
//! поэтому [`TranslateManager::sync_document`] её сбрасывает.
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use lumen_dom::page_translate::{collect_translatable, detect_language, page_language, TextSegment, TranslationSession};
use lumen_dom::Document;

/// Язык перевода: (название для промпта модели, код BCP 47).
const TARGET: (&str, &str) = ("Russian", "ru");
/// Модель по умолчанию; переопределяется `LUMEN_TRANSLATE_MODEL`.
#[cfg(feature = "ai")]
const DEFAULT_MODEL: &str = "phi3:mini";

type Translated = Vec<Option<String>>;

struct Job {
    /// Документ, с которого собраны сегменты (проверка `Arc::ptr_eq` при применении).
    doc: Arc<Mutex<Document>>,
    segments: Vec<TextSegment>,
    rx: mpsc::Receiver<Result<Translated, String>>,
}

/// Исход одного тика [`TranslateManager::poll`] для показа пользователю.
pub(crate) enum TranslateOutcome {
    /// Подменено `n` текстовых узлов — нужен relayout.
    Applied(usize),
    /// Перевод не удался (модель недоступна и т. п.).
    Failed(String),
}

#[derive(Default)]
pub(crate) struct TranslateManager {
    session: TranslationSession,
    session_doc: Option<Arc<Mutex<Document>>>,
    job: Option<Job>,
}

impl TranslateManager {
    /// Идёт ли запрос к модели.
    pub(crate) fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Применён ли перевод на текущей странице.
    pub(crate) fn is_active(&self) -> bool {
        self.session.is_active()
    }

    /// Сбрасывает сессию и задачу, если страница сменилась.
    pub(crate) fn sync_document(&mut self, current: Option<&Arc<Mutex<Document>>>) {
        let same = |d: &Arc<Mutex<Document>>| current.is_some_and(|c| Arc::ptr_eq(c, d));
        if self.session_doc.as_ref().is_some_and(|d| !same(d)) {
            self.session = TranslationSession::default();
            self.session_doc = None;
        }
        if self.job.as_ref().is_some_and(|j| !same(&j.doc)) {
            self.job = None;
        }
    }

    /// Запускает перевод страницы. `false` — уже идёт запрос, нечего переводить
    /// или страница уже на целевом языке.
    pub(crate) fn start(&mut self, doc: &Arc<Mutex<Document>>) -> bool {
        if self.job.is_some() {
            return false;
        }
        let (segments, source) = {
            let Ok(d) = doc.lock() else { return false };
            let segments = collect_translatable(&d);
            // Без `<html lang>` язык берём по письменности текста.
            let source = page_language(&d).or_else(|| detect_language(&segments));
            if source.as_deref() == Some(TARGET.1) {
                return false;
            }
            (segments, source)
        };
        if segments.is_empty() {
            return false;
        }
        let texts: Vec<String> = segments.iter().map(|s| s.text.clone()).collect();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("lumen-translate".into()).spawn(move || {
            let _ = tx.send(run_translate(&texts, source.as_deref()));
        });
        if spawned.is_err() {
            return false;
        }
        self.job = Some(Job { doc: Arc::clone(doc), segments, rx });
        true
    }

    /// Откатывает применённый перевод; число восстановленных узлов.
    pub(crate) fn revert(&mut self, doc: &Arc<Mutex<Document>>) -> usize {
        let Ok(mut d) = doc.lock() else { return 0 };
        let n = self.session.revert(&mut d);
        self.session_doc = None;
        n
    }

    /// Забирает результат фонового запроса и подменяет текст узлов.
    pub(crate) fn poll(&mut self) -> Option<TranslateOutcome> {
        let msg = match self.job.as_ref()?.rx.try_recv() {
            Ok(m) => m,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("поток перевода завершился".to_owned()),
        };
        let job = self.job.take()?;
        match msg {
            Ok(translated) => {
                let Ok(mut d) = job.doc.lock() else { return None };
                let n = self.session.apply(&mut d, &job.segments, &translated);
                drop(d);
                self.session_doc = Some(job.doc);
                Some(TranslateOutcome::Applied(n))
            }
            Err(e) => Some(TranslateOutcome::Failed(e)),
        }
    }
}

#[cfg(feature = "ai")]
fn run_translate(texts: &[String], source: Option<&str>) -> Result<Translated, String> {
    let model = std::env::var("LUMEN_TRANSLATE_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_owned());
    let backend = lumen_ai::generation::OllamaGenerationBackend::new(&model);
    lumen_ai::translate::translate_segments(&backend, texts, TARGET.0, source).map_err(|e| e.to_string())
}

#[cfg(not(feature = "ai"))]
fn run_translate(_texts: &[String], _source: Option<&str>) -> Result<Translated, String> {
    Err("браузер собран без --features ai: локальной модели нет".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_document_drops_session_of_other_page() {
        let a = Arc::new(Mutex::new(Document::new()));
        let b = Arc::new(Mutex::new(Document::new()));
        let mut m = TranslateManager { session_doc: Some(Arc::clone(&a)), ..Default::default() };
        m.sync_document(Some(&a));
        assert!(m.session_doc.is_some());
        m.sync_document(Some(&b));
        assert!(m.session_doc.is_none());
    }

    #[test]
    fn start_on_empty_document_is_noop() {
        let d = Arc::new(Mutex::new(Document::new()));
        let mut m = TranslateManager::default();
        assert!(!m.start(&d));
        assert!(!m.is_running());
    }
}
