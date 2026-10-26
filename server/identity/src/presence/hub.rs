//! Ephemeral presence. Nothing here is written to PostgreSQL.
//!
//! A member is online while an authenticated app socket lease is live, the
//! short grace after that socket drops is still running, or LiveKit still
//! lists them. A refresh session alone does not create a lease.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use cordel_protocol::{
    PresenceChange, PresenceDelta, PresenceMember, PresenceShare, PresenceSnapshot, Revision,
};
use serde_json::{Map, Value};
use uuid::Uuid;

/// How long a socket lease or a short disconnect stays online.
pub(crate) const LEASE: Duration = Duration::from_secs(90);

#[derive(Debug, Clone)]
pub(crate) struct DirectoryEntry {
    pub user_id: Uuid,
    pub display_name: String,
    pub allowed: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ObservedShare {
    pub share_id: Uuid,
    pub voice_channel_id: Uuid,
    pub track_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct MediaMember {
    pub user_id: Uuid,
    pub voice_channel_id: Option<Uuid>,
    pub shares: Vec<ObservedShare>,
}

#[derive(Debug, Clone)]
struct LiveShare {
    voice_channel_id: Uuid,
    track_id: String,
}

#[derive(Debug, Default)]
struct Slot {
    display_name: String,
    connections: HashMap<Uuid, Instant>,
    grace_until: Option<Instant>,
    voice_channel_id: Option<Uuid>,
    shares: HashMap<Uuid, LiveShare>,
}

#[derive(Debug, Default)]
struct Community {
    revision: u64,
    members: HashMap<Uuid, Slot>,
}

#[derive(Debug)]
pub(crate) struct Hub {
    stream_id: Uuid,
    communities: HashMap<Uuid, Community>,
}

impl Hub {
    pub(crate) fn new() -> Self {
        Self {
            stream_id: Uuid::new_v4(),
            communities: HashMap::new(),
        }
    }

    /// Process restart: a new stream id and no carried leases or shares.
    /// Production uses [`Hub::new`] at process start. Tests call this to
    /// prove a client must refetch after the epoch changes.
    #[cfg(test)]
    pub(crate) fn rebuild(&mut self) {
        self.stream_id = Uuid::new_v4();
        self.communities.clear();
    }

    pub(crate) fn stream_id(&self) -> Uuid {
        self.stream_id
    }

    pub(crate) fn revision(&self, community_id: Uuid) -> u64 {
        self.communities
            .get(&community_id)
            .map(|community| community.revision)
            .unwrap_or(0)
    }

    pub(crate) fn retain(
        &mut self,
        community_id: Uuid,
        user_id: Uuid,
        connection_id: Uuid,
        display_name: &str,
        now: Instant,
    ) -> Vec<PresenceDelta> {
        let mut deltas = self.expire(community_id, now);
        let came_online = {
            let community = self.communities.entry(community_id).or_default();
            let was_online = community
                .members
                .get(&user_id)
                .is_some_and(|slot| online(slot, now));
            let slot = community.members.entry(user_id).or_default();
            if !display_name.is_empty() {
                slot.display_name = display_name.to_owned();
            }
            slot.connections.insert(connection_id, now + LEASE);
            slot.grace_until = None;
            if was_online {
                None
            } else {
                Some(slot.display_name.clone())
            }
        };
        if let Some(name) = came_online {
            deltas.extend(self.delta(
                community_id,
                vec![change(
                    "member_online",
                    &[
                        ("user_id", Value::String(user_id.to_string())),
                        ("display_name", Value::String(name)),
                    ],
                )],
            ));
        }
        deltas
    }

    /// The socket is gone. Keep the member online until the lease grace ends
    /// so a short network drop does not look like a kick.
    pub(crate) fn release(
        &mut self,
        community_id: Uuid,
        user_id: Uuid,
        connection_id: Uuid,
        now: Instant,
    ) {
        let Some(slot) = self
            .communities
            .get_mut(&community_id)
            .and_then(|community| community.members.get_mut(&user_id))
        else {
            return;
        };
        slot.connections.remove(&connection_id);
        if slot.connections.is_empty() && slot.voice_channel_id.is_none() && slot.shares.is_empty()
        {
            slot.grace_until = Some(now + LEASE);
        }
    }

    pub(crate) fn expire(&mut self, community_id: Uuid, now: Instant) -> Vec<PresenceDelta> {
        let changes = {
            let Some(community) = self.communities.get_mut(&community_id) else {
                return Vec::new();
            };
            let mut offline = Vec::new();
            let mut voice_left = Vec::new();
            let mut shares_ended = Vec::new();
            for (user_id, slot) in community.members.iter_mut() {
                let was_online = occupied(slot);
                slot.connections.retain(|_, until| *until > now);
                if slot.grace_until.is_some_and(|until| until <= now) {
                    slot.grace_until = None;
                }
                if was_online && !online(slot, now) {
                    if slot.voice_channel_id.take().is_some() {
                        voice_left.push(*user_id);
                    }
                    shares_ended.extend(
                        slot.shares
                            .drain()
                            .map(|(share_id, _)| (*user_id, share_id)),
                    );
                    offline.push(*user_id);
                }
            }
            for user_id in &offline {
                community.members.remove(user_id);
            }
            let mut changes = Vec::new();
            for user_id in voice_left {
                changes.push(change(
                    "voice_left",
                    &[("user_id", Value::String(user_id.to_string()))],
                ));
            }
            for (_, share_id) in shares_ended {
                changes.push(change(
                    "share_ended",
                    &[("share_id", Value::String(share_id.to_string()))],
                ));
            }
            for user_id in offline {
                changes.push(change(
                    "member_offline",
                    &[("user_id", Value::String(user_id.to_string()))],
                ));
            }
            changes
        };
        self.delta(community_id, changes)
    }

    pub(crate) fn revoke(
        &mut self,
        community_id: Uuid,
        user_id: Uuid,
        now: Instant,
    ) -> Vec<PresenceDelta> {
        let Some(slot) = self
            .communities
            .get_mut(&community_id)
            .and_then(|community| community.members.remove(&user_id))
        else {
            return Vec::new();
        };
        if !online(&slot, now) {
            return Vec::new();
        }
        let mut changes = Vec::new();
        if slot.voice_channel_id.is_some() {
            changes.push(change(
                "voice_left",
                &[("user_id", Value::String(user_id.to_string()))],
            ));
        }
        for share_id in slot.shares.keys() {
            changes.push(change(
                "share_ended",
                &[("share_id", Value::String(share_id.to_string()))],
            ));
        }
        changes.push(change(
            "member_offline",
            &[("user_id", Value::String(user_id.to_string()))],
        ));
        self.delta(community_id, changes)
    }

    pub(crate) fn reconcile(
        &mut self,
        community_id: Uuid,
        directory: &[DirectoryEntry],
        media: &[MediaMember],
        now: Instant,
    ) -> Vec<PresenceDelta> {
        let allowed: HashMap<Uuid, &DirectoryEntry> = directory
            .iter()
            .map(|entry| (entry.user_id, entry))
            .collect();
        let mut wanted: HashMap<Uuid, &MediaMember> = HashMap::new();
        for member in media {
            if allowed.contains_key(&member.user_id) {
                wanted.insert(member.user_id, member);
            }
        }
        let known: HashSet<Uuid> = self
            .communities
            .get(&community_id)
            .map(|community| community.members.keys().copied().collect())
            .unwrap_or_default();
        let mut deltas = Vec::new();
        for user_id in &known {
            let entry = allowed.get(user_id).copied();
            let media_member = wanted.get(user_id).copied();
            if entry.is_none_or(|entry| !entry.allowed) {
                deltas.extend(self.revoke(community_id, *user_id, now));
                continue;
            }
            deltas.extend(self.set_media(
                community_id,
                entry.expect("allowed entry"),
                media_member,
                now,
            ));
        }
        for member in media {
            if known.contains(&member.user_id) {
                continue;
            }
            let Some(entry) = allowed.get(&member.user_id).copied() else {
                continue;
            };
            if !entry.allowed {
                continue;
            }
            deltas.extend(self.set_media(community_id, entry, Some(member), now));
        }
        deltas
    }

    pub(crate) fn view(
        &mut self,
        community_id: Uuid,
        directory: &[DirectoryEntry],
        now: Instant,
    ) -> (PresenceSnapshot, Vec<PresenceDelta>) {
        let stream_id = self.stream_id;
        let deltas = self.expire(community_id, now);
        let allowed: HashSet<Uuid> = directory
            .iter()
            .filter(|entry| entry.allowed)
            .map(|entry| entry.user_id)
            .collect();
        let community = self.communities.entry(community_id).or_default();
        let mut members = Vec::new();
        let mut shares = Vec::new();
        let mut user_ids: Vec<Uuid> = community
            .members
            .keys()
            .copied()
            .filter(|user_id| allowed.contains(user_id))
            .collect();
        user_ids.sort();
        for user_id in user_ids {
            let Some(slot) = community.members.get(&user_id) else {
                continue;
            };
            if !online(slot, now) {
                continue;
            }
            let mut share_ids: Vec<Uuid> = slot.shares.keys().copied().collect();
            share_ids.sort();
            for share_id in share_ids {
                let Some(share) = slot.shares.get(&share_id) else {
                    continue;
                };
                shares.push(PresenceShare {
                    share_id: share_id.to_string(),
                    publisher_id: user_id.to_string(),
                    voice_channel_id: share.voice_channel_id.to_string(),
                    track_id: share.track_id.clone(),
                });
            }
            members.push(PresenceMember {
                user_id: user_id.to_string(),
                display_name: slot.display_name.clone(),
                online: true,
                voice_channel_id: slot.voice_channel_id.map(|id| id.to_string()),
                sharing: !slot.shares.is_empty(),
            });
        }
        let revision = community.revision;
        (
            PresenceSnapshot {
                schema_version: 1,
                community_id: community_id.to_string(),
                stream_id: stream_id.to_string(),
                revision: Revision(revision),
                members,
                shares,
            },
            deltas,
        )
    }

    fn set_media(
        &mut self,
        community_id: Uuid,
        entry: &DirectoryEntry,
        media: Option<&MediaMember>,
        now: Instant,
    ) -> Vec<PresenceDelta> {
        let voice = media.and_then(|member| member.voice_channel_id);
        let shares: HashMap<Uuid, LiveShare> = media
            .map(|member| {
                member
                    .shares
                    .iter()
                    .map(|share| {
                        (
                            share.share_id,
                            LiveShare {
                                voice_channel_id: share.voice_channel_id,
                                track_id: share.track_id.clone(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let changes = {
            let community = self.communities.entry(community_id).or_default();
            let was_online = community
                .members
                .get(&entry.user_id)
                .is_some_and(|slot| online(slot, now));
            if !was_online && voice.is_none() && shares.is_empty() {
                return Vec::new();
            }
            let slot = community.members.entry(entry.user_id).or_default();
            if !entry.display_name.is_empty() {
                slot.display_name = entry.display_name.clone();
            }
            let mut changes = Vec::new();
            if slot.voice_channel_id != voice {
                if slot.voice_channel_id.is_some() {
                    changes.push(change(
                        "voice_left",
                        &[("user_id", Value::String(entry.user_id.to_string()))],
                    ));
                }
                if let Some(voice_channel_id) = voice {
                    changes.push(change(
                        "voice_joined",
                        &[
                            ("user_id", Value::String(entry.user_id.to_string())),
                            (
                                "voice_channel_id",
                                Value::String(voice_channel_id.to_string()),
                            ),
                        ],
                    ));
                }
                slot.voice_channel_id = voice;
            }
            let old_ids: HashSet<Uuid> = slot.shares.keys().copied().collect();
            let new_ids: HashSet<Uuid> = shares.keys().copied().collect();
            for share_id in old_ids.difference(&new_ids) {
                changes.push(change(
                    "share_ended",
                    &[("share_id", Value::String(share_id.to_string()))],
                ));
            }
            for share_id in new_ids.difference(&old_ids) {
                let Some(share) = shares.get(share_id) else {
                    continue;
                };
                changes.push(change(
                    "share_started",
                    &[
                        ("share_id", Value::String(share_id.to_string())),
                        ("publisher_id", Value::String(entry.user_id.to_string())),
                        (
                            "voice_channel_id",
                            Value::String(share.voice_channel_id.to_string()),
                        ),
                        ("track_id", Value::String(share.track_id.clone())),
                    ],
                ));
            }
            slot.shares = shares;
            let now_online = online(slot, now);
            if now_online && !was_online {
                changes.insert(
                    0,
                    change(
                        "member_online",
                        &[
                            ("user_id", Value::String(entry.user_id.to_string())),
                            ("display_name", Value::String(slot.display_name.clone())),
                        ],
                    ),
                );
            }
            if !now_online && was_online {
                changes.push(change(
                    "member_offline",
                    &[("user_id", Value::String(entry.user_id.to_string()))],
                ));
            }
            let remove = !now_online;
            let user_id = entry.user_id;
            if remove {
                community.members.remove(&user_id);
            }
            changes
        };
        self.delta(community_id, changes)
    }

    fn delta(&mut self, community_id: Uuid, changes: Vec<PresenceChange>) -> Vec<PresenceDelta> {
        if changes.is_empty() {
            return Vec::new();
        }
        let stream_id = self.stream_id;
        let community = self.communities.entry(community_id).or_default();
        let base = community.revision;
        community.revision = community.revision.saturating_add(1);
        vec![PresenceDelta {
            schema_version: 1,
            event_type: "presence.delta".to_owned(),
            event_id: Uuid::new_v4().to_string(),
            community_id: community_id.to_string(),
            stream_id: stream_id.to_string(),
            base_revision: Revision(base),
            revision: Revision(community.revision),
            changes,
        }]
    }
}

fn occupied(slot: &Slot) -> bool {
    !slot.connections.is_empty()
        || slot.grace_until.is_some()
        || slot.voice_channel_id.is_some()
        || !slot.shares.is_empty()
}

fn online(slot: &Slot, now: Instant) -> bool {
    slot.connections.values().any(|until| *until > now)
        || slot.grace_until.is_some_and(|until| until > now)
        || slot.voice_channel_id.is_some()
        || !slot.shares.is_empty()
}

fn change(kind: &str, fields: &[(&str, Value)]) -> PresenceChange {
    let mut map = Map::new();
    for (key, value) in fields {
        map.insert((*key).to_owned(), value.clone());
    }
    PresenceChange {
        kind: kind.to_owned(),
        fields: map,
    }
}
