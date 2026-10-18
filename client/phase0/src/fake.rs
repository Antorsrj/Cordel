//! Synthetic screen shares for the `--fake-media` demo.
//!
//! `docs/ux/journal.md` asks for the three share states to be demonstrated with
//! fake data before server integration. This feeds the same [`MediaEvent`] queue
//! the room does, so the stage, the detached window and the identity rules are
//! exercised with no room, no token and no server.
//!
//! The publishers are named `publisher-1`, `publisher-2`, ... rather than after
//! people: nothing here should look like a real member.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use cordel_media::frame::Frame;

use crate::share::MediaEvent;
use crate::ShareState;

/// How often a synthetic share produces a frame. Slower than a real screen share
/// on purpose: this is about states and layout, not throughput.
const FRAME_INTERVAL: Duration = Duration::from_millis(500);

/// How long an interruption is held before the shares come back.
const INTERRUPTION: Duration = Duration::from_millis(3000);

/// Frame size and phase per publisher. The phase reaches `synthetic_bgra`'s red
/// channel, so the two publishers are visibly different and can be told apart in
/// a rendered image.
const LOOK: [(u32, u32, u8); 2] = [(640, 360, 0), (480, 270, 200)];

/// What the demo buttons ask for.
pub enum Control {
    Start,
    End,
    Interrupt,
}

struct Publisher {
    key: String,
    label: String,
    look: usize,
    phase: u8,
}

/// Starts the synthetic shares and returns the handle the demo buttons use.
pub fn spawn(events: Sender<MediaEvent>) -> Sender<Control> {
    let (control, commands) = mpsc::channel();
    thread::spawn(move || run(events, commands));
    control
}

fn run(events: Sender<MediaEvent>, commands: Receiver<Control>) {
    let mut publishers = Vec::new();
    let mut next = 0usize;
    if !add(&events, &mut publishers, &mut next) || !add(&events, &mut publishers, &mut next) {
        return;
    }
    let mut resuming: Option<Instant> = None;

    loop {
        match commands.recv_timeout(FRAME_INTERVAL) {
            Ok(Control::Start) => {
                if !add(&events, &mut publishers, &mut next) {
                    return;
                }
            }
            Ok(Control::End) => {
                // The newest one goes first, so ending twice reaches the state
                // with no share at all.
                if let Some(publisher) = publishers.pop() {
                    if events
                        .send(MediaEvent::Ended { key: publisher.key })
                        .is_err()
                    {
                        return;
                    }
                }
            }
            Ok(Control::Interrupt) => {
                // Frames stop arriving, which is what a real interruption looks
                // like to the stage, and the tiles say so.
                resuming = Some(Instant::now() + INTERRUPTION);
                for publisher in &publishers {
                    let _ = events.send(MediaEvent::State {
                        key: publisher.key.clone(),
                        state: ShareState::Reconnecting,
                    });
                }
                let _ = events.send(MediaEvent::Status(format!(
                    "Demo interruption: {} shares marked reconnecting for {} ms.",
                    publishers.len(),
                    INTERRUPTION.as_millis()
                )));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        if resuming.is_some_and(|deadline| Instant::now() >= deadline) {
            resuming = None;
            for publisher in &publishers {
                let _ = events.send(MediaEvent::State {
                    key: publisher.key.clone(),
                    state: ShareState::Live,
                });
            }
            let _ = events.send(MediaEvent::Status(
                "Demo shares are live again after the interruption.".into(),
            ));
        }

        if resuming.is_none() {
            for publisher in &mut publishers {
                publisher.phase = publisher.phase.wrapping_add(8);
                if !share(&events, publisher) {
                    return;
                }
            }
        }
    }
}

fn add(events: &Sender<MediaEvent>, publishers: &mut Vec<Publisher>, next: &mut usize) -> bool {
    let look = *next % LOOK.len();
    let publisher = Publisher {
        key: format!("fake-{}", *next + 1),
        label: format!("publisher-{}", *next + 1),
        look,
        phase: LOOK[look].2,
    };
    *next += 1;
    let keep_going = share(events, &publisher);
    publishers.push(publisher);
    keep_going
}

fn share(events: &Sender<MediaEvent>, publisher: &Publisher) -> bool {
    let (width, height, _) = LOOK[publisher.look];
    let frame: Frame = cordel_media::frame::synthetic_bgra(width, height, publisher.phase);
    events
        .send(MediaEvent::Frame {
            key: publisher.key.clone(),
            publisher: publisher.label.clone(),
            state: ShareState::Live,
            frame,
        })
        .is_ok()
}
