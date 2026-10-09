//! JSON-RPC 2.0 messages and the two framings `serve` speaks over standard input and output: one
//! message per line for MCP, `Content-Length` headers for LSP.
//!
//! - Plan: [Wave 3, Steps 19 and 20](../../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#25-steps-for-sub-wave-3e-serve---mcp-and-serve---lsp)
//!   (stdio only, no socket)
//! - Requirement: [FR-CLI-06](../../../../docs/prd.md#fr-cli-06), [NFR-SEC-01](../../../../docs/prd.md#nfr-sec-01)
//!
//! No crate is used for either protocol: the servers answer a handful of methods with
//! `serde_json` values, so nothing beyond what the binary already links is added, and no
//! transport but standard input and output exists to be opened.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// The JSON-RPC error codes the servers use.
pub mod code {
    /// The message is not JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// The message is JSON but not a request.
    pub const INVALID_REQUEST: i64 = -32600;
    /// No such method.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// The parameters do not fit the method.
    pub const INVALID_PARAMS: i64 = -32602;
}

/// One message read from a client.
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    /// The request id; `None` for a notification.
    pub id: Option<Value>,
    /// The method.
    pub method: String,
    /// The parameters, `null` when absent.
    pub params: Value,
}

/// Parses one message: a request or notification, or the error response to send instead.
///
/// # Errors
/// The response to write back: a parse error for text that is not JSON, an invalid request for
/// JSON without a method.
pub fn parse(text: &str) -> Result<Incoming, Value> {
    let value: Value = serde_json::from_str(text)
        .map_err(|e| error(&Value::Null, code::PARSE_ERROR, &format!("not JSON: {e}")))?;
    let id = value.get("id").cloned().filter(|id| !id.is_null());
    let Some(method) = value.get("method").and_then(Value::as_str) else {
        return Err(error(
            id.as_ref().unwrap_or(&Value::Null),
            code::INVALID_REQUEST,
            "a request names a method",
        ));
    };
    Ok(Incoming {
        id,
        method: method.to_owned(),
        params: value.get("params").cloned().unwrap_or(Value::Null),
    })
}

/// A successful response.
pub fn result(id: &Value, result: Value) -> Value {
    let mut message = json!({ "jsonrpc": "2.0", "id": id });
    message["result"] = result;
    message
}

/// An error response.
pub fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A notification from the server.
pub fn notification(method: &str, params: Value) -> Value {
    let mut message = json!({ "jsonrpc": "2.0", "method": method });
    message["params"] = params;
    message
}

/// Reads the next line-framed message (MCP): `None` at the end of input. Blank lines are skipped.
///
/// # Errors
/// An I/O error reading the input.
pub fn read_line(input: &mut dyn BufRead) -> std::io::Result<Option<String>> {
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed.to_owned()));
        }
    }
}

/// Writes one line-framed message (MCP).
///
/// # Errors
/// An I/O error writing the output.
pub fn write_line(output: &mut dyn Write, message: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string(message).map_err(std::io::Error::other)?;
    output.write_all(text.as_bytes())?;
    output.write_all(b"\n")?;
    output.flush()
}

/// Reads the next `Content-Length`-framed message (LSP): `None` at the end of input.
///
/// # Errors
/// An I/O error, or a header block without a usable `Content-Length`.
pub fn read_framed(input: &mut dyn BufRead) -> std::io::Result<Option<String>> {
    let mut length = None;
    let mut headers = false;
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            if headers {
                break;
            }
            continue;
        }
        headers = true;
        if let Some((name, value)) = header.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let length = length.ok_or_else(|| std::io::Error::other("no Content-Length header"))?;
    let mut body = vec![0_u8; length];
    input.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(std::io::Error::other)
}

/// Writes one `Content-Length`-framed message (LSP).
///
/// # Errors
/// An I/O error writing the output.
pub fn write_framed(output: &mut dyn Write, message: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string(message).map_err(std::io::Error::other)?;
    write!(output, "Content-Length: {}\r\n\r\n{text}", text.len())?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_notifications_and_bad_messages_are_told_apart() {
        let request = parse(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#);
        assert_eq!(
            request,
            Ok(Incoming {
                id: Some(json!(7)),
                method: "tools/list".into(),
                params: Value::Null
            })
        );
        let note = parse(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#);
        assert!(note.is_ok_and(|n| n.id.is_none() && n.params == json!({})));
        let not_json = parse("{oops");
        assert!(not_json.is_err_and(|e| e["error"]["code"] == code::PARSE_ERROR));
        let no_method = parse(r#"{"jsonrpc":"2.0","id":"a"}"#);
        assert!(
            no_method.is_err_and(|e| e["error"]["code"] == code::INVALID_REQUEST && e["id"] == "a")
        );
        assert_eq!(
            notification("x", json!(1)),
            json!({"jsonrpc":"2.0","method":"x","params":1})
        );
        assert_eq!(result(&json!(1), json!({}))["result"], json!({}));
    }

    #[test]
    fn both_framings_round_trip() -> std::io::Result<()> {
        let message = json!({"jsonrpc":"2.0","id":1,"result":{"text":"é\nline"}});
        let mut lines = Vec::new();
        write_line(&mut lines, &message)?;
        write_line(&mut lines, &json!(2))?;
        assert_eq!(
            String::from_utf8_lossy(&lines).lines().count(),
            2,
            "one line each"
        );
        let mut reader = std::io::Cursor::new([b"\n\n".as_slice(), &lines].concat());
        assert_eq!(read_line(&mut reader)?, Some(message.to_string()));
        assert_eq!(read_line(&mut reader)?, Some("2".into()));
        assert_eq!(read_line(&mut reader)?, None);

        let mut framed = Vec::new();
        write_framed(&mut framed, &message)?;
        write_framed(&mut framed, &json!("next"))?;
        let mut reader = std::io::Cursor::new(framed);
        assert_eq!(read_framed(&mut reader)?, Some(message.to_string()));
        assert_eq!(read_framed(&mut reader)?, Some("\"next\"".into()));
        assert_eq!(read_framed(&mut reader)?, None);
        // Another header beside the length, and a lower-case name.
        let mut reader = std::io::Cursor::new(
            b"content-length: 2\r\nContent-Type: application/vscode-jsonrpc\r\n\r\n{}".to_vec(),
        );
        assert_eq!(read_framed(&mut reader)?, Some("{}".into()));
        let mut reader = std::io::Cursor::new(b"X-Other: 1\r\n\r\n".to_vec());
        assert!(read_framed(&mut reader).is_err());
        Ok(())
    }
}
