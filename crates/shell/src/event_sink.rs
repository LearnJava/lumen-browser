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

/// EventSink, который печатает сетевые события в stdout — это и есть
/// «network log» Phase 0, реализующий принцип №4 «каждый исходящий байт
/// виден». Позже заменится на структурированный UI-логгер.
pub(crate) struct StdoutEventSink;

impl EventSink for StdoutEventSink {
    fn emit(&self, event: &Event) {
        // Сетевой лог идёт в stderr, чтобы stdout dump-режимов оставался чистым
        // (на нём — только сериализованный результат pipeline-а). В оконном
        // режиме разница невидима: оба потока попадают в терминал.
        match event {
            Event::RequestStarted { url, .. } => eprintln!("→ GET {url}"),
            Event::RequestCompleted { url, status, .. } => eprintln!("← {status} {url}"),
            Event::RequestBlocked { url, reason, .. } => eprintln!("✗ {url} ({reason})"),
            Event::RequestFailed { url, stage, reason, .. } => {
                eprintln!("✗ {url} ({}: {reason})", stage.as_str());
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
                eprintln!("⤷ preload {label} [{prio}] {url}");
            }
            Event::FormSubmit { method, action, body, .. } => {
                if body.is_empty() {
                    eprintln!("⊢ form {method} {action}");
                } else {
                    eprintln!("⊢ form {method} {action} body={body}");
                }
            }
            _ => {}
        }
    }
}
