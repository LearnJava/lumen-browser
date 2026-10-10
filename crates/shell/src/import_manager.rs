//! UX-IMPORT: background thread for importing bookmarks/history/passwords
//! from Chrome/Edge/Firefox profiles.
//!
//! Mirrors [`crate::download::DownloadManager`]'s shape: `start()` spawns a
//! `std::thread` that only *reads* the other browsers' profiles (file I/O +
//! decryption — the slow part) and reports each profile's results over an
//! `mpsc` channel; nothing here touches `Bookmarks`/`History`/`SavedLogins`
//! directly, since those are owned fields on `Lumen`, not `Arc`-shared.
//! [`ImportManager::poll`] — called from the shell event loop, same spot as
//! `DownloadManager::poll` — drains the channel and does the (cheap, local
//! SQLite) writes on the UI thread.
use std::sync::mpsc;

use lumen_storage::import::{ImportedBookmark, ImportedVisit, SourceBrowser};
use lumen_storage::import_logins::ImportedLogin;
use lumen_storage::{Bookmarks, History};

/// One profile's read results, as reported by the background thread.
struct ProfileRead {
    browser: SourceBrowser,
    bookmarks: Result<Vec<ImportedBookmark>, String>,
    history: Result<Vec<ImportedVisit>, String>,
    logins: Result<Vec<ImportedLogin>, String>,
}

enum ImportMsg {
    Profile(ProfileRead),
    /// No profiles found at all (distinct from zero-profile `Finished` so
    /// the caller can report "nothing to import" instead of staying silent).
    NoneFound,
    Finished,
}

/// Running totals, accumulated across profiles as [`ImportManager::poll`]
/// drains [`ImportMsg::Profile`] messages.
#[derive(Default)]
struct Totals {
    profiles: usize,
    bookmarks: usize,
    history: usize,
    logins: usize,
    errors: usize,
}

impl Totals {
    fn summary(&self) -> String {
        if self.profiles == 0 {
            return "профили Chrome/Edge/Firefox не найдены".to_owned();
        }
        format!(
            "{} профил{}: закладки {}, история {}, пароли {}{}",
            self.profiles,
            match self.profiles {
                1 => "ь",
                2..=4 => "я",
                _ => "ей",
            },
            self.bookmarks,
            self.history,
            self.logins,
            if self.errors > 0 { format!(", ошибок {}", self.errors) } else { String::new() },
        )
    }
}

/// Background import: one run at a time (`start()` is a no-op while already
/// [`Self::running`]).
pub(crate) struct ImportManager {
    rx: mpsc::Receiver<ImportMsg>,
    tx: mpsc::Sender<ImportMsg>,
    running: bool,
    totals: Totals,
}

impl ImportManager {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { rx, tx, running: false, totals: Totals::default() }
    }

    /// Start a background scan of Chrome/Edge/Firefox profiles. No-op (returns
    /// `false`) if a run is already in flight.
    pub(crate) fn start(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        self.totals = Totals::default();
        let tx = self.tx.clone();
        std::thread::Builder::new()
            .name("lumen-import".into())
            .spawn(move || run_import(&tx))
            .ok();
        true
    }

    /// Drain the channel, writing results into `bookmarks`/`history` (and
    /// `logins_store`, if a password store is open — absent in private
    /// modes). Returns `Some(summary)` exactly once, the tick [`ImportMsg::Finished`]
    /// arrives — the caller uses it for a one-shot notification/log line.
    pub(crate) fn poll(
        &mut self,
        bookmarks: &Bookmarks,
        history: &History,
        logins_store: Option<&lumen_storage::SavedLogins>,
        now_unix: i64,
    ) -> Option<String> {
        let mut finished = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ImportMsg::Profile(p) => {
                    self.totals.profiles += 1;
                    match p.bookmarks {
                        Ok(items) => {
                            if let Ok(rep) = lumen_storage::import::import_bookmarks(bookmarks, &items) {
                                self.totals.bookmarks += rep.imported;
                            } else {
                                self.totals.errors += 1;
                            }
                        }
                        Err(_) => self.totals.errors += 1,
                    }
                    match p.history {
                        Ok(items) => {
                            if let Ok(rep) = lumen_storage::import::import_history(history, &items) {
                                self.totals.history += rep.imported;
                            } else {
                                self.totals.errors += 1;
                            }
                        }
                        Err(_) => self.totals.errors += 1,
                    }
                    match (p.logins, logins_store) {
                        (Ok(items), Some(store)) => {
                            if let Ok(rep) =
                                lumen_storage::import_logins::import_logins(store, &items, now_unix)
                            {
                                self.totals.logins += rep.added;
                            } else {
                                self.totals.errors += 1;
                            }
                        }
                        (Err(_), _) => self.totals.errors += 1,
                        (Ok(_), None) => {} // приватный режим — пароли не сохраняем, не ошибка
                    }
                    eprintln!("[import] {:?}: профиль обработан", p.browser);
                }
                ImportMsg::NoneFound | ImportMsg::Finished => finished = true,
            }
        }
        if finished {
            self.running = false;
            Some(self.totals.summary())
        } else {
            None
        }
    }
}

/// Runs on the background thread: pure reads, no `Lumen` state touched.
fn run_import(tx: &mpsc::Sender<ImportMsg>) {
    let profiles = lumen_storage::import::detect_profiles();
    if profiles.is_empty() {
        let _ = tx.send(ImportMsg::NoneFound);
        return;
    }
    for p in &profiles {
        let bookmarks = lumen_storage::import::read_bookmarks(p).map_err(|e| e.to_string());
        let history = lumen_storage::import::read_history(p).map_err(|e| e.to_string());
        let logins = match p.browser {
            SourceBrowser::Chrome | SourceBrowser::Edge => {
                lumen_storage::import_logins::read_chromium_logins(&p.dir)
                    .map(|r| r.logins)
                    .map_err(|e| e.to_string())
            }
            SourceBrowser::Firefox => lumen_storage::import_logins_firefox::read_firefox_logins(&p.dir)
                .map(|r| r.logins)
                .map_err(|e| e.to_string()),
        };
        let _ = tx.send(ImportMsg::Profile(ProfileRead { browser: p.browser, bookmarks, history, logins }));
    }
    let _ = tx.send(ImportMsg::Finished);
}
