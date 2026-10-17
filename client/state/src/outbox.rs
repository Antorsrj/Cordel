use std::collections::BTreeMap;

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientMessageId(String);

impl ClientMessageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendState {
    Sending,
    Acknowledged { seq: u64 },
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSend {
    pub channel_id: String,
    pub body: String,
    pub state: SendState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckResult {
    Acknowledged,
    Duplicate,
    Unknown,
}

#[derive(Debug, Default)]
pub struct Outbox {
    pending: BTreeMap<String, PendingSend>,
}

impl Outbox {
    pub fn enqueue(
        &mut self,
        channel_id: impl Into<String>,
        body: impl Into<String>,
    ) -> ClientMessageId {
        let id = Uuid::new_v4().to_string();
        self.pending.insert(
            id.clone(),
            PendingSend {
                channel_id: channel_id.into(),
                body: body.into(),
                state: SendState::Sending,
            },
        );
        ClientMessageId(id)
    }

    pub fn retry(&self, id: &ClientMessageId) -> Option<&PendingSend> {
        let send = self.pending.get(id.as_str())?;
        if send.state == SendState::Sending || send.state == SendState::Failed {
            Some(send)
        } else {
            None
        }
    }

    pub fn mark_failed(&mut self, id: &ClientMessageId) {
        if let Some(send) = self.pending.get_mut(id.as_str()) {
            if !matches!(send.state, SendState::Acknowledged { .. }) {
                send.state = SendState::Failed;
            }
        }
    }

    pub fn acknowledge(&mut self, id: &str, seq: u64) -> AckResult {
        let Some(send) = self.pending.get_mut(id) else {
            return AckResult::Unknown;
        };
        match send.state {
            SendState::Acknowledged { .. } => AckResult::Duplicate,
            SendState::Sending | SendState::Failed => {
                send.state = SendState::Acknowledged { seq };
                AckResult::Acknowledged
            }
        }
    }

    pub fn visible(&self) -> impl Iterator<Item = (&str, &PendingSend)> {
        self.pending.iter().map(|(id, send)| (id.as_str(), send))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_keeps_the_same_client_message_id() {
        let mut outbox = Outbox::default();
        let id = outbox.enqueue("general", "hello");
        outbox.mark_failed(&id);
        let again = outbox.retry(&id).unwrap();
        assert_eq!(again.body, "hello");
        assert_eq!(outbox.visible().count(), 1);
        assert_eq!(id.as_str(), outbox.visible().next().unwrap().0);
    }

    #[test]
    fn duplicate_acknowledgement_does_not_create_another_send() {
        let mut outbox = Outbox::default();
        let id = outbox.enqueue("general", "hello");
        assert_eq!(outbox.acknowledge(id.as_str(), 7), AckResult::Acknowledged);
        assert_eq!(outbox.acknowledge(id.as_str(), 7), AckResult::Duplicate);
        assert_eq!(outbox.visible().count(), 1);
        assert_eq!(
            outbox.visible().next().unwrap().1.state,
            SendState::Acknowledged { seq: 7 }
        );
    }
}
