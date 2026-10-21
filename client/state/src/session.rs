use std::collections::BTreeMap;

use crate::catchup::{catch_up, AppliedEvent, CatchUp};
use crate::credentials::{RefreshCredential, RefreshStore, VolatileTokens};
use crate::outbox::{AckResult, ClientMessageId, Outbox};
use crate::protocol::ChatEvent;
use crate::retry::{plan_interrupt, AccessBlock, Interrupt, NextAction};

/// Disposable cursor cache. Losing it only forces a wider history fetch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CursorCache {
    cursors: BTreeMap<String, u64>,
}

impl CursorCache {
    pub fn cursor(&self, channel_id: &str) -> u64 {
        self.cursors.get(channel_id).copied().unwrap_or(0)
    }

    pub fn set_cursor(&mut self, channel_id: impl Into<String>, seq: u64) {
        self.cursors.insert(channel_id.into(), seq);
    }
}

#[derive(Debug)]
pub struct ClientState<S> {
    store: S,
    tokens: VolatileTokens,
    cursors: CursorCache,
    outbox: Outbox,
    blocked: Option<AccessBlock>,
    live_buffer: Vec<ChatEvent>,
    buffering: bool,
}

impl<S: RefreshStore> ClientState<S> {
    /// Process start. Access and LiveKit tokens are empty until Axum issues new ones.
    pub fn restore(store: S, cache: Option<CursorCache>) -> Self {
        Self {
            store,
            tokens: VolatileTokens::default(),
            cursors: cache.unwrap_or_default(),
            outbox: Outbox::default(),
            blocked: None,
            live_buffer: Vec::new(),
            buffering: false,
        }
    }

    pub fn save_refresh(
        &mut self,
        credential: RefreshCredential,
    ) -> Result<(), crate::credentials::StoreError> {
        self.store.save(credential)
    }

    pub fn refresh(&self) -> Option<RefreshCredential> {
        self.store.load()
    }

    pub fn tokens(&self) -> &VolatileTokens {
        &self.tokens
    }

    pub fn tokens_mut(&mut self) -> &mut VolatileTokens {
        &mut self.tokens
    }

    /// Erase durable and volatile credentials before an uncertain refresh or logout.
    /// A failed durable clear prevents a request that might consume the old token.
    pub fn clear_session(&mut self) -> Result<(), crate::credentials::StoreError> {
        self.store.clear()?;
        self.tokens.begin_full_media_reconnect();
        Ok(())
    }

    pub fn cursor(&self, channel_id: &str) -> u64 {
        self.cursors.cursor(channel_id)
    }

    pub fn enqueue(
        &mut self,
        channel_id: impl Into<String>,
        body: impl Into<String>,
    ) -> ClientMessageId {
        self.outbox.enqueue(channel_id, body)
    }

    pub fn acknowledge(&mut self, client_message_id: &str, seq: u64) -> AckResult {
        self.outbox.acknowledge(client_message_id, seq)
    }

    pub fn outbox_len(&self) -> usize {
        self.outbox.visible().count()
    }

    /// Start a reconnect. Live events are buffered until history is applied.
    pub fn begin_catch_up(
        &mut self,
        interrupt: Interrupt,
        attempt: u32,
        jitter_ms: u64,
    ) -> NextAction {
        let action = plan_interrupt(interrupt, attempt, jitter_ms);
        match action {
            NextAction::Stop { reason } => {
                self.blocked = Some(reason);
                self.tokens.begin_full_media_reconnect();
                self.buffering = false;
                self.live_buffer.clear();
                let _ = self.store.clear();
            }
            NextAction::RetryChat { .. } => {
                self.tokens.begin_reauthorization();
                self.buffering = true;
            }
            NextAction::RetryMedia { .. } => {
                self.tokens.begin_full_media_reconnect();
                self.buffering = true;
            }
        }
        action
    }

    pub fn blocked(&self) -> Option<AccessBlock> {
        self.blocked
    }

    /// Apply an in-order event immediately, or report a gap so the caller can
    /// request history. Events received during catch-up stay buffered.
    pub fn observe_live(&mut self, event: ChatEvent) -> Option<CatchUp> {
        if self.blocked.is_some() {
            return None;
        }
        if self.buffering {
            self.live_buffer.push(event);
            return None;
        }
        let channel_id = event.channel_id.clone();
        let caught = catch_up(
            &channel_id,
            self.cursors.cursor(&channel_id),
            &[],
            std::slice::from_ref(&event),
        );
        if caught.missing_seq.is_some() {
            self.buffering = true;
            self.live_buffer.push(event);
        } else {
            self.commit(&channel_id, &caught);
        }
        Some(caught)
    }

    pub fn apply_history(&mut self, channel_id: &str, history: &[ChatEvent]) -> CatchUp {
        if self.blocked.is_some() {
            return CatchUp {
                applied: Vec::new(),
                cursor: self.cursors.cursor(channel_id),
                missing_seq: None,
            };
        }
        let caught = catch_up(
            channel_id,
            self.cursors.cursor(channel_id),
            history,
            &self.live_buffer,
        );
        self.commit(channel_id, &caught);
        if caught.missing_seq.is_none() {
            self.live_buffer
                .retain(|event| event.channel_id != channel_id || event.seq > caught.cursor);
            if self.live_buffer.is_empty() {
                self.buffering = false;
            }
        }
        caught
    }

    fn commit(&mut self, channel_id: &str, caught: &CatchUp) {
        if caught.cursor > self.cursors.cursor(channel_id) {
            self.cursors.set_cursor(channel_id, caught.cursor);
        }
        for event in &caught.applied {
            if let Some(id) = &event.client_message_id {
                self.outbox.acknowledge(id, event.seq);
            }
        }
    }
}

pub fn visible_bodies(applied: &[AppliedEvent]) -> Vec<String> {
    applied
        .iter()
        .filter(|event| event.known_message)
        .filter_map(|event| event.body.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::{MemoryRefreshStore, RefreshCredential};
    use crate::protocol::ChatEvent;
    use crate::retry::{AccessBlock, Interrupt, NextAction, FIRST_RETRY};

    fn message(seq: u64, body: &str, client_message_id: Option<&str>) -> ChatEvent {
        ChatEvent {
            channel_id: "general".to_owned(),
            seq,
            kind: "message".to_owned(),
            client_message_id: client_message_id.map(str::to_owned),
            body: Some(body.to_owned()),
            extra: Default::default(),
        }
    }

    #[test]
    fn restart_keeps_refresh_and_discards_an_absent_cache() {
        let mut store = MemoryRefreshStore::default();
        store.save(RefreshCredential::new("refresh-1")).unwrap();
        let restored = ClientState::restore(store, None);
        assert_eq!(restored.refresh().unwrap().expose(), "refresh-1");
        assert_eq!(restored.tokens().access(), None);
        assert_eq!(restored.tokens().livekit(), None);
        assert_eq!(restored.cursor("general"), 0);
    }

    #[test]
    fn disposable_cache_restores_the_channel_cursor() {
        let mut cache = CursorCache::default();
        cache.set_cursor("general", 8);
        let state = ClientState::restore(MemoryRefreshStore::default(), Some(cache));
        assert_eq!(state.cursor("general"), 8);
    }

    #[test]
    fn pending_send_appears_once_after_duplicate_history_and_ack() {
        let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
        let id = state.enqueue("general", "hello");
        state.begin_catch_up(Interrupt::Transport, 0, 0);
        state.observe_live(message(1, "hello", Some(id.as_str())));
        let caught = state.apply_history("general", &[message(1, "hello", Some(id.as_str()))]);
        assert_eq!(visible_bodies(&caught.applied), vec!["hello".to_owned()]);
        assert_eq!(state.acknowledge(id.as_str(), 1), AckResult::Duplicate);
        assert_eq!(state.outbox_len(), 1);
        assert_eq!(state.cursor("general"), 1);
    }

    #[test]
    fn live_events_apply_once_and_a_gap_waits_for_history() {
        let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
        let first = state.observe_live(message(1, "first", None)).unwrap();
        assert_eq!(visible_bodies(&first.applied), vec!["first".to_owned()]);
        assert_eq!(state.cursor("general"), 1);

        let duplicate = state.observe_live(message(1, "first", None)).unwrap();
        assert!(duplicate.applied.is_empty());

        let gap = state.observe_live(message(3, "third", None)).unwrap();
        assert_eq!(gap.missing_seq, Some(2));
        assert_eq!(state.cursor("general"), 1);
        assert!(state.observe_live(message(4, "fourth", None)).is_none());

        let caught = state.apply_history("general", &[message(2, "second", None)]);
        assert_eq!(
            visible_bodies(&caught.applied).join(","),
            "second,third,fourth"
        );
        assert_eq!(state.cursor("general"), 4);
        assert_eq!(caught.missing_seq, None);
    }

    #[test]
    fn transport_drop_reauthorizes_without_discarding_livekit_until_full_reconnect() {
        let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
        state.tokens_mut().set_access("access");
        state.tokens_mut().set_livekit("room");
        let action = state.begin_catch_up(Interrupt::Transport, 0, 0);
        assert_eq!(action, NextAction::RetryChat { delay: FIRST_RETRY });
        assert_eq!(state.tokens().access(), None);
        assert_eq!(state.tokens().livekit(), Some("room"));

        let action = state.begin_catch_up(Interrupt::FullMediaReconnect, 0, 0);
        assert!(matches!(
            action,
            NextAction::RetryMedia {
                request_fresh_livekit_token: true,
                ..
            }
        ));
        assert_eq!(state.tokens().livekit(), None);
    }

    #[test]
    fn kicked_client_stops_and_drops_buffered_events() {
        let mut state = ClientState::restore(MemoryRefreshStore::default(), None);
        state
            .save_refresh(RefreshCredential::new("refresh-1"))
            .unwrap();
        state.begin_catch_up(Interrupt::Transport, 0, 0);
        state.observe_live(message(1, "late", None));
        let action = state.begin_catch_up(Interrupt::Blocked(AccessBlock::Ban), 1, 0);
        assert_eq!(
            action,
            NextAction::Stop {
                reason: AccessBlock::Ban
            }
        );
        let caught = state.apply_history("general", &[message(1, "late", None)]);
        assert!(caught.applied.is_empty());
        assert_eq!(state.refresh(), None);
        assert_eq!(state.blocked(), Some(AccessBlock::Ban));
    }
}
