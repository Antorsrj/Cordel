//! Opt-in local proof for the chat worker.
//!
//! It needs a running Axum instance and a disposable test account. The
//! variables carry disposable test values only; nothing here reaches a
//! deployed environment.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::session::{run_with, Config};
use super::{AccessSource, AccountEvent, Command, UiEvent};
use crate::identity::Command as IdentityCommand;

const WAIT: Duration = Duration::from_secs(20);

#[test]
#[ignore = "requires a local Axum server and the CORDEL_PHASE0_TEST_* variables"]
fn the_worker_connects_sends_and_receives_a_message() {
    let origin = std::env::var("CORDEL_PHASE0_TEST_ORIGIN").expect("origin required");
    let channel_id = std::env::var("CORDEL_PHASE0_TEST_CHANNEL").expect("channel required");
    let token = std::env::var("CORDEL_PHASE0_TEST_TOKEN").expect("token required");
    let user_id = std::env::var("CORDEL_PHASE0_TEST_USER_ID").expect("user id required");
    let login = std::env::var("CORDEL_PHASE0_TEST_LOGIN").unwrap_or_default();

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

    let body = "phase 0 worker smoke";
    command_tx
        .send(Command::Send(body.to_owned()))
        .expect("the worker owns the command channel");

    let mut received = false;
    while Instant::now() < deadline {
        match ui_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(UiEvent::Message { body: text, .. }) if text == body => {
                received = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(received, "the worker never received its own message");

    // A manual reconnect closes the socket and subscribes again with the same
    // cursor, so the already delivered messages are not repeated.
    command_tx
        .send(Command::Reconnect)
        .expect("the worker owns the command channel");
    let mut reconnected = false;
    while Instant::now() < deadline {
        match ui_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(UiEvent::Connected(true)) => {
                reconnected = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(reconnected, "the worker did not reconnect");

    let second = "phase 0 worker smoke after reconnect";
    command_tx
        .send(Command::Send(second.to_owned()))
        .expect("the worker owns the command channel");
    let mut received_again = false;
    while Instant::now() < deadline {
        match ui_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(UiEvent::Message { body: text, .. }) if text == second => {
                received_again = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(
        received_again,
        "the worker could not send after a reconnect"
    );

    drop(command_tx);
    drop(account_tx);
    worker.join().expect("the worker stopped");
}
