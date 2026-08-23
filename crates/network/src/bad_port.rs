//! Fetch Standard §3.9 «bad ports» — список портов, к которым веб-контенту
//! запрещено подключаться.
//!
//! Смысл списка — защита от cross-protocol-атак: страница просит браузер
//! открыть соединение с портом чужой службы (telnet/smtp/irc/…) и подбирает
//! данные так, чтобы служба на том конце приняла их за валидный запрос своего
//! протокола. Поэтому проверка обязана срабатывать **до** DNS-резолва и до
//! открытия сокета: сам факт TCP-подключения уже является атакой.
//!
//! Список одинаков для всех схем, идущих через fetch: `http`/`https`
//! (`require_http_scheme`) и `ws`/`wss` (`require_ws_scheme`).
//!
//! Источник: <https://fetch.spec.whatwg.org/#port-blocking> (проверено против
//! `tests/wpt/websockets/Create-blocked-port.any.js`, 83 значения).

/// Отсортированный по возрастанию список заблокированных портов.
///
/// Порядок — инвариант: [`is_bad_port`] ищет по нему бинарным поиском.
/// Проверяется тестом `bad_ports_list_is_sorted`.
const BAD_PORTS: [u16; 83] = [
    0,     // (порт 0 не адресует службу)
    1,     // tcpmux
    7,     // echo
    9,     // discard
    11,    // systat
    13,    // daytime
    15,    // netstat
    17,    // qotd
    19,    // chargen
    20,    // ftp-data
    21,    // ftp
    22,    // ssh
    23,    // telnet
    25,    // smtp
    37,    // time
    42,    // name
    43,    // nicname
    53,    // domain
    69,    // tftp
    77,    // priv-rjs
    79,    // finger
    87,    // ttylink
    95,    // supdup
    101,   // hostriame
    102,   // iso-tsap
    103,   // gppitnp
    104,   // acr-nema
    109,   // pop2
    110,   // pop3
    111,   // sunrpc
    113,   // auth
    115,   // sftp
    117,   // uucp-path
    119,   // nntp
    123,   // ntp
    135,   // loc-srv / epmap
    137,   // netbios-ns
    139,   // netbios-ssn
    143,   // imap2
    161,   // snmp
    179,   // bgp
    389,   // ldap
    427,   // afp (alternate)
    465,   // smtp (alternate)
    512,   // print / exec
    513,   // login
    514,   // shell
    515,   // printer
    526,   // tempo
    530,   // courier
    531,   // chat
    532,   // netnews
    540,   // uucp
    548,   // afp
    554,   // rtsp
    556,   // remotefs
    563,   // nntp+ssl
    587,   // smtp (outgoing)
    601,   // syslog-conn
    636,   // ldap+ssl
    989,   // ftps-data
    990,   // ftps
    993,   // imap+ssl
    995,   // pop3+ssl
    1719,  // h323gatestat
    1720,  // h323hostcall
    1723,  // pptp
    2049,  // nfs
    3659,  // apple-sasl
    4045,  // lockd
    4190,  // sieve
    5060,  // sip
    5061,  // sips
    6000,  // x11
    6566,  // sane-port
    6665,  // irc (alternate)
    6666,  // irc (alternate)
    6667,  // irc (default)
    6668,  // irc (alternate)
    6669,  // irc (alternate)
    6679,  // osaut
    6697,  // irc+tls
    10080, // amanda
];

/// `true`, если `port` входит в список «bad ports» Fetch §3.9.
pub(crate) fn is_bad_port(port: u16) -> bool {
    BAD_PORTS.binary_search(&port).is_ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn bad_ports_list_is_sorted() {
        // Инвариант binary_search: без него is_bad_port молча даёт false
        // на части списка, и проверка перестаёт работать незаметно.
        assert!(BAD_PORTS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn blocked_ports_are_rejected() {
        for p in [0u16, 1, 22, 25, 53, 6000, 6667, 6679, 10080] {
            assert!(is_bad_port(p), "порт {p} должен быть заблокирован");
        }
    }

    #[test]
    fn ordinary_ports_are_allowed() {
        // Порты, которыми реально пользуется проект: дефолтные http(s),
        // порты WPT-сервера (tests/wpt/config.json) и типичный dev-порт.
        for p in [80u16, 443, 3000, 8080, 8443, 18300, 18443, 18888, 18889, 19000] {
            assert!(!is_bad_port(p), "порт {p} блокировать нельзя");
        }
    }

    #[test]
    fn neighbours_of_blocked_ports_are_allowed() {
        // Границы диапазонов: 6664/6670 рядом с irc-блоком, 10081 — за amanda.
        for p in [2u16, 6664, 6670, 6698, 10079, 10081, u16::MAX] {
            assert!(!is_bad_port(p), "порт {p} блокировать нельзя");
        }
    }
}
