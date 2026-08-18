//! URL Pattern API (WHATWG URLPattern §3).
//!
//! Pure JavaScript implementation of URLPattern: `new URLPattern(...)` with methods
//! `.test(input)` and `.exec(input)`. Supports wildcard `*`, named groups `:name`,
//! and optional patterns in curly braces.

/// V8 port of the former rquickjs `install_url_pattern_api` (Ph3 V8 migration S5-S7,
/// rquickjs side removed in S12b-B6): identical JS shim, evaluated via
/// [`lumen_core::ext::JsRuntime::eval`] instead of `rquickjs::Ctx::eval`.
///
/// Defines `globalThis.URLPattern` class with:
/// - Constructor: `new URLPattern({pathname, search, hash, hostname})`
/// - Method `test(input)`: returns `true` if input matches all patterns
/// - Method `exec(input)`: returns object with named groups if match, `null` otherwise
#[cfg(feature = "v8-backend")]
pub(crate) fn install_url_pattern_api_v8(rt: &crate::v8_runtime::V8JsRuntime) -> lumen_core::JsResult<()> {
    use lumen_core::ext::JsRuntime as _;
    rt.eval(URL_PATTERN_SHIM)?;
    Ok(())
}

/// JavaScript shim: URLPattern class with pattern parsing and matching.
#[cfg(feature = "v8-backend")]
const URL_PATTERN_SHIM: &str = r#"(function() {
  'use strict';

  /// Parse a pattern string into segments: literal, wildcard, named, optional.
  function parsePattern(patternStr) {
    const segments = [];
    let current = '';
    let i = 0;

    while (i < patternStr.length) {
      const ch = patternStr[i];

      if (ch === '*') {
        if (current) {
          segments.push({type: 'literal', value: current});
          current = '';
        }
        segments.push({type: 'wildcard'});
        i++;
      } else if (ch === ':') {
        if (current) {
          segments.push({type: 'literal', value: current});
          current = '';
        }
        i++;
        // Consume identifier: alphanumeric + underscore
        let name = '';
        while (i < patternStr.length) {
          const nc = patternStr[i];
          if (/[a-zA-Z0-9_]/.test(nc)) {
            name += nc;
            i++;
          } else {
            break;
          }
        }
        if (name) {
          segments.push({type: 'named', value: name});
        }
      } else if (ch === '{') {
        if (current) {
          segments.push({type: 'literal', value: current});
          current = '';
        }
        i++;
        // Consume text until '?}'
        let optText = '';
        while (i < patternStr.length) {
          const nc = patternStr[i];
          if (nc === '?') {
            i++;
            if (i < patternStr.length && patternStr[i] === '}') {
              segments.push({type: 'optional', value: optText});
              i++;
              break;
            } else {
              optText += '?';
            }
          } else if (nc === '}') {
            // No '?', treat as literal
            current += '{' + optText + '}';
            i++;
            break;
          } else {
            optText += nc;
            i++;
          }
        }
      } else {
        current += ch;
        i++;
      }
    }

    if (current) {
      segments.push({type: 'literal', value: current});
    }

    return segments;
  }

  /// Test if input matches pattern segments. Return groups object or null.
  function matchSegments(segments, input) {
    const groups = {};
    let pos = 0;

    for (const seg of segments) {
      if (seg.type === 'literal') {
        if (!input.startsWith(seg.value, pos)) {
          return null;
        }
        pos += seg.value.length;
      } else if (seg.type === 'wildcard') {
        // Wildcard: consume everything remaining (greedy)
        pos = input.length;
      } else if (seg.type === 'named') {
        // Named group: match until delimiter (/, ?, #, &) or end
        let delim = input.length;
        for (let j = pos; j < input.length; j++) {
          if (/[/?&#]/.test(input[j])) {
            delim = j;
            break;
          }
        }
        const value = input.substring(pos, delim);
        groups[seg.value] = value;
        pos = delim;
      } else if (seg.type === 'optional') {
        // Optional: try to match, but don't fail if not there
        if (input.startsWith(seg.value, pos)) {
          pos += seg.value.length;
        }
      }
    }

    // Ensure we consumed entire input
    if (pos === input.length) {
      return groups;
    }
    return null;
  }

  /// URLPattern constructor
  class URLPattern {
    constructor(init) {
      init = init || {};
      this.pathname = init.pathname || '';
      this.search = init.search || '';
      this.hash = init.hash || '';
      this.hostname = init.hostname || '';

      // Pre-parse patterns
      this._pathSegments = parsePattern(this.pathname);
      this._searchSegments = parsePattern(this.search);
      this._hashSegments = parsePattern(this.hash);
      this._hostnameSegments = parsePattern(this.hostname);
    }

    /// Test if input matches all patterns
    test(input) {
      const pathGroups = matchSegments(this._pathSegments, input);
      if (pathGroups === null) return false;

      if (this.search && matchSegments(this._searchSegments, input) === null) {
        return false;
      }

      if (this.hash && matchSegments(this._hashSegments, input) === null) {
        return false;
      }

      if (this.hostname && matchSegments(this._hostnameSegments, input) === null) {
        return false;
      }

      return true;
    }

    /// Execute pattern matching and return groups
    exec(input) {
      const pathGroups = matchSegments(this._pathSegments, input);
      if (pathGroups === null) return null;

      if (this.search && matchSegments(this._searchSegments, input) === null) {
        return null;
      }

      if (this.hash && matchSegments(this._hashSegments, input) === null) {
        return null;
      }

      if (this.hostname && matchSegments(this._hostnameSegments, input) === null) {
        return null;
      }

      return pathGroups;
    }
  }

  globalThis.URLPattern = URLPattern;
})();
"#;

#[cfg(all(test, feature = "v8-backend"))]
mod tests {
    // Хелперы тестового модуля: исключение из clippy.toml покрывает
    // только тело `#[test]` (docs/lint-policy.md §10).
    #![allow(clippy::unwrap_used)]
    use crate::v8_runtime::V8JsRuntime;
    use lumen_core::ext::JsRuntime as _;
    use lumen_core::JsValue;

    fn with_url_pattern(f: impl FnOnce(&V8JsRuntime)) {
        let rt = V8JsRuntime::new().unwrap();
        super::install_url_pattern_api_v8(&rt).unwrap();
        f(&rt);
    }

    #[test]
    fn test_url_pattern_basic() {
        with_url_pattern(|rt| {
            let result = rt
                .eval(
                    r#"
                    const pattern = new URLPattern({pathname: '/users/:id'});
                    pattern.test('/users/123');
                "#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_url_pattern_exec() {
        with_url_pattern(|rt| {
            let result = rt
                .eval(
                    r#"
                    const pattern = new URLPattern({pathname: '/users/:id'});
                    const groups = pattern.exec('/users/456');
                    groups ? groups.id : 'no-match';
                "#,
                )
                .unwrap();
            assert_eq!(result, JsValue::String("456".to_string()));
        });
    }

    #[test]
    fn test_url_pattern_no_match() {
        with_url_pattern(|rt| {
            let result = rt
                .eval(
                    r#"
                    const pattern = new URLPattern({pathname: '/users/:id'});
                    const groups = pattern.exec('/posts/123');
                    groups !== null;
                "#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(false));
        });
    }

    #[test]
    fn test_url_pattern_wildcard() {
        with_url_pattern(|rt| {
            let result = rt
                .eval(
                    r#"
                    const pattern = new URLPattern({pathname: '/api/*'});
                    pattern.test('/api/users/123');
                "#,
                )
                .unwrap();
            assert_eq!(result, JsValue::Bool(true));
        });
    }

    #[test]
    fn test_url_pattern_multiple_groups() {
        with_url_pattern(|rt| {
            let result = rt
                .eval(
                    r#"
                    const pattern = new URLPattern({pathname: '/users/:user_id/posts/:post_id'});
                    const groups = pattern.exec('/users/42/posts/789');
                    groups && groups.user_id === '42' && groups.post_id === '789' ? 'match' : 'no-match';
                "#,
                )
                .unwrap();
            assert_eq!(result, JsValue::String("match".to_string()));
        });
    }
}
