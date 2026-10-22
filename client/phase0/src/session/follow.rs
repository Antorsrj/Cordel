//! Which remote screen shares this client is decoding.
//!
//! One participant publishes one screen share. A new track stops the decoder
//! of the previous one before the viewer can paint both. Stopping is what
//! drops the native video stream: leaving that task alive overflowed its
//! queue and kept feeding the tile the end event had removed.

use std::collections::HashMap;

use tokio::sync::watch;

struct Followed {
    publisher: String,
    /// A screen share. A second one from the same participant replaces it. A
    /// camera, if one ever arrives, does not.
    exclusive: bool,
    epoch: u64,
    stop: watch::Sender<bool>,
}

/// What [`RemoteFollows::watch`] started, and which previous decoders it stopped.
pub(super) struct Watched {
    pub epoch: u64,
    pub stop: watch::Receiver<bool>,
    /// `false` when this track is already being decoded. The caller must not
    /// start a second task for it.
    pub fresh: bool,
    /// Tracks whose decoders were just stopped, oldest publication first.
    pub ended: Vec<(String, u64)>,
}

/// The remote video tracks the shell is following.
pub(super) struct RemoteFollows {
    next_epoch: u64,
    follows: HashMap<String, Followed>,
}

impl RemoteFollows {
    pub(super) fn new() -> Self {
        Self {
            next_epoch: 0,
            follows: HashMap::new(),
        }
    }

    pub(super) fn keys(&self) -> Vec<String> {
        self.follows.keys().cloned().collect()
    }

    /// Follow `track_sid` from `publisher`.
    ///
    /// When `exclusive` is set, every other exclusive track from that
    /// participant is stopped. The returned receiver is how the decoder task
    /// learns that it must drop the stream.
    pub(super) fn watch(&mut self, publisher: &str, track_sid: &str, exclusive: bool) -> Watched {
        let key = format!("{publisher}/{track_sid}");
        if let Some(existing) = self.follows.get(&key) {
            return Watched {
                epoch: existing.epoch,
                stop: existing.stop.subscribe(),
                fresh: false,
                ended: Vec::new(),
            };
        }
        let mut ended = Vec::new();
        if exclusive {
            let stale: Vec<String> = self
                .follows
                .iter()
                .filter(|(_, follow)| follow.exclusive && follow.publisher == publisher)
                .map(|(key, _)| key.clone())
                .collect();
            for old in stale {
                if let Some(stopped) = self.stop(&old) {
                    ended.push(stopped);
                }
            }
        }
        self.next_epoch = self.next_epoch.saturating_add(1);
        let epoch = self.next_epoch;
        let (stop, receiver) = watch::channel(false);
        self.follows.insert(
            key,
            Followed {
                publisher: publisher.to_string(),
                exclusive,
                epoch,
                stop,
            },
        );
        Watched {
            epoch,
            stop: receiver,
            fresh: true,
            ended,
        }
    }

    /// Stops the decoder for one track. `None` when it was not being followed.
    pub(super) fn end(&mut self, key: &str) -> Option<u64> {
        self.stop(key).map(|(_, epoch)| epoch)
    }

    pub(super) fn end_publisher(&mut self, publisher: &str) -> Vec<(String, u64)> {
        let keys: Vec<String> = self
            .follows
            .iter()
            .filter(|(_, follow)| follow.publisher == publisher)
            .map(|(key, _)| key.clone())
            .collect();
        keys.into_iter().filter_map(|key| self.stop(&key)).collect()
    }

    pub(super) fn end_all(&mut self) -> Vec<(String, u64)> {
        let keys: Vec<String> = self.follows.keys().cloned().collect();
        keys.into_iter().filter_map(|key| self.stop(&key)).collect()
    }

    fn stop(&mut self, key: &str) -> Option<(String, u64)> {
        let follow = self.follows.remove(key)?;
        let _ = follow.stop.send(true);
        Some((key.to_string(), follow.epoch))
    }
}
