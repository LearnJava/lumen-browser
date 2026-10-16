//! COMPAT-GATE-DETECT: разбор антибот-стен и баннеров «браузер не поддерживается»
//! в ответе главного документа (`crate::gate_detect`).
//!
//! Стена, показанная один-два раза, ничего не меняет: Cloudflare и другие сами
//! решают JS-проверку и перезагружают страницу. Третья стена того же сайта за
//! минуту — петля (COMPAT-ERR-10): вместо ещё одного прохода пайплайна вкладка
//! показывает панель `crate::gate_page`. Баннер «браузер не поддерживается»
//! (COMPAT-ERR-11/12) пока только пишется в журнал сигналов.

use crate::compat_signals::site_of;
use crate::external_browser::is_handoff_url;
use crate::gate_detect::{check_id, GateLoop, Verdict};
use crate::resource_base::ResourceBase;
use crate::*;

use super::handoff::now_unix;

impl Lumen {
    /// Обработать опознанный в `raw` признак. `true` — страница заменена
    /// панелью и обычную загрузку продолжать не нужно.
    pub(crate) fn intercept_gate(&mut self, raw: &page_source::RawPage) -> bool {
        let Some(verdict) = raw.gate else { return false };
        let ResourceBase::Url(url) = &raw.base else { return false };
        if !is_handoff_url(url) {
            return false;
        }
        let now = now_unix();
        match verdict {
            Verdict::UnsupportedBanner => {
                if !self.active_profile_is_anonymous()
                    && let Some(signal) = self.compat_signals.record_unsupported_banner(url, now)
                {
                    self.persist_signal(&signal);
                }
                false
            }
            Verdict::Antibot(vendor) => {
                let site = site_of(url);
                let count = self.gate_loop.record(&site, now);
                eprintln!("[gate] {} site={site} подряд={count}", vendor.code());
                if !GateLoop::is_loop(count) {
                    return false;
                }
                if count == crate::gate_detect::LOOP_COUNT && !self.active_profile_is_anonymous() {
                    let signal = self.compat_signals.record_antibot_loop(url, vendor.code(), now);
                    self.persist_signal(&signal);
                }
                if count == crate::gate_detect::LOOP_COUNT {
                    self.frustration_antibot_loop(url);
                }
                let shown = self.source.url_str().unwrap_or(url).to_owned();
                let page = gate_page::gate_source(&shown, vendor, &check_id(&site, vendor, now));
                self.nav_start = None;
                self.stream_builder = None;
                self.stream_sheet = lumen_css_parser::Stylesheet::default();
                self.pending_mpa_view_transition_snapshot = None;
                let msg = gate_page::title(vendor);
                eprintln!("Антибот-петля {shown}: {msg}");
                self.show_static_error_page(page, &msg);
                true
            }
        }
    }
}
