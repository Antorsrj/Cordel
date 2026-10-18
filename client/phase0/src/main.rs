mod chat;
mod identity;
mod pixels;
mod screen;
mod session;

use std::rc::Rc;
use std::time::Instant;

use slint::{ComponentHandle, ModelRc, VecModel};

use crate::chat::phase0_messages;
use crate::identity::Command as IdentityCommand;
use crate::screen::CapturePath;
use crate::session::Command;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let ui = AppWindow::new()?;
    let started = Instant::now();
    let lines: Vec<ChatLine> = phase0_messages()
        .into_iter()
        .map(|message| ChatLine {
            author: message.author.into(),
            body: message.body.into(),
        })
        .collect();
    let line_count = lines.len();
    let model = ModelRc::from(Rc::new(VecModel::from(lines)));
    ui.set_messages(model);
    let chat_ms = started.elapsed().as_millis();
    ui.set_status(
        format!(
            "{line_count} chat lines built in {chat_ms} ms. RSS {} KiB. Capture path: {}. Set CORDEL_LIVEKIT_URL and CORDEL_LIVEKIT_TOKEN before joining.",
            rss_kib().unwrap_or(0),
            CapturePath::detect().label(),
        )
        .into(),
    );

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let ui_events = ui.as_weak();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        runtime.block_on(session::run(rx, ui_events));
    });

    ui.on_join({
        let tx = tx.clone();
        move || {
            let _ = tx.send(Command::Join);
        }
    });
    ui.on_capture_frame({
        let tx = tx.clone();
        move || {
            let _ = tx.send(Command::CaptureFrame);
        }
    });
    ui.on_leave({
        let tx = tx.clone();
        move || {
            let _ = tx.send(Command::Leave);
        }
    });

    let (identity_tx, identity_rx) = std::sync::mpsc::channel();
    let identity_ui = ui.as_weak();
    std::thread::spawn(move || identity::run(identity_rx, identity_ui));
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
    ui.on_identity_logout({
        let media_tx = tx.clone();
        move || {
            let _ = media_tx.send(Command::Leave);
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
