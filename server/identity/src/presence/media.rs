//! Turn LiveKit room observations into voice and screen-share facts.
//!
//! A webhook is not authorization. This only reads the observation store
//! Server 005 already keeps.

use std::collections::{BTreeMap, HashMap};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::media::gate::parse_room;
use crate::media::webhook::RoomObservation;

use super::hub::{MediaMember, ObservedShare};

/// Stable id for one published screen track. It is derived, not a secret.
const SHARE_NAMESPACE: Uuid = Uuid::from_u128(0x6f0e_2a3c_8b41_4e0a_9c5d_7a1b_2c3d_4e5f);

pub(crate) fn share_id(publisher: Uuid, track_id: &str) -> Uuid {
    let mut hasher = Sha256::new();
    hasher.update(SHARE_NAMESPACE.as_bytes());
    hasher.update(publisher.as_bytes());
    hasher.update(track_id.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

/// One entry per account. When two rooms still list the account, the higher
/// room generation wins, which is the room Axum most recently admitted.
pub(crate) fn media_members(rooms: &BTreeMap<String, RoomObservation>) -> Vec<MediaMember> {
    let mut best: HashMap<Uuid, (u64, MediaMember)> = HashMap::new();
    for (room_name, room) in rooms {
        let Some(parsed) = parse_room(room_name) else {
            continue;
        };
        for participant in room.participants.values() {
            let Ok(user_id) = Uuid::parse_str(&participant.identity) else {
                continue;
            };
            let shares = participant
                .tracks
                .values()
                .filter(|track| is_screen_share(&track.source) && !track.track_id.is_empty())
                .map(|track| ObservedShare {
                    share_id: share_id(user_id, &track.track_id),
                    voice_channel_id: parsed.channel_id,
                    track_id: track.track_id.clone(),
                })
                .collect();
            let member = MediaMember {
                user_id,
                voice_channel_id: Some(parsed.channel_id),
                shares,
            };
            let replace = best
                .get(&user_id)
                .map(|(generation, _)| *generation <= parsed.generation)
                .unwrap_or(true);
            if replace {
                best.insert(user_id, (parsed.generation, member));
            }
        }
    }
    best.into_values().map(|(_, member)| member).collect()
}

fn is_screen_share(source: &str) -> bool {
    source.eq_ignore_ascii_case("SCREEN_SHARE")
}
