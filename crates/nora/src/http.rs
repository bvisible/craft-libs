// //// Neoffice — added file (no upstream equivalent): the I/O-free HTTP types of the Nora client core.

use serde_json::Value;

use crate::error::NoraError;

/// Identifies a request so the host can hand its answer back.
pub type RequestId = u64;

/// A call the host has to make. It is always a `POST` of a JSON body to `path` on the Neoffice
/// instance; the host adds the origin, the credentials (the Frappe session in the Drive page, an
/// OAuth bearer in NeoDesk) and whatever CSRF header its session needs.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub id: RequestId,
    pub path: String,
    pub body: Value,
}

/// What the host got back. The body is the raw text; the core does the parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub id: RequestId,
    pub status: u16,
    pub body: String,
}

/// Unwraps Frappe's `{"message": …}` envelope, or explains the failure.
///
/// Frappe reports a refused call with an error status and, in the body, `_server_messages` (a JSON
/// list of JSON strings, each with a `message`) or an `exception` text. The first readable one
/// wins; otherwise the status is all there is.
pub fn frappe_message(response: &HttpResponse) -> Result<Value, NoraError> {
    let parsed: Option<Value> = serde_json::from_str(&response.body).ok();
    if (200..300).contains(&response.status) {
        let value = parsed.ok_or_else(|| NoraError::Malformed("the body is not JSON".into()))?;
        return value.get("message").cloned().ok_or_else(|| NoraError::Malformed("no `message` in the answer".into()));
    }
    if let Some(text) = parsed.as_ref().and_then(server_text) {
        return Err(NoraError::Server(text));
    }
    Err(NoraError::Http(response.status))
}

fn server_text(body: &Value) -> Option<String> {
    if let Some(raw) = body.get("_server_messages").and_then(Value::as_str)
        && let Ok(list) = serde_json::from_str::<Vec<String>>(raw)
    {
        for item in list {
            if let Ok(entry) = serde_json::from_str::<Value>(&item)
                && let Some(message) = entry.get("message").and_then(Value::as_str)
                && !message.trim().is_empty()
            {
                return Some(message.trim().to_string());
            }
        }
    }
    body.get("exception").and_then(Value::as_str).map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse { id: 1, status, body: body.into() }
    }

    #[test]
    fn unwraps_the_message_envelope() {
        let value = frappe_message(&response(200, r#"{"message": {"thread_id": "t1"}}"#)).unwrap();
        assert_eq!(value["thread_id"], "t1");
    }

    #[test]
    fn a_success_without_message_is_malformed() {
        assert!(matches!(frappe_message(&response(200, "{}")), Err(NoraError::Malformed(_))));
        assert!(matches!(frappe_message(&response(200, "not json")), Err(NoraError::Malformed(_))));
    }

    #[test]
    fn reads_the_server_message_of_a_refusal() {
        let inner = serde_json::to_string(&serde_json::json!({"message": "Not permitted"})).unwrap();
        let server_messages = serde_json::to_string(&vec![inner]).unwrap();
        let body = serde_json::json!({"exc_type": "PermissionError", "_server_messages": server_messages}).to_string();
        assert_eq!(frappe_message(&response(403, &body)), Err(NoraError::Server("Not permitted".into())));
    }

    #[test]
    fn falls_back_to_the_exception_then_the_status() {
        assert_eq!(
            frappe_message(&response(417, r#"{"exception": "frappe.ValidationError: nope"}"#)),
            Err(NoraError::Server("frappe.ValidationError: nope".into()))
        );
        assert_eq!(frappe_message(&response(502, "<html>bad gateway</html>")), Err(NoraError::Http(502)));
        assert!(NoraError::Http(401).is_auth());
        assert!(!NoraError::Http(502).is_auth());
    }
}
