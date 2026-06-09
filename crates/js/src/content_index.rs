//! Content Index API Phase 0 (W3C Content Index Level 1).
//!
//! Enables Progressive Web Apps to register offline-available content with the
//! browser so it can surface that content in system-level UI (e.g., an "Offline
//! content" drawer). The API hangs off `ServiceWorkerRegistration`.
//!
//! Phase 0: in-memory index with no persistence or OS UI integration.
//!
//! Shell Phase 1: `_lumen_content_index_add(json)` / `_lumen_content_index_getAll()` /
//! `_lumen_content_index_delete(id)` native bindings for SQLite persistence.

use rquickjs::Ctx;

/// Install Content Index API on `ServiceWorkerRegistration.prototype`.
///
/// Must run after the service-worker shim so that `ServiceWorkerRegistration`
/// is already defined on `globalThis`.
pub fn install_content_index_api(ctx: &Ctx) -> rquickjs::Result<()> {
    ctx.eval::<(), _>(CONTENT_INDEX_SHIM)?;
    Ok(())
}

const CONTENT_INDEX_SHIM: &str = r#"(function() {
  'use strict';

  // ContentIndex class — represents registration.index (§4.1)
  function ContentIndex() {
    Object.defineProperty(this, '_entries', { value: [], writable: true, configurable: true });
  }

  // add(description) → Promise<undefined> (§4.2.1)
  // description: { id, title, description, url, category?, icons? }
  ContentIndex.prototype.add = function(description) {
    if (!description || typeof description.id !== 'string' || !description.id) {
      return Promise.reject(new TypeError('description.id must be a non-empty string'));
    }
    if (typeof description.title !== 'string') {
      return Promise.reject(new TypeError('description.title must be a string'));
    }
    if (typeof description.url !== 'string') {
      return Promise.reject(new TypeError('description.url must be a string'));
    }
    var entry = {
      id:          description.id,
      title:       description.title,
      description: description.description || '',
      url:         description.url,
      category:    description.category || 'page',
      icons:       description.icons    || []
    };
    // Remove any existing entry with the same id (upsert semantics §4.2.1 step 7)
    this._entries = this._entries.filter(function(e) { return e.id !== entry.id; });
    this._entries.push(entry);
    return Promise.resolve(undefined);
  };

  // getAll() → Promise<ContentDescription[]> (§4.2.2)
  ContentIndex.prototype.getAll = function() {
    return Promise.resolve(this._entries.slice());
  };

  // delete(id) → Promise<undefined> (§4.2.3)
  ContentIndex.prototype.delete = function(id) {
    this._entries = this._entries.filter(function(e) { return e.id !== id; });
    return Promise.resolve(undefined);
  };

  globalThis.ContentIndex = ContentIndex;

  // Wire onto ServiceWorkerRegistration.prototype if it exists
  if (typeof ServiceWorkerRegistration !== 'undefined') {
    if (!ServiceWorkerRegistration.prototype.index) {
      Object.defineProperty(ServiceWorkerRegistration.prototype, 'index', {
        configurable: true,
        enumerable: true,
        get: function() {
          if (!this._contentIndex) this._contentIndex = new ContentIndex();
          return this._contentIndex;
        }
      });
    }
  }

})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rquickjs::{Context, Runtime};

    fn make_ctx() -> (Runtime, Context) {
        let rt = Runtime::new().unwrap();
        let ctx = Context::full(&rt).unwrap();
        (rt, ctx)
    }

    fn install_prereqs(ctx: &rquickjs::Ctx) {
        ctx.eval::<(), _>(
            r#"
            if (typeof ServiceWorkerRegistration === 'undefined') {
                function ServiceWorkerRegistration() {}
                globalThis.ServiceWorkerRegistration = ServiceWorkerRegistration;
            }
            "#,
        )
        .unwrap();
        install_content_index_api(ctx).unwrap();
    }

    #[test]
    fn content_index_class_exists() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx.eval("typeof ContentIndex === 'function'").unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn content_index_add_and_get_all() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            // add() pushes to _entries synchronously; Promise.resolve wraps the return
            let ok: bool = ctx
                .eval(
                    r#"
                    var idx = new ContentIndex();
                    idx.add({ id: '1', title: 'Page One', url: '/one' });
                    idx._entries.length === 1 && idx._entries[0].id === '1'
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn content_index_delete() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var idx = new ContentIndex();
                    idx.add({ id: 'x', title: 'X', url: '/x' });
                    idx.delete('x');
                    idx._entries.length === 0
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn content_index_add_validates_required_fields() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            // Missing id → returns rejected Promise (synchronously rejected)
            let ok: bool = ctx
                .eval(
                    r#"
                    var idx = new ContentIndex();
                    var p = idx.add({});
                    p instanceof Promise
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }

    #[test]
    fn service_worker_registration_has_index() {
        let (_rt, ctx) = make_ctx();
        ctx.with(|ctx| {
            install_prereqs(&ctx);
            let ok: bool = ctx
                .eval(
                    r#"
                    var reg = new ServiceWorkerRegistration();
                    reg.index instanceof ContentIndex
                    "#,
                )
                .unwrap();
            assert!(ok);
        });
    }
}
