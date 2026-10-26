use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use uuid::Uuid;

use super::hub::{DirectoryEntry, Hub, MediaMember, ObservedShare, LEASE};
use super::media::{media_members, share_id};
use crate::media::webhook::{ParticipantObservation, RoomObservation, TrackObservation};

fn entry(user_id: Uuid, name: &str, allowed: bool) -> DirectoryEntry {
    DirectoryEntry {
        user_id,
        display_name: name.to_owned(),
        allowed,
    }
}

#[test]
fn a_login_session_without_a_socket_is_offline() {
    let mut hub = Hub::new();
    let community = Uuid::new_v4();
    let user = Uuid::new_v4();
    let (snapshot, _) = hub.view(community, &[entry(user, "ada", true)], Instant::now());
    assert!(snapshot.members.is_empty());
    assert_eq!(snapshot.revision.0, 0);
}

#[test]
fn text_only_member_is_online_without_a_voice_room() {
    let mut hub = Hub::new();
    let community = Uuid::new_v4();
    let user = Uuid::new_v4();
    let now = Instant::now();
    let deltas = hub.retain(community, user, Uuid::new_v4(), "ada", now);
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0].changes[0].kind, "member_online");
    let (snapshot, _) = hub.view(community, &[entry(user, "ada", true)], now);
    assert_eq!(snapshot.members.len(), 1);
    assert!(snapshot.members[0].online);
    assert_eq!(snapshot.members[0].voice_channel_id, None);
    assert!(!snapshot.members[0].sharing);
}

#[test]
fn two_devices_are_one_member_and_one_device_can_leave() {
    let mut hub = Hub::new();
    let community = Uuid::new_v4();
    let user = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let now = Instant::now();
    hub.retain(community, user, first, "ada", now);
    let again = hub.retain(community, user, second, "ada", now);
    assert!(again.is_empty());
    hub.release(community, user, first, now);
    let (snapshot, _) = hub.view(community, &[entry(user, "ada", true)], now);
    assert_eq!(snapshot.members.len(), 1);
}

#[test]
fn a_short_socket_drop_stays_online_until_the_lease_ends() {
    let mut hub = Hub::new();
    let community = Uuid::new_v4();
    let user = Uuid::new_v4();
    let connection = Uuid::new_v4();
    let now = Instant::now();
    hub.retain(community, user, connection, "ada", now);
    hub.release(community, user, connection, now);
    let (during, _) = hub.view(community, &[entry(user, "ada", true)], now);
    assert_eq!(during.members.len(), 1);
    let later = now + LEASE + Duration::from_secs(1);
    let (after, deltas) = hub.view(community, &[entry(user, "ada", true)], later);
    assert!(after.members.is_empty());
    assert!(deltas.iter().any(|delta| {
        delta
            .changes
            .iter()
            .any(|change| change.kind == "member_offline")
    }));
}

#[test]
fn voice_keeps_the_member_online_when_the_app_socket_drops() {
    let mut hub = Hub::new();
    let community = Uuid::new_v4();
    let user = Uuid::new_v4();
    let voice = Uuid::new_v4();
    let now = Instant::now();
    let connection = Uuid::new_v4();
    hub.retain(community, user, connection, "ada", now);
    let media = MediaMember {
        user_id: user,
        voice_channel_id: Some(voice),
        shares: Vec::new(),
    };
    hub.reconcile(community, &[entry(user, "ada", true)], &[media], now);
    hub.release(community, user, connection, now);
    let later = now + LEASE + Duration::from_secs(1);
    let (snapshot, _) = hub.view(community, &[entry(user, "ada", true)], later);
    assert_eq!(snapshot.members.len(), 1);
    assert_eq!(
        snapshot.members[0].voice_channel_id.as_deref(),
        Some(voice.to_string()).as_deref()
    );
}

#[test]
fn two_shares_and_a_lost_unpublish_reconcile() {
    let community = Uuid::new_v4();
    let voice = Uuid::new_v4();
    let ada = Uuid::new_v4();
    let bea = Uuid::new_v4();
    let mut rooms = BTreeMap::new();
    rooms.insert(
        format!("voice:{voice}:1"),
        room(&[(ada, &["TR_a"]), (bea, &["TR_b"])]),
    );
    let observed = media_members(&rooms);
    assert_eq!(observed.len(), 2);
    let mut hub = Hub::new();
    let now = Instant::now();
    let directory = vec![entry(ada, "ada", true), entry(bea, "bea", true)];
    let deltas = hub.reconcile(community, &directory, &observed, now);
    let started = deltas
        .iter()
        .flat_map(|delta| &delta.changes)
        .filter(|change| change.kind == "share_started")
        .count();
    assert_eq!(started, 2);
    let (snapshot, _) = hub.view(community, &directory, now);
    assert_eq!(snapshot.shares.len(), 2);
    assert!(snapshot.members.iter().all(|member| member.sharing));

    rooms.insert(
        format!("voice:{voice}:1"),
        room(&[(ada, &["TR_a"]), (bea, &[])]),
    );
    let observed = media_members(&rooms);
    let deltas = hub.reconcile(community, &directory, &observed, now);
    assert!(deltas.iter().any(|delta| {
        delta
            .changes
            .iter()
            .any(|change| change.kind == "share_ended")
    }));
    let (snapshot, _) = hub.view(community, &directory, now);
    assert_eq!(snapshot.shares.len(), 1);
    assert_eq!(
        snapshot.shares[0].share_id,
        share_id(ada, "TR_a").to_string()
    );
    let bea_member = snapshot
        .members
        .iter()
        .find(|member| member.user_id == bea.to_string())
        .unwrap();
    assert!(!bea_member.sharing);
    assert!(bea_member.online);
}

#[test]
fn a_lost_publish_appears_on_the_next_reconcile() {
    let community = Uuid::new_v4();
    let voice = Uuid::new_v4();
    let ada = Uuid::new_v4();
    let directory = vec![entry(ada, "ada", true)];
    let mut hub = Hub::new();
    let now = Instant::now();
    let mut rooms = BTreeMap::new();
    rooms.insert(format!("voice:{voice}:0"), room(&[(ada, &[])]));
    hub.reconcile(community, &directory, &media_members(&rooms), now);
    rooms.insert(format!("voice:{voice}:0"), room(&[(ada, &["TR_late"])]));
    let deltas = hub.reconcile(community, &directory, &media_members(&rooms), now);
    assert!(deltas.iter().any(|delta| {
        delta
            .changes
            .iter()
            .any(|change| change.kind == "share_started")
    }));
}

#[test]
fn a_kick_removes_the_member_even_if_livekit_still_lists_them() {
    let mut hub = Hub::new();
    let community = Uuid::new_v4();
    let user = Uuid::new_v4();
    let voice = Uuid::new_v4();
    let now = Instant::now();
    hub.retain(community, user, Uuid::new_v4(), "ada", now);
    let share = ObservedShare {
        share_id: share_id(user, "TR_a"),
        voice_channel_id: voice,
        track_id: "TR_a".to_owned(),
    };
    hub.reconcile(
        community,
        &[entry(user, "ada", true)],
        &[MediaMember {
            user_id: user,
            voice_channel_id: Some(voice),
            shares: vec![share],
        }],
        now,
    );
    let deltas = hub.revoke(community, user, now);
    assert!(deltas.iter().any(|delta| {
        delta
            .changes
            .iter()
            .any(|change| change.kind == "member_offline")
    }));
    hub.reconcile(
        community,
        &[entry(user, "ada", false)],
        &[MediaMember {
            user_id: user,
            voice_channel_id: Some(voice),
            shares: vec![ObservedShare {
                share_id: share_id(user, "TR_a"),
                voice_channel_id: voice,
                track_id: "TR_a".to_owned(),
            }],
        }],
        now,
    );
    let (snapshot, _) = hub.view(community, &[entry(user, "ada", false)], now);
    assert!(snapshot.members.is_empty());
    assert!(snapshot.shares.is_empty());
    hub.release(community, user, Uuid::new_v4(), now);
    let (snapshot, _) = hub.view(community, &[entry(user, "ada", false)], now);
    assert!(snapshot.members.is_empty());
}

#[test]
fn a_restart_changes_the_stream_and_reconciliation_rebuilds_shares() {
    let community = Uuid::new_v4();
    let voice = Uuid::new_v4();
    let ada = Uuid::new_v4();
    let directory = vec![entry(ada, "ada", true)];
    let mut rooms = BTreeMap::new();
    rooms.insert(format!("voice:{voice}:2"), room(&[(ada, &["TR_a"])]));
    let observed = media_members(&rooms);
    let mut hub = Hub::new();
    let now = Instant::now();
    hub.reconcile(community, &directory, &observed, now);
    let first = hub.stream_id();
    hub.rebuild();
    let (empty, _) = hub.view(community, &directory, now);
    assert_ne!(empty.stream_id, first.to_string());
    assert_eq!(empty.revision.0, 0);
    assert!(empty.shares.is_empty());
    hub.reconcile(community, &directory, &observed, now);
    let (snapshot, _) = hub.view(community, &directory, now);
    assert_eq!(snapshot.shares.len(), 1);
    assert_ne!(snapshot.stream_id, first.to_string());
}

#[test]
fn a_microphone_track_is_not_a_share() {
    let voice = Uuid::new_v4();
    let ada = Uuid::new_v4();
    let mut participant = ParticipantObservation {
        identity: ada.to_string(),
        state: "ACTIVE".to_owned(),
        tracks: BTreeMap::new(),
    };
    participant.tracks.insert(
        "mic".to_owned(),
        TrackObservation {
            track_id: "TR_mic".to_owned(),
            source: "MICROPHONE".to_owned(),
            name: String::new(),
        },
    );
    let mut room = RoomObservation::default();
    room.participants.insert(ada.to_string(), participant);
    let mut rooms = BTreeMap::new();
    rooms.insert(format!("voice:{voice}:0"), room);
    let observed = media_members(&rooms);
    assert!(observed[0].shares.is_empty());
    assert_eq!(observed[0].voice_channel_id, Some(voice));
}

fn room(people: &[(Uuid, &[&str])]) -> RoomObservation {
    let mut room = RoomObservation::default();
    for (user, tracks) in people {
        let mut participant = ParticipantObservation {
            identity: user.to_string(),
            state: "ACTIVE".to_owned(),
            tracks: BTreeMap::new(),
        };
        for track in *tracks {
            participant.tracks.insert(
                (*track).to_owned(),
                TrackObservation {
                    track_id: (*track).to_owned(),
                    source: "SCREEN_SHARE".to_owned(),
                    name: String::new(),
                },
            );
        }
        room.participants.insert(user.to_string(), participant);
    }
    room
}
