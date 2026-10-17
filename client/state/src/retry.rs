use std::time::Duration;

pub const FIRST_RETRY: Duration = Duration::from_millis(200);
pub const RETRY_CAP: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessBlock {
    Kick,
    Ban,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// Wi-Fi loss, network change, sleep, or a dropped socket. Not moderation.
    Transport,
    /// LiveKit could not resume and must join again with a new Axum token.
    FullMediaReconnect,
    Blocked(AccessBlock),
    /// ADR-006 room generation changed. Ask Axum for the current room token.
    RoomGenerationChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextAction {
    RetryChat {
        delay: Duration,
    },
    RetryMedia {
        delay: Duration,
        request_fresh_livekit_token: bool,
    },
    Stop {
        reason: AccessBlock,
    },
}

pub fn plan_interrupt(interrupt: Interrupt, attempt: u32, jitter_ms: u64) -> NextAction {
    match interrupt {
        Interrupt::Blocked(reason) => NextAction::Stop { reason },
        Interrupt::Transport => NextAction::RetryChat {
            delay: retry_delay(attempt, jitter_ms),
        },
        Interrupt::FullMediaReconnect | Interrupt::RoomGenerationChanged => {
            NextAction::RetryMedia {
                delay: retry_delay(attempt, jitter_ms),
                request_fresh_livekit_token: true,
            }
        }
    }
}

pub fn retry_delay(attempt: u32, jitter_ms: u64) -> Duration {
    if attempt == 0 {
        return FIRST_RETRY;
    }
    let shift = (attempt - 1).min(8);
    let exponential = FIRST_RETRY.as_millis() as u64 * (1_u64 << shift);
    let jitter = jitter_ms % FIRST_RETRY.as_millis() as u64;
    Duration::from_millis(exponential.saturating_add(jitter)).min(RETRY_CAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_retry_is_quicker_than_later_backoff() {
        let first = retry_delay(0, 150);
        let later = retry_delay(3, 150);
        assert_eq!(first, FIRST_RETRY);
        assert!(later > first);
        assert!(later <= RETRY_CAP);
    }

    #[test]
    fn kick_and_ban_stop_retrying() {
        assert_eq!(
            plan_interrupt(Interrupt::Blocked(AccessBlock::Kick), 0, 0),
            NextAction::Stop {
                reason: AccessBlock::Kick
            }
        );
        assert_eq!(
            plan_interrupt(Interrupt::Blocked(AccessBlock::Ban), 4, 10),
            NextAction::Stop {
                reason: AccessBlock::Ban
            }
        );
    }

    #[test]
    fn full_media_reconnect_asks_for_a_new_token() {
        assert_eq!(
            plan_interrupt(Interrupt::FullMediaReconnect, 0, 0),
            NextAction::RetryMedia {
                delay: FIRST_RETRY,
                request_fresh_livekit_token: true,
            }
        );
    }
}
