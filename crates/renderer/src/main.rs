//! `lumen-renderer` — GPU process skeleton (PH3-GPUSANDBOX Phase A, срез A2).
//!
//! Spawned by the shell as a child process, mirroring the
//! `lumen-network-service` pattern (`crates/network/src/bin/network_service.rs`):
//! ```text
//! lumen-renderer
//! ```
//! Process:
//! 1. Binds to a random loopback port and prints it to stdout.
//! 2. Accepts one TCP connection (from the shell).
//! 3. Handles `GpuInit`/`GpuRender`/`GpuResize`/`GpuSurfaceLost` in a loop.
//!
//! This srez is IPC plumbing only — there is no `wgpu::Instance`, no device,
//! no actual frame submission yet. `GpuRender` is acknowledged with
//! `GpuFrameDone` without touching `display_list`. Wiring a real render
//! backend (moving `renderer.rs:1582`'s `wgpu::Instance::new()` here) and
//! spawning this binary from the shell (`RendererProcessHandle`) are later
//! srezes — see `docs/tasks/ph3-gpu-process-sandbox.md` Phase A steps 1/3-6.

use std::io::Write as _;

use lumen_ipc::{IpcRequest, IpcResponse, IpcServer};

fn main() {
    // Bind on random loopback port and tell the shell which port we got.
    let (server, port) = IpcServer::bind().unwrap_or_else(|e| {
        eprintln!("lumen-renderer: bind failed: {e}");
        std::process::exit(1);
    });

    // One line to stdout — the shell reads this to know where to connect.
    println!("{port}");
    std::io::stdout().flush().ok();

    // Accept exactly one connection (from the shell). If the shell crashes,
    // the accept() call will unblock because the kernel closes the socket
    // and we exit.
    let mut conn = server.accept().unwrap_or_else(|e| {
        eprintln!("lumen-renderer: accept failed: {e}");
        std::process::exit(1);
    });

    // Request handling loop.
    loop {
        let req = match conn.recv::<IpcRequest>() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("lumen-renderer: recv error (shell disconnected?): {e}");
                break;
            }
        };

        let resp = match req {
            IpcRequest::GpuInit { surface: _, width: _, height: _ } => {
                // Srez A2: no wgpu::Instance/Device yet — just acknowledge.
                // A future srez creates the device here and validates the
                // surface handle before replying GpuReady.
                IpcResponse::GpuReady
            }
            IpcRequest::GpuRender { display_list: _ } => {
                // Srez A2: no render backend wired up yet — acknowledge
                // without submitting any GPU work.
                IpcResponse::GpuFrameDone
            }
            IpcRequest::GpuResize { width: _, height: _ } => IpcResponse::GpuReady,
            IpcRequest::GpuSurfaceLost => IpcResponse::GpuReady,
            // Fetch/Ping/Shutdown/Auth/CreateTab/CloseTab/NavigateTab/Screenshot
            // belong to the network-service or shell `--ipc-server` channels,
            // not the GPU renderer channel — reject them so the match stays
            // exhaustive and any future misrouted request surfaces as a clear
            // error instead of silently doing nothing.
            IpcRequest::Fetch(_)
            | IpcRequest::Ping
            | IpcRequest::Shutdown
            | IpcRequest::Auth { .. }
            | IpcRequest::CreateTab
            | IpcRequest::CloseTab { .. }
            | IpcRequest::NavigateTab { .. }
            | IpcRequest::Screenshot { .. } => IpcResponse::GpuError {
                message: "lumen-renderer only accepts GPU channel messages".to_string(),
            },
        };

        if let Err(e) = conn.send(&resp) {
            eprintln!("lumen-renderer: send error: {e}");
            break;
        }
    }
}
