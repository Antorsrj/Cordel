//! ADR-010 presence snapshot and ephemeral deltas.
//!
//! Revisions are decimal strings. A client applies a delta only when its
//! stream id matches and its base revision is the snapshot's current revision.

use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::chat::PROTOCOL_VERSION;
use crate::ws::WsFrame;

/// Unsigned revision carried as a decimal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Revision(pub u64);

impl Serialize for Revision {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Revision {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let value = raw.parse::<u64>().map_err(D::Error::custom)?;
        Ok(Revision(value))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceMember {
    pub user_id: String,
    pub display_name: String,
    pub online: bool,
    pub voice_channel_id: Option<String>,
    pub sharing: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceShare {
    pub share_id: String,
    pub publisher_id: String,
    pub voice_channel_id: String,
    pub track_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceSnapshot {
    pub schema_version: u32,
    pub community_id: String,
    pub stream_id: String,
    pub revision: Revision,
    pub members: Vec<PresenceMember>,
    pub shares: Vec<PresenceShare>,
}

/// One roster change. Unknown `kind` values are ignored by [`PresenceSnapshot::apply_delta`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceChange {
    pub kind: String,
    #[serde(default, flatten)]
    pub fields: Map<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresenceDelta {
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub event_type: String,
    pub event_id: String,
    pub community_id: String,
    pub stream_id: String,
    pub base_revision: Revision,
    pub revision: Revision,
    pub changes: Vec<PresenceChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceApply {
    Applied,
    /// The server rebuilt ephemeral state. The client fetches a new snapshot.
    StreamChanged,
    /// A revision is missing or the delta does not start at the current revision.
    Gap,
}

impl PresenceSnapshot {
    /// Apply one contiguous delta. The snapshot is unchanged on a gap or a new stream.
    pub fn apply_delta(&mut self, delta: &PresenceDelta) -> PresenceApply {
        if delta.stream_id != self.stream_id {
            return PresenceApply::StreamChanged;
        }
        if delta.base_revision.0 != self.revision.0 || delta.revision.0 != self.revision.0 + 1 {
            return PresenceApply::Gap;
        }
        for change in &delta.changes {
            apply_change(self, change);
        }
        self.revision = delta.revision;
        PresenceApply::Applied
    }
}

impl PresenceDelta {
    /// Native socket frame. `v` and `kind` sit beside the v1 delta fields.
    pub fn to_frame(&self) -> Result<WsFrame, serde_json::Error> {
        let value = serde_json::to_value(self)?;
        let fields = match value {
            Value::Object(fields) => fields,
            _ => Map::new(),
        };
        Ok(WsFrame {
            v: PROTOCOL_VERSION,
            kind: "presence.delta".to_owned(),
            fields,
        })
    }

    pub fn from_frame(frame: &WsFrame) -> Option<Self> {
        if frame.kind != "presence.delta" {
            return None;
        }
        serde_json::from_value(Value::Object(frame.fields.clone())).ok()
    }
}

/// Ack for a presence subscription. `revision` is the decimal snapshot revision.
pub fn presence_subscribed_frame(stream_id: &str, revision: u64) -> WsFrame {
    let mut fields = Map::new();
    fields.insert("presence".to_owned(), Value::Bool(true));
    fields.insert("stream_id".to_owned(), Value::String(stream_id.to_owned()));
    fields.insert("revision".to_owned(), Value::String(revision.to_string()));
    WsFrame {
        v: PROTOCOL_VERSION,
        kind: "subscribed".to_owned(),
        fields,
    }
}

/// The server dropped one or more presence deltas. The client refetches.
pub fn presence_resync_frame(community_id: &str, stream_id: &str) -> WsFrame {
    let mut fields = Map::new();
    fields.insert(
        "community_id".to_owned(),
        Value::String(community_id.to_owned()),
    );
    fields.insert("stream_id".to_owned(), Value::String(stream_id.to_owned()));
    WsFrame {
        v: PROTOCOL_VERSION,
        kind: "presence.resync".to_owned(),
        fields,
    }
}

fn apply_change(snapshot: &mut PresenceSnapshot, change: &PresenceChange) {
    match change.kind.as_str() {
        "share_started" => {
            let (Some(share_id), Some(publisher_id), Some(voice_channel_id), Some(track_id)) = (
                text(change, "share_id"),
                text(change, "publisher_id"),
                text(change, "voice_channel_id"),
                text(change, "track_id"),
            ) else {
                return;
            };
            snapshot.shares.retain(|share| share.share_id != share_id);
            snapshot.shares.push(PresenceShare {
                share_id,
                publisher_id: publisher_id.clone(),
                voice_channel_id: voice_channel_id.clone(),
                track_id,
            });
            let member = member_mut(snapshot, &publisher_id);
            if member.voice_channel_id.is_none() {
                member.voice_channel_id = Some(voice_channel_id);
            }
            member.online = true;
            member.sharing = true;
        }
        "share_ended" => {
            let Some(share_id) = text(change, "share_id") else {
                return;
            };
            let publisher = snapshot
                .shares
                .iter()
                .find(|share| share.share_id == share_id)
                .map(|share| share.publisher_id.clone());
            snapshot.shares.retain(|share| share.share_id != share_id);
            if let Some(publisher) = publisher {
                let still = snapshot
                    .shares
                    .iter()
                    .any(|share| share.publisher_id == publisher);
                if let Some(member) = snapshot
                    .members
                    .iter_mut()
                    .find(|member| member.user_id == publisher)
                {
                    member.sharing = still;
                }
            }
        }
        "member_online" => {
            let Some(user_id) = text(change, "user_id") else {
                return;
            };
            let name = text(change, "display_name").unwrap_or_default();
            let member = member_mut(snapshot, &user_id);
            if !name.is_empty() {
                member.display_name = name;
            }
            member.online = true;
        }
        "member_offline" => {
            let Some(user_id) = text(change, "user_id") else {
                return;
            };
            snapshot.members.retain(|member| member.user_id != user_id);
            snapshot
                .shares
                .retain(|share| share.publisher_id != user_id);
        }
        "voice_joined" => {
            let (Some(user_id), Some(voice_channel_id)) =
                (text(change, "user_id"), text(change, "voice_channel_id"))
            else {
                return;
            };
            let member = member_mut(snapshot, &user_id);
            member.voice_channel_id = Some(voice_channel_id);
            member.online = true;
        }
        "voice_left" => {
            let Some(user_id) = text(change, "user_id") else {
                return;
            };
            if let Some(member) = snapshot
                .members
                .iter_mut()
                .find(|member| member.user_id == user_id)
            {
                member.voice_channel_id = None;
            }
        }
        _ => {}
    }
}

fn member_mut<'a>(snapshot: &'a mut PresenceSnapshot, user_id: &str) -> &'a mut PresenceMember {
    if let Some(index) = snapshot
        .members
        .iter()
        .position(|member| member.user_id == user_id)
    {
        return &mut snapshot.members[index];
    }
    snapshot.members.push(PresenceMember {
        user_id: user_id.to_owned(),
        display_name: String::new(),
        online: true,
        voice_channel_id: None,
        sharing: false,
    });
    snapshot.members.last_mut().expect("member was just pushed")
}

fn text(change: &PresenceChange, key: &str) -> Option<String> {
    change
        .fields
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> PresenceSnapshot {
        serde_json::from_str(include_str!(
            "../../docs/contracts/fixtures/v1/presence_snapshot.json"
        ))
        .unwrap()
    }

    fn ended() -> PresenceDelta {
        serde_json::from_str(include_str!(
            "../../docs/contracts/fixtures/v1/presence_delta.json"
        ))
        .unwrap()
    }

    #[test]
    fn fixture_share_end_updates_the_two_share_roster() {
        let mut roster = snapshot();
        assert_eq!(roster.revision.0, 71);
        assert_eq!(roster.shares.len(), 2);
        assert_eq!(roster.apply_delta(&ended()), PresenceApply::Applied);
        assert_eq!(roster.revision.0, 72);
        assert!(roster
            .shares
            .iter()
            .all(|share| share.share_id != "c9089ac7-a306-464b-8b24-f5900dc0c9b7"));
        let publisher = roster
            .members
            .iter()
            .find(|member| member.user_id == "6daa8a59-0ac7-4737-bdad-768d60607767")
            .unwrap();
        assert!(!publisher.sharing);
        let other = roster
            .members
            .iter()
            .find(|member| member.user_id == "e61a2d70-2c27-4889-a041-e94a663f46d6")
            .unwrap();
        assert!(other.sharing);
    }

    #[test]
    fn a_skipped_revision_is_a_gap_and_leaves_the_roster() {
        let mut roster = snapshot();
        let gap: PresenceDelta = serde_json::from_str(include_str!(
            "../../docs/contracts/fixtures/v1/presence_gap_delta.json"
        ))
        .unwrap();
        assert_eq!(roster.apply_delta(&gap), PresenceApply::Gap);
        assert_eq!(roster.revision.0, 71);
        assert_eq!(roster.shares.len(), 2);
    }

    #[test]
    fn a_new_stream_is_not_applied() {
        let mut roster = snapshot();
        let mut delta = ended();
        delta.stream_id = "11111111-1111-1111-1111-111111111111".to_owned();
        assert_eq!(roster.apply_delta(&delta), PresenceApply::StreamChanged);
        assert_eq!(roster.revision.0, 71);
    }

    #[test]
    fn revisions_compare_numerically() {
        let mut roster = snapshot();
        roster.revision = Revision(99);
        let mut delta = ended();
        delta.base_revision = Revision(99);
        delta.revision = Revision(100);
        assert_eq!(roster.apply_delta(&delta), PresenceApply::Applied);
        assert_eq!(roster.revision.0, 100);
    }

    #[test]
    fn an_unknown_change_advances_the_revision() {
        let mut roster = snapshot();
        let mut delta = ended();
        delta.changes = vec![PresenceChange {
            kind: "future_hint".to_owned(),
            fields: Map::new(),
        }];
        assert_eq!(roster.apply_delta(&delta), PresenceApply::Applied);
        assert_eq!(roster.shares.len(), 2);
    }

    #[test]
    fn native_frame_round_trips_and_is_not_chat() {
        let delta = ended();
        let frame = delta.to_frame().unwrap();
        assert_eq!(frame.kind, "presence.delta");
        assert_eq!(frame.as_chat_event().unwrap(), None);
        assert_eq!(PresenceDelta::from_frame(&frame).unwrap(), delta);
        assert_eq!(
            presence_resync_frame("community", "stream").as_chat_event(),
            Ok(None)
        );
    }
}
