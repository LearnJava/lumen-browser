//! LOAD-3A: классические `<script async src>` не держат страницу.
//!
//! Раньше внешний `async`-скрипт шёл в общий список «в порядке документа», и конвейер
//! (`parse_and_layout`) не отдавал страницу, пока не скачается последний из них. HTML LS
//! §4.12.1.1 шаг 31 требует обратного: такой скрипт исполняется «как можно скорее» после
//! загрузки, не блокирует разбор и не задерживает `DOMContentLoaded`, но `load` его ждёт.
//!
//! Живое окно собирает такие скрипты отдельным списком, а [`AsyncScriptFeed::spawn`]
//! качает их на ≤ 8 потоках параллельно финальному конвейеру. Готовые тела падают в
//! очередь; UI-поток забирает их в `about_to_wait` ([`Lumen::drive_async_scripts`]) и
//! исполняет через JS-хэндл страницы, как таймер-задачу. Пока очередь не отработана,
//! `load` (`window_load_due`) не доставляется.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::scripts::{
    resolve_one_script, ResolvedScript, ScriptElementMeta, ScriptFetchEnv, ScriptSource,
};
use crate::*;

/// Потоков загрузки на страницу — как у `parallel_fetch` и пула прогрева (LOAD-3C).
const ASYNC_FETCH_THREADS: usize = 8;

struct FeedState {
    /// Скачанные (или провалившиеся — им нужен `error`) тела, ещё не отданные UI.
    ready: VecDeque<ResolvedScript>,
    /// Сколько скриптов UI уже исполнил.
    executed: usize,
}

/// Очередь `async`-скриптов одной страницы: фоновые потоки кладут, UI-поток забирает.
pub(crate) struct AsyncScriptFeed {
    state: Mutex<FeedState>,
    total: usize,
}

impl AsyncScriptFeed {
    /// Запустить загрузку `items` и сразу вернуть очередь — ничего не ждёт.
    ///
    /// `wake` зовётся из потока загрузки после каждого готового тела (в живом окне — будит
    /// event loop, иначе скрипт ждал бы случайного пробуждения).
    pub(crate) fn spawn(
        items: Vec<(ScriptSource, ScriptElementMeta)>,
        env: ScriptFetchEnv,
        base: ResourceBase,
        sink: Arc<dyn EventSink>,
        cookie_jar: Option<Arc<lumen_storage::CookieJar>>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Arc<Self> {
        let total = items.len();
        let feed = Arc::new(Self {
            state: Mutex::new(FeedState { ready: VecDeque::new(), executed: 0 }),
            total,
        });
        let queue = Arc::new(Mutex::new(VecDeque::from(items)));
        let shared = Arc::new((env, base, sink, cookie_jar, wake));
        let mut workers = 0;
        for n in 0..total.min(ASYNC_FETCH_THREADS) {
            let (feed, queue, shared) = (Arc::clone(&feed), Arc::clone(&queue), Arc::clone(&shared));
            let spawned = std::thread::Builder::new()
                .name(format!("lumen-async-script-{n}"))
                .spawn(move || {
                    let (env, base, sink, cookie_jar, wake) = &*shared;
                    loop {
                        let next = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                        let Some((item, meta)) = next else { break };
                        let resolved = resolve_one_script(&item, &meta, base, sink, cookie_jar, env)
                            .unwrap_or_else(|| ResolvedScript::failed(item.node()));
                        feed.state.lock().unwrap_or_else(|e| e.into_inner()).ready.push_back(resolved);
                        wake();
                    }
                });
            match spawned {
                Ok(_) => workers += 1,
                // Оставшиеся скрипты подберут уже запущенные воркеры.
                Err(err) => eprintln!("не удалось запустить поток async-скриптов: {err}"),
            }
        }
        if workers == 0 {
            // Ни одного потока: без этого очередь не опустеет и `load` не придёт никогда.
            let mut st = feed.state.lock().unwrap_or_else(|e| e.into_inner());
            for (item, _) in queue.lock().unwrap_or_else(|e| e.into_inner()).drain(..) {
                st.ready.push_back(ResolvedScript::failed(item.node()));
            }
        }
        feed
    }

    /// Готовые тела, которые UI ещё не исполнял.
    pub(crate) fn take_ready(&self) -> Vec<ResolvedScript> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).ready.drain(..).collect()
    }

    /// UI исполнил `n` скриптов из последней порции [`Self::take_ready`].
    pub(crate) fn mark_executed(&self, n: usize) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).executed += n;
    }

    /// Сколько скриптов ещё не исполнено — в полёте или лежат в очереди. Пока их больше
    /// нуля, `load` страницы доставлять рано.
    pub(crate) fn pending(&self) -> usize {
        let executed = self.state.lock().unwrap_or_else(|e| e.into_inner()).executed;
        self.total.saturating_sub(executed)
    }
}
