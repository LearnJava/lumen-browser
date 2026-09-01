//! The HTML form-submission algorithm as the shell runs it (HTML LS §4.10.21.4).
//!
//! `run_form_submission` is shared by the two ways a submission starts: a real
//! click on a submit control (which arrives through `handle_click_at`) and a
//! script-initiated `form.submit()`/`requestSubmit()` reaching the shell over
//! `NavigateRequest::SubmitForm`, so both run the same encoding, enctype and
//! navigation code. `dispatch_submit_event` is step 11 — the cancelable
//! `submit` event — and is called only from there.
//!
//! SPLIT-SH6 (2026-08-27): moved verbatim out of `main.rs`. Behaviour and
//! method bodies are unchanged; only the module path and the visibility of
//! `run_form_submission` differ.

use crate::*;

impl Lumen {
    /// HTML LS §4.10.21.4 step 11 — fire a cancelable `submit` event at `form`
    /// (with `submitter` exposed as `SubmitEvent.submitter`) and report whether
    /// the submission may proceed.
    ///
    /// Returns `false` only when a page handler called `preventDefault()`. With
    /// no JS runtime installed, or if the shim call itself throws, it returns
    /// `true` — a script-less page must submit exactly as it did before BUG-437,
    /// and a broken dispatch must never silently swallow a real submission.
    ///
    /// Any navigation the handler queued (`location.href = …`, how an SPA
    /// normally takes the form over) is picked up here, mirroring the
    /// click-dispatch path in [`Self::handle_click_at`] — a *cancelled*
    /// submission still has to honour it.
    /// Run the HTML form-submission algorithm for `form` (HTML LS §4.10.21.4).
    ///
    /// `submitter` is the activated submit control, or `None` when the page
    /// submitted the form from script with no control (`form.submit()`).
    /// `fire_submit_event` controls step 11 — the cancelable `submit` event:
    /// a real click passes `true`, while the script paths pass `false` because
    /// `requestSubmit()` already fired the event on the JS side and `submit()`
    /// is defined to skip it entirely (§4.10.21.3).
    ///
    /// Extracted from the click handler (BUG-383) so `form.submit()` reaching
    /// the shell over `NavigateRequest::SubmitForm` runs the very same encoding,
    /// enctype and navigation code a button press does.
    pub(crate) fn run_form_submission(
        &mut self,
        form: NodeId,
        submitter: Option<NodeId>,
        fire_submit_event: bool,
    ) {
        // BUG-437: everything the document lock is needed for is read in
        // one scoped borrow *before* any JS runs. Dispatching the
        // `submit` event below re-enters the JS runtime, which locks the
        // very same `Arc<Mutex<Document>>` — holding `doc` across that
        // call would deadlock the UI thread.
        let prepared = self.layout_source.as_ref().and_then(|src| {
            let doc = src.document.lock().ok()?;
            let submit_event = lumen_dom::submit_form(&doc, form);
            let enctype = forms::enctype_of_form(&doc, form);
            let dialog_node =
                lumen_dom::find_ancestor_dialog(&doc, submitter.unwrap_or(form));
            Some((submit_event, enctype, dialog_node))
        });
        if let Some((submit_event, enctype, dialog_node)) = prepared {
            match submit_event {
                lumen_dom::FormSubmitEvent::Valid { action, method, fields } => {
                    // HTML LS §4.10.21.4 step 11: fire a **cancelable**
                    // `submit` event at the form before submitting.
                    // BUG-437: this step was missing entirely — the shell
                    // went straight to the native submission below, so a
                    // page's own `submit` handler never ran and could not
                    // `preventDefault()` the navigation. That made every
                    // SPA login form (Keycloak, Next.js) unusable, through
                    // the UI and through MCP/BiDi `click` alike.
                    if fire_submit_event
                        && let Some(sub) = submitter
                        && !self.dispatch_submit_event(form, sub)
                    {
                        return;
                    }
                    // Form passed validation — encode using enctype (HTML LS §4.10.21.6).
                    let body = if enctype == "multipart/form-data" {
                        // Multipart: deterministic boundary for Phase 0.
                        let boundary = "----LumenFormBoundary0000000000000000";
                        let (_ct, bytes) = forms::encode_form_fields_multipart(&fields, boundary);
                        String::from_utf8_lossy(&bytes).into_owned()
                    } else {
                        forms::encode_form_fields(&fields)
                    };
                    use lumen_core::event::{Event, TabId};
                    self.event_sink.emit(&Event::FormSubmit {
                        tab_id: TabId(0),
                        action: action.clone(),
                        method: method.clone(),
                        body: body.clone(),
                    });
                    match method.as_str() {
                        "dialog" => {
                            // HTML LS §4.10.18.3: form with method="dialog" closes
                            // the nearest ancestor <dialog>, setting its returnValue
                            // to the submit button's value attribute.
                            let rv = fields.iter()
                                .find(|(n, _)| n.is_empty() || n == "value")
                                .map(|(_, v)| v.as_str())
                                .unwrap_or("");
                            if let Some(dnid) = dialog_node {
                                let dnid_idx = dnid.index() as u32;
                                let rv = rv.to_string();
                                // ADR-016 M2.2c-2d: fire-and-forget dialog-close через
                                // маршрутизатор — под флагом off-UI-thread, без флага
                                // байт-идентично прежнему `js.fire_dialog_close`.
                                route_task_js(
                                    self.engine_thread.as_ref(),
                                    self.js_ctx.as_ref(),
                                    move |j| j.fire_dialog_close(dnid_idx, &rv),
                                );
                            }
                        }
                        "get" => {
                            // HTML LS §form-submission step 23: navigate
                            // to action + query-string (only urlencoded for GET).
                            let url_body = if enctype == "multipart/form-data" {
                                forms::encode_form_fields(&fields)
                            } else {
                                body.clone()
                            };
                            let get_url = forms::make_get_url(&action, &url_body);
                            let resolved = self.source.resolve_href(&get_url);
                            self.navigate_to(PageSource::from_arg(Some(&resolved)));
                        }
                        _ => {
                            // POST: emit event; real network send is P3 task.
                            eprintln!("[forms] POST {} enctype={} body-len={}", action, enctype, body.len());
                        }
                    }
                }
                lumen_dom::FormSubmitEvent::Invalid { invalid_controls } => {
                    // Form contains invalid controls — show first error.
                    // HTML LS §4.10.21.4 step 4 rejects the submission
                    // before step 11, so no `submit` event is fired here.
                    if let Some(&first_invalid) = invalid_controls.first() {
                        let tooltip = self.layout_source.as_ref().and_then(|src| {
                            let doc = src.document.lock().ok()?;
                            let lb = self.layout_box.as_ref()?;
                            forms::find_control_rect_and_error(lb, &doc, first_invalid)
                        });
                        if let Some((rect, msg)) = tooltip {
                            self.validation_tooltip = Some((rect, msg));
                            if let Some(w) = self.window.as_ref() {
                                w.request_redraw();
                            }
                        }
                        eprintln!(
                            "forms: submit blocked — {} control(s) failed constraint validation",
                            invalid_controls.len()
                        );
                    }
                }
            }
        }
    }

    fn dispatch_submit_event(&mut self, form: NodeId, submitter: NodeId) -> bool {
        let script = format!(
            "_lumen_dispatch_submit_event({}, {})",
            form.index(),
            submitter.index(),
        );
        // `_lumen_dispatch_rich` returns `!event.defaultPrevented`, JSON-encoded
        // by `eval_js_value` — so only a literal `false` cancels.
        let proceed = match route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            move |j| j.eval_js_value(&script),
        ) {
            Some(Ok(json)) => json.trim() != "false",
            Some(Err(_)) | None => true,
        };
        if let Some(Some(nav)) = route_query_js(
            self.engine_thread.as_ref(),
            self.js_ctx.as_ref(),
            |j| j.take_navigate_request(),
        ) {
            self.pending_js_navigate = Some(nav);
        }
        proceed
    }
}
