//! Presence frames on the ADR-004 event socket.
use axum::extract::ws::{Message, WebSocket};
use cordel_protocol::WsFrame;
use futures_util::stream::SplitSink;
use futures_util::SinkExt;

use super::*;
use crate::events::{access, send_error, send_frame};

pub(crate) async fn access_still_open(
    state: &AppState,
    principal: &Principal,
    sender: &mut SplitSink<WebSocket, Message>,
) -> bool {
    match access(state, principal).await {
        Ok(current) if current.allowed => true,
        Ok(current) => {
            let _ = send_frame(
                sender,
                &WsFrame::access_revoked(
                    principal.community_id.to_string(),
                    current.revision.max(0) as u64,
                ),
            )
            .await;
            let deltas = state.presence.revoke(
                principal.community_id,
                principal.account_id,
                std::time::Instant::now(),
            );
            state.presence.publish(&deltas);
            false
        }
        Err(_) => {
            let _ = send_error(sender, "internal", "authorization check failed").await;
            false
        }
    }
}

pub(crate) async fn deliver_presence(
    state: &AppState,
    principal: &Principal,
    sender: &mut SplitSink<WebSocket, Message>,
    json: &str,
) -> bool {
    let Ok(frame) = serde_json::from_str::<WsFrame>(json) else {
        return true;
    };
    let community = principal.community_id.to_string();
    if frame
        .fields
        .get("community_id")
        .and_then(serde_json::Value::as_str)
        != Some(community.as_str())
    {
        return true;
    }
    if !access_still_open(state, principal, sender).await {
        return false;
    }
    sender
        .send(Message::Text(json.to_owned().into()))
        .await
        .is_ok()
}
