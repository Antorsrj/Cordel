//! LiveKit webhook validation and ephemeral observation.
//!
//! A webhook is a signed observation, never an authorization. The route
//! proves that LiveKit sent the body, records what it says, and nothing
//! else. A forged or replayed event can at worst make the observation
//! wrong, and it can never grant a client access to a room.

use std::collections::{BTreeMap, HashMap, VecDeque};

use axum::body::Bytes;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

use super::*;

/// The claims LiveKit puts in the webhook signature.
#[derive(Debug, Deserialize)]
pub(crate) struct WebhookClaims {
    iss: String,
    #[serde(default)]
    sha256: Option<String>,
}

/// The part of the webhook body this slice reads.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct WebhookBody {
    #[serde(default)]
    event: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    room: Option<RoomPart>,
    #[serde(default)]
    participant: Option<ParticipantPart>,
    #[serde(default)]
    track: Option<TrackPart>,
}

#[derive(Debug, Default, Deserialize)]
struct RoomPart {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct ParticipantPart {
    #[serde(default)]
    identity: String,
    #[serde(default)]
    state: String,
}

#[derive(Debug, Default, Deserialize)]
struct TrackPart {
    #[serde(default)]
    sid: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    source: String,
}

/// One observed track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackObservation {
    pub track_id: String,
    pub source: String,
    pub name: String,
}

/// One observed participant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipantObservation {
    pub identity: String,
    pub state: String,
    pub tracks: BTreeMap<String, TrackObservation>,
}

/// The observed state of one room.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomObservation {
    pub participants: BTreeMap<String, ParticipantObservation>,
}

/// The ephemeral observation the webhook keeps. It is not durable: Server 010
/// owns the snapshot and the reconciliation after a restart.
#[derive(Debug, Default)]
pub struct Observations {
    rooms: Mutex<HashMap<String, RoomObservation>>,
    seen_events: Mutex<VecDeque<String>>,
}

/// How many recent event ids are remembered for duplicate suppression.
const RECENT_EVENTS: usize = 2048;

impl Observations {
    /// Apply one event. Returns false when the event id was already seen.
    pub(crate) fn apply(&self, event_id: &str, body: &WebhookBody) -> bool {
        if !event_id.is_empty() {
            let mut seen = match self.seen_events.lock() {
                Ok(seen) => seen,
                Err(_) => return false,
            };
            if seen.iter().any(|id| id == event_id) {
                return false;
            }
            seen.push_back(event_id.to_owned());
            while seen.len() > RECENT_EVENTS {
                seen.pop_front();
            }
        }
        let room = body
            .room
            .as_ref()
            .map(|room| room.name.clone())
            .unwrap_or_default();
        if room.is_empty() {
            return true;
        }
        let mut rooms = match self.rooms.lock() {
            Ok(rooms) => rooms,
            Err(_) => return false,
        };
        let entry = rooms.entry(room).or_default();
        match body.event.as_str() {
            "room_started" => {}
            "room_finished" => {
                rooms.remove(
                    &body
                        .room
                        .as_ref()
                        .map(|r| r.name.clone())
                        .unwrap_or_default(),
                );
            }
            "participant_joined" => {
                if let Some(participant) = &body.participant {
                    entry.participants.insert(
                        participant.identity.clone(),
                        ParticipantObservation {
                            identity: participant.identity.clone(),
                            state: if participant.state.is_empty() {
                                "ACTIVE".to_owned()
                            } else {
                                participant.state.clone()
                            },
                            tracks: BTreeMap::new(),
                        },
                    );
                }
            }
            "participant_left" => {
                if let Some(participant) = &body.participant {
                    entry.participants.remove(&participant.identity);
                }
            }
            "track_published" => {
                if let (Some(participant), Some(track)) = (&body.participant, &body.track) {
                    if let Some(state) = entry.participants.get_mut(&participant.identity) {
                        state.tracks.insert(
                            track.sid.clone(),
                            TrackObservation {
                                track_id: track.sid.clone(),
                                source: track.source.clone(),
                                name: track.name.clone(),
                            },
                        );
                    }
                }
            }
            "track_unpublished" => {
                if let (Some(participant), Some(track)) = (&body.participant, &body.track) {
                    if let Some(state) = entry.participants.get_mut(&participant.identity) {
                        state.tracks.remove(&track.sid);
                    }
                }
            }
            _ => {}
        }
        true
    }

    /// A copy of the observed rooms, for tests and for the future Server 010
    /// snapshot.
    pub fn snapshot(&self) -> BTreeMap<String, RoomObservation> {
        match self.rooms.lock() {
            Ok(rooms) => rooms
                .iter()
                .map(|(name, room)| (name.clone(), room.clone()))
                .collect(),
            Err(_) => BTreeMap::new(),
        }
    }
}

/// Verify the LiveKit webhook signature over the raw body.
///
/// LiveKit signs a JWT with the API secret and puts the base64 SHA-256 of the
/// raw body in the `sha256` claim. The header is verified first, then the
/// claim is compared with the digest of the exact bytes that arrived.
fn verify(media: &MediaConfig, headers: &HeaderMap, raw: &[u8]) -> Result<(), ApiError> {
    let header = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(ApiError::unauthenticated)?;
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[media.api_key.as_str()]);
    validation.set_required_spec_claims(&["exp", "iss"]);
    // LiveKit retries a delivery, and a retry can carry a signature that is
    // already close to its expiry.
    validation.leeway = 300;
    let claims = decode::<WebhookClaims>(
        header,
        &DecodingKey::from_secret(media.api_secret.as_bytes()),
        &validation,
    )
    .map_err(|_| ApiError::unauthenticated())?
    .claims;
    if claims.iss != media.api_key {
        return Err(ApiError::unauthenticated());
    }
    let expected = super::gate::digest(raw);
    match claims.sha256 {
        Some(digest) if digest == expected => Ok(()),
        _ => Err(ApiError::unauthenticated()),
    }
}

/// `POST /v1/livekit/webhook` — record a signed observation.
pub(crate) async fn handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let media = state.media.as_ref().ok_or_else(ApiError::not_found)?;
    verify(&media.config, &headers, &body)?;
    let parsed: WebhookBody = serde_json::from_slice(&body).map_err(|_| ApiError::invalid())?;
    media.observation.apply(&parsed.id, &parsed);
    Ok(StatusCode::OK)
}
