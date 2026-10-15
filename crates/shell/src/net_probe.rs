//! COMPAT-ERRORS (COMPAT-ERR-01): отличить «сайт недоступен» от «у компьютера
//! нет интернета».
//!
//! DNS-сбой, молчание и «нет маршрута» выглядят одинаково и при упавшем сайте,
//! и при отключённой сети, а советы разные. Поэтому перед показом страницы
//! ошибки сбой проверяется пробой `lumen_network::connectivity`: ни один из
//! контрольных публичных адресов не ответил — код меняется на
//! `ERR_INTERNET_DISCONNECTED`. Блокирует до 2 с, поэтому вызывается только из
//! потока загрузки, не из UI-потока.

use lumen_core::net_error::NetErrorCode;

/// Код для страницы ошибки навигации на `url`: исходный `code` либо
/// [`NetErrorCode::InternetDisconnected`], если сбой мог быть вызван отсутствием
/// сети и проба это подтвердила. Пробы нет там, где она соврала бы: у сбоев,
/// после которых связь заведомо была; у адресов своей машины и локальной сети;
/// за прокси и Tor (прямое соединение к публичным адресам там либо закрыто,
/// либо не должно открываться).
pub(crate) fn refine(code: NetErrorCode, url: &str) -> NetErrorCode {
    if !code.may_mean_offline() {
        return code;
    }
    let host = lumen_core::url::Url::parse(url).map(|u| u.host().to_owned()).unwrap_or_default();
    if lumen_network::connectivity::is_local_host(&host) {
        return code;
    }
    let profile = crate::config::global();
    if profile.proxy.is_some() || profile.effective_socks5_proxy().is_some() {
        return code;
    }
    if lumen_network::connectivity::is_offline() {
        NetErrorCode::InternetDisconnected
    } else {
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_that_prove_a_link_are_never_probed() {
        for code in [NetErrorCode::ConnectionRefused, NetErrorCode::ConnectionReset, NetErrorCode::HttpStatus(503), NetErrorCode::Failed] {
            assert_eq!(refine(code, "https://example.com/"), code);
        }
    }

    #[test]
    fn local_addresses_are_never_probed() {
        for url in ["http://localhost:8080/", "http://192.168.0.7/", "http://nas/", "file:///x.html", ""] {
            assert_eq!(refine(NetErrorCode::ConnectionTimedOut, url), NetErrorCode::ConnectionTimedOut, "{url}");
        }
    }
}
