//! `lumen-renderer` — GPU process skeleton (PH3-GPUSANDBOX Phase A, срез A5).
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
//! Срез A5 adds a real `wgpu::Device` (see `gpu_device.rs`), created headless
//! on the first `GpuInit` — no surface yet, so `GpuRender` still acknowledges
//! without submitting any GPU work. Reconstructing a `raw-window-handle` from
//! the `GpuSurfaceHandle` the shell sends and wiring an actual present loop is
//! a later срez — see `docs/tasks/ph3-gpu-process-sandbox.md` Phase A step 5.

use std::io::Write as _;

use lumen_ipc::{IpcRequest, IpcResponse, IpcServer};

mod gpu_device;

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

    // Live GPU handles once `GpuInit` succeeds; `None` until then (or if
    // device creation failed — the process stays up and reports GpuError
    // on every subsequent request instead of retrying silently).
    let mut gpu: Option<gpu_device::GpuDevice> = None;

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
                // Срез A5: real headless wgpu::Device (see gpu_device.rs).
                // Surface reconstruction from `GpuSurfaceHandle` is not done
                // yet, so the device cannot present anything — GpuRender
                // still just acknowledges (see below).
                match gpu_device::init() {
                    Some(device) => {
                        eprintln!(
                            "lumen-renderer: GPU device ready ({:?})",
                            device.adapter.get_info().backend
                        );
                        gpu = Some(device);
                        IpcResponse::GpuReady
                    }
                    None => IpcResponse::GpuError {
                        message: "no GPU adapter/device available".to_string(),
                    },
                }
            }
            IpcRequest::GpuRender { display_list: _ } => {
                // Срез A5: device exists but there is no surface to present
                // to yet, so a real frame still cannot be submitted here —
                // require GpuInit to have succeeded first so a misordered
                // shell sees an error instead of a silently faked frame.
                if gpu.is_some() {
                    IpcResponse::GpuFrameDone
                } else {
                    IpcResponse::GpuError { message: "GpuRender before a successful GpuInit".to_string() }
                }
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
