use std::collections::BTreeMap;

use crate::protocol::ChatEvent;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedEvent {
    pub channel_id: String,
    pub seq: u64,
    pub client_message_id: Option<String>,
    pub body: Option<String>,
    /// Author identity when the server includes it in the event's extra fields.
    pub author_id: Option<String>,
    pub known_message: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatchUp {
    pub applied: Vec<AppliedEvent>,
    pub cursor: u64,
    /// Next sequence the history request must cover before later events can apply.
    pub missing_seq: Option<u64>,
}

/// Merge REST history and the live buffer. History wins when the same sequence differs.
/// Events at or below `cursor` are duplicates. A gap stops application.
pub fn catch_up(
    channel_id: &str,
    cursor: u64,
    history: &[ChatEvent],
    buffered: &[ChatEvent],
) -> CatchUp {
    let mut by_seq: BTreeMap<u64, ChatEvent> = BTreeMap::new();
    for event in buffered
        .iter()
        .filter(|event| event.channel_id == channel_id)
    {
        by_seq.insert(event.seq, event.clone());
    }
    for event in history
        .iter()
        .filter(|event| event.channel_id == channel_id)
    {
        by_seq.insert(event.seq, event.clone());
    }

    let mut applied = Vec::new();
    let mut next = cursor;
    for (seq, event) in by_seq {
        if seq <= cursor {
            continue;
        }
        if seq != next + 1 {
            return CatchUp {
                applied,
                cursor: next,
                missing_seq: Some(next + 1),
            };
        }
        let known_message = event.is_known_message();
        let author_id = event
            .extra
            .get("author_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        applied.push(AppliedEvent {
            channel_id: event.channel_id,
            seq: event.seq,
            client_message_id: event.client_message_id,
            body: event.body,
            author_id,
            known_message,
        });
        next = seq;
    }
    CatchUp {
        applied,
        cursor: next,
        missing_seq: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ChatEvent;

    fn event(seq: u64, body: &str) -> ChatEvent {
        ChatEvent {
            channel_id: "general".to_owned(),
            seq,
            kind: "message".to_owned(),
            client_message_id: None,
            body: Some(body.to_owned()),
            extra: Default::default(),
        }
    }

    #[test]
    fn delayed_history_and_buffer_apply_once_in_order() {
        let buffered = vec![event(3, "live")];
        let history = vec![event(1, "one"), event(2, "two"), event(3, "history")];
        let caught = catch_up("general", 0, &history, &buffered);
        assert_eq!(
            caught
                .applied
                .iter()
                .map(|event| event.body.as_deref().unwrap())
                .collect::<Vec<_>>(),
            vec!["one", "two", "history"]
        );
        assert_eq!(caught.cursor, 3);
        assert_eq!(caught.missing_seq, None);
    }

    #[test]
    fn dropped_live_event_waits_for_missing_sequence() {
        let buffered = vec![event(3, "later")];
        let caught = catch_up("general", 1, &[], &buffered);
        assert!(caught.applied.is_empty());
        assert_eq!(caught.cursor, 1);
        assert_eq!(caught.missing_seq, Some(2));

        let history = vec![event(2, "missed")];
        let caught = catch_up("general", caught.cursor, &history, &buffered);
        assert_eq!(caught.applied.len(), 2);
        assert_eq!(caught.cursor, 3);
    }

    #[test]
    fn duplicate_sequence_does_not_apply_twice() {
        let history = vec![event(2, "from-history")];
        let buffered = vec![event(2, "from-socket")];
        let caught = catch_up("general", 1, &history, &buffered);
        assert_eq!(caught.applied.len(), 1);
        assert_eq!(caught.applied[0].body.as_deref(), Some("from-history"));
    }

    #[test]
    fn applied_events_keep_the_author_identifier() {
        let mut event = event(1, "hello");
        event
            .extra
            .insert("author_id".to_owned(), serde_json::json!("author-1"));
        let caught = catch_up("general", 0, &[event], &[]);
        assert_eq!(caught.applied[0].author_id.as_deref(), Some("author-1"));
    }

    #[test]
    fn unknown_kind_advances_the_cursor() {
        let mut event = event(2, "");
        event.kind = "reaction".to_owned();
        event.body = None;
        let caught = catch_up("general", 1, &[event], &[]);
        assert_eq!(caught.cursor, 2);
        assert!(!caught.applied[0].known_message);
    }
}
