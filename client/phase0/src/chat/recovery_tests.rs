//! Opt-in proof that the chat worker recovers after a network interruption.
//!
//! The client connects to a real Axum instance, signals that it is ready, and
//! then waits for a message that the harness sends after the interruption. The
//! measured time is the time from the interruption to usable chat, which is
//! what the ADR-102 verification asks for.
//!
//! It needs a running Axum instance, a disposable test account, and the
//! CORDEL_PHASE0_TEST_* variables. The harness coordinates the interruption
//! through the ready file named by CORDEL_PHASE0_TEST_READY_FILE.

use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use super::session::{run_with, Config};
use super::UiEvent;
use crate::account::{AccessSource, AccountEvent};
use crate::identity::Command as IdentityCommand;

const WAIT: Duration = Duration::from_secs(90);

#[test]
#[ignore = "requires a local Axum server, a harness that interrupts the network, and the CORDEL_PHASE0_TEST_* variables"]
fn the_worker_recovers_after_a_network_interruption() {
    let origin = std::env::var("CORDEL_PHASE0_TEST_ORIGIN").expect("origin required");
    let channel_id = std::env::var("CORDEL_PHASE0_TEST_CHANNEL").expect("channel required");
    let token = std::env::var("CORDEL_PHASE0_TEST_TOKEN").expect("token required");
    let user_id = std::env::var("CORDEL_PHASE0_TEST_USER_ID").expect("user id required");
    let login = std::env::var("CORDEL_PHASE0_TEST_LOGIN").unwrap_or_default();
    let ready_file =
        std::env::var("CORDEL_PHASE0_TEST_READY_FILE").expect("the ready file path is required");

    let (command_tx, command_rx) = mpsc::channel();
    let (account_tx, account_rx) = mpsc::channel();
    let (ui_tx, ui_rx) = mpsc::channel();
    let (identity_tx, _identity_rx) = mpsc::channel::<IdentityCommand>();
    let worker = std::thread::spawn(move || {
        run_with(
            Config { origin, channel_id },
            command_rx,
            account_rx,
            ui_tx,
            identity_tx,
        )
    });

    account_tx
        .send(AccountEvent::Access {
            token,
            user_id,
            login,
            source: AccessSource::Login,
        })
        .expect("the worker owns the account channel");

    let deadline = Instant::now() + WAIT;

    // The client is connected and subscribed. Tell the harness to interrupt the
    // network now.
    let mut connected = false;
    while Instant::now() < deadline {
        match ui_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(UiEvent::Connected(true)) => {
                connected = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(connected, "the worker never reported a live connection");
    fs::write(&ready_file, b"ready").expect("the harness reads the ready file");

    // The harness writes the moment it cut the network. The recovery is the
    // time from that moment to the message arriving.
    let outage_file = format!("{ready_file}.outage");
    let mut outage = None;
    while Instant::now() < deadline && outage.is_none() {
        outage = fs::read_to_string(&outage_file)
            .ok()
            .and_then(|value| value.trim().parse::<u128>().ok());
        if outage.is_none() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let outage: u128 = outage.expect("the harness did not record the interruption");

    // The harness sends a message once the network is back. Receiving it proves
    // the chat is usable again.
    let mut recovered = false;
    let mut elapsed = Duration::ZERO;
    while Instant::now() < deadline {
        match ui_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(UiEvent::Message { body, .. }) if body == "after the interruption" => {
                recovered = true;
                let now = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .expect("the clock is after the epoch")
                    .as_millis();
                elapsed = Duration::from_millis(now.saturating_sub(outage) as u64);
                break;
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    assert!(
        recovered,
        "the worker never received the message sent after the interruption"
    );
    // The time is measured from the moment the network was cut, so it
    // includes the length of the interruption itself.
    println!(
        "recovery after the interruption: {} ms",
        elapsed.as_millis()
    );
    assert!(
        elapsed < Duration::from_secs(60),
        "the recovery took {} ms",
        elapsed.as_millis()
    );

    let _ = fs::remove_file(&ready_file);
    drop(command_tx);
    drop(account_tx);
    worker.join().expect("the worker stopped");
}
