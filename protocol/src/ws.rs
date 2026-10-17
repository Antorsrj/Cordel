use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::chat::{ChatEvent, MessageAck, SendMessage, PROTOCOL_VERSION};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WsFrame {
    pub v: u32,
    pub kind: String,
    #[serde(default, flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    UnsupportedVersion { v: u32 },
    MissingField { field: &'static str },
}

impl WsFrame {
    pub fn message(event: &ChatEvent) -> Result<Self, serde_json::Error> {
        let mut fields = Map::new();
        fields.insert(
            "channel_id".to_owned(),
            Value::String(event.channel_id.clone()),
        );
        fields.insert("seq".to_owned(), Value::from(event.seq));
        if let Some(id) = &event.client_message_id {
            fields.insert("client_message_id".to_owned(), Value::String(id.clone()));
        }
        if let Some(body) = &event.body {
            fields.insert("body".to_owned(), Value::String(body.clone()));
        }
        for (key, value) in &event.extra {
            fields.insert(key.clone(), value.clone());
        }
        Ok(Self {
            v: PROTOCOL_VERSION,
            kind: event.kind.clone(),
            fields,
        })
    }

    pub fn send(message: &SendMessage) -> Self {
        let mut fields = Map::new();
        fields.insert(
            "channel_id".to_owned(),
            Value::String(message.channel_id.clone()),
        );
        fields.insert(
            "client_message_id".to_owned(),
            Value::String(message.client_message_id.clone()),
        );
        fields.insert("body".to_owned(), Value::String(message.body.clone()));
        Self {
            v: PROTOCOL_VERSION,
            kind: "send".to_owned(),
            fields,
        }
    }

    pub fn ack(ack: &MessageAck) -> Self {
        let mut fields = Map::new();
        fields.insert(
            "channel_id".to_owned(),
            Value::String(ack.channel_id.clone()),
        );
        fields.insert(
            "client_message_id".to_owned(),
            Value::String(ack.client_message_id.clone()),
        );
        fields.insert("seq".to_owned(), Value::from(ack.seq));
        Self {
            v: PROTOCOL_VERSION,
            kind: "ack".to_owned(),
            fields,
        }
    }

    pub fn lagged(channel_id: impl Into<String>, after_seq: u64) -> Self {
        let mut fields = Map::new();
        fields.insert("channel_id".to_owned(), Value::String(channel_id.into()));
        fields.insert("after_seq".to_owned(), Value::from(after_seq));
        Self {
            v: PROTOCOL_VERSION,
            kind: "lagged".to_owned(),
            fields,
        }
    }

    pub fn check_version(&self) -> Result<(), FrameError> {
        if self.v == PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(FrameError::UnsupportedVersion { v: self.v })
        }
    }

    /// Events that carry a channel sequence can be buffered during catch-up.
    /// Control frames such as `ack` and `lagged` are not chat events.
    pub fn as_chat_event(&self) -> Result<Option<ChatEvent>, FrameError> {
        self.check_version()?;
        if matches!(
            self.kind.as_str(),
            "ack" | "lagged" | "send" | "subscribe" | "error"
        ) {
            return Ok(None);
        }
        let channel_id = self.string_field("channel_id")?;
        let seq = self
            .fields
            .get("seq")
            .and_then(Value::as_u64)
            .ok_or(FrameError::MissingField { field: "seq" })?;
        Ok(Some(ChatEvent {
            channel_id,
            seq,
            kind: self.kind.clone(),
            client_message_id: self.optional_string("client_message_id"),
            body: self.optional_string("body"),
            extra: self
                .fields
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "channel_id" | "seq" | "client_message_id" | "body"
                    )
                })
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
        }))
    }

    fn string_field(&self, field: &'static str) -> Result<String, FrameError> {
        self.fields
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or(FrameError::MissingField { field })
    }

    fn optional_string(&self, field: &str) -> Option<String> {
        self.fields
            .get(field)
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::ChatEvent;
    use std::collections::BTreeMap;

    #[test]
    fn unknown_kind_survives_a_versioned_frame() {
        let raw = r#"{"v":1,"kind":"reaction","channel_id":"general","seq":2,"emoji":"ok"}"#;
        let frame: WsFrame = serde_json::from_str(raw).unwrap();
        let event = frame.as_chat_event().unwrap().unwrap();
        assert_eq!(event.kind, "reaction");
        assert_eq!(event.seq, 2);
        assert_eq!(
            event.extra.get("emoji").and_then(|value| value.as_str()),
            Some("ok")
        );
    }

    #[test]
    fn lagged_frame_is_not_a_chat_event() {
        let frame = WsFrame::lagged("general", 4);
        let parsed: WsFrame =
            serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert_eq!(parsed.as_chat_event().unwrap(), None);
        assert_eq!(
            parsed
                .fields
                .get("after_seq")
                .and_then(serde_json::Value::as_u64),
            Some(4)
        );
    }

    #[test]
    fn message_frame_round_trips_to_a_chat_event() {
        let event = ChatEvent {
            channel_id: "general".to_owned(),
            seq: 1,
            kind: "message".to_owned(),
            client_message_id: Some("client-1".to_owned()),
            body: Some("hello".to_owned()),
            extra: BTreeMap::from([("future_flag".to_owned(), serde_json::Value::Bool(true))]),
        };
        let frame = WsFrame::message(&event).unwrap();
        let parsed: WsFrame =
            serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert_eq!(parsed.as_chat_event().unwrap().unwrap(), event);
    }

    #[test]
    fn a_newer_frame_version_is_rejected() {
        let frame = WsFrame {
            v: 99,
            kind: "message".to_owned(),
            fields: Default::default(),
        };
        assert_eq!(
            frame.check_version(),
            Err(FrameError::UnsupportedVersion { v: 99 })
        );
    }
}
