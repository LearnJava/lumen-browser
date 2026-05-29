---
name: feedback-doc-comments-mandatory
description: "All public structs, fields, and functions must have /// doc comments explaining semantics — parallel sessions depend on them"
metadata: 
  node_type: memory
  type: feedback
  originSessionId: bc7332ec-973e-4a5e-91e1-b34953becc18
---

All public structs, fields, and functions must have `///` doc comments.

**Why:** Multiple parallel Claude Code sessions work on the codebase simultaneously. Without doc comments, a session working in `paint` has to reverse-engineer layout semantics by reading implementation code (e.g., "does `rect` store border-box or content-box?"). This wastes tokens and risks incorrect assumptions.

**How to apply:** When writing or touching any `pub` item (struct, field, function, enum variant), add a `///` comment explaining: what the value represents, coordinate system / box model, units, what it includes/excludes. Keep it to 1–2 lines. Example: `/// Border-box rectangle: includes padding + border, excludes margin.` Related: [[feedback_mark_task_in_progress]].
