//! End-to-end test of the `--ipc-server` tab control channel (TAB-4/TAB-5).
//!
//! Spawns the real `lumen` binary in `--ipc-server` mode, reads the port it
//! prints, then drives a full `CreateTab → NavigateTab → Screenshot → CloseTab
//! → Shutdown` sequence over IPC and asserts a valid PNG comes back.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use lumen_ipc::{IpcClient, IpcRequest, IpcResponse};

/// Read the `LUMEN_IPC_PORT=<port>` line from the child's stdout, then keep a
/// background thread draining the rest of stdout so the event-sink output the
/// shell emits during a render cannot fill the pipe and block the child.
fn read_port_and_drain(stdout: std::process::ChildStdout) -> u16 {
    let mut reader = BufReader::new(stdout);
    let port = loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            panic!("child stdout closed before printing LUMEN_IPC_PORT");
        }
        if let Some(rest) = line.trim().strip_prefix("LUMEN_IPC_PORT=") {
            break rest.parse::<u16>().ok();
        }
    };
    // Drain the remainder so the child never blocks on a full stdout pipe.
    std::thread::spawn(move || {
        let mut buf = String::new();
        while reader.read_line(&mut buf).unwrap_or(0) > 0 {
            buf.clear();
        }
    });
    port.expect("failed to parse LUMEN_IPC_PORT value")
}

#[test]
fn ipc_server_create_navigate_screenshot_close() {
    // Minimal valid HTML page in a temp file (no network needed).
    let mut html_path = std::env::temp_dir();
    html_path.push(format!("lumen_ipc_test_{}.html", std::process::id()));
    std::fs::write(
        &html_path,
        "<!doctype html><html><body style=\"background:#0a0;\">\
         <div style=\"width:200px;height:100px;background:#00f\"></div></body></html>",
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_lumen"))
        .arg("--ipc-server")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn lumen --ipc-server");

    let port = read_port_and_drain(child.stdout.take().expect("child stdout"));

    let mut client = IpcClient::connect(port).expect("connect to ipc-server");

    // CreateTab → TabCreated
    let tab_id = match client.request(&IpcRequest::CreateTab).unwrap() {
        IpcResponse::TabCreated { tab_id } => tab_id,
        other => panic!("expected TabCreated, got {other:?}"),
    };

    // NavigateTab(file) → Navigated
    let url = html_path.to_string_lossy().into_owned();
    match client
        .request(&IpcRequest::NavigateTab { tab_id, url })
        .unwrap()
    {
        IpcResponse::Navigated { tab_id: t } => assert_eq!(t, tab_id),
        other => panic!("expected Navigated, got {other:?}"),
    }

    // Screenshot → PNG bytes
    match client.request(&IpcRequest::Screenshot { tab_id }).unwrap() {
        IpcResponse::Screenshot { tab_id: t, png } => {
            assert_eq!(t, tab_id);
            assert!(png.len() > 8, "PNG too small: {} bytes", png.len());
            // PNG magic: 89 50 4E 47 0D 0A 1A 0A
            assert_eq!(&png[0..8], &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        }
        other => panic!("expected Screenshot, got {other:?}"),
    }

    // Screenshot of an unknown tab → TabError
    match client.request(&IpcRequest::Screenshot { tab_id: 9999 }).unwrap() {
        IpcResponse::TabError { tab_id: t, .. } => assert_eq!(t, 9999),
        other => panic!("expected TabError for unknown tab, got {other:?}"),
    }

    // CloseTab → TabClosed
    match client.request(&IpcRequest::CloseTab { tab_id }).unwrap() {
        IpcResponse::TabClosed { tab_id: t } => assert_eq!(t, tab_id),
        other => panic!("expected TabClosed, got {other:?}"),
    }

    // Shutdown → Shutdown, then the process exits.
    match client.request(&IpcRequest::Shutdown).unwrap() {
        IpcResponse::Shutdown => {}
        other => panic!("expected Shutdown, got {other:?}"),
    }

    let status = child.wait().expect("wait for child exit");
    assert!(status.success(), "ipc-server exited with {status:?}");

    let _ = std::fs::remove_file(&html_path);
}
