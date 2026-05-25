# ADR-001: Custom rendering engine (not a browser wrapper)

## Status

Accepted

## Date

2026-05-01

## Context

Lumen needs a rendering engine. The dominant choices are:
1. Wrap an existing engine (Chromium/WebKit via WebView2/wry/CEF)
2. Build on a partial Rust engine (Servo)
3. Write a standalone engine from scratch

## Decision

Build a fully standalone rendering engine in Rust. Not a wrapper, not a fork.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| WebView2 / wry / CEF wrapper | This would be a different project — Lumen's identity is the engine itself. Transparency and custom features (knowledge layer, AI, adblock at engine level) require owning every layer. |
| Servo integration | Servo is incomplete and architecturally coupled to Firefox infrastructure. We would inherit its constraints without gaining a working browser. |

## Consequences

- **Positive:** full control over every rendering decision; unique features (knowledge layer §12, AI integration, transparent privacy model) are possible at the engine level; educational value; no supply-chain exposure from upstream browser vendors.
- **Negative:** much longer to reach a working browser; every subsystem must be implemented (HTML/CSS/layout/paint/font/network); high maintenance burden.
- **Future:** this decision is foundational and is not revisited. The only exception planned is iOS, where Apple policy requires WebKit — a thin shell over WKWebView for iOS only is acceptable without violating the principle.
