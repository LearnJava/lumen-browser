//! `lumen --dump-tls <host|url|@file>...` (COMPAT-CORPUS-5): the TLS trust
//! probe of `lumen_network::tls::probe` as a CLI, one JSON object per line on
//! stdout. `scripts/tls_trust_robot.py` runs it daily over the top-200 RU list
//! and diffs the lines against the previous day's.

use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};

use lumen_network::tls::probe::{parse_target, probe_host};

/// Parallel handshakes. 200 hosts at ≤15 s each must fit in 15 minutes even
/// when a quarter of them hang to the timeout.
const PROBE_JOBS: usize = 24;

/// Expand `@file` targets (one host per line, `#` comments) into hosts.
fn expand_targets(targets: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for t in targets {
        match t.strip_prefix('@') {
            Some(path) => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
                out.extend(
                    text.lines()
                        .map(|l| l.split('#').next().unwrap_or("").trim())
                        .filter(|l| !l.is_empty())
                        .map(str::to_owned),
                );
            }
            None => out.push(t.clone()),
        }
    }
    Ok(out)
}

pub(crate) fn run_dump_tls(targets: &[String]) -> ExitCode {
    let hosts = match expand_targets(targets) {
        Ok(h) => h,
        Err(err) => {
            eprintln!("Ошибка --dump-tls: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut parsed = Vec::with_capacity(hosts.len());
    for h in &hosts {
        match parse_target(h) {
            Some(hp) => parsed.push(hp),
            None => {
                eprintln!("Ошибка --dump-tls: не разобрать цель «{h}»");
                return ExitCode::FAILURE;
            }
        }
    }
    let next = AtomicUsize::new(0);
    let stdout = std::sync::Mutex::new(std::io::stdout());
    std::thread::scope(|scope| {
        for _ in 0..PROBE_JOBS.min(parsed.len()) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((host, port)) = parsed.get(i) else { break };
                    let line = probe_host(host, *port).to_json_line();
                    if let Ok(mut out) = stdout.lock() {
                        let _ = writeln!(out, "{line}");
                        let _ = out.flush();
                    }
                }
            });
        }
    });
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_file_expands_and_skips_comments() {
        let dir = std::env::temp_dir().join(format!("lumen-dump-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp dir");
        let list = dir.join("hosts.txt");
        std::fs::write(&list, "# шапка\nexample.com\n\n  gosuslugi.ru  # госуслуги\n").expect("write");
        let got = expand_targets(&[format!("@{}", list.display()), "sber.ru".into()]).expect("expand");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got, ["example.com", "gosuslugi.ru", "sber.ru"]);
    }

    #[test]
    fn missing_at_file_is_an_error() {
        assert!(expand_targets(&["@/no/such/file.txt".into()]).is_err());
    }
}
