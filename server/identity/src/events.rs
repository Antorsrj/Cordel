//! ADR-004 live chat events. One authenticated multiplexed socket per client.
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use cordel_protocol::{presence_resync_frame, presence_subscribed_frame, ChatEvent, WsFrame};
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use sqlx::Row;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use super::*;

const HEARTBEAT_SECONDS: u64 = 30;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct Access {
    pub(crate) allowed: bool,
    pub(crate) revision: i64,
}

pub(super) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, ApiError> {
    let start = browser::open_socket(&state, &headers).await?;
    Ok(ws
        .max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| session(socket, state, start)))
}

async fn session(socket: WebSocket, state: AppState, start: browser::SocketStart) {
    let (mut sender, mut receiver) = socket.split();
    let principal = match start {
        browser::SocketStart::Ready(principal) => principal,
        browser::SocketStart::Browser { principal, csrf } => {
            let arrived = tokio::time::timeout(Duration::from_secs(5), receiver.next()).await;
            let Ok(Some(Ok(Message::Text(text)))) = arrived else {
                return;
            };
            if !browser::csrf_matches(&text, &csrf) {
                return;
            }
            principal
        }
    };
    let mut chat_events = state.chat_events.subscribe();
    let mut presence_events = state.presence.subscribe();
    let mut subscribed: HashSet<Uuid> = HashSet::new();
    // Highest sequence forwarded per channel, used only to advise catch-up after
    // a broadcast lag. Events are not dropped based on it: two transactions can
    // publish out of order, and the client is responsible for gap recovery.
    let mut seen: HashMap<Uuid, u64> = HashMap::new();
    let mut presence_subscribed = false;
    let connection_id = Uuid::new_v4();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(HEARTBEAT_SECONDS));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    if send_frame(
        &mut sender,
        &WsFrame::ready(principal.community_id.to_string()),
    )
    .await
    .is_err()
    {
        return;
    }

    let deltas = state.presence.retain(
        principal.community_id,
        principal.account_id,
        connection_id,
        &principal.login,
        std::time::Instant::now(),
    );
    state.presence.publish(&deltas);

    loop {
        tokio::select! {
            incoming = receiver.next() => {
                let Some(message) = incoming else { break };
                match message {
                    Ok(Message::Text(text)) => {
                        if !handle_text(
                            &state,
                            &principal,
                            &mut sender,
                            &mut subscribed,
                            &mut seen,
                            &mut presence_subscribed,
                            text.as_str(),
                        )
                        .await
                        {
                            break;
                        }
                    }
                    Ok(Message::Close(_)) | Err(_) => break,
                    Ok(Message::Binary(_)) => {
                        let _ = send_error(
                            &mut sender,
                            "invalid_frame",
                            "binary frames are not accepted",
                        )
                        .await;
                        break;
                    }
                    Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                }
            }
            event = chat_events.recv() => {
                match event {
                    Ok(event) => {
                        if !deliver(
                            &state,
                            &principal,
                            &mut sender,
                            &subscribed,
                            &mut seen,
                            event,
                        )
                        .await
                        {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {
                        let mut failed = false;
                        for frame in lag_frames(&subscribed, &seen) {
                            if send_frame(&mut sender, &frame).await.is_err() {
                                failed = true;
                                break;
                            }
                        }
                        if failed {
                            break;
                        }
                        // The client recovers from history and deduplicates live
                        // events that were already applied.
                        seen.clear();
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            event = presence_events.recv() => {
                match event {
                    Ok(json) => {
                        if presence_subscribed && !presence::deliver_presence(
                            &state,
                            &principal,
                            &mut sender,
                            json.as_ref(),
                        )
                        .await
                        {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {
                        if presence_subscribed {
                            let (stream_id, _) = state.presence.epoch(principal.community_id);
                            if send_frame(
                                &mut sender,
                                &presence_resync_frame(
                                    &principal.community_id.to_string(),
                                    &stream_id,
                                ),
                            )
                            .await
                            .is_err()
                            {
                                break;
                            }
                        }
                    }
                    Err(RecvError::Closed) => break,
                }
            }
            _ = heartbeat.tick() => {
                match access(&state, &principal).await {
                    Ok(current) if !current.allowed => {
                        let _ = send_frame(
                            &mut sender,
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
                        break;
                    }
                    Ok(_) => {
                        let deltas = state.presence.retain(
                            principal.community_id,
                            principal.account_id,
                            connection_id,
                            &principal.login,
                            std::time::Instant::now(),
                        );
                        state.presence.publish(&deltas);
                        // A missed observation is recovered on the next webhook
                        // or snapshot. It does not close the chat socket.
                        let _ = presence::sync_media(&state, false).await;
                        if sender
                            .send(Message::Ping(Default::default()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = send_error(
                            &mut sender,
                            "internal",
                            "authorization check failed",
                        )
                        .await;
                        break;
                    }
                }
            }
        }
    }

    state.presence.release(
        principal.community_id,
        principal.account_id,
        connection_id,
        std::time::Instant::now(),
    );
    let _ = sender.send(Message::Close(None)).await;
}

async fn handle_text(
    state: &AppState,
    principal: &Principal,
    sender: &mut SplitSink<WebSocket, Message>,
    subscribed: &mut HashSet<Uuid>,
    seen: &mut HashMap<Uuid, u64>,
    presence_subscribed: &mut bool,
    text: &str,
) -> bool {
    let frame: WsFrame = match serde_json::from_str(text) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = send_error(sender, "invalid_frame", "malformed frame").await;
            return true;
        }
    };
    if frame.check_version().is_err() {
        let _ = send_error(
            sender,
            "unsupported_version",
            "unsupported protocol version",
        )
        .await;
        return false;
    }

    match frame.kind.as_str() {
        "subscribe" => {
            let presence = frame
                .fields
                .get("presence")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if presence {
                if frame.fields.contains_key("channel_id") {
                    let _ = send_error(
                        sender,
                        "invalid_frame",
                        "presence and channel_id are different subscriptions",
                    )
                    .await;
                    return true;
                }
                if !presence::access_still_open(state, principal, sender).await {
                    return false;
                }
                *presence_subscribed = true;
                let (stream_id, revision) = state.presence.epoch(principal.community_id);
                let _ = send_frame(sender, &presence_subscribed_frame(&stream_id, revision)).await;
                return true;
            }
            let Some(raw_channel_id) = frame
                .fields
                .get("channel_id")
                .and_then(serde_json::Value::as_str)
            else {
                let _ = send_error(sender, "invalid_frame", "channel_id is required").await;
                return true;
            };
            let Ok(channel_id) = Uuid::parse_str(raw_channel_id) else {
                let _ = send_error(sender, "invalid_channel", "channel_id must be a UUID").await;
                return true;
            };
            let current = match access(state, principal).await {
                Ok(current) => current,
                Err(_) => {
                    let _ = send_error(sender, "internal", "authorization check failed").await;
                    return false;
                }
            };
            if !current.allowed {
                let _ = send_frame(
                    sender,
                    &WsFrame::access_revoked(
                        principal.community_id.to_string(),
                        current.revision.max(0) as u64,
                    ),
                )
                .await;
                return false;
            }
            let row =
                sqlx::query("SELECT kind,last_seq FROM channels WHERE id=$1 AND community_id=$2")
                    .bind(channel_id)
                    .bind(principal.community_id)
                    .fetch_optional(&state.pool)
                    .await;
            let row = match row {
                Ok(Some(row)) => row,
                Ok(None) => {
                    let _ = send_error(sender, "not_found", "channel not found").await;
                    return true;
                }
                Err(_) => {
                    let _ = send_error(sender, "internal", "channel lookup failed").await;
                    return false;
                }
            };
            if row.get::<String, _>("kind") != "text" {
                let _ = send_error(
                    sender,
                    "invalid_channel",
                    "chat subscriptions require a text channel",
                )
                .await;
                return true;
            }
            let last_seq = row.get::<i64, _>("last_seq").max(0) as u64;
            subscribed.insert(channel_id);
            seen.remove(&channel_id);
            let _ = send_frame(
                sender,
                &WsFrame::subscribed(channel_id.to_string(), last_seq),
            )
            .await;
            true
        }
        "unsubscribe" => {
            let presence = frame
                .fields
                .get("presence")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if presence {
                *presence_subscribed = false;
                let mut fields = serde_json::Map::new();
                fields.insert("presence".to_owned(), serde_json::Value::Bool(true));
                let _ = send_frame(
                    sender,
                    &WsFrame {
                        v: 1,
                        kind: "unsubscribed".to_owned(),
                        fields,
                    },
                )
                .await;
                return true;
            }
            let Some(raw_channel_id) = frame
                .fields
                .get("channel_id")
                .and_then(serde_json::Value::as_str)
            else {
                let _ = send_error(sender, "invalid_frame", "channel_id is required").await;
                return true;
            };
            let Ok(channel_id) = Uuid::parse_str(raw_channel_id) else {
                let _ = send_error(sender, "invalid_channel", "channel_id must be a UUID").await;
                return true;
            };
            subscribed.remove(&channel_id);
            seen.remove(&channel_id);
            let _ = send_frame(sender, &WsFrame::unsubscribed(channel_id.to_string())).await;
            true
        }
        "send" => {
            let _ = send_error(sender, "unsupported_frame", "send messages over REST").await;
            true
        }
        _ => {
            let _ = send_error(sender, "unsupported_frame", "unsupported frame kind").await;
            true
        }
    }
}

async fn deliver(
    state: &AppState,
    principal: &Principal,
    sender: &mut SplitSink<WebSocket, Message>,
    subscribed: &HashSet<Uuid>,
    seen: &mut HashMap<Uuid, u64>,
    event: ChatEvent,
) -> bool {
    let Ok(channel_id) = Uuid::parse_str(&event.channel_id) else {
        return true;
    };
    if !subscribed.contains(&channel_id) {
        return true;
    }

    let current = match access(state, principal).await {
        Ok(current) => current,
        Err(_) => {
            let _ = send_error(sender, "internal", "authorization check failed").await;
            return false;
        }
    };
    if !current.allowed {
        let _ = send_frame(
            sender,
            &WsFrame::access_revoked(
                principal.community_id.to_string(),
                current.revision.max(0) as u64,
            ),
        )
        .await;
        return false;
    }

    // Forward every event, including duplicates and out-of-order arrivals. The
    // client buffers while it fetches history and deduplicates by (channel, seq).
    let frame = match WsFrame::message(&event) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = send_error(sender, "internal", "event serialization failed").await;
            return false;
        }
    };
    if send_frame(sender, &frame).await.is_err() {
        return false;
    }
    seen.entry(channel_id)
        .and_modify(|highest| *highest = (*highest).max(event.seq))
        .or_insert(event.seq);
    true
}

fn lag_frames(subscribed: &HashSet<Uuid>, seen: &HashMap<Uuid, u64>) -> Vec<WsFrame> {
    subscribed
        .iter()
        .map(|channel_id| {
            WsFrame::lagged(
                channel_id.to_string(),
                seen.get(channel_id).copied().unwrap_or(0),
            )
        })
        .collect()
}

pub(crate) async fn access(state: &AppState, principal: &Principal) -> Result<Access, ApiError> {
    let row = sqlx::query(
        "SELECT c.authorization_revision, \
         (m.account_id IS NOT NULL AND s.id IS NOT NULL \
          AND m.banned_at IS NULL \
          AND (m.kicked_until IS NULL OR m.kicked_until<=now())) AS allowed \
         FROM communities c \
         LEFT JOIN memberships m ON m.community_id=c.id AND m.account_id=$1 \
         LEFT JOIN sessions s ON s.id=$2 AND s.account_id=$1 \
          AND s.revoked_at IS NULL AND s.expires_at>now() \
         WHERE c.id=$3",
    )
    .bind(principal.account_id)
    .bind(principal.session_id)
    .bind(principal.community_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?
    .ok_or_else(ApiError::internal)?;
    Ok(Access {
        allowed: row.get("allowed"),
        revision: row.get("authorization_revision"),
    })
}

pub(crate) async fn send_error(
    sender: &mut SplitSink<WebSocket, Message>,
    code: &str,
    message: &str,
) -> Result<(), ()> {
    send_frame(sender, &WsFrame::error(code, message)).await
}

pub(crate) async fn send_frame(
    sender: &mut SplitSink<WebSocket, Message>,
    frame: &WsFrame,
) -> Result<(), ()> {
    let json = serde_json::to_string(frame).map_err(|_| ())?;
    sender
        .send(Message::Text(json.into()))
        .await
        .map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::sync::broadcast;
    use tokio::sync::broadcast::error::RecvError;

    #[test]
    fn lag_frames_cover_each_subscription_with_its_highest_seen_sequence() {
        let with_events = Uuid::new_v4();
        let without_events = Uuid::new_v4();
        let subscribed = HashSet::from([with_events, without_events]);
        let seen = HashMap::from([(with_events, 7)]);
        let by_channel: HashMap<String, u64> = lag_frames(&subscribed, &seen)
            .into_iter()
            .map(|frame| {
                (
                    frame
                        .fields
                        .get("channel_id")
                        .and_then(Value::as_str)
                        .unwrap()
                        .to_owned(),
                    frame
                        .fields
                        .get("after_seq")
                        .and_then(Value::as_u64)
                        .unwrap(),
                )
            })
            .collect();
        assert_eq!(by_channel[&with_events.to_string()], 7);
        assert_eq!(by_channel[&without_events.to_string()], 0);
    }

    #[tokio::test]
    async fn a_lagged_broadcast_receiver_is_reported() {
        let (sender, mut receiver) = broadcast::channel(1);
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        assert!(matches!(receiver.recv().await, Err(RecvError::Lagged(_))));
    }
}
