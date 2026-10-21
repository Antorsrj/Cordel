mod chat;
mod fake;
mod identity;
mod session;
mod share;

use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cordel_media::capture::SessionKind;
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};

use crate::chat::{phase0_messages, AccountEvent, Command as ChatCommand, UiEvent as ChatUiEvent};
use crate::identity::Command as IdentityCommand;
use crate::session::Command;
use crate::share::{DetachedWindow, MediaEvent, Shares};

slint::include_modules!();

/// How often the UI thread looks for media events. Frames arrive a few times per
/// second per share, so this is responsive without being a busy loop.
const DRAIN_INTERVAL: Duration = Duration::from_millis(30);

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let started = Instant::now();
    let ui = AppWindow::new()?;
    let lines: Vec<ChatLine> = phase0_messages()
        .into_iter()
        .map(|message| ChatLine {
            author: message.author.into(),
            body: message.body.into(),
        })
        .collect();
    let line_count = lines.len();
    let messages = Rc::new(VecModel::from(lines));
    ui.set_messages(ModelRc::from(messages.clone()));
    let chat_ms = started.elapsed().as_millis();

    let fake_media = std::env::args().any(|arg| arg == "--fake-media");

    // Media runs off the UI thread and never touches a Slint type: `slint::Image`
    // and the models are bound to this thread. The tasks send events, and the
    // timer below is the only place that applies them, so there is one writer of
    // the stage model and one writer of the status line, in the order things
    // happened.
    let (events, incoming) = mpsc::channel::<MediaEvent>();

    let shares = Shares::new();
    ui.set_shares(shares.model());
    let detached = DetachedWindow::new({
        let shares = shares.clone();
        move |key| shares.dock(key)
    });

    ui.on_emphasize_share({
        let shares = shares.clone();
        move |key| shares.toggle_emphasis(&key)
    });
    ui.on_detach_share({
        let shares = shares.clone();
        let detached = detached.clone();
        move |key| {
            shares.detach(&key);
            detached.sync(shares.detached_tile());
        }
    });

    // Fake media: the same events the room sends, so the stage, the identity rules
    // and the detached window are all exercised with no room and no server.
    let fake_control = fake_media.then(|| fake::spawn(events.clone()));
    ui.set_fake_media(fake_media);
    if let Some(fake) = &fake_control {
        ui.on_fake_start_share({
            let fake = fake.clone();
            move || {
                let _ = fake.send(fake::Control::Start);
            }
        });
        ui.on_fake_end_share({
            let fake = fake.clone();
            move || {
                let _ = fake.send(fake::Control::End);
            }
        });
        ui.on_fake_interrupt({
            let fake = fake.clone();
            move || {
                let _ = fake.send(fake::Control::Interrupt);
            }
        });
    }

    let (commands, command_events) = tokio::sync::mpsc::unbounded_channel::<Command>();
    std::thread::spawn({
        let events = events.clone();
        move || {
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            runtime.block_on(session::run(command_events, events));
        }
    });

    ui.on_join({
        let commands = commands.clone();
        move || {
            let _ = commands.send(Command::Join);
        }
    });
    ui.on_capture_frame({
        let commands = commands.clone();
        move || {
            let _ = commands.send(Command::CaptureFrame);
        }
    });
    ui.on_leave({
        let commands = commands.clone();
        move || {
            let _ = commands.send(Command::Leave);
        }
    });

    ui.set_status(
        if fake_media {
            format!(
                "{line_count} chat lines built in {chat_ms} ms. Demo media: synthetic shares only, \
                 no room and no server."
            )
        } else {
            format!(
                "{line_count} chat lines built in {chat_ms} ms. RSS {} KiB. Session: {}. Set \
                 CORDEL_LIVEKIT_URL and CORDEL_LIVEKIT_TOKEN before joining.",
                rss_kib().unwrap_or(0),
                SessionKind::detect()
                    .map(SessionKind::as_str)
                    .unwrap_or("unknown"),
            )
        }
        .into(),
    );

    let (chat_tx, chat_rx) = mpsc::channel::<ChatCommand>();
    let (account_tx, account_rx) = mpsc::channel::<AccountEvent>();
    let (chat_ui_tx, chat_ui_rx) = mpsc::channel::<ChatUiEvent>();

    // The UI thread's only job for media and chat events. The timer has to
    // outlive `run`, so it is held here rather than in a temporary.
    let drain = Timer::default();
    drain.start(TimerMode::Repeated, DRAIN_INTERVAL, {
        let shares = shares.clone();
        let detached = detached.clone();
        let messages = messages.clone();
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else {
                return;
            };
            while let Ok(event) = chat_ui_rx.try_recv() {
                match event {
                    ChatUiEvent::Reset => messages.set_vec(Vec::<ChatLine>::new()),
                    ChatUiEvent::Message { author, body } => messages.push(ChatLine {
                        author: author.into(),
                        body: body.into(),
                    }),
                    ChatUiEvent::Connected(value) => ui.set_chat_connected(value),
                    ChatUiEvent::Status(value) => ui.set_chat_status(value.into()),
                }
            }
            while let Ok(event) = incoming.try_recv() {
                match event {
                    MediaEvent::Status(text) => ui.set_status(text.into()),
                    MediaEvent::Busy(busy) => ui.set_busy(busy),
                    MediaEvent::Joined(joined) => ui.set_joined(joined),
                    event => {
                        if let Some(note) = shares.apply(event) {
                            ui.set_status(note.into());
                        }
                        // A detached share shows what the tile currently holds, and
                        // a share that ended takes its window with it.
                        detached.sync(shares.detached_tile());
                    }
                }
            }
        }
    });

    let channel_id = std::env::var("CORDEL_CHANNEL_ID").unwrap_or_default();
    ui.set_chat_channel(
        if channel_id.is_empty() {
            "not configured"
        } else {
            channel_id.get(..8).unwrap_or(&channel_id)
        }
        .into(),
    );
    ui.set_chat_status(
        if channel_id.is_empty() {
            "Set CORDEL_CHANNEL_ID to a text channel UUID to enable live chat."
        } else {
            "Sign in to start live chat."
        }
        .into(),
    );

    let (identity_tx, identity_rx) = mpsc::channel();
    let identity_ui = ui.as_weak();
    std::thread::spawn(move || identity::run(identity_rx, identity_ui, account_tx));
    std::thread::spawn({
        let identity_tx = identity_tx.clone();
        move || chat::run(chat_rx, account_rx, chat_ui_tx, identity_tx)
    });
    ui.on_identity_login({
        let tx = identity_tx.clone();
        move |login, password| {
            let _ = tx.send(IdentityCommand::Login {
                login: login.to_string(),
                password: password.to_string(),
            });
        }
    });
    ui.on_identity_check_profile({
        let tx = identity_tx.clone();
        move || {
            let _ = tx.send(IdentityCommand::CheckProfile);
        }
    });
    ui.on_identity_unlock({
        let tx = identity_tx.clone();
        move |password| {
            let _ = tx.send(IdentityCommand::Unlock {
                password: password.to_string(),
            });
        }
    });
    ui.on_identity_lock({
        let tx = identity_tx.clone();
        move || {
            let _ = tx.send(IdentityCommand::Lock);
        }
    });
    ui.on_chat_send({
        let tx = chat_tx.clone();
        move |body| {
            let _ = tx.send(ChatCommand::Send(body.to_string()));
        }
    });
    ui.on_chat_reconnect({
        let tx = chat_tx.clone();
        move || {
            let _ = tx.send(ChatCommand::Reconnect);
        }
    });
    ui.on_identity_logout({
        let media_tx = commands.clone();
        let chat_tx = chat_tx.clone();
        move || {
            let _ = media_tx.send(Command::Leave);
            let _ = chat_tx.send(ChatCommand::SignOut);
            let _ = identity_tx.send(IdentityCommand::Logout);
        }
    });

    ui.run()?;
    Ok(())
}

fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        let Some(rest) = line.strip_prefix("VmRSS:") else {
            continue;
        };
        return rest.split_whitespace().next()?.parse().ok();
    }
    None
}
