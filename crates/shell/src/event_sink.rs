//! The default [`EventSink`]: everything the network layer reports, printed to
//! stderr as the Phase 0 “network log”.
//!
//! stderr rather than stdout on purpose — the headless dump modes serialize
//! their result to stdout and it has to stay clean; in the window mode both
//! streams land in the same terminal, so the split is invisible there.
//!
//! The DevTools network panel and the shields panel wrap this sink rather than
//! replace it, which is why it stays `pub(crate)` and constructible.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`; only visibility
//! changed.

use crate::*;

/// EventSink, РєРѕС‚РѕСЂС‹Р№ РїРµС‡Р°С‚Р°РµС‚ СЃРµС‚РµРІС‹Рµ СЃРѕР±С‹С‚РёСЏ РІ stdout вЂ” СЌС‚Рѕ Рё РµСЃС‚СЊ
/// В«network logВ» Phase 0, СЂРµР°Р»РёР·СѓСЋС‰РёР№ РїСЂРёРЅС†РёРї в„–4 В«РєР°Р¶РґС‹Р№ РёСЃС…РѕРґСЏС‰РёР№ Р±Р°Р№С‚
/// РІРёРґРµРЅВ». РџРѕР·Р¶Рµ Р·Р°РјРµРЅРёС‚СЃСЏ РЅР° СЃС‚СЂСѓРєС‚СѓСЂРёСЂРѕРІР°РЅРЅС‹Р№ UI-Р»РѕРіРіРµСЂ.
pub(crate) struct StdoutEventSink;

impl EventSink for StdoutEventSink {
    fn emit(&self, event: &Event) {
        // РЎРµС‚РµРІРѕР№ Р»РѕРі РёРґС‘С‚ РІ stderr, С‡С‚РѕР±С‹ stdout dump-СЂРµР¶РёРјРѕРІ РѕСЃС‚Р°РІР°Р»СЃСЏ С‡РёСЃС‚С‹Рј
        // (РЅР° РЅС‘Рј вЂ” С‚РѕР»СЊРєРѕ СЃРµСЂРёР°Р»РёР·РѕРІР°РЅРЅС‹Р№ СЂРµР·СѓР»СЊС‚Р°С‚ pipeline-Р°). Р’ РѕРєРѕРЅРЅРѕРј
        // СЂРµР¶РёРјРµ СЂР°Р·РЅРёС†Р° РЅРµРІРёРґРёРјР°: РѕР±Р° РїРѕС‚РѕРєР° РїРѕРїР°РґР°СЋС‚ РІ С‚РµСЂРјРёРЅР°Р».
        match event {
            Event::RequestStarted { url, .. } => eprintln!("в†’ GET {url}"),
            Event::RequestCompleted { url, status, .. } => eprintln!("в†ђ {status} {url}"),
            Event::RequestBlocked { url, reason, .. } => eprintln!("вњ— {url} ({reason})"),
            Event::RequestFailed { url, stage, reason, .. } => {
                eprintln!("вњ— {url} ({}: {reason})", stage.as_str());
            }
            Event::SubresourceHintFound { url, kind, priority } => {
                let label = match kind {
                    SubresourceKind::Stylesheet => "css",
                    SubresourceKind::Script => "js",
                    SubresourceKind::Image => "img",
                    SubresourceKind::Font => "font",
                    SubresourceKind::Preconnect { dns_only: true } => "dns-prefetch",
                    SubresourceKind::Preconnect { dns_only: false } => "preconnect",
                    SubresourceKind::Other { .. } => "preload",
                };
                let prio = match priority {
                    FetchPriority::High => "high",
                    FetchPriority::Medium => "medium",
                    FetchPriority::Low => "low",
                };
                eprintln!("в¤· preload {label} [{prio}] {url}");
            }
            Event::FormSubmit { method, action, body, .. } => {
                if body.is_empty() {
                    eprintln!("вЉў form {method} {action}");
                } else {
                    eprintln!("вЉў form {method} {action} body={body}");
                }
            }
            _ => {}
        }
    }
}
