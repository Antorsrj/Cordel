//! Opt-in local proof for the chat worker.
//!
//! It needs a running Axum instance and a disposable test account. The
//! variables carry disposable test values only; nothing here reaches a
//! deployed environment.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use cordel_state::{ChatTransport, NativeChatTransport};

use super::session::{run_with, Config};
use super::{Command, UiEvent};
use crate::account::{AccessSource, AccountEvent};
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

#[test]
#[ignore = "requires a local Axum server and the CORDEL_PHASE0_TEST_* variables"]
fn two_clients_exchange_messages_over_the_live_session() {
    let origin = std::env::var("CORDEL_PHASE0_TEST_ORIGIN").expect("origin required");
    let channel_id = std::env::var("CORDEL_PHASE0_TEST_CHANNEL").expect("channel required");
    let token = std::env::var("CORDEL_PHASE0_TEST_TOKEN").expect("token required");
    let user_id = std::env::var("CORDEL_PHASE0_TEST_USER_ID").expect("user id required");
    let login = std::env::var("CORDEL_PHASE0_TEST_LOGIN").unwrap_or_default();
    let peer_token = std::env::var("CORDEL_PHASE0_TEST_PEER_TOKEN").expect("peer token required");

    let (command_tx, command_rx) = mpsc::channel();
    let (account_tx, account_rx) = mpsc::channel();
    let (ui_tx, ui_rx) = mpsc::channel();
    let (identity_tx, _identity_rx) = mpsc::channel::<IdentityCommand>();
    let worker = std::thread::spawn({
        let origin = origin.clone();
        let channel_id = channel_id.clone();
        move || {
            run_with(
                Config { origin, channel_id },
                command_rx,
                account_rx,
                ui_tx,
                identity_tx,
            )
        }
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

    // The second client posts through the same REST route the native client
    // uses. The first client must see it on the live socket. The channel may
    // already hold messages from an earlier run, so only the order matters.
    let peer = NativeChatTransport::new_plain(&format!("{origin}/")).expect("peer transport");
    let peer_body = "hello from the second client";
    let ack = peer
        .send(&peer_token, &channel_id, "peer-client-message-1", peer_body)
        .expect("the second client could not send");
    assert!(ack.seq >= 1, "the server did not allocate a sequence");

    let mut received = false;
    while Instant::now() < deadline {
        match ui_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(UiEvent::Message { body: text, .. }) if text == peer_body => {
                received = true;
                break;
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(
        received,
        "the first client never received the second client's message"
    );

    // The other direction: the first client sends and the second client reads
    // the committed message from history.
    let body = "hello from the first client";
    command_tx
        .send(Command::Send(body.to_owned()))
        .expect("the worker owns the command channel");

    let mut committed = false;
    while Instant::now() < deadline {
        let page = peer
            .history(&peer_token, &channel_id, 0, 50)
            .expect("the second client could not read history");
        if page
            .messages
            .iter()
            .any(|message| message.body.as_deref() == Some(body))
        {
            committed = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(
        committed,
        "the second client never saw the first client's message"
    );

    drop(command_tx);
    drop(account_tx);
    worker.join().expect("the worker stopped");
}
