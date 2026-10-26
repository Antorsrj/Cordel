//! Client recovery for an ADR-010 presence stream.
//!
//! Subscribe, buffer deltas, then fetch the snapshot and apply only the
//! buffered deltas that continue its stream and revision. A gap, a new
//! stream id, or a `presence.resync` frame means the caller fetches again.

use cordel_protocol::{PresenceApply, PresenceDelta, PresenceSnapshot, WsFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceStatus {
    Ready,
    Refetch,
}

#[derive(Debug, Default)]
pub struct PresenceSync {
    roster: Option<PresenceSnapshot>,
    pending: Vec<PresenceDelta>,
    resync: bool,
}

impl PresenceSync {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn roster(&self) -> Option<&PresenceSnapshot> {
        self.roster.as_ref()
    }

    /// A `presence.resync` frame arrived and no later snapshot has caught up.
    pub fn needs_refetch(&self) -> bool {
        self.resync
    }

    pub fn observe_frame(&mut self, frame: &WsFrame) {
        if frame.kind == "presence.resync" {
            self.resync = true;
            return;
        }
        if let Some(delta) = PresenceDelta::from_frame(frame) {
            self.pending.push(delta);
        }
    }

    pub fn observe_delta(&mut self, delta: PresenceDelta) {
        self.pending.push(delta);
    }

    /// Install a snapshot, then apply buffered deltas that follow it in order.
    pub fn catch_up(&mut self, snapshot: PresenceSnapshot) -> PresenceStatus {
        self.roster = Some(snapshot);
        self.resync = false;
        loop {
            let Some(roster) = self.roster.as_ref() else {
                return PresenceStatus::Refetch;
            };
            let revision = roster.revision.0;
            let stream_id = roster.stream_id.clone();
            let Some(position) = self.pending.iter().position(|delta| {
                delta.stream_id == stream_id && delta.base_revision.0 == revision
            }) else {
                break;
            };
            let delta = self.pending.remove(position);
            let Some(roster) = self.roster.as_mut() else {
                return PresenceStatus::Refetch;
            };
            if roster.apply_delta(&delta) != PresenceApply::Applied {
                self.pending.clear();
                self.resync = true;
                return PresenceStatus::Refetch;
            }
        }
        let Some(roster) = self.roster.as_ref() else {
            return PresenceStatus::Refetch;
        };
        let revision = roster.revision.0;
        let stream_id = roster.stream_id.clone();
        let skipped = self
            .pending
            .iter()
            .any(|delta| delta.stream_id != stream_id || delta.base_revision.0 > revision);
        self.pending
            .retain(|delta| delta.stream_id == stream_id && delta.base_revision.0 >= revision);
        if skipped {
            self.pending.clear();
            self.resync = true;
            return PresenceStatus::Refetch;
        }
        PresenceStatus::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cordel_protocol::{PresenceChange, Revision};

    fn snapshot() -> PresenceSnapshot {
        serde_json::from_str(include_str!(
            "../../../docs/contracts/fixtures/v1/presence_snapshot.json"
        ))
        .unwrap()
    }

    fn ended() -> PresenceDelta {
        serde_json::from_str(include_str!(
            "../../../docs/contracts/fixtures/v1/presence_delta.json"
        ))
        .unwrap()
    }

    #[test]
    fn buffered_share_end_applies_after_the_snapshot() {
        let mut sync = PresenceSync::new();
        sync.observe_delta(ended());
        assert_eq!(sync.catch_up(snapshot()), PresenceStatus::Ready);
        let roster = sync.roster().unwrap();
        assert_eq!(roster.revision.0, 72);
        assert_eq!(roster.shares.len(), 1);
    }

    #[test]
    fn a_lost_delta_asks_for_another_snapshot() {
        let mut sync = PresenceSync::new();
        let gap: PresenceDelta = serde_json::from_str(include_str!(
            "../../../docs/contracts/fixtures/v1/presence_gap_delta.json"
        ))
        .unwrap();
        sync.observe_delta(gap);
        assert_eq!(sync.catch_up(snapshot()), PresenceStatus::Refetch);
        assert!(sync.needs_refetch());
        assert_eq!(sync.roster().unwrap().revision.0, 71);
        assert_eq!(sync.roster().unwrap().shares.len(), 2);
    }

    #[test]
    fn out_of_order_deltas_apply_when_both_are_buffered() {
        let mut sync = PresenceSync::new();
        let mut second = ended();
        second.base_revision = Revision(72);
        second.revision = Revision(73);
        second.changes = vec![PresenceChange {
            kind: "voice_left".to_owned(),
            fields: serde_json::Map::from_iter([(
                "user_id".to_owned(),
                serde_json::Value::String("0c900df0-7a32-4776-b996-827825ad6298".to_owned()),
            )]),
        }];
        sync.observe_delta(second);
        sync.observe_delta(ended());
        assert_eq!(sync.catch_up(snapshot()), PresenceStatus::Ready);
        assert_eq!(sync.roster().unwrap().revision.0, 73);
    }

    #[test]
    fn a_resync_frame_waits_for_the_next_snapshot() {
        let mut sync = PresenceSync::new();
        sync.observe_frame(&cordel_protocol::presence_resync_frame(
            "5aee793e-3cde-44fc-9608-48b7e945fb53",
            "448aa588-0e69-4453-a03e-138664dcaac6",
        ));
        assert!(sync.needs_refetch());
        assert_eq!(sync.catch_up(snapshot()), PresenceStatus::Ready);
        assert!(!sync.needs_refetch());
    }

    #[test]
    fn a_restarted_stream_is_not_merged_into_the_old_roster() {
        let mut sync = PresenceSync::new();
        let mut delta = ended();
        delta.stream_id = "11111111-1111-1111-1111-111111111111".to_owned();
        delta.base_revision = Revision(0);
        delta.revision = Revision(1);
        sync.observe_delta(delta);
        assert_eq!(sync.catch_up(snapshot()), PresenceStatus::Refetch);
        assert_eq!(
            sync.roster().unwrap().stream_id,
            "448aa588-0e69-4453-a03e-138664dcaac6"
        );
    }
}
