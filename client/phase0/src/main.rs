mod chat;
mod session;

use std::rc::Rc;
use std::time::Instant;

use cordel_media::capture::SessionKind;
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::chat::phase0_messages;
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
            SessionKind::detect()
                .map(SessionKind::as_str)
                .unwrap_or("unknown"),
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
