//! The HTTP routes of the service.
//!
//! They live apart from the state so each file stays small enough to read.

use super::*;

/// The gate router, for the media admission.
///
/// It is bound to its own loopback listener and is never forwarded to by the
/// public path. The reverse proxy calls it before it proxies the LiveKit
/// signaling upgrade, and a non-2xx answer keeps the client out of the room.
pub fn gate_router(state: AppState) -> Router {
    Router::new()
        .route("/v1/livekit/authorize", get(media::gate::authorize))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found", "Not found") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed",
            )
        })
        .with_state(state)
}

pub fn router(state: AppState) -> Router {
    let mut router = Router::new()
        .route("/", get(diagnostic::page))
        .route("/diagnostic.js", get(diagnostic::script))
        .route("/diagnostic.css", get(diagnostic::style))
        .route("/v1/accounts", post(registration_disabled))
        .route("/v1/browser/csrf", get(browser::csrf))
        .route("/v1/accounts/from-invite", post(browser::redeem_route))
        .route("/v1/sessions", post(browser::login_route))
        .route("/v1/sessions/refresh", post(browser::refresh_route))
        .route("/v1/sessions/current", delete(browser::logout_route))
        .route(
            "/v1/sessions/admin-grant",
            post(unlock_admin).delete(lock_admin),
        )
        .route("/v1/me", get(me))
        .route(
            "/v1/communities/{community_id}/invitations",
            post(create_invite),
        )
        .route(
            "/v1/communities/{community_id}",
            get(community::get_community).patch(community::update_community),
        )
        .route(
            "/v1/communities/{community_id}/channels",
            get(community::list_channels).post(community::create_channel),
        )
        .route(
            "/v1/communities/{community_id}/presence",
            get(presence::get_presence),
        )
        .route(
            "/v1/communities/{community_id}/members/{member_id}/kick",
            post(community::kick),
        )
        .route(
            "/v1/communities/{community_id}/members/{member_id}/ban",
            post(community::ban).delete(community::unban),
        )
        .route(
            "/v1/channels/{channel_id}/messages",
            get(chat::history).post(chat::send_message),
        )
        .route("/v1/events", get(events::events));

    // The media routes exist only when the host configured a LiveKit service.
    if state.media.is_some() {
        router = router
            .route(
                "/v1/voice-channels/{channel_id}/admission",
                post(media::admission),
            )
            .route(
                "/v1/livekit/webhook",
                post(media::webhook::handler).layer(DefaultBodyLimit::max(64 * 1024)),
            );
    }

    router
        .layer(DefaultBodyLimit::max(16 * 1024))
        .fallback(|| async { ApiError::new(StatusCode::NOT_FOUND, "not_found", "Not found") })
        .method_not_allowed_fallback(|| async {
            ApiError::new(
                StatusCode::METHOD_NOT_ALLOWED,
                "method_not_allowed",
                "Method not allowed",
            )
        })
        .with_state(state)
}
