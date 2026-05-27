//! Phoenix Channels **v2** wire protocol codec — the framing the `phoenix` JS
//! client (v1.8) uses over WebSocket. A message is a 5-element JSON array:
//!
//! ```text
//! [join_ref, ref, topic, event, payload]
//! ```
//!
//! - `join_ref` / `ref` are opaque strings the client mints; the server echoes
//!   them on replies. Server-initiated pushes use `null` for both.
//! - Replies use event `"phx_reply"` with payload `{"status", "response"}`.
//! - Lifecycle events: `phx_join`, `phx_leave`, `phx_close`, `phx_error`; the
//!   client also heartbeats on topic `"phoenix"` / event `"heartbeat"`.
//!
//! This module is pure and unit-tested; the async server (`bin/server.rs`)
//! sits on top.

use serde_json::{json, Value};

/// A decoded Phoenix channel message.
#[derive(Debug, Clone, PartialEq)]
pub struct PhxMessage {
    pub join_ref: Option<String>,
    pub reference: Option<String>,
    pub topic: String,
    pub event: String,
    pub payload: Value,
}

impl PhxMessage {
    /// Decode a v2 frame `[join_ref, ref, topic, event, payload]`.
    pub fn decode(text: &str) -> Result<PhxMessage, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let arr = v.as_array().ok_or("frame is not a JSON array")?;
        if arr.len() != 5 {
            return Err(format!("expected 5 elements, got {}", arr.len()));
        }
        Ok(PhxMessage {
            join_ref: str_or_null(&arr[0]),
            reference: str_or_null(&arr[1]),
            topic: arr[2].as_str().ok_or("topic not a string")?.to_string(),
            event: arr[3].as_str().ok_or("event not a string")?.to_string(),
            payload: arr[4].clone(),
        })
    }

    /// Encode to a v2 frame.
    pub fn encode(&self) -> String {
        Value::Array(vec![
            opt_str(&self.join_ref),
            opt_str(&self.reference),
            Value::String(self.topic.clone()),
            Value::String(self.event.clone()),
            self.payload.clone(),
        ])
        .to_string()
    }

    /// A reply to this message, echoing its refs and topic.
    pub fn reply(&self, status: &str, response: Value) -> PhxMessage {
        PhxMessage {
            join_ref: self.join_ref.clone(),
            reference: self.reference.clone(),
            topic: self.topic.clone(),
            event: "phx_reply".to_string(),
            payload: json!({ "status": status, "response": response }),
        }
    }

    /// An `ok` reply with an empty response object.
    pub fn ok(&self) -> PhxMessage {
        self.reply("ok", json!({}))
    }

    /// An `error` reply carrying `{reason}`.
    pub fn error_reason(&self, reason: &str) -> PhxMessage {
        self.reply("error", json!({ "reason": reason }))
    }
}

/// A server-initiated push to `topic` (no refs), e.g. a snapshot broadcast.
pub fn push(topic: &str, event: &str, payload: Value) -> PhxMessage {
    PhxMessage {
        join_ref: None,
        reference: None,
        topic: topic.to_string(),
        event: event.to_string(),
        payload,
    }
}

fn str_or_null(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_str(o: &Option<String>) -> Value {
    match o {
        Some(s) => Value::String(s.clone()),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_join_frame() {
        let m = PhxMessage::decode(r#"["1","2","player:alice","phx_join",{"username":"alice"}]"#)
            .unwrap();
        assert_eq!(m.join_ref.as_deref(), Some("1"));
        assert_eq!(m.reference.as_deref(), Some("2"));
        assert_eq!(m.topic, "player:alice");
        assert_eq!(m.event, "phx_join");
        assert_eq!(m.payload["username"], "alice");
    }

    #[test]
    fn reply_shape_matches_phoenix() {
        let m = PhxMessage::decode(r#"["1","2","player:alice","phx_join",{}]"#).unwrap();
        let r = m.ok();
        assert_eq!(r.event, "phx_reply");
        // [join_ref, ref, topic, "phx_reply", {status, response}]
        let encoded = r.encode();
        let v: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(v[0], "1");
        assert_eq!(v[1], "2");
        assert_eq!(v[2], "player:alice");
        assert_eq!(v[3], "phx_reply");
        assert_eq!(v[4]["status"], "ok");
        assert_eq!(v[4]["response"], json!({}));
    }

    #[test]
    fn error_reply_carries_reason() {
        let m = PhxMessage::decode(r#"["1","2","player:alice","harvest",{}]"#).unwrap();
        let r = m.error_reason("too_far");
        let v: Value = serde_json::from_str(&r.encode()).unwrap();
        assert_eq!(v[4]["status"], "error");
        assert_eq!(v[4]["response"]["reason"], "too_far");
    }

    #[test]
    fn server_push_has_null_refs() {
        let p = push("chunk:0:0", "snapshot", json!({"players":{}}));
        let v: Value = serde_json::from_str(&p.encode()).unwrap();
        assert_eq!(v[0], Value::Null);
        assert_eq!(v[1], Value::Null);
        assert_eq!(v[2], "chunk:0:0");
        assert_eq!(v[3], "snapshot");
    }

    #[test]
    fn heartbeat_round_trips() {
        let m = PhxMessage::decode(r#"[null,"5","phoenix","heartbeat",{}]"#).unwrap();
        assert_eq!(m.topic, "phoenix");
        assert_eq!(m.event, "heartbeat");
        assert!(m.join_ref.is_none());
        assert_eq!(m.reference.as_deref(), Some("5"));
    }
}
