use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// A server chat event. Extra fields are kept and do not fail parsing.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ChatEvent {
    pub channel_id: String,
    pub seq: u64,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub client_message_id: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn default_kind() -> String {
    "message".to_owned()
}

impl ChatEvent {
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    pub fn is_known_message(&self) -> bool {
        self.kind == "message"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_additive_fields_parse() {
        let event = ChatEvent::parse(
            r#"{"channel_id":"general","seq":4,"body":"hello","future_flag":true,"nested":{"n":1}}"#,
        )
        .unwrap();
        assert_eq!(event.channel_id, "general");
        assert_eq!(event.seq, 4);
        assert_eq!(event.body.as_deref(), Some("hello"));
        assert_eq!(event.extra.get("future_flag"), Some(&Value::Bool(true)));
        assert!(event.extra.contains_key("nested"));
    }

    #[test]
    fn unknown_kind_is_retained() {
        let event =
            ChatEvent::parse(r#"{"channel_id":"general","seq":2,"kind":"reaction","emoji":"ok"}"#)
                .unwrap();
        assert!(!event.is_known_message());
        assert_eq!(event.extra.get("emoji").and_then(Value::as_str), Some("ok"));
    }
}
