//! One screen share per participant, and no tile after that share has ended.
//!
//! The session logs the end and sends it on. The decoder of the old track
//! still has a picture in flight, and the drain applies that picture after
//! the end. Without a record of the ended publication the picture creates the
//! tile again, which is how one person ended up with a tile per click.

use super::*;

impl Shares {
    /// Shows the newest picture of a share, or ignores it when that
    /// publication has already ended.
    pub(super) fn show_frame(
        &self,
        key: String,
        publisher: String,
        state: ShareState,
    ) -> Option<String> {
        let (frame, state, epoch) = {
            let mut slots = self.slots.lock().expect("the frame slots are not poisoned");
            match slots.get_mut(&key) {
                Some(waiting) if waiting.pending => {
                    waiting.pending = false;
                    (waiting.frame.take(), waiting.state, waiting.epoch)
                }
                // The note is old and the picture was already taken. The epoch
                // still says which publication it belonged to.
                Some(waiting) => (None, waiting.state, waiting.epoch),
                None => (None, state, 0),
            }
        };
        if self.is_stale(&key, epoch) {
            // A newer publication of this same track is already on the stage.
            // Dropping the renderer here would blank that picture.
            let newer = self
                .live_epoch
                .borrow()
                .get(&key)
                .is_some_and(|live| *live > epoch);
            if !newer {
                self.release(&key);
            }
            return None;
        }
        // A second track from the same person replaces the tile. The old
        // track's later pictures are stale and cannot bring that tile back.
        self.collapse(&publisher, &key);
        self.live_epoch.borrow_mut().insert(key.clone(), epoch);
        let Some(frame) = frame else {
            return self.set_state(&key, state);
        };
        if self.paint(&key, publisher, state, frame) {
            None
        } else {
            self.set_state(&key, state)
        }
    }

    pub(super) fn end(&self, key: &str, epoch: u64) -> Option<String> {
        if self
            .live_epoch
            .borrow()
            .get(key)
            .is_some_and(|live| *live > epoch)
        {
            // This end belongs to an older publication of the same track.
            self.mark_ended(key, epoch);
            return None;
        }
        self.mark_ended(key, epoch);
        self.live_epoch.borrow_mut().remove(key);
        self.release(key);

        let mut detached = self.detached.borrow_mut();
        if detached.as_ref().is_some_and(|tile| tile.key == key) {
            let tile = detached.take().expect("just checked");
            drop(detached);
            self.ensure_emphasis();
            return Some(format!("{} stopped sharing.", tile.publisher));
        }
        drop(detached);

        let row = self.row_of(key)?;
        let tile = self.tiles.row_data(row).expect("the row was just found");
        self.tiles.remove(row);
        self.ensure_emphasis();
        Some(format!("{} stopped sharing.", tile.publisher))
    }

    /// Drops every other screen from `publisher` so the stage keeps `keep`.
    fn collapse(&self, publisher: &str, keep: &str) {
        let mut drop_rows = Vec::new();
        for row in 0..self.tiles.row_count() {
            let Some(tile) = self.tiles.row_data(row) else {
                continue;
            };
            if tile.publisher == publisher && tile.key != keep {
                drop_rows.push(row);
            }
        }
        for row in drop_rows.into_iter().rev() {
            let Some(tile) = self.tiles.row_data(row) else {
                continue;
            };
            let key = tile.key.to_string();
            self.retire(&key);
            self.tiles.remove(row);
        }
        let detached_key = self.detached.borrow().as_ref().and_then(|tile| {
            (tile.publisher == publisher && tile.key != keep).then(|| tile.key.to_string())
        });
        if let Some(key) = detached_key {
            self.retire(&key);
            if let Some(tile) = self.detached.borrow_mut().as_mut() {
                tile.key = keep.into();
            }
        }
        self.ensure_emphasis();
    }

    fn retire(&self, key: &str) {
        let epoch = self.live_epoch.borrow().get(key).copied().unwrap_or(0);
        self.mark_ended(key, epoch);
        self.live_epoch.borrow_mut().remove(key);
        self.release(key);
    }

    fn mark_ended(&self, key: &str, epoch: u64) {
        let mut ended = self.ended_epoch.borrow_mut();
        let stored = ended.entry(key.to_string()).or_insert(0);
        if epoch > *stored {
            *stored = epoch;
        }
    }

    fn is_stale(&self, key: &str, epoch: u64) -> bool {
        self.ended_epoch
            .borrow()
            .get(key)
            .is_some_and(|ended| epoch <= *ended)
    }

    /// Drops the picture, the paint timestamp and the renderer.
    fn release(&self, key: &str) {
        self.slots
            .lock()
            .expect("the frame slots are not poisoned")
            .remove(key);
        self.painted.borrow_mut().remove(key);
        self.sinks.borrow_mut().remove(key);
    }
}
