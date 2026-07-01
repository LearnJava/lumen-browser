//! Request/reply plumbing for the live-window automation channel (SDC-2).
//!
//! SDC-1b wired [`AutomationCommand`] execution into the live shell window's
//! event loop, but callers outside the shell process had no way to receive a
//! reply — the shell only exposed a bare `Sender<AutomationCommand>`, and all
//! replies were sent into a channel whose receiver nobody read. This module
//! is the missing half: a per-call reply channel plus a blocking, cloneable
//! handle that external front-ends (`lumen-bidi-server`, `lumen-mcp`) use to
//! drive a live window and get an answer back.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lumen_core::error::{Error, Result};

use crate::{AutomationCommand, AutomationReply};

/// One outstanding request to the live shell window: a command plus the
/// one-shot channel its reply must be sent back on.
///
/// Defined here (not inline in `lumen-shell`) so external front-ends can
/// construct and drain this pair without depending on shell internals.
pub type AutomationRequest = (AutomationCommand, Sender<AutomationReply>);

/// A callback that interrupts a parked (`winit::event_loop::ControlFlow::Wait`)
/// event loop so it promptly drains a newly-enqueued [`AutomationRequest`].
///
/// Opaque `Arc<dyn Fn>` rather than a concrete winit type: `lumen-driver` sits
/// below `lumen-shell` in the crate graph and must not depend on it (or on
/// `winit`) — the shell supplies this as `move || { let _ =
/// load_proxy.send_event(LoadEvent::AutomationWake); }` via [`AutomationHandle::set_wake`].
pub type WakeFn = Arc<dyn Fn() + Send + Sync>;

/// Thread-safe, cloneable handle for sending [`AutomationCommand`]s to a live
/// shell window and blocking for the reply.
///
/// The shell's event loop drains its `Receiver<AutomationRequest>` once per
/// frame (see `Lumen::about_to_wait` automation-dispatch block) and answers
/// on the reply sender carried alongside each command. One [`AutomationHandle`]
/// can be cloned across threads/connections — each [`execute`](Self::execute)
/// call creates a fresh reply channel, so concurrent callers never cross
/// replies.
#[derive(Clone)]
pub struct AutomationHandle {
    tx: Sender<AutomationRequest>,
    /// Shared so a wake callback attached later (`set_wake`, once the event
    /// loop exists) is visible to clones already handed out — `main()`
    /// constructs the handle and spawns `--bidi-port`/`--mcp-live-port`
    /// front-ends *before* the window (and its `EventLoopProxy`) exists.
    wake: Arc<Mutex<Option<WakeFn>>>,
}

impl AutomationHandle {
    /// Wrap the sending half of a shell's automation channel. No wake
    /// callback yet — see [`set_wake`](Self::set_wake).
    pub fn new(tx: Sender<AutomationRequest>) -> Self {
        Self { tx, wake: Arc::new(Mutex::new(None)) }
    }

    /// Attach (or replace) the event-loop wake callback. Visible immediately
    /// to every existing and future clone of this handle (shared cell).
    pub fn set_wake(&self, wake: WakeFn) {
        if let Ok(mut guard) = self.wake.lock() {
            *guard = Some(wake);
        }
    }

    /// Send `command` to the live window and block for its reply, up to `timeout`.
    ///
    /// Returns `Err` if the shell's event loop is not running (channel
    /// disconnected — e.g. no window was opened) or the command was not
    /// answered within `timeout`.
    pub fn execute(&self, command: AutomationCommand, timeout: Duration) -> Result<AutomationReply> {
        let (reply_tx, reply_rx): (Sender<AutomationReply>, Receiver<AutomationReply>) = channel();
        self.tx
            .send((command, reply_tx))
            .map_err(|_| Error::Other("automation channel closed — no live window running".into()))?;
        if let Ok(guard) = self.wake.lock()
            && let Some(wake) = guard.as_ref()
        {
            wake();
        }
        reply_rx.recv_timeout(timeout).map_err(|e| match e {
            RecvTimeoutError::Timeout => Error::Other("automation command timed out".into()),
            RecvTimeoutError::Disconnected => {
                Error::Other("live window closed before replying".into())
            }
        })
    }
}
