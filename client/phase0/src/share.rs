//! The screen-share stage: its catalog and the detached window.
//!
//! The media side (the room, the local capture, the synthetic demo) never touches
//! a Slint type. `slint::Image`, `VecModel` and the windows are bound to the UI
//! thread, so those tasks announce their pictures through [`MediaOut`] and the
//! UI thread drains them here. That keeps one conversion path — the sink from
//! `cordel_media::render` — and one writer of the stage model.
//!
//! A picture does not travel in the queue. The room delivers frames faster than
//! any interface can paint them, and a frame of a screen is megabytes of
//! pixels, so only the newest picture of a share waits in a slot and the queue
//! carries a short note about it. An interface that cannot keep up falls
//! behind the room on purpose instead of growing a backlog it can never catch
//! up with — and the events that end a share are never stuck behind pixels.
//!
//! Two behaviors from `docs/ux/journal.md` live here rather than in the markup:
//! a share is keyed by its media track, so it keeps its identity when another
//! share ends or is detached, and closing a detached window docks the share back
//! instead of dropping it.
//!
//! The stage model holds exactly what the main window draws. A detached share is
//! removed from it and held separately: Slint keeps the layout space of an
//! invisible item, so hiding a tile would leave the rest of the stage squeezed
//! beside a gap.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use cordel_media::frame::Frame;
use cordel_media::render::{FrameSink, SlintFrameSink};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, VecModel};

use crate::{ShareState, ShareTile, ShareWindow};

mod events;

pub use events::{FrameSlots, MediaEvent, MediaOut};

/// The shell's own capture is a view like any other; a stable key means a second
/// capture replaces the first instead of adding a tile.
pub const LOCAL_SHARE_KEY: &str = "local/screen-frame";

/// How often a share's picture reaches the interface.
///
/// The room delivers more frames than any interface can paint, and every
/// picture costs the interface a texture it has to upload. A picture that is
/// skipped here is not lost: the newest one waits in the slot and is painted
/// instead.
const PAINT_INTERVAL: Duration = Duration::from_millis(100);

/// The stage's model plus the share that is presented in its own window.
///
/// Every method runs on the UI thread, which is what allows a tile to hold a
/// `slint::Image`.
pub struct Shares {
    /// What the main window draws, in the order the shares appeared.
    tiles: Rc<VecModel<ShareTile>>,
    /// The one share shown in the detached window. One window, one share.
    detached: RefCell<Option<ShareTile>>,
    /// One sink per share, so a tile keeps its previous image and the measured
    /// conversion is the product path rather than a fresh-buffer worst case.
    sinks: RefCell<HashMap<String, SlintFrameSink>>,
    /// The waiting pictures of the shares.
    slots: FrameSlots,
    /// When each share was painted last. The rate is the interface's to
    /// afford, and the room's to exceed.
    painted: RefCell<HashMap<String, Instant>>,
}

impl Shares {
    pub fn new(slots: FrameSlots) -> Rc<Self> {
        Rc::new(Self {
            tiles: Rc::new(VecModel::default()),
            detached: RefCell::new(None),
            sinks: RefCell::new(HashMap::new()),
            slots,
            painted: RefCell::new(HashMap::new()),
        })
    }

    /// The model to hand to the main window. It stays live, so the UI follows
    /// every change below without being rebuilt.
    pub fn model(&self) -> ModelRc<ShareTile> {
        ModelRc::from(self.tiles.clone())
    }

    /// Applies one event and returns a status line to show, when there is one to
    /// show. Callers run this on the UI thread.
    ///
    /// The window-wide events ([`MediaEvent::Status`], [`MediaEvent::Busy`],
    /// [`MediaEvent::Joined`]) are the drain loop's business, not the stage's, so
    /// they are ignored here.
    pub fn apply(&self, event: MediaEvent) -> Option<String> {
        match event {
            MediaEvent::Frame {
                key,
                publisher,
                state,
            } => {
                // The picture waits in the slot. The interface takes the newest
                // one and lets the room's replacement stand in for the picture
                // it never painted.
                let frame = {
                    let mut slots = self.slots.lock().expect("the frame slots are not poisoned");
                    slots.remove(&key)
                };
                let Some(frame) = frame else {
                    // The picture was already taken; a newer one is on its way.
                    // The state still says what the share is doing.
                    return self.set_state(&key, state);
                };
                // The picture is what the interface can afford to skip. The
                // state is not: it says what the share is doing, and a share
                // that came back has to say so even when its picture waits.
                if self.paint(&key, publisher, state, frame) {
                    None
                } else {
                    self.set_state(&key, state)
                }
            }
            MediaEvent::Ended { key } => self.end(&key),
            MediaEvent::State { key, state } => self.set_state(&key, state),
            MediaEvent::Status(_) | MediaEvent::Busy(_) | MediaEvent::Joined(_) => None,
        }
    }

    /// Emphasizing one share clears the emphasis on the others: the stage can show
    /// several shares at once, but only one of them is the emphasized one.
    pub fn toggle_emphasis(&self, key: &str) {
        let Some(row) = self.row_of(key) else {
            return;
        };
        let emphasized = self.tiles.row_data(row).is_some_and(|t| t.emphasized);
        for i in 0..self.tiles.row_count() {
            let Some(mut tile) = self.tiles.row_data(i) else {
                continue;
            };
            let wanted = i == row && !emphasized;
            if tile.emphasized != wanted {
                tile.emphasized = wanted;
                self.tiles.set_row_data(i, tile);
            }
        }
    }

    /// Detaching is a presentation change: the share keeps publishing and keeps
    /// its key, and only moves from the stage to its own window.
    pub fn detach(&self, key: &str) {
        if self
            .detached
            .borrow()
            .as_ref()
            .is_some_and(|tile| tile.key == key)
        {
            return;
        }
        // One window, one share: a second detach docks the first one back.
        if let Some(previous) = self.detached.replace(None) {
            self.push_to_stage(previous);
        }
        let Some(row) = self.row_of(key) else {
            return;
        };
        let Some(mut tile) = self.tiles.row_data(row) else {
            return;
        };
        self.tiles.remove(row);
        // Emphasis is a decision about the stage, and this share is leaving it.
        tile.emphasized = false;
        *self.detached.borrow_mut() = Some(tile);
        self.ensure_emphasis();
    }

    /// The reverse of [`Shares::detach`]: the share goes back into the stage. It is
    /// also what a closed detached window does, so a close never loses a share.
    pub fn dock(&self, key: &str) {
        if !self
            .detached
            .borrow()
            .as_ref()
            .is_some_and(|tile| tile.key == key)
        {
            return;
        }
        let tile = self.detached.borrow_mut().take();
        if let Some(tile) = tile {
            self.push_to_stage(tile);
        }
    }

    /// The one share presented in its own window, if there is one.
    pub fn detached_tile(&self) -> Option<ShareTile> {
        self.detached.borrow().clone()
    }

    fn push_to_stage(&self, mut tile: ShareTile) {
        tile.emphasized = self.tiles.row_count() == 0;
        self.tiles.push(tile);
    }

    /// A stage with shares always has an emphasized one, even after the emphasized
    /// share ends or is detached.
    fn ensure_emphasis(&self) {
        if self.tiles.row_count() == 0 || self.emphasized_row().is_some() {
            return;
        }
        if let Some(mut first) = self.tiles.row_data(0) {
            first.emphasized = true;
            self.tiles.set_row_data(0, first);
        }
    }

    fn emphasized_row(&self) -> Option<usize> {
        (0..self.tiles.row_count()).find(|&i| self.tiles.row_data(i).is_some_and(|t| t.emphasized))
    }

    fn row_of(&self, key: &str) -> Option<usize> {
        (0..self.tiles.row_count()).find(|&i| self.tiles.row_data(i).is_some_and(|t| t.key == key))
    }

    /// Paints a picture, at a rate the interface can afford.
    ///
    /// It answers whether the picture was painted. A picture that arrives too
    /// soon is dropped, and the newest one in the slot takes its place. The
    /// first picture of a share is always painted: it is what puts the share
    /// on the stage.
    fn paint(&self, key: &str, publisher: String, state: ShareState, frame: Frame) -> bool {
        let on_stage = self.row_of(key).is_some()
            || self
                .detached
                .borrow()
                .as_ref()
                .is_some_and(|tile| tile.key == key);
        let mut painted = self.painted.borrow_mut();
        if on_stage
            && painted
                .get(key)
                .is_some_and(|last| last.elapsed() < PAINT_INTERVAL)
        {
            return false;
        }
        painted.insert(key.to_string(), Instant::now());
        drop(painted);
        self.upsert(key.to_string(), publisher, state, frame);
        true
    }

    fn upsert(
        &self,
        key: String,
        publisher: String,
        state: ShareState,
        frame: Frame,
    ) -> Option<String> {
        // The borrow is held for the whole conversion, so bind it instead of
        // keeping a temporary alive across the statements below.
        let mut sinks = self.sinks.borrow_mut();
        let sink = sinks.entry(key.clone()).or_default();
        if let Err(err) = sink.present(&frame) {
            return Some(format!(
                "A {}x{} frame from {publisher} could not be shown: {err}",
                frame.width(),
                frame.height()
            ));
        }
        let image = sink.image().unwrap_or_default();
        drop(sinks);

        // The detached share is not in the stage model, and it keeps updating: its
        // window shows the newest frame.
        if let Some(tile) = self.detached.borrow_mut().as_mut() {
            if tile.key == key {
                tile.publisher = publisher.into();
                tile.state = state;
                tile.frame = image;
                return None;
            }
        }

        match self.row_of(&key) {
            Some(row) => {
                let mut tile = self.tiles.row_data(row).expect("the row was just found");
                tile.publisher = publisher.into();
                tile.state = state;
                tile.frame = image;
                self.tiles.set_row_data(row, tile);
            }
            None => {
                let emphasized = self.tiles.row_count() == 0;
                self.tiles.push(ShareTile {
                    key: key.into(),
                    publisher: publisher.into(),
                    state,
                    frame: image,
                    emphasized,
                });
            }
        }
        None
    }

    fn end(&self, key: &str) -> Option<String> {
        // The picture of a share that ended is not worth keeping.
        self.slots
            .lock()
            .expect("the frame slots are not poisoned")
            .remove(key);
        self.painted.borrow_mut().remove(key);

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

    fn set_state(&self, key: &str, state: ShareState) -> Option<String> {
        if let Some(tile) = self.detached.borrow_mut().as_mut() {
            if tile.key == key {
                tile.state = state;
                return None;
            }
        }
        let row = self.row_of(key)?;
        let mut tile = self.tiles.row_data(row)?;
        if tile.state == state {
            return None;
        }
        tile.state = state;
        self.tiles.set_row_data(row, tile);
        None
    }
}

/// What a dock request is forwarded to.
pub type DockHandler = Box<dyn FnMut(&str)>;

/// The one share that is presented in its own native window.
///
/// The window is created once and reused: detaching another share moves this
/// window to it. It is hidden, not destroyed, when the share is docked, so no
/// window lifecycle can end someone else's publication.
pub struct DetachedWindow {
    window: RefCell<Option<Rc<ShareWindow>>>,
    /// The share the window is presenting. The dock action reads it when it runs,
    /// so a window that was reused for another share docks the one it is actually
    /// showing instead of the one it was created for.
    presented: RefCell<Option<String>>,
    /// Called with the share key when the window asks to dock: either from its
    /// button or from the window manager's close request.
    on_dock: RefCell<DockHandler>,
}

impl DetachedWindow {
    pub fn new(on_dock: impl FnMut(&str) + 'static) -> Rc<Self> {
        Rc::new(Self {
            window: RefCell::new(None),
            presented: RefCell::new(None),
            on_dock: RefCell::new(Box::new(on_dock)),
        })
    }

    /// Shows `tile` in the detached window, or hides the window when there is no
    /// detached share any more (it ended, or it was docked).
    pub fn sync(self: &Rc<Self>, tile: Option<ShareTile>) {
        let Some(tile) = tile else {
            *self.presented.borrow_mut() = None;
            self.hide();
            return;
        };

        let existing = self.window.borrow().as_ref().cloned();
        let window = match existing {
            Some(window) => window,
            None => {
                let window = match ShareWindow::new() {
                    Ok(window) => Rc::new(window),
                    Err(err) => {
                        log::warn!("the detached share window could not be created: {err}");
                        return;
                    }
                };
                let dock = {
                    let me = Rc::downgrade(self);
                    move || {
                        let Some(me) = me.upgrade() else {
                            return;
                        };
                        let key = me.presented.borrow().clone();
                        if let Some(key) = key {
                            (me.on_dock.borrow_mut())(&key);
                        }
                        me.hide();
                    }
                };
                window.on_dock(dock.clone());
                // Closing the window is a dock request, so a window manager action
                // can never lose a share. The response hides the window instead of
                // ending it.
                window.window().on_close_requested({
                    let dock = dock;
                    move || {
                        dock();
                        CloseRequestResponse::HideWindow
                    }
                });
                *self.window.borrow_mut() = Some(window.clone());
                window
            }
        };

        *self.presented.borrow_mut() = Some(tile.key.to_string());
        window.set_tile(tile);
        if !window.window().is_visible() {
            if let Err(err) = window.show() {
                log::warn!("the detached share window could not be shown: {err}");
            }
        }
    }

    pub fn hide(&self) {
        if let Some(window) = self.window.borrow().as_ref() {
            let _ = window.hide();
        }
    }

    /// The live window handle, for a test that simulates a window manager close.
    #[cfg(test)]
    pub fn handle(&self) -> Option<slint::Weak<ShareWindow>> {
        self.window.borrow().as_ref().map(|window| window.as_weak())
    }
}

/// The stage's behaviors and its rendering, under a headless platform.
///
/// The catalog tests need no display and no window: they are the identity,
/// emphasis, detach/dock and state rules. The stage tests below them render the
/// three states from `docs/ux/journal.md` as images — composing them into a PPM
/// while inspecting them by hand — and drive the detached window's lifecycle,
/// including the close request a window manager sends. Joined, the two are the
/// automated half of the journal's "fake media before server integration" check,
/// and the app running with `--fake-media` is the other half.
#[cfg(test)]
#[path = "share/tests.rs"]
mod tests;
