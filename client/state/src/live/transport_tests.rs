use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use cordel_protocol::WsFrame;
use tungstenite::handshake::server::{Request, Response};
use tungstenite::Message;

use super::{
    ChatTransport, LiveTransport, LiveTransportError, NativeChatTransport, NativeLiveTransport,
};

const CHANNEL: &str = "7efc7a7b-b06f-4632-8780-bb611b8cd0a8";

fn listener() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn read_http_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut data = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        if let Some(position) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "request ended before headers");
        data.extend_from_slice(&buffer[..read]);
    };
    let headers = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or(0);
    while data.len() < header_end + length {
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0, "request ended before body");
        data.extend_from_slice(&buffer[..read]);
    }
    (headers, data[header_end..header_end + length].to_vec())
}

#[test]
#[allow(clippy::result_large_err)]
fn native_live_transport_round_trips_frames_with_bearer() {
    let (listener, port) = listener();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut authorization = None;
        let mut socket =
            tungstenite::accept_hdr(stream, |request: &Request, response: Response| {
                authorization = request
                    .headers()
                    .get("authorization")
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                Ok(response)
            })
            .unwrap();
        let ready = WsFrame::ready("5aee793e-3cde-44fc-9608-48b7e945fb53");
        socket
            .send(Message::Text(serde_json::to_string(&ready).unwrap().into()))
            .unwrap();
        let received = socket.read().unwrap();
        let frame = match received {
            Message::Text(text) => serde_json::from_str::<WsFrame>(text.as_str()).unwrap(),
            other => panic!("expected a text frame, got {other:?}"),
        };
        let _ = socket.close(None);
        (authorization, frame)
    });

    let url = format!("ws://127.0.0.1:{port}/v1/events");
    let mut transport = NativeLiveTransport::new_plain(&url).unwrap();
    transport.connect(&url, "test-token").unwrap();
    assert_eq!(transport.receive().unwrap().kind, "ready");
    transport.send(&WsFrame::subscribe(CHANNEL)).unwrap();
    transport.close();

    let (authorization, received) = server.join().unwrap();
    assert_eq!(authorization.as_deref(), Some("Bearer test-token"));
    assert_eq!(received.kind, "subscribe");
}

#[test]
fn native_live_transport_resumes_after_a_read_timeout() {
    let (listener, port) = listener();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut socket = tungstenite::accept(stream).unwrap();
        let ready = WsFrame::ready("5aee793e-3cde-44fc-9608-48b7e945fb53");
        socket
            .send(Message::Text(serde_json::to_string(&ready).unwrap().into()))
            .unwrap();
        let _ = socket.read().unwrap();
        thread::sleep(Duration::from_millis(150));
        let message = WsFrame::message(&cordel_protocol::ChatEvent {
            channel_id: CHANNEL.to_owned(),
            seq: 1,
            kind: "message".to_owned(),
            client_message_id: None,
            body: Some("after timeout".to_owned()),
            extra: Default::default(),
        })
        .unwrap();
        socket
            .send(Message::Text(
                serde_json::to_string(&message).unwrap().into(),
            ))
            .unwrap();
        let _ = socket.close(None);
    });

    let url = format!("ws://127.0.0.1:{port}/v1/events");
    let mut transport = NativeLiveTransport::new_plain(&url).unwrap();
    transport.connect(&url, "test-token").unwrap();
    assert_eq!(transport.receive().unwrap().kind, "ready");
    transport.send(&WsFrame::subscribe(CHANNEL)).unwrap();
    transport
        .set_read_timeout(Duration::from_millis(50))
        .unwrap();
    assert_eq!(transport.receive(), Err(LiveTransportError::Timeout));
    transport
        .set_read_timeout(Duration::from_millis(500))
        .unwrap();
    let frame = transport.receive().unwrap();
    assert_eq!(frame.kind, "message");
    transport.close();
    server.join().unwrap();
}

#[test]
fn native_chat_transport_parses_the_history_fixture() {
    let fixture = include_str!("../../../../docs/contracts/fixtures/native/rest_history_page.json");
    let (listener, port) = listener();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (headers, _) = read_http_request(&mut stream);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            fixture.len(),
            fixture
        );
        stream.write_all(response.as_bytes()).unwrap();
        headers
    });

    let client = NativeChatTransport::new_plain(&format!("http://127.0.0.1:{port}/")).unwrap();
    let page = client.history("test-token", CHANNEL, 41, 200).unwrap();
    assert_eq!(page.channel_id, CHANNEL);
    assert_eq!(page.messages[0].seq, 42);
    assert_eq!(page.messages[0].body.as_deref(), Some("Hello"));
    assert!(!page.has_more);

    let headers = server.join().unwrap();
    assert!(headers.starts_with(&format!("GET /v1/channels/{CHANNEL}/messages?")));
    assert!(headers.contains("after_seq=41"));
    assert!(headers.contains("limit=200"));
    assert!(headers
        .to_ascii_lowercase()
        .contains("authorization: bearer test-token"));
}

#[test]
fn native_chat_transport_posts_with_bearer_and_parses_ack() {
    let fixture = include_str!("../../../../docs/contracts/fixtures/native/rest_ack.json");
    let (listener, port) = listener();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let (headers, body) = read_http_request(&mut stream);
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            fixture.len(),
            fixture
        );
        stream.write_all(response.as_bytes()).unwrap();
        (headers, body)
    });

    let client = NativeChatTransport::new_plain(&format!("http://127.0.0.1:{port}/")).unwrap();
    let ack = client
        .send("test-token", CHANNEL, "client-1", "hello")
        .unwrap();
    assert_eq!(ack.seq, 42);
    assert_eq!(
        ack.client_message_id,
        "567e491b-4c30-47ec-9fc0-6744c12a6e87"
    );

    let (headers, body) = server.join().unwrap();
    assert!(headers.starts_with(&format!("POST /v1/channels/{CHANNEL}/messages ")));
    assert!(headers
        .to_ascii_lowercase()
        .contains("authorization: bearer test-token"));
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("\"client_message_id\":\"client-1\""));
    assert!(body.contains("\"body\":\"hello\""));
}
