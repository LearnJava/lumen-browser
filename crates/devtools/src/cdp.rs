//! Chrome DevTools Protocol — минимальный диспетчер (5C).
//!
//! Разбирает JSON-RPC сообщение `{"id":N,"method":"D.m","params":{...}}`,
//! маршрутизирует на обработчик, возвращает ответ в виде JSON-строки.
//!
//! Поддерживаемые методы:
//! - Browser.getVersion
//! - DOM.getDocument
//! - Network.enable / CSS.enable / Page.enable / Runtime.enable  (ACK)
//!
//! Всё остальное → JSON-RPC error -32601.

use std::collections::BTreeMap;

use lumen_core::json::{parse as parse_json, JsonValue};

/// Обработать одно CDP сообщение, вернуть JSON-строку для отправки клиенту.
pub fn dispatch(message: &str) -> String {
    match try_dispatch(message) {
        Ok(s) => s,
        Err(e) => make_error(0, -32700, &format!("Parse error: {e}")),
    }
}

fn try_dispatch(message: &str) -> Result<String, String> {
    let val = parse_json(message).map_err(|e| e.to_string())?;
    let id = val
        .get("id")
        .and_then(|v| v.as_number())
        .map(|n| n as i64)
        .unwrap_or(0);
    let method = val
        .get("method")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing method".to_string())?;
    let params = val.get("params").cloned().unwrap_or(JsonValue::Null);

    let result = match method {
        "Browser.getVersion" => handle_browser_get_version(),
        "DOM.getDocument" => handle_dom_get_document(&params),
        "Network.enable" | "CSS.enable" | "Page.enable" | "Runtime.enable" => {
            JsonValue::Object(BTreeMap::new())
        }
        other => return Ok(make_error(id, -32601, &format!("Method not found: {other}"))),
    };

    Ok(make_result(id, result))
}

fn handle_browser_get_version() -> JsonValue {
    let mut obj = BTreeMap::new();
    obj.insert("jsVersion".into(), JsonValue::String(env!("CARGO_PKG_VERSION").into()));
    obj.insert("product".into(), JsonValue::String(format!("Lumen/{}", env!("CARGO_PKG_VERSION"))));
    obj.insert("protocolVersion".into(), JsonValue::String("1.3".into()));
    obj.insert("revision".into(), JsonValue::String("rev0".into()));
    obj.insert("userAgent".into(), JsonValue::String(format!("Lumen/{}", env!("CARGO_PKG_VERSION"))));
    JsonValue::Object(obj)
}

fn handle_dom_get_document(_params: &JsonValue) -> JsonValue {
    // Stub: возвращаем минимальный Document node (nodeType=9).
    let mut root = BTreeMap::new();
    root.insert("backendNodeId".into(), JsonValue::Number(1.0));
    root.insert("childNodeCount".into(), JsonValue::Number(0.0));
    root.insert("children".into(), JsonValue::Array(vec![]));
    root.insert("localName".into(), JsonValue::String(String::new()));
    root.insert("nodeId".into(), JsonValue::Number(1.0));
    root.insert("nodeName".into(), JsonValue::String("#document".into()));
    root.insert("nodeType".into(), JsonValue::Number(9.0));
    root.insert("nodeValue".into(), JsonValue::String(String::new()));

    let mut obj = BTreeMap::new();
    obj.insert("root".into(), JsonValue::Object(root));
    JsonValue::Object(obj)
}

fn make_result(id: i64, result: JsonValue) -> String {
    let mut obj = BTreeMap::new();
    obj.insert("id".into(), JsonValue::Number(id as f64));
    obj.insert("result".into(), result);
    JsonValue::Object(obj).to_string()
}

fn make_error(id: i64, code: i64, message: &str) -> String {
    let mut err = BTreeMap::new();
    err.insert("code".into(), JsonValue::Number(code as f64));
    err.insert("message".into(), JsonValue::String(message.into()));

    let mut obj = BTreeMap::new();
    obj.insert("error".into(), JsonValue::Object(err));
    obj.insert("id".into(), JsonValue::Number(id as f64));
    JsonValue::Object(obj).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_get_version_returns_version() {
        let resp = dispatch(r#"{"id":1,"method":"Browser.getVersion","params":{}}"#);
        let val = parse_json(&resp).unwrap();
        assert_eq!(val.get("id").and_then(|v| v.as_number()), Some(1.0));
        let result = val.get("result").unwrap();
        assert_eq!(result.get("protocolVersion").and_then(|v| v.as_str()), Some("1.3"));
        assert!(result.get("product").and_then(|v| v.as_str()).unwrap().starts_with("Lumen/"));
    }

    #[test]
    fn network_enable_returns_empty_result() {
        let resp = dispatch(r#"{"id":2,"method":"Network.enable","params":{}}"#);
        let val = parse_json(&resp).unwrap();
        assert_eq!(val.get("id").and_then(|v| v.as_number()), Some(2.0));
        assert!(val.get("result").is_some());
        assert!(val.get("error").is_none());
    }

    #[test]
    fn dom_get_document_stub_returns_document_node() {
        let resp = dispatch(r#"{"id":3,"method":"DOM.getDocument","params":{}}"#);
        let val = parse_json(&resp).unwrap();
        let root = val.get("result").unwrap().get("root").unwrap();
        assert_eq!(root.get("nodeType").and_then(|v| v.as_number()), Some(9.0));
        assert_eq!(root.get("nodeName").and_then(|v| v.as_str()), Some("#document"));
    }

    #[test]
    fn unknown_method_returns_error_minus_32601() {
        let resp = dispatch(r#"{"id":4,"method":"Unknown.foo","params":{}}"#);
        let val = parse_json(&resp).unwrap();
        assert_eq!(val.get("id").and_then(|v| v.as_number()), Some(4.0));
        let err = val.get("error").unwrap();
        assert_eq!(err.get("code").and_then(|v| v.as_number()), Some(-32601.0));
    }

    #[test]
    fn invalid_json_returns_parse_error() {
        let resp = dispatch("not json");
        let val = parse_json(&resp).unwrap();
        assert!(val.get("error").is_some());
    }
}
