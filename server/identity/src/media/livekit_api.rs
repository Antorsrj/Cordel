//! The LiveKit server API, used only to drain a room after a moderation
//! action.
//!
//! The API secret never leaves the server. The client is bound to the host's
//! own LiveKit instance, so it speaks plain HTTP on loopback and needs no TLS
//! stack of its own.

use std::time::Duration;

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use serde_json::json;

use super::*;

/// How long the administrative token for one drain call is valid. The call
/// is immediate.
const ADMIN_TOKEN_SECONDS: i64 = 120;

/// The request timeout for one server API call.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Serialize)]
struct AdminVideo {
    room: String,
    #[serde(rename = "roomJoin")]
    room_join: bool,
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
struct AdminClaims {
    exp: i64,
    nbf: i64,
    iat: i64,
    iss: String,
    sub: String,
    video: AdminVideo,
}

/// Why a drain did not finish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainError {
    /// The server API could not be reached or refused the call.
    Failed,
}

/// The LiveKit server API.
#[derive(Clone)]
pub struct LiveKitApi {
    api_key: String,
    api_secret: String,
    /// The host-local base URL of the LiveKit server API.
    base: String,
    client: reqwest::Client,
}

impl LiveKitApi {
    pub fn new(api_key: String, api_secret: String, base: String) -> Result<Self, String> {
        if base.trim().is_empty() {
            return Err("CORDEL_LIVEKIT_API_URL must not be empty".to_owned());
        }
        let client = reqwest::Client::builder()
            .timeout(CALL_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "could not build the LiveKit API client".to_owned())?;
        Ok(Self {
            api_key,
            api_secret,
            base: base.trim_end_matches('/').to_owned(),
            client,
        })
    }

    /// A token the server API accepts. It is scoped to one room, so it cannot
    /// reach another one, except for the room list, which needs no room.
    fn admin_token(&self, room: Option<&str>) -> Result<String, DrainError> {
        let now = Utc::now().timestamp();
        let claims = AdminClaims {
            exp: now + ADMIN_TOKEN_SECONDS,
            nbf: now,
            iat: now,
            iss: self.api_key.clone(),
            sub: "cordel-axum".to_owned(),
            video: AdminVideo {
                room: room.unwrap_or_default().to_owned(),
                room_join: true,
                room_create: true,
                room_list: true,
                room_admin: true,
                room_record: false,
                hidden: false,
                recording: false,
            },
        };
        encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(self.api_secret.as_bytes()),
        )
        .map_err(|_| DrainError::Failed)
    }

    /// The names of the rooms the server currently holds.
    #[cfg(test)]
    pub(crate) async fn list_rooms(&self) -> Result<Vec<String>, DrainError> {
        let token = self.admin_token(None)?;
        let response = self
            .client
            .post(format!("{}/twirp/livekit.RoomService/ListRooms", self.base))
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| DrainError::Failed)?;
        if response.status().as_u16() != 200 {
            return Err(DrainError::Failed);
        }
        let body: serde_json::Value = response.json().await.map_err(|_| DrainError::Failed)?;
        let rooms = body
            .get("rooms")
            .and_then(|value| value.as_array())
            .ok_or(DrainError::Failed)?;
        Ok(rooms
            .iter()
            .filter_map(|room| {
                room.get("name")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .collect())
    }

    /// Create a room, so a test can put a participant in it.
    #[cfg(test)]
    pub(crate) async fn create_room(&self, room: &str) -> Result<(), DrainError> {
        let token = self.admin_token(Some(room))?;
        let response = self
            .client
            .post(format!(
                "{}/twirp/livekit.RoomService/CreateRoom",
                self.base
            ))
            .bearer_auth(token)
            .json(&json!({ "name": room }))
            .send()
            .await
            .map_err(|_| DrainError::Failed)?;
        match response.status().as_u16() {
            200 | 409 => Ok(()),
            _ => Err(DrainError::Failed),
        }
    }

    /// Delete a room. LiveKit disconnects everyone in it, which is the drain a
    /// moderation action needs: the members rejoin in the current room.
    ///
    /// A room that does not exist is already drained.
    pub(crate) async fn delete_room(&self, room: &str) -> Result<(), DrainError> {
        let token = self.admin_token(Some(room))?;
        let response = self
            .client
            .post(format!(
                "{}/twirp/livekit.RoomService/DeleteRoom",
                self.base
            ))
            .bearer_auth(token)
            .json(&json!({ "room": room }))
            .send()
            .await
            .map_err(|_| DrainError::Failed)?;
        match response.status().as_u16() {
            200 | 404 => Ok(()),
            _ => Err(DrainError::Failed),
        }
    }
}
