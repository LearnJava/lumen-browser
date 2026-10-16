//! Cause of a certificate error (COMPAT-TRUST-CAUSES).
//!
//! More than half of certificate errors are not the site's doing: a wrong
//! system clock, an antivirus that re-signs HTTPS, a Wi-Fi login page, the
//! Russian state CA, a server that forgot its intermediate. One screen
//! "attackers can intercept your passwords" for all of them teaches users to
//! click through warnings, so the screen names the cause and keeps the scary
//! wording for errors that stay unexplained.
//!
//! [`classify`] is pure: the network side (the chain that failed, the control
//! request) is gathered by the caller — on the load thread, see
//! `page_load.rs` — and passed in.

use lumen_core::error::CertError;
use lumen_network::captive::PortalProbe;
use lumen_network::tls::failure::{CertSummary, ChainEvidence};

/// Clock difference from which a date error is blamed on the clock.
const CLOCK_SKEW_SECS: i64 = 86_400;

/// A moment this build is known to be later than (2026-10-01). A system clock
/// before it is wrong whatever the certificate says. Raising it is always safe.
const CLOCK_FLOOR_UNIX: i64 = 1_790_812_800;

/// Why the certificate was rejected, as far as can be told.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CertCause {
    /// Nothing more is known: the generic warning stays.
    #[default]
    Unexplained,
    /// The system clock is off. `reference_unix` is the network's idea of now,
    /// when a control reply carried one.
    WrongClock { system_unix: i64, reference_unix: Option<i64> },
    /// An antivirus (or its proxy) re-signs HTTPS with its own root.
    Antivirus { product: &'static str },
    /// The network answers every address with a login page.
    CaptivePortal,
    /// The chain leads to the Russian state CA (НУЦ Минцифры).
    GovCa,
    /// The user trusts the Russian state CA for this host, but the certificate lacks the
    /// Certificate Transparency evidence (two log operators) that trust is conditional on.
    GovCaCt,
    /// The server did not send the intermediate certificate (or the chain
    /// ends at a CA this client does not know).
    IncompleteChain,
    /// The issuer revoked the certificate.
    Revoked,
}

/// What the caller learned besides the error itself.
#[derive(Debug, Clone, Default)]
pub struct CauseInputs {
    /// The chain the server presented, if the verifier recorded it.
    pub chain: Option<ChainEvidence>,
    /// Reply to the control request, `None` if it was not sent or got no reply.
    pub probe: Option<PortalProbe>,
    /// System clock now, Unix seconds.
    pub now_unix: i64,
}

/// Antivirus products that re-sign HTTPS: lowercase fragment of the issuer
/// name → display name.
const ANTIVIRUS_ROOTS: [(&str, &str); 14] = [
    ("kaspersky", "Kaspersky"),
    ("eset ssl filter", "ESET"),
    ("eset", "ESET"),
    ("avast", "Avast"),
    ("avg web", "AVG"),
    ("bitdefender", "Bitdefender"),
    ("dr.web", "Dr.Web"),
    ("doctor web", "Dr.Web"),
    ("norton", "Norton"),
    ("mcafee", "McAfee"),
    ("trend micro", "Trend Micro"),
    ("sophos", "Sophos"),
    ("g data", "G DATA"),
    ("malwarebytes", "Malwarebytes"),
];

/// Name an antivirus product from the issuer of any certificate in the chain.
fn antivirus_product(certs: &[CertSummary]) -> Option<&'static str> {
    certs.iter().find_map(|c| {
        let issuer = format!("{} {}", c.issuer_cn, c.issuer_org).to_lowercase();
        ANTIVIRUS_ROOTS.iter().find(|(needle, _)| issuer.contains(needle)).map(|(_, name)| *name)
    })
}

/// Gather what [`classify`] needs for a failed navigation to `host` and decide.
/// Blocks on the control request (2 s at most), so call it off the UI thread.
#[must_use]
pub fn diagnose(host: &str, err: &CertError) -> CertCause {
    let profile = crate::config::global();
    // A portal lives on the local network: through a proxy or Tor the request
    // would not see it, and Tor must not make extra direct connections.
    let direct = profile.proxy.is_none() && profile.effective_socks5_proxy().is_none();
    let wanted = !matches!(err, CertError::Revoked | CertError::CtInsufficient);
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    classify(
        err,
        &CauseInputs {
            chain: lumen_network::tls::failure::take(host),
            probe: if direct && wanted { lumen_network::captive::probe() } else { None },
            now_unix,
        },
    )
}

/// Open the Windows date and time settings (`ms-settings:dateandtime`).
/// Elsewhere there is no such page to open: returns `false`.
pub fn open_time_settings() -> bool {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg("ms-settings:dateandtime").spawn().is_ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// Decide the cause of `err`.
#[must_use]
pub fn classify(err: &CertError, inputs: &CauseInputs) -> CertCause {
    let now = inputs.chain.as_ref().map_or(inputs.now_unix, |c| c.now_unix);
    match err {
        CertError::Revoked => return CertCause::Revoked,
        CertError::Expired | CertError::NotValidYet => {
            return if clock_is_wrong(err, now, inputs) {
                CertCause::WrongClock {
                    system_unix: now,
                    reference_unix: inputs.probe.and_then(|p| p.server_date_unix),
                }
            } else {
                CertCause::Unexplained
            };
        }
        CertError::Untrusted(_) | CertError::SelfSigned | CertError::NameMismatch | CertError::Other(_) => {}
        CertError::CtInsufficient => {
            return if inputs.chain.as_ref().is_some_and(|c| c.nuc) { CertCause::GovCaCt } else { CertCause::Unexplained };
        }
    }
    if inputs.probe.is_some_and(|p| p.captive) {
        return CertCause::CaptivePortal;
    }
    let (CertError::Untrusted(_), Some(chain)) = (err, inputs.chain.as_ref()) else {
        return CertCause::Unexplained;
    };
    if let Some(product) = antivirus_product(&chain.certs) {
        CertCause::Antivirus { product }
    } else if chain.nuc {
        CertCause::GovCa
    } else if !chain.last_self_signed && !chain.last_issuer_is_anchor {
        CertCause::IncompleteChain
    } else {
        CertCause::Unexplained
    }
}

/// A date error is the clock's fault when the clock disagrees with the
/// network's by more than a day; without a network reply — when the
/// certificate is not valid yet by more than a day, or the clock is earlier
/// than this build.
fn clock_is_wrong(err: &CertError, now: i64, inputs: &CauseInputs) -> bool {
    if let Some(reference) = inputs.probe.and_then(|p| p.server_date_unix) {
        return (now - reference).abs() > CLOCK_SKEW_SECS;
    }
    if now < CLOCK_FLOOR_UNIX {
        return true;
    }
    let leaf_not_before = inputs.chain.as_ref().and_then(|c| c.certs.first()).and_then(|c| c.not_before);
    matches!(err, CertError::NotValidYet) && leaf_not_before.is_some_and(|nb| nb - now > CLOCK_SKEW_SECS)
}

/// `dd.mm.yyyy` of a Unix timestamp (UTC).
#[must_use]
pub fn format_date(unix: i64) -> String {
    // Howard Hinnant's `civil_from_days`.
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{day:02}.{month:02}.{year}")
}

/// Screen text for a chain that ends at the Russian state CA (НУЦ Минцифры). `consented`: the
/// user already agreed to that CA for `host`, so the chain failed the check against its root.
#[must_use]
pub fn gov_ca_text(host: &str, consented: bool) -> (String, String) {
    if consented {
        (
            "Сертификат Минцифры не прошёл проверку".to_owned(),
            format!(
                "Вы разрешили доверять удостоверяющему центру Минцифры для {host}, но цепочка сертификатов не ведёт к его корню или повреждена. Это может быть подмена."
            ),
        )
    } else {
        (
            "Сайт использует российский госсертификат".to_owned(),
            format!(
                "Сертификат {host} выдан удостоверяющим центром Минцифры. Так работают многие российские банки и госсервисы. Этого центра нет в списках браузеров, поэтому Lumen не доверяет ему сам. Вы можете разрешить его для {host}: сертификат будет принят, только если его подтвердят журналы прозрачности двух независимых операторов. Корень в Windows не устанавливается."
            ),
        )
    }
}

impl CertCause {
    /// Headline and body of the error screen for `host`; `None` keeps the
    /// generic warning. No class but [`Self::Unexplained`] talks about
    /// attackers or threats: for them the cause is known and it is not that.
    #[must_use]
    pub fn describe(&self, host: &str) -> Option<(String, String)> {
        let (title, message) = match self {
            Self::Unexplained => return None,
            Self::WrongClock { system_unix, reference_unix } => (
                "На компьютере неверная дата".to_owned(),
                format!(
                    "На компьютере стоит {}{}, поэтому защищённые сайты, включая {host}, не открываются: сертификат кажется недействительным. Исправьте дату и время, затем обновите страницу.",
                    format_date(*system_unix),
                    reference_unix.map_or_else(String::new, |r| format!(", а по данным сети сейчас {}", format_date(r))),
                ),
            ),
            Self::Antivirus { product } => (
                format!("{product} проверяет защищённые соединения"),
                format!(
                    "Сертификат сайта {host} выдан не сайтом, а программой {product}, которая подменяет сертификаты для проверки трафика. Lumen пока не доверяет её корневому сертификату. Отключите проверку HTTPS в {product} или откройте сайт в другом браузере."
                ),
            ),
            Self::CaptivePortal => (
                "Сеть требует входа".to_owned(),
                format!(
                    "Эта сеть (Wi-Fi в кафе, отеле или аэропорту) не пускает в интернет, пока вы не войдёте. Поэтому вместо {host} отвечает страница сети. Войдите в сеть и обновите страницу."
                ),
            ),
            Self::GovCa => gov_ca_text(host, false),
            Self::GovCaCt => (
                "Нет подтверждения прозрачности сертификата".to_owned(),
                format!(
                    "Вы разрешили доверять удостоверяющему центру Минцифры для {host}, но сертификат не подтверждён журналами прозрачности (нужны два независимых оператора). Так бывает, когда центр выдал сертификат втайне от журналов. Не вводите здесь пароли и данные карты; откройте сайт в другом браузере."
                ),
            ),
            Self::IncompleteChain => (
                "Сервер прислал неполную цепочку сертификатов".to_owned(),
                format!(
                    "Сайт {host} не прислал промежуточный сертификат (или цепочка кончается неизвестным центром). Другие браузеры достраивают такие цепочки сами, Lumen пока нет. Это недочёт настройки сайта."
                ),
            ),
            Self::Revoked => (
                "Сертификат отозван".to_owned(),
                format!(
                    "Центр сертификации отозвал сертификат сайта {host}. Это решение центра сертификации, а не Lumen; обойти его нельзя."
                ),
            ),
        };
        Some((title, message))
    }

    /// Whether the screen offers the Windows date and time settings.
    #[must_use]
    pub fn offers_time_settings(&self) -> bool {
        matches!(self, Self::WrongClock { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-14 12:00:00 UTC.
    const NOW: i64 = 1_791_979_200;
    const DAY: i64 = 86_400;

    fn cert(issuer_cn: &str, issuer_org: &str) -> CertSummary {
        CertSummary {
            subject_cn: "site.example".to_owned(),
            issuer_cn: issuer_cn.to_owned(),
            issuer_org: issuer_org.to_owned(),
            not_before: Some(NOW - 30 * DAY),
            not_after: Some(NOW + 60 * DAY),
            ..CertSummary::default()
        }
    }

    fn chain(certs: Vec<CertSummary>, last_self_signed: bool, last_issuer_is_anchor: bool, nuc: bool) -> ChainEvidence {
        ChainEvidence { certs, now_unix: NOW, last_self_signed, last_issuer_is_anchor, nuc }
    }

    fn inputs(chain: Option<ChainEvidence>, probe: Option<PortalProbe>) -> CauseInputs {
        CauseInputs { chain, probe, now_unix: NOW }
    }

    fn ok_probe(date: Option<i64>) -> Option<PortalProbe> {
        Some(PortalProbe { captive: false, server_date_unix: date })
    }

    fn untrusted() -> CertError {
        CertError::Untrusted("UnknownIssuer".to_owned())
    }

    #[test]
    fn clock_behind_by_network_reference() {
        let cause = classify(&CertError::NotValidYet, &inputs(None, ok_probe(Some(NOW + 400 * DAY))));
        assert_eq!(cause, CertCause::WrongClock { system_unix: NOW, reference_unix: Some(NOW + 400 * DAY) });
        assert!(cause.offers_time_settings());
    }

    #[test]
    fn clock_ahead_makes_a_good_certificate_look_expired() {
        let cause = classify(&CertError::Expired, &inputs(None, ok_probe(Some(NOW - 3 * DAY))));
        assert!(matches!(cause, CertCause::WrongClock { .. }));
    }

    #[test]
    fn matching_clocks_leave_a_real_expiry_alone() {
        let cause = classify(&CertError::Expired, &inputs(None, ok_probe(Some(NOW + 3_600))));
        assert_eq!(cause, CertCause::Unexplained);
    }

    #[test]
    fn without_a_reference_far_future_not_before_blames_the_clock() {
        let mut leaf = cert("Some CA", "");
        leaf.not_before = Some(NOW + 5 * DAY);
        let c = chain(vec![leaf], false, true, false);
        assert!(matches!(classify(&CertError::NotValidYet, &inputs(Some(c), None)), CertCause::WrongClock { .. }));
    }

    #[test]
    fn without_a_reference_a_clock_before_the_build_is_wrong() {
        let mut i = inputs(None, None);
        i.now_unix = CLOCK_FLOOR_UNIX - 400 * DAY;
        assert!(matches!(classify(&CertError::Expired, &i), CertCause::WrongClock { .. }));
        // Merely expired, clock plausible: no verdict.
        assert_eq!(classify(&CertError::Expired, &inputs(None, None)), CertCause::Unexplained);
    }

    #[test]
    fn antivirus_is_named_from_the_issuer() {
        let c = chain(vec![cert("Kaspersky Anti-Virus Personal Root Certificate", "AO Kaspersky Lab")], true, false, false);
        let cause = classify(&untrusted(), &inputs(Some(c), ok_probe(None)));
        assert_eq!(cause, CertCause::Antivirus { product: "Kaspersky" });
        let (title, message) = cause.describe("bank.example").expect("explained");
        assert!(title.contains("Kaspersky") && message.contains("bank.example"));
        let c = chain(vec![cert("ESET SSL Filter CA", "ESET, spol. s r. o.")], true, false, false);
        assert_eq!(classify(&untrusted(), &inputs(Some(c), None)), CertCause::Antivirus { product: "ESET" });
    }

    #[test]
    fn captive_portal_wins_over_the_chain() {
        let c = chain(vec![cert("Router Login", "")], true, false, false);
        let probe = Some(PortalProbe { captive: true, server_date_unix: Some(NOW) });
        assert_eq!(classify(&untrusted(), &inputs(Some(c), probe)), CertCause::CaptivePortal);
        assert_eq!(classify(&CertError::NameMismatch, &inputs(None, probe)), CertCause::CaptivePortal);
    }

    #[test]
    fn russian_state_ca_chain() {
        let c = chain(
            vec![cert("Russian Trusted Sub CA", "The Ministry of Digital Development"), cert("Russian Trusted Root CA", "")],
            true,
            false,
            true,
        );
        let cause = classify(&untrusted(), &inputs(Some(c), ok_probe(None)));
        assert_eq!(cause, CertCause::GovCa);
        assert!(cause.describe("sberbank.ru").is_some());
    }

    #[test]
    fn missing_ct_evidence_on_a_state_ca_chain_has_its_own_cause() {
        let nuc = chain(vec![cert("Russian Trusted Sub CA", "")], false, false, true);
        assert_eq!(classify(&CertError::CtInsufficient, &inputs(Some(nuc), None)), CertCause::GovCaCt);
        let other = chain(vec![cert("Some CA", "")], false, false, false);
        assert_eq!(classify(&CertError::CtInsufficient, &inputs(Some(other), None)), CertCause::Unexplained);
    }

    #[test]
    fn state_ca_text_differs_before_and_after_the_users_consent() {
        let (_, offer) = gov_ca_text("sberbank.ru", false);
        let (_, failed) = gov_ca_text("sberbank.ru", true);
        assert!(offer.contains("разрешить") && offer.contains("журналы прозрачности"), "{offer}");
        assert!(failed.contains("не ведёт к его корню"), "{failed}");
    }

    #[test]
    fn leaf_without_intermediate_is_an_incomplete_chain() {
        let c = chain(vec![cert("Example Issuing CA 1", "Example Trust")], false, false, false);
        assert_eq!(classify(&untrusted(), &inputs(Some(c), ok_probe(None))), CertCause::IncompleteChain);
        // A chain that ends at a known anchor but still fails is not a gap.
        let c = chain(vec![cert("Example Issuing CA 1", "Example Trust")], false, true, false);
        assert_eq!(classify(&untrusted(), &inputs(Some(c), None)), CertCause::Unexplained);
        // A self-signed leaf is not a missing intermediate either.
        let c = chain(vec![cert("site.example", "")], true, false, false);
        assert_eq!(classify(&untrusted(), &inputs(Some(c), None)), CertCause::Unexplained);
    }

    #[test]
    fn revoked_is_its_own_class_and_skips_the_probe() {
        let probe = Some(PortalProbe { captive: true, server_date_unix: None });
        assert_eq!(classify(&CertError::Revoked, &inputs(None, probe)), CertCause::Revoked);
    }

    #[test]
    fn unexplained_without_evidence_keeps_the_generic_warning() {
        assert_eq!(classify(&untrusted(), &inputs(None, None)), CertCause::Unexplained);
        assert_eq!(classify(&CertError::CtInsufficient, &inputs(None, None)), CertCause::Unexplained);
        assert!(CertCause::Unexplained.describe("x.example").is_none());
    }

    #[test]
    fn explained_classes_do_not_talk_about_attackers() {
        let all = [
            CertCause::WrongClock { system_unix: NOW, reference_unix: None },
            CertCause::Antivirus { product: "Avast" },
            CertCause::CaptivePortal,
            CertCause::GovCa,
            CertCause::GovCaCt,
            CertCause::IncompleteChain,
            CertCause::Revoked,
        ];
        for cause in all {
            let (title, message) = cause.describe("site.example").expect("explained");
            let text = format!("{title} {message}").to_lowercase();
            assert!(title.chars().count() <= 80, "{title}");
            for word in ["злоумышленн", "угроз", "атак", "взлом"] {
                assert!(!text.contains(word), "{cause:?}: {word}");
            }
        }
    }

    #[test]
    fn dates_format_as_dd_mm_yyyy() {
        assert_eq!(format_date(0), "01.01.1970");
        assert_eq!(format_date(NOW), "14.10.2026");
        assert_eq!(format_date(951_868_800), "01.03.2000");
    }
}
