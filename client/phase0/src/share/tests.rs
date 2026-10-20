
use super::*;
use crate::{AppWindow, ChatLine};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, PlatformError, WindowAdapter, WindowEvent};
use slint::{ComponentHandle, PhysicalSize, Rgb8Pixel};
use std::fs::File;
use std::io::Write;

fn frame_event(key: &str, publisher: &str, phase: u8) -> MediaEvent {
    MediaEvent::Frame {
        key: key.into(),
        publisher: publisher.into(),
        state: ShareState::Live,
        frame: cordel_media::frame::synthetic_bgra(320, 180, phase),
    }
}

fn share_frame(key: &str, publisher: &str, size: (u32, u32), phase: u8) -> MediaEvent {
    MediaEvent::Frame {
        key: key.into(),
        publisher: publisher.into(),
        state: ShareState::Live,
        frame: cordel_media::frame::synthetic_bgra(size.0, size.1, phase),
    }
}

/// What the main window draws, in order.
fn stage_tiles(shares: &Shares) -> Vec<ShareTile> {
    (0..shares.model().row_count())
        .filter_map(|i| shares.model().row_data(i))
        .collect()
}

fn stage_keys(shares: &Shares) -> Vec<String> {
    stage_tiles(shares)
        .into_iter()
        .map(|t| t.key.to_string())
        .collect()
}

/// Two shares with distinguishable frames: `synthetic_bgra` puts the phase in
/// the red channel, so ana is red-0 and bruno is red-200.
fn two_shares() -> Rc<Shares> {
    let shares = Shares::new();
    shares.apply(share_frame("a", "ana", (640, 360), 0));
    shares.apply(share_frame("b", "bruno", (480, 270), 200));
    shares
}

/// A few chat lines, so the conversation area paints something the stretch
/// assertions below can find.
fn conversation() -> ModelRc<ChatLine> {
    ModelRc::from(Rc::new(VecModel::from(vec![
        ChatLine {
            author: "ana".into(),
            body: "the stage should not reserve room when nobody shares".into(),
        },
        ChatLine {
            author: "bruno".into(),
            body: "and it should take the top when two people do".into(),
        },
    ])))
}

/// The top of the conversation area: the first row where the chat row
/// background `#282623` covers a good part of the width. A threshold is needed
/// because a handful of antialiased pixels near the buttons carry the same
/// color.
fn conversation_top(width: u32, pixels: &[Rgb8Pixel]) -> Option<usize> {
    let height = pixels.len() / width as usize;
    let mut per_row = vec![0usize; height];
    for (i, p) in pixels.iter().enumerate() {
        if p.r == 40 && p.g == 38 && p.b == 35 {
            per_row[i / width as usize] += 1;
        }
    }
    per_row.iter().position(|count| *count > width as usize / 5)
}

#[test]
fn a_frame_creates_a_tile_and_later_frames_update_it_in_place() {
    let shares = Shares::new();
    assert!(shares.apply(frame_event("a", "ana", 0)).is_none());
    assert_eq!(shares.model().row_count(), 1);

    assert!(shares.apply(frame_event("a", "ana", 40)).is_none());
    assert_eq!(shares.model().row_count(), 1, "a frame must not add a tile");
}

#[test]
fn two_publishers_are_separate_tiles_and_keep_their_identity_when_one_ends() {
    let shares = Shares::new();
    shares.apply(frame_event("a", "ana", 0));
    shares.apply(frame_event("b", "bruno", 200));
    assert_eq!(stage_keys(&shares), ["a", "b"]);

    let note = shares.apply(MediaEvent::Ended { key: "a".into() });
    assert_eq!(note.as_deref(), Some("ana stopped sharing."));
    assert_eq!(
        stage_keys(&shares),
        ["b"],
        "the second share keeps its own identity"
    );

    let remaining = stage_tiles(&shares);
    assert_eq!(remaining[0].publisher, "bruno");
    assert!(
        remaining[0].emphasized,
        "the remaining share takes the stage"
    );
}

#[test]
fn the_first_tile_starts_emphasized_and_emphasis_is_exclusive() {
    let shares = Shares::new();
    shares.apply(frame_event("a", "ana", 0));
    shares.apply(frame_event("b", "bruno", 200));
    assert!(stage_tiles(&shares)[0].emphasized);

    shares.toggle_emphasis("b");
    let tiles = stage_tiles(&shares);
    assert!(!tiles[0].emphasized && tiles[1].emphasized);

    // Emphasizing the emphasized one returns the stage to no emphasis.
    shares.toggle_emphasis("b");
    assert!(stage_tiles(&shares).iter().all(|t| !t.emphasized));
}

#[test]
fn a_detached_share_leaves_the_stage_and_moves_to_its_own_window() {
    let shares = Shares::new();
    shares.apply(frame_event("a", "ana", 0));
    shares.apply(frame_event("b", "bruno", 200));
    // "a" starts emphasized, so the stage is ana-heavy before the detach.
    assert!(stage_tiles(&shares)[0].emphasized);

    shares.detach("a");
    assert_eq!(
        stage_keys(&shares),
        ["b"],
        "the stage keeps only what it draws"
    );
    assert_eq!(
        shares.detached_tile().map(|t| t.key.to_string()),
        Some("a".into())
    );
    assert!(
        stage_tiles(&shares)[0].emphasized,
        "the remaining share takes the emphasis the detached one gave up"
    );

    // One window, one share: detaching another moves the window to it.
    shares.detach("b");
    assert_eq!(stage_keys(&shares), ["a"]);
    assert_eq!(
        shares.detached_tile().map(|t| t.key.to_string()),
        Some("b".into())
    );

    shares.dock("b");
    assert_eq!(
        stage_keys(&shares),
        ["a", "b"],
        "docking puts the share back"
    );
    assert!(shares.detached_tile().is_none());
}

#[test]
fn a_detached_share_keeps_receiving_frames_while_it_is_away() {
    let shares = Shares::new();
    shares.apply(frame_event("a", "ana", 0));
    shares.detach("a");

    shares.apply(frame_event("a", "ana", 120));
    assert_eq!(
        shares.model().row_count(),
        0,
        "a frame must not put the detached share back into the stage"
    );
    assert_eq!(
        shares.detached_tile().map(|t| t.key.to_string()),
        Some("a".into())
    );

    // And a reconnection reaches it too.
    shares.apply(MediaEvent::State {
        key: "a".into(),
        state: ShareState::Reconnecting,
    });
    assert_eq!(
        shares.detached_tile().map(|t| t.state),
        Some(ShareState::Reconnecting)
    );
}

#[test]
fn a_reconnecting_share_is_marked_and_a_new_frame_restores_it() {
    let shares = Shares::new();
    shares.apply(frame_event("a", "ana", 0));
    shares.apply(frame_event("b", "bruno", 200));

    shares.apply(MediaEvent::State {
        key: "a".into(),
        state: ShareState::Reconnecting,
    });
    let tiles = stage_tiles(&shares);
    assert_eq!(tiles[0].state, ShareState::Reconnecting);
    assert_eq!(
        tiles[1].state,
        ShareState::Live,
        "the other share is untouched"
    );

    shares.apply(frame_event("a", "ana", 80));
    assert_eq!(stage_tiles(&shares)[0].state, ShareState::Live);
}

#[test]
fn a_local_capture_is_one_tile_that_can_change_state() {
    let shares = Shares::new();
    shares.apply(MediaEvent::Frame {
        key: LOCAL_SHARE_KEY.into(),
        publisher: "you".into(),
        state: ShareState::Preview,
        frame: cordel_media::frame::synthetic_bgra(640, 360, 0),
    });
    assert_eq!(stage_tiles(&shares)[0].state, ShareState::Preview);

    shares.apply(MediaEvent::Frame {
        key: LOCAL_SHARE_KEY.into(),
        publisher: "you".into(),
        state: ShareState::Live,
        frame: cordel_media::frame::synthetic_bgra(640, 360, 8),
    });
    assert_eq!(shares.model().row_count(), 1);
    assert_eq!(stage_tiles(&shares)[0].state, ShareState::Live);
}

#[test]
fn an_unknown_share_is_ignored_rather_than_invented() {
    let shares = Shares::new();
    assert!(shares
        .apply(MediaEvent::Ended {
            key: "never-seen".into()
        })
        .is_none());
    assert!(shares
        .apply(MediaEvent::State {
            key: "never-seen".into(),
            state: ShareState::Reconnecting
        })
        .is_none());
    assert_eq!(shares.model().row_count(), 0);
}

thread_local! {
    /// One adapter per component: two components sharing one adapter would draw
    /// into the same window.
    static WINDOWS: RefCell<Vec<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(Vec::new()) };
}

struct TestPlatform;

impl Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        WINDOWS.with(|windows| windows.borrow_mut().push(window.clone()));
        Ok(window)
    }
}

/// Installs the platform. This has to happen *before* a component is built: a
/// component asks its platform for a window while it is being created, and the
/// default backend cannot create a second winit event loop in a test process.
fn install_platform() {
    let _ = slint::platform::set_platform(Box::new(TestPlatform));
}

/// Layout and draw `component` at `width` x `height`, and return the pixels.
///
/// Set `CORDEL_STAGE_RENDER_OUT` to a directory to also write a PPM there,
/// which is how the composition was inspected while building this.
fn render(name: &str, width: u32, height: u32, component: &impl ComponentHandle) -> Vec<Rgb8Pixel> {
    component.show().expect("showing the window");
    let target = component.window() as *const slint::Window;
    let window = WINDOWS
        .with(|windows| {
            windows
                .borrow()
                .iter()
                .find(|window| std::ptr::eq(window.window(), target))
                .cloned()
        })
        .expect("the component was built on this test's own platform");
    window.set_size(PhysicalSize::new(width, height));
    slint::platform::update_timers_and_animations();
    window.request_redraw();

    let mut buffer = vec![Rgb8Pixel::new(0, 0, 0); (width * height) as usize];
    let drawn = window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, width as usize);
    });
    assert!(drawn, "the window reported that nothing needed drawing");

    if let Ok(directory) = std::env::var("CORDEL_STAGE_RENDER_OUT") {
        dump(&format!("{directory}/{name}.ppm"), &buffer, width, height);
    }
    buffer
}

fn dump(path: &str, pixels: &[Rgb8Pixel], width: u32, height: u32) {
    let Ok(mut file) = File::create(path) else {
        return;
    };
    let _ = write!(file, "P6\n{width} {height}\n255\n");
    let mut bytes = Vec::with_capacity(pixels.len() * 3);
    for pixel in pixels {
        bytes.extend_from_slice(&[pixel.r, pixel.g, pixel.b]);
    }
    let _ = file.write_all(&bytes);
    println!("wrote {path}");
}

/// The synthetic frames are told apart by their red channel: ana is built with
/// phase 0 (`synthetic_bgra` sets red to the phase) and bruno with phase 200.
/// Black and the cream text sit outside both signatures, so a count of zero
/// really means "this share did not paint here".
fn blue_pixels(pixels: &[Rgb8Pixel]) -> usize {
    pixels.iter().filter(|p| p.r < 10 && p.b > 100).count()
}

fn amber_pixels(pixels: &[Rgb8Pixel]) -> usize {
    pixels
        .iter()
        .filter(|p| (195..=205).contains(&p.r) && p.g < 60 && p.b > 60)
        .count()
}

/// Where the amber share starts horizontally. This is what tells a rendered
/// stage apart from a laid-out one: a share that left the stage gave its room
/// to the others instead of leaving a gap.
fn leftmost_amber(width: u32, pixels: &[Rgb8Pixel]) -> Option<usize> {
    pixels
        .iter()
        .enumerate()
        .filter(|(_, p)| (195..=205).contains(&p.r) && p.g < 60 && p.b > 60)
        .map(|(i, _)| i % width as usize)
        .min()
}

#[test]
fn no_share_leaves_no_stage_and_two_shares_render_together() {
    install_platform();
    let ui = AppWindow::new().expect("the main window builds");
    ui.set_messages(conversation());
    let empty = render("state-1-no-share", 1100, 720, &ui);
    assert_eq!(blue_pixels(&empty), 0, "no share, no stage");
    assert_eq!(amber_pixels(&empty), 0, "no share, no stage");
    assert!(
        conversation_top(1100, &empty).is_some_and(|y| y < 300),
        "with no share the conversation has the room the stage would take: {:?}",
        conversation_top(1100, &empty)
    );

    let shares = two_shares();
    ui.set_shares(shares.model());
    let both = render("state-2-two-shares", 1100, 720, &ui);
    assert!(blue_pixels(&both) > 1000, "ana: {}", blue_pixels(&both));
    assert!(amber_pixels(&both) > 1000, "bruno: {}", amber_pixels(&both));
    assert!(
        leftmost_amber(1100, &both).is_some_and(|x| x > 500),
        "with two shares each one keeps about half the stage: {:?}",
        leftmost_amber(1100, &both)
    );
    assert!(
        conversation_top(1100, &both).is_some_and(|y| y > 300),
        "with two shares the stage takes the top and chat moves down: {:?}",
        conversation_top(1100, &both)
    );
}

#[test]
fn a_detached_share_leaves_the_main_stage() {
    install_platform();
    let shares = two_shares();
    shares.detach("a");
    let ui = AppWindow::new().expect("the main window builds");
    ui.set_shares(shares.model());

    let main = render("state-3-one-detached", 1100, 720, &ui);
    assert_eq!(blue_pixels(&main), 0, "the detached share left the stage");
    assert!(amber_pixels(&main) > 1000, "bruno: {}", amber_pixels(&main));
    assert!(
        leftmost_amber(1100, &main).is_some_and(|x| x < 400),
        "the remaining share takes the room the detached one gave up: {:?}",
        leftmost_amber(1100, &main)
    );

    // The detached window is the one that shows it now, and only it.
    let detached = ShareWindow::new().expect("the detached window builds");
    detached.set_tile(shares.detached_tile().expect("a detached share"));
    let window = render("state-3-detached-window", 720, 480, &detached);
    assert!(blue_pixels(&window) > 1000, "ana: {}", blue_pixels(&window));
    assert_eq!(
        amber_pixels(&window),
        0,
        "the other share is not in this window"
    );
}

#[test]
fn an_ended_share_takes_its_tile_and_its_window_with_it() {
    install_platform();
    let shares = two_shares();
    shares.detach("a");
    assert!(shares.detached_tile().is_some());

    shares.apply(MediaEvent::Ended { key: "a".into() });
    assert!(shares.detached_tile().is_none(), "nothing left to present");
    assert_eq!(shares.model().row_count(), 1);

    let ui = AppWindow::new().expect("the main window builds");
    ui.set_shares(shares.model());
    let pixels = render("ended-share-removed", 1100, 720, &ui);
    assert_eq!(blue_pixels(&pixels), 0);
    assert!(amber_pixels(&pixels) > 1000);
}

/// The journal's rule for the detached window: closing it docks the share back,
/// and it never stops anyone's publication. `CloseRequested` is the event a
/// window manager sends, so this drives the real close path instead of calling
/// the dock action directly.
#[test]
fn closing_the_detached_window_docks_the_share_back() {
    install_platform();
    let shares = two_shares();
    shares.detach("a");
    let docked: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let window = DetachedWindow::new({
        let shares = shares.clone();
        let docked = docked.clone();
        move |key| {
            docked.borrow_mut().push(key.to_string());
            shares.dock(key);
        }
    });

    window.sync(shares.detached_tile());
    let first = window
        .handle()
        .and_then(|weak| weak.upgrade())
        .expect("the detached window was created");
    assert!(first.window().is_visible(), "the detached window is up");
    assert!(
        shares.detached_tile().is_some(),
        "nothing is docked until the window asks"
    );

    first.window().dispatch_event(WindowEvent::CloseRequested);
    assert_eq!(docked.borrow().as_slice(), ["a".to_string()]);
    assert!(
        shares.detached_tile().is_none(),
        "the share is back in the stage"
    );
    assert_eq!(stage_tiles(&shares).len(), 2);
    assert!(
        !first.window().is_visible(),
        "the window is hidden, not kept up"
    );

    // Detaching again reuses that window rather than opening a second one.
    shares.detach("b");
    window.sync(shares.detached_tile());
    let reused = window
        .handle()
        .and_then(|weak| weak.upgrade())
        .expect("the window is reused");
    assert!(
        std::ptr::eq(reused.window(), first.window()),
        "the same window was reused"
    );
    assert!(reused.window().is_visible());
    reused.window().dispatch_event(WindowEvent::CloseRequested);
    assert_eq!(
        docked.borrow().as_slice(),
        ["a".to_string(), "b".to_string()]
    );
    assert!(shares.detached_tile().is_none());
}

/// A share that ends while it is detached takes its window with it: there is
/// nothing left to present, so the window hides instead of keeping a stale
/// frame on screen.
#[test]
fn a_detached_window_hides_when_its_share_ends() {
    install_platform();
    let shares = two_shares();
    shares.detach("a");
    let window = DetachedWindow::new(|_| {});
    window.sync(shares.detached_tile());
    let open = window
        .handle()
        .and_then(|weak| weak.upgrade())
        .expect("the detached window was created");
    assert!(open.window().is_visible());

    shares.apply(MediaEvent::Ended { key: "a".into() });
    window.sync(shares.detached_tile());
    assert!(
        !open.window().is_visible(),
        "the window is hidden once there is nothing to present"
    );
}
