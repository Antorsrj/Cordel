//! ADR-010 snapshot, leases, and LiveKit reconciliation.
mod fanout;
mod hub;
mod media;

pub(crate) use fanout::{access_still_open, deliver_presence};
#[cfg(test)]
mod tests;

use std::time::Duration;

use cordel_protocol::PresenceDelta;

use crate::policy::{self, Action};

use self::hub::{DirectoryEntry, Hub, MediaMember};
use super::*;

const SYNC_INTERVAL: Duration = Duration::from_secs(30);

pub(crate) struct PresenceNode {
    hub: Mutex<Hub>,
    events: broadcast::Sender<Arc<str>>,
    last_sync: Mutex<Option<Instant>>,
}

impl PresenceNode {
    pub(crate) fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(256);
        Arc::new(Self {
            hub: Mutex::new(Hub::new()),
            events,
            last_sync: Mutex::new(None),
        })
    }

    pub(crate) fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.events.subscribe()
    }

    pub(crate) fn publish(&self, deltas: &[PresenceDelta]) {
        for delta in deltas {
            let Ok(frame) = delta.to_frame() else {
                continue;
            };
            let Ok(json) = serde_json::to_string(&frame) else {
                continue;
            };
            let _ = self.events.send(Arc::from(json));
        }
    }

    pub(crate) fn epoch(&self, community_id: Uuid) -> (String, u64) {
        let hub = self.lock();
        (hub.stream_id().to_string(), hub.revision(community_id))
    }

    pub(crate) fn retain(
        &self,
        community_id: Uuid,
        user_id: Uuid,
        connection_id: Uuid,
        display_name: &str,
        now: Instant,
    ) -> Vec<PresenceDelta> {
        self.lock()
            .retain(community_id, user_id, connection_id, display_name, now)
    }

    pub(crate) fn release(
        &self,
        community_id: Uuid,
        user_id: Uuid,
        connection_id: Uuid,
        now: Instant,
    ) {
        self.lock()
            .release(community_id, user_id, connection_id, now);
    }

    pub(crate) fn revoke(
        &self,
        community_id: Uuid,
        user_id: Uuid,
        now: Instant,
    ) -> Vec<PresenceDelta> {
        self.lock().revoke(community_id, user_id, now)
    }

    pub(crate) fn reconcile(
        &self,
        community_id: Uuid,
        directory: &[DirectoryEntry],
        media: &[MediaMember],
        now: Instant,
    ) -> Vec<PresenceDelta> {
        self.lock().reconcile(community_id, directory, media, now)
    }

    pub(crate) fn view(
        &self,
        community_id: Uuid,
        directory: &[DirectoryEntry],
        now: Instant,
    ) -> (cordel_protocol::PresenceSnapshot, Vec<PresenceDelta>) {
        self.lock().view(community_id, directory, now)
    }

    fn sync_due(&self) -> bool {
        self.last_sync
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_none_or(|last| last.elapsed() >= SYNC_INTERVAL)
    }

    fn mark_synced(&self) {
        *self
            .last_sync
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Instant::now());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Hub> {
        self.hub.lock().unwrap_or_else(|error| error.into_inner())
    }
}

pub(super) async fn get_presence(
    State(state): State<AppState>,
    Path(raw_community_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<cordel_protocol::PresenceSnapshot>, ApiError> {
    let community_id = Uuid::parse_str(&raw_community_id).map_err(|_| ApiError::invalid())?;
    policy::authorize(&state, &headers, community_id, Action::Member).await?;
    if state.has_media() {
        sync_media(&state, true).await?;
    }
    let directory = directory(&state, community_id).await?;
    let (snapshot, deltas) = state
        .presence
        .view(community_id, &directory, Instant::now());
    state.presence.publish(&deltas);
    Ok(Json(snapshot))
}

/// Fold the current LiveKit observations into the roster.
///
/// `force` is the webhook and the snapshot read. Heartbeats pass `false` and
/// skip the database when this process synced a moment ago.
pub(crate) async fn sync_media(state: &AppState, force: bool) -> Result<(), ApiError> {
    if !state.has_media() {
        return Ok(());
    }
    if !force && !state.presence.sync_due() {
        return Ok(());
    }
    let observed = media::media_members(&state.media_observation());
    let communities: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM communities")
        .fetch_all(&state.pool)
        .await
        .map_err(|_| ApiError::internal())?;
    for community_id in communities {
        let directory = directory(state, community_id).await?;
        let known: Vec<Uuid> = directory.iter().map(|entry| entry.user_id).collect();
        let relevant: Vec<MediaMember> = observed
            .iter()
            .filter(|member| known.contains(&member.user_id))
            .cloned()
            .collect();
        let deltas = state
            .presence
            .reconcile(community_id, &directory, &relevant, Instant::now());
        state.presence.publish(&deltas);
    }
    state.presence.mark_synced();
    Ok(())
}

async fn directory(state: &AppState, community_id: Uuid) -> Result<Vec<DirectoryEntry>, ApiError> {
    let rows = sqlx::query(
        "SELECT a.id, a.login, \
         (m.banned_at IS NULL AND (m.kicked_until IS NULL OR m.kicked_until <= now())) AS allowed \
         FROM memberships m JOIN accounts a ON a.id = m.account_id \
         WHERE m.community_id = $1",
    )
    .bind(community_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| ApiError::internal())?;
    Ok(rows
        .iter()
        .map(|row| DirectoryEntry {
            user_id: row.get("id"),
            display_name: row.get("login"),
            allowed: row.get("allowed"),
        })
        .collect())
}
