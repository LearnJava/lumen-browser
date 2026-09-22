//! Bundled Certificate Transparency log identities — ph3-tls-hardening, part A4.
//!
//! A CT log's identity, for the purpose of [`super::ct`]'s policy check, is the SHA-256
//! hash of its public key (`LogID` in RFC 6962 §3.2 — the same 32 bytes an
//! [`super::ct::extract_sct_log_ids`] SCT carries). [`KNOWN_LOG_IDS`] is meant to hold the
//! log IDs of logs this build trusts, sourced from a maintained, published list (e.g.
//! Chrome's or Apple's CT log list) the way [`webpki_roots`] sources root CAs.
//!
//! ## Why this starts empty
//!
//! Unlike `webpki-roots` (vendored wholesale, one crate, one trust decision), a CT log
//! list is a curated, actively-changing trust judgment — logs get distrusted
//! (misissuance, downtime past SLA), new logs get qualified, and getting even one entry
//! wrong has a real consequence: an ID copied from the wrong source, or hand-transcribed
//! incorrectly, either makes every legitimate SCT from that log silently uncounted
//! (weakening the policy check below its intended threshold with no visible symptom) or —
//! if the table were ever used to hard-fail — could reject a legitimately logged
//! certificate outright. [`super::ct::evaluate_ct`] is soft-fail-only precisely because this
//! table is empty today, mirroring `TRUSTED_KEYS` in `crates/shell/src/update.rs` (UPD-3),
//! which starts empty for the same reason: verification logic that is correct but has
//! nothing trustworthy to check against yet.
//!
//! **Graduation criterion**: populate from a maintained, machine-readable log list (e.g.
//! <https://www.gstatic.com/ct/log_list/v3/log_list.json>, cross-checked against a second
//! independent source before merging), vendored and refreshed the way `webpki-roots` is —
//! a separate task, not this slice.
//!
//! [`webpki_roots`]: https://docs.rs/webpki-roots

/// SHA-256 `LogID` values (RFC 6962 §3.2) of CT logs this build recognizes for the
/// distinct-logs policy check in [`super::ct::evaluate_ct`]. Empty until graduation — see
/// the module-level docs.
pub const KNOWN_LOG_IDS: &[[u8; 32]] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_log_ids_is_empty_pending_graduation() {
        assert!(KNOWN_LOG_IDS.is_empty());
    }
}
