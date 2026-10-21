use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const PROTOCOL_VERSION: u32 = 1;

/// One ordered chat event. Unknown fields and unknown kinds stay intact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessage {
    pub channel_id: String,
    pub client_message_id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageAck {
    pub channel_id: String,
    pub client_message_id: String,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub channel_id: String,
    pub after_seq: u64,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub channel_id: String,
    pub messages: Vec<ChatEvent>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    WrongChannel { expected: String, found: String },
    SequenceNotAfterCursor { seq: u64, after_seq: u64 },
    SequenceGap { expected: u64, found: u64 },
    DuplicateClientMessageId { client_message_id: String },
}

pub fn validate_history(after_seq: u64, page: &HistoryPage) -> Result<(), HistoryError> {
    let mut previous = after_seq;
    let mut seen_ids = BTreeSet::new();
    for message in &page.messages {
        if message.channel_id != page.channel_id {
            return Err(HistoryError::WrongChannel {
                expected: page.channel_id.clone(),
                found: message.channel_id.clone(),
            });
        }
        if message.seq <= previous {
            return Err(HistoryError::SequenceNotAfterCursor {
                seq: message.seq,
                after_seq: previous,
            });
        }
        if message.seq != previous + 1 {
            return Err(HistoryError::SequenceGap {
                expected: previous + 1,
                found: message.seq,
            });
        }
        if let Some(id) = &message.client_message_id {
            if !seen_ids.insert(id.clone()) {
                return Err(HistoryError::DuplicateClientMessageId {
                    client_message_id: id.clone(),
                });
            }
        }
        previous = message.seq;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(seq: u64, body: &str) -> ChatEvent {
        ChatEvent {
            channel_id: "general".to_owned(),
            seq,
            kind: "message".to_owned(),
            client_message_id: Some(format!("id-{seq}")),
            body: Some(body.to_owned()),
            extra: BTreeMap::new(),
        }
    }

    #[test]
    fn unknown_additive_fields_and_kinds_parse() {
        let event = ChatEvent::parse(
            r#"{"channel_id":"general","seq":4,"body":"hello","future_flag":true,"nested":{"n":1}}"#,
        )
        .unwrap();
        assert_eq!(event.extra.get("future_flag"), Some(&Value::Bool(true)));
        assert!(event.is_known_message());

        let reaction =
            ChatEvent::parse(r#"{"channel_id":"general","seq":2,"kind":"reaction","emoji":"ok"}"#)
                .unwrap();
        assert!(!reaction.is_known_message());
        assert_eq!(
            reaction.extra.get("emoji").and_then(Value::as_str),
            Some("ok")
        );
    }

    #[test]
    fn history_must_continue_after_seq_without_gaps_or_duplicate_ids() {
        let page = HistoryPage {
            channel_id: "general".to_owned(),
            messages: vec![message(2, "two"), message(3, "three")],
            has_more: false,
        };
        assert!(validate_history(1, &page).is_ok());

        let gapped = HistoryPage {
            channel_id: "general".to_owned(),
            messages: vec![message(3, "three")],
            has_more: false,
        };
        assert_eq!(
            validate_history(1, &gapped),
            Err(HistoryError::SequenceGap {
                expected: 2,
                found: 3
            })
        );

        let mut duplicated = message(2, "two");
        duplicated.client_message_id = Some("same".to_owned());
        let mut second = message(3, "three");
        second.client_message_id = Some("same".to_owned());
        let page = HistoryPage {
            channel_id: "general".to_owned(),
            messages: vec![duplicated, second],
            has_more: false,
        };
        assert!(matches!(
            validate_history(1, &page),
            Err(HistoryError::DuplicateClientMessageId { .. })
        ));
    }

    #[test]
    fn native_history_and_ack_fixtures_parse() {
        let page: HistoryPage = serde_json::from_str(include_str!(
            "../../docs/contracts/fixtures/native/rest_history_page.json"
        ))
        .unwrap();
        assert_eq!(page.channel_id, "7efc7a7b-b06f-4632-8780-bb611b8cd0a8");
        assert_eq!(page.messages.len(), 1);
        assert_eq!(page.messages[0].seq, 42);
        assert_eq!(page.messages[0].body.as_deref(), Some("Hello"));
        assert!(!page.has_more);

        let ack: MessageAck = serde_json::from_str(include_str!(
            "../../docs/contracts/fixtures/native/rest_ack.json"
        ))
        .unwrap();
        assert_eq!(ack.channel_id, page.channel_id);
        assert_eq!(
            ack.client_message_id,
            "567e491b-4c30-47ec-9fc0-6744c12a6e87"
        );
        assert_eq!(ack.seq, 42);
    }

    #[test]
    fn acknowledgement_names_the_client_message_and_sequence() {
        let ack = MessageAck {
            channel_id: "general".to_owned(),
            client_message_id: "client-1".to_owned(),
            seq: 9,
        };
        let parsed: MessageAck =
            serde_json::from_str(&serde_json::to_string(&ack).unwrap()).unwrap();
        assert_eq!(parsed, ack);
        let query = HistoryQuery {
            channel_id: "general".to_owned(),
            after_seq: 9,
            limit: 50,
        };
        assert_eq!(query.after_seq, ack.seq);
    }
}
