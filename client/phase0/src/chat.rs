//! Phase 0 chat: the long-list sample and the live-session worker.
//!
//! The sample stays until the first successful live subscription, so the
//! feasibility list is still measurable without a server. The worker owns the
//! blocking transports and sends owned UI events; it never touches a Slint type.

#[cfg(all(test, feature = "local-smoke"))]
mod recovery_tests;
mod session;
#[cfg(all(test, feature = "local-smoke"))]
mod session_tests;

pub use session::{run, Command, UiEvent};

pub struct ChatMessage {
    pub author: &'static str,
    pub body: String,
}

pub const CHAT_LINE_COUNT: usize = 10_000;

pub fn phase0_messages() -> Vec<ChatMessage> {
    (1..=CHAT_LINE_COUNT)
        .map(|n| ChatMessage {
            author: if n % 2 == 0 { "ada" } else { "kai" },
            body: format!("phase 0 line {n}: readable sample Aa0"),
        })
        .collect()
}

/// Derive the native WebSocket endpoint from the bare API origin. The native
/// transport requires the exact `/v1/events` path and rejects query strings.
pub fn events_url(origin: &str) -> Option<String> {
    let origin = origin.trim_end_matches('/');
    if let Some(rest) = origin.strip_prefix("https://") {
        if !rest.is_empty() {
            return Some(format!("wss://{rest}/v1/events"));
        }
    } else if let Some(rest) = origin.strip_prefix("http://") {
        if !rest.is_empty() {
            return Some(format!("ws://{rest}/v1/events"));
        }
    }
    None
}

/// The native event carries `author_id` but not a display name. Show the
/// signed-in login for the local account and a short identifier for others.
pub fn author_label(author_id: Option<&str>, own_user_id: &str, own_login: &str) -> String {
    match author_id {
        Some(id) if !own_user_id.is_empty() && id == own_user_id => {
            if own_login.is_empty() {
                "you".to_owned()
            } else {
                own_login.to_owned()
            }
        }
        Some(id) => id.get(..8).unwrap_or(id).to_owned(),
        None => "member".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_sample_has_ten_thousand_lines() {
        let lines = phase0_messages();
        assert_eq!(lines.len(), CHAT_LINE_COUNT);
        assert_eq!(lines[0].author, "kai");
        assert!(lines[0].body.contains("line 1"));
        assert_eq!(lines[CHAT_LINE_COUNT - 1].author, "ada");
        assert!(lines[CHAT_LINE_COUNT - 1].body.contains("line 10000"));
    }

    #[test]
    fn events_url_uses_the_origin_scheme() {
        assert_eq!(
            events_url("https://media.example"),
            Some("wss://media.example/v1/events".to_owned())
        );
        assert_eq!(
            events_url("http://127.0.0.1:3000/"),
            Some("ws://127.0.0.1:3000/v1/events".to_owned())
        );
        assert_eq!(events_url("ftp://media.example"), None);
        assert_eq!(events_url("https://"), None);
    }

    #[test]
    fn author_labels_prefer_the_local_login() {
        assert_eq!(
            author_label(Some("user-1"), "user-1", "test-user"),
            "test-user"
        );
        assert_eq!(
            author_label(
                Some("12345678-aaaa-bbbb-cccc-000000000000"),
                "me",
                "test-user"
            ),
            "12345678"
        );
        assert_eq!(author_label(None, "me", "test-user"), "member");
        assert_eq!(author_label(Some("user-1"), "user-1", ""), "you");
    }
}
