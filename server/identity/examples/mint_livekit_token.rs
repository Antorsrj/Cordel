//! Mint a LiveKit access token for a local disposable test.
//!
//! The production minter belongs to the Server 005 media admission module. This
//! example exists so a local environment can produce a token before that route
//! exists, and so the probe script and the implementation agree on the claim
//! set instead of maintaining two copies.
//!
//! Environment:
//!   CORDEL_LIVEKIT_API_KEY     required, the LiveKit API key
//!   CORDEL_LIVEKIT_API_SECRET  required, the LiveKit API secret
//!   CORDEL_LIVEKIT_IDENTITY    required, the participant identity
//!   CORDEL_LIVEKIT_ROOM        required, the room name
//!   CORDEL_LIVEKIT_TTL_SECS    optional, default 300
//!   CORDEL_LIVEKIT_ADMIN       optional, "1" to mint a room-admin token
//!
//! The token is printed to stdout. The secret is never printed.

use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize)]
struct VideoGrant {
    room: String,
    #[serde(rename = "roomJoin")]
    room_join: bool,
    #[serde(rename = "canPublish")]
    can_publish: bool,
    #[serde(rename = "canSubscribe")]
    can_subscribe: bool,
    #[serde(rename = "canPublishData")]
    can_publish_data: bool,
    #[serde(rename = "roomCreate")]
    room_create: bool,
    #[serde(rename = "roomList")]
    room_list: bool,
    #[serde(rename = "roomAdmin")]
    room_admin: bool,
    #[serde(rename = "roomRecord")]
    room_record: bool,
    hidden: bool,
    recording: bool,
}

#[derive(Serialize)]
struct Claims {
    exp: i64,
    nbf: i64,
    iat: i64,
    iss: String,
    sub: String,
    name: String,
    video: VideoGrant,
}

fn required(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn main() {
    let api_key = required("CORDEL_LIVEKIT_API_KEY");
    let api_secret = required("CORDEL_LIVEKIT_API_SECRET");
    let identity = required("CORDEL_LIVEKIT_IDENTITY");
    let room = required("CORDEL_LIVEKIT_ROOM");
    let ttl: u64 = std::env::var("CORDEL_LIVEKIT_TTL_SECS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(300);
    let admin = std::env::var("CORDEL_LIVEKIT_ADMIN").as_deref() == Ok("1");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before the epoch")
        .as_secs() as i64;
    let claims = Claims {
        exp: now + ttl as i64,
        nbf: now,
        iat: now,
        iss: api_key,
        sub: identity.clone(),
        name: identity,
        video: VideoGrant {
            room,
            room_join: true,
            can_publish: !admin,
            can_subscribe: !admin,
            can_publish_data: false,
            room_create: admin,
            room_list: admin,
            room_admin: admin,
            room_record: false,
            hidden: false,
            recording: false,
        },
    };
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(api_secret.as_bytes()),
    )
    .expect("signing the token");
    println!("{token}");
}
