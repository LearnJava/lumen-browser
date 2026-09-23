//! UPD-9: the self-update UI — the `#updateBar` infobar, the settings-page
//! "Обновления" block and the OS notification, wired to the UPD-1..8 backend
//! in [`crate::update`].
//!
//! [`UpdateUi`] is the whole state machine and is deliberately free of
//! `Lumen`: the check runs on its own thread and reports through an `mpsc`
//! channel (the `download::DownloadManager` pattern), and every transition is
//! a plain method, so the tests below drive it without a window. The `impl
//! Lumen` block at the bottom is only the glue — polling from `about_to_wait`
//! and dispatching the five `ChromeAction`s.
//!
//! Flow: check → [`UpdateStage::Available`] (bar + notification) → «Скачать» →
//! [`UpdateStage::Downloading`] → [`UpdateStage::Ready`] → «Перезапустить и
//! обновить» → [`crate::update::apply_staged_update`] → the event loop exits
//! and `window_mode::run_window_mode` spawns the new binary once `run_app`
//! has returned (so the old process still unwinds and saves its session).

use std::path::PathBuf;
use std::sync::mpsc;

use lumen_chrome::{ChromeUpdateAction, ChromeUpdateModel};

use crate::update::{
    self, CheckOutcome, UpdateAsset, UpdateDownloadManager, UpdateDownloadStatus, UpdateState,
};

/// What one check thread reports back: the state to persist plus the
/// outcome, or the network error of an unthrottled (manual) check.
type CheckMessage = Result<(UpdateState, CheckOutcome), String>;

/// Where the self-update flow stands — drives the infobar's text and button.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UpdateStage {
    /// Nothing to offer: never checked, up to date, or the last check failed.
    Idle,
    /// A signature-verified newer version with an asset for this platform.
    Available { version: String, asset: UpdateAsset },
    /// `asset` is being downloaded and hash-checked on a background thread.
    Downloading { version: String, asset: UpdateAsset },
    /// The archive is verified and staged at `path`, ready to apply. `asset`
    /// is kept so a failed apply can fall back to a fresh download.
    Ready { version: String, asset: UpdateAsset, path: PathBuf },
    /// Download or apply failed; `asset` is kept so «Скачать» can retry.
    Failed { version: String, asset: UpdateAsset, reason: String },
}

/// Something the shell must do after [`UpdateUi::poll`].
#[derive(Debug, Default, PartialEq)]
pub(crate) struct UpdatePoll {
    /// The chrome model changed — relayout the chrome host.
    pub(crate) changed: bool,
    /// `(title, body)` for `notification::show_os_notification`.
    pub(crate) notify: Option<(String, String)>,
}

/// Self-update UI state (UPD-9). See the module docs.
pub(crate) struct UpdateUi {
    stage: UpdateStage,
    bar_open: bool,
    auto_check: bool,
    check_status: String,
    /// Receiver of the check thread in flight, and whether that check was
    /// started by the user (a manual check reports "up to date" / errors in
    /// the settings row; an automatic one only surfaces an update).
    check: Option<(mpsc::Receiver<CheckMessage>, bool)>,
    downloads: UpdateDownloadManager,
    restart_requested: bool,
}

impl UpdateUi {
    /// A fresh UI with the persisted auto-check preference. Reads
    /// `data/update/state.json` once (a few bytes) — the only disk access on
    /// the UI thread besides [`Self::toggle_auto_check`].
    pub(crate) fn new() -> Self {
        Self::with_auto_check(update::load_state().auto_check_updates)
    }

    fn with_auto_check(auto_check: bool) -> Self {
        Self {
            stage: UpdateStage::Idle,
            bar_open: false,
            auto_check,
            check_status: format!("Установлена версия {}", update::current_version()),
            check: None,
            downloads: UpdateDownloadManager::new(),
            restart_requested: false,
        }
    }

    /// Starts a check on a background thread. `manual` bypasses the 24 h
    /// throttle and the auto-check opt-out ([`update::check_for_update_now`]);
    /// otherwise [`update::check_for_update`] applies both. A no-op while a
    /// check is already in flight.
    pub(crate) fn start_check(&mut self, manual: bool) {
        if self.check.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("update-check".to_owned())
            .spawn(move || {
                let client = update::update_http_client();
                let state = update::load_state();
                let msg = if manual {
                    update::check_for_update_now(&client, state)
                } else {
                    Ok(update::check_for_update(&client, state))
                };
                let _ = tx.send(msg);
            });
        if spawned.is_ok() {
            self.check = Some((rx, manual));
            if manual {
                self.check_status = "Проверка…".to_owned();
            }
        }
    }

    /// Drains the check thread and the download manager. Must be called
    /// regularly from `about_to_wait`, like `DownloadManager::poll`.
    pub(crate) fn poll(&mut self) -> UpdatePoll {
        let mut out = UpdatePoll::default();
        if let Some((rx, manual)) = &self.check {
            let manual = *manual;
            match rx.try_recv() {
                Ok(msg) => {
                    self.check = None;
                    if let Ok((state, _)) = &msg {
                        // The UI thread owns the opt-out: a toggle flipped
                        // while the thread ran must not be overwritten by
                        // the state it loaded before the flip.
                        let mut state = state.clone();
                        state.auto_check_updates = self.auto_check;
                        update::save_state(&state);
                    }
                    out.notify = self.on_check_result(msg, manual);
                    out.changed = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.check = None;
                    if manual {
                        self.check_status = "Не удалось проверить обновления".to_owned();
                        out.changed = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        self.downloads.poll();
        out.changed |= self.on_download_status();
        out
    }

    /// Applies one check result. Returns the OS notification to show, if any
    /// — only for an automatic check that found a new version (a manual one
    /// was clicked from the settings page, the user is already looking).
    fn on_check_result(&mut self, msg: CheckMessage, manual: bool) -> Option<(String, String)> {
        let outcome = match msg {
            Ok((_, outcome)) => outcome,
            Err(e) => {
                self.check_status = format!("Не удалось проверить: {e}");
                return None;
            }
        };
        match outcome {
            CheckOutcome::Available(manifest) => {
                let version = manifest.version.clone();
                let Some(asset) = update::select_platform_asset(&manifest.assets).cloned() else {
                    self.check_status = format!("Доступна версия {version}, но не для этой платформы");
                    return None;
                };
                self.check_status = format!("Доступна версия {version}");
                let same_in_progress = matches!(
                    &self.stage,
                    UpdateStage::Downloading { version: v, .. } | UpdateStage::Ready { version: v, .. }
                        if *v == version
                );
                if !same_in_progress {
                    self.stage = UpdateStage::Available { version: version.clone(), asset };
                }
                self.bar_open = true;
                (!manual).then(|| {
                    (
                        "Доступно обновление Lumen".to_owned(),
                        format!("Версия {version} готова к загрузке"),
                    )
                })
            }
            CheckOutcome::UpToDate => {
                if manual {
                    self.check_status =
                        format!("Установлена последняя версия ({})", update::current_version());
                }
                None
            }
            CheckOutcome::Malformed => {
                self.check_status = "Манифест обновления повреждён".to_owned();
                None
            }
            CheckOutcome::Untrusted(_) => {
                self.check_status = "Манифест обновления не прошёл проверку подписи".to_owned();
                None
            }
        }
    }

    /// Moves `Downloading` on once the download manager reports a result.
    /// Returns whether the stage changed.
    fn on_download_status(&mut self) -> bool {
        let UpdateStage::Downloading { version, asset } = &self.stage else { return false };
        let next = match self.downloads.status() {
            UpdateDownloadStatus::Staged { path } => UpdateStage::Ready {
                version: version.clone(),
                asset: asset.clone(),
                path: path.clone(),
            },
            UpdateDownloadStatus::HashMismatch => UpdateStage::Failed {
                version: version.clone(),
                asset: asset.clone(),
                reason: "контрольная сумма архива не совпала с манифестом".to_owned(),
            },
            UpdateDownloadStatus::Failed(reason) => UpdateStage::Failed {
                version: version.clone(),
                asset: asset.clone(),
                reason: reason.clone(),
            },
            UpdateDownloadStatus::Idle
            | UpdateDownloadStatus::InProgress
            | UpdateDownloadStatus::Cancelled => return false,
        };
        self.stage = next;
        true
    }

    /// «Скачать» — starts (or retries) the download of the offered version.
    pub(crate) fn start_download(&mut self) {
        let (UpdateStage::Available { version, asset } | UpdateStage::Failed { version, asset, .. }) =
            &self.stage
        else {
            return;
        };
        let (version, asset) = (version.clone(), asset.clone());
        self.downloads.start(version.clone(), asset.clone());
        self.stage = UpdateStage::Downloading { version, asset };
    }

    /// «Перезапустить и обновить» — swaps the binaries in place
    /// ([`update::apply_staged_update`]). On success the caller exits the
    /// event loop; [`Self::restart_requested`] then tells `run_window_mode`
    /// to spawn the new binary. On failure the stage becomes
    /// [`UpdateStage::Failed`] (the binaries are left untouched — the apply
    /// is atomic) and `false` is returned.
    pub(crate) fn apply_and_request_restart(&mut self) -> bool {
        let UpdateStage::Ready { version, asset, path } = &self.stage else { return false };
        let result = match update::exe_dir() {
            Some(dir) => update::apply_staged_update(path, &dir).map_err(|e| e.to_string()),
            None => Err("не найден каталог исполняемого файла".to_owned()),
        };
        match result {
            Ok(()) => {
                self.restart_requested = true;
                true
            }
            Err(reason) => {
                // A staged archive that failed to apply is not retried as-is
                // (a corrupt extract would fail the same way) — «Скачать»
                // fetches it afresh.
                self.stage = UpdateStage::Failed { version: version.clone(), asset: asset.clone(), reason };
                false
            }
        }
    }

    /// Hides the infobar. A running download keeps going; a later manual
    /// check re-opens the bar at whatever stage the flow reached.
    pub(crate) fn dismiss(&mut self) {
        self.bar_open = false;
    }

    /// Flips and persists `UpdateState::auto_check_updates`.
    pub(crate) fn toggle_auto_check(&mut self) {
        self.auto_check = !self.auto_check;
        let mut state = update::load_state();
        state.auto_check_updates = self.auto_check;
        update::save_state(&state);
    }

    /// Whether «Перезапустить и обновить» succeeded — read by
    /// `run_window_mode` after the event loop returns.
    pub(crate) fn restart_requested(&self) -> bool {
        self.restart_requested
    }

    /// The `#updateBar` + settings snapshot for `chrome_model_snapshot`.
    pub(crate) fn chrome_model(&self) -> ChromeUpdateModel {
        let current = update::current_version();
        let (title, meta, action) = match &self.stage {
            UpdateStage::Idle => (String::new(), String::new(), ChromeUpdateAction::None),
            UpdateStage::Available { version, asset } => (
                format!("Доступна версия {version}"),
                format!("Установлена {current} · {}", crate::download::human_bytes(asset.size)),
                ChromeUpdateAction::Download,
            ),
            UpdateStage::Downloading { version, .. } => {
                (format!("Доступна версия {version}"), "Загрузка…".to_owned(), ChromeUpdateAction::None)
            }
            UpdateStage::Ready { version, .. } => (
                format!("Версия {version} загружена"),
                "Архив проверен — перезапустите браузер".to_owned(),
                ChromeUpdateAction::Restart,
            ),
            UpdateStage::Failed { version, reason, .. } => (
                format!("Обновление до {version} не удалось"),
                reason.clone(),
                ChromeUpdateAction::Download,
            ),
        };
        ChromeUpdateModel {
            bar_open: self.bar_open && self.stage != UpdateStage::Idle,
            title,
            meta,
            action,
            auto_check: self.auto_check,
            check_status: self.check_status.clone(),
        }
    }
}

impl crate::Lumen {
    /// UPD-9: per-tick poll from `about_to_wait` — relayouts the chrome when
    /// the update flow moved and shows the OS notification it asked for.
    pub(crate) fn poll_update_ui(&mut self) {
        let poll = self.update_ui.poll();
        if let Some((title, body)) = poll.notify {
            crate::notification::show_os_notification(&title, &body);
        }
        if poll.changed {
            self.relayout_chrome_host();
            self.request_redraw();
        }
    }

    /// UPD-9: the five update `ChromeAction`s. `RestartToUpdate` saves the
    /// session exactly like `CloseRequested` before exiting the loop.
    pub(crate) fn dispatch_update_action(
        &mut self,
        action: lumen_chrome::ChromeAction,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) {
        use lumen_chrome::ChromeAction;
        match action {
            ChromeAction::DismissUpdate => self.update_ui.dismiss(),
            ChromeAction::DownloadUpdate => self.update_ui.start_download(),
            ChromeAction::CheckForUpdates => self.update_ui.start_check(true),
            ChromeAction::ToggleAutoUpdate => self.update_ui.toggle_auto_check(),
            ChromeAction::RestartToUpdate => {
                if self.update_ui.apply_and_request_restart() {
                    self.save_session_on_close();
                    self.save_full_session();
                    event_loop.exit();
                    return;
                }
            }
            _ => return,
        }
        self.relayout_chrome_host();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::UpdateManifest;

    fn asset_for_this_platform() -> UpdateAsset {
        UpdateAsset {
            name: format!("lumen-{}-x86_64-v9.9.9.zip", std::env::consts::OS),
            sha256: "00".to_owned(),
            size: 2 * 1024 * 1024,
        }
    }

    fn available(version: &str, assets: Vec<UpdateAsset>) -> CheckMessage {
        Ok((
            UpdateState::default(),
            CheckOutcome::Available(UpdateManifest {
                version: version.to_owned(),
                assets,
                key_id: "k".to_owned(),
                signature: String::new(),
            }),
        ))
    }

    #[test]
    fn fresh_ui_hides_the_bar_and_names_the_current_version() {
        let ui = UpdateUi::with_auto_check(true);
        let m = ui.chrome_model();
        assert!(!m.bar_open);
        assert!(m.auto_check);
        assert!(m.check_status.contains(&update::current_version().to_string()));
    }

    #[test]
    fn automatic_check_finding_an_update_opens_the_bar_and_notifies() {
        let mut ui = UpdateUi::with_auto_check(true);
        let notify = ui.on_check_result(available("9.9.9", vec![asset_for_this_platform()]), false);
        assert!(notify.is_some_and(|(_, body)| body.contains("9.9.9")));
        let m = ui.chrome_model();
        assert!(m.bar_open);
        assert_eq!(m.title, "Доступна версия 9.9.9");
        assert_eq!(m.action, ChromeUpdateAction::Download);
    }

    #[test]
    fn manual_check_does_not_notify() {
        let mut ui = UpdateUi::with_auto_check(true);
        assert!(ui.on_check_result(available("9.9.9", vec![asset_for_this_platform()]), true).is_none());
        assert!(ui.chrome_model().bar_open);
    }

    #[test]
    fn update_without_a_platform_asset_is_reported_but_not_offered() {
        let mut ui = UpdateUi::with_auto_check(true);
        let foreign = UpdateAsset { name: "lumen-plan9.zip".to_owned(), sha256: "00".to_owned(), size: 1 };
        assert!(ui.on_check_result(available("9.9.9", vec![foreign]), false).is_none());
        let m = ui.chrome_model();
        assert!(!m.bar_open);
        assert!(m.check_status.contains("не для этой платформы"));
    }

    #[test]
    fn up_to_date_only_rewrites_the_status_for_a_manual_check() {
        let mut ui = UpdateUi::with_auto_check(true);
        let before = ui.chrome_model().check_status;
        ui.on_check_result(Ok((UpdateState::default(), CheckOutcome::UpToDate)), false);
        assert_eq!(ui.chrome_model().check_status, before, "a throttled auto check proves nothing");
        ui.on_check_result(Ok((UpdateState::default(), CheckOutcome::UpToDate)), true);
        assert!(ui.chrome_model().check_status.starts_with("Установлена последняя версия"));
    }

    #[test]
    fn manual_network_error_is_not_reported_as_up_to_date() {
        let mut ui = UpdateUi::with_auto_check(true);
        ui.on_check_result(Err("dns".to_owned()), true);
        assert_eq!(ui.chrome_model().check_status, "Не удалось проверить: dns");
    }

    #[test]
    fn untrusted_manifest_is_never_offered() {
        let mut ui = UpdateUi::with_auto_check(true);
        ui.on_check_result(
            Ok((
                UpdateState::default(),
                CheckOutcome::Untrusted(update::ManifestVerifyError::UnknownKeyId),
            )),
            false,
        );
        let m = ui.chrome_model();
        assert!(!m.bar_open);
        assert!(m.check_status.contains("подписи"));
    }

    #[test]
    fn download_result_moves_the_stage_to_ready_or_failed() {
        let mut ui = UpdateUi::with_auto_check(true);
        let asset = asset_for_this_platform();
        ui.stage = UpdateStage::Downloading { version: "9.9.9".to_owned(), asset: asset.clone() };
        // Idle manager status: nothing to report yet.
        assert!(!ui.on_download_status());

        ui.stage = UpdateStage::Failed { version: "9.9.9".to_owned(), asset: asset.clone(), reason: "x".to_owned() };
        ui.bar_open = true;
        let m = ui.chrome_model();
        assert_eq!(m.action, ChromeUpdateAction::Download, "a failed download can be retried");
        assert_eq!(m.meta, "x");

        ui.stage = UpdateStage::Ready { version: "9.9.9".to_owned(), asset, path: PathBuf::from("a.zip") };
        let m = ui.chrome_model();
        assert_eq!(m.action, ChromeUpdateAction::Restart);
        assert_eq!(m.title, "Версия 9.9.9 загружена");
    }

    #[test]
    fn a_repeat_check_does_not_reset_a_staged_update() {
        let mut ui = UpdateUi::with_auto_check(true);
        ui.stage = UpdateStage::Ready {
            version: "9.9.9".to_owned(),
            asset: asset_for_this_platform(),
            path: PathBuf::from("a.zip"),
        };
        ui.on_check_result(available("9.9.9", vec![asset_for_this_platform()]), true);
        assert!(matches!(ui.stage, UpdateStage::Ready { .. }));
        assert!(ui.chrome_model().bar_open, "a manual check re-opens a dismissed bar");
    }

    #[test]
    fn dismiss_hides_the_bar_without_losing_the_stage() {
        let mut ui = UpdateUi::with_auto_check(true);
        ui.on_check_result(available("9.9.9", vec![asset_for_this_platform()]), true);
        ui.dismiss();
        assert!(!ui.chrome_model().bar_open);
        assert!(matches!(ui.stage, UpdateStage::Available { .. }));
    }

    #[test]
    fn apply_outside_ready_is_a_no_op() {
        let mut ui = UpdateUi::with_auto_check(true);
        assert!(!ui.apply_and_request_restart());
        assert!(!ui.restart_requested());
    }
}
