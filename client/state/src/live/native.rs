use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

use cordel_protocol::{ChatEvent, HistoryPage, MessageAck, WsFrame};
use serde::Deserialize;
use tungstenite::client::{connect_with_config, IntoClientRequest};
use tungstenite::http::header::AUTHORIZATION;
use tungstenite::http::StatusCode;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};
use uuid::Uuid;

use super::{ChatTransport, ChatTransportError, LiveError, LiveTransport, LiveTransportError};

const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

pub struct NativeLiveTransport {
    endpoint: String,
    socket: Option<WebSocket<MaybeTlsStream<TcpStream>>>,
}

impl NativeLiveTransport {
    /// A production endpoint must be an absolute `wss://.../v1/events` URL.
    pub fn new(endpoint: &str) -> Result<Self, LiveError> {
        Self::with_scheme(endpoint, "wss")
    }

    /// Plain `ws` endpoint for local integration tests. Enable the
    /// `local-smoke` feature; production uses `new`.
    #[cfg(any(test, feature = "local-smoke"))]
    pub fn new_plain(endpoint: &str) -> Result<Self, LiveError> {
        Self::with_scheme(endpoint, "ws")
    }

    fn with_scheme(endpoint: &str, scheme: &str) -> Result<Self, LiveError> {
        let uri: tungstenite::http::Uri =
            endpoint.parse().map_err(|_| LiveError::InvalidEndpoint)?;
        if uri.scheme_str() != Some(scheme)
            || uri.host().is_none()
            || uri.path() != "/v1/events"
            || uri.query().is_some()
            || uri
                .authority()
                .is_some_and(|authority| authority.as_str().contains('@'))
        {
            return Err(LiveError::InvalidEndpoint);
        }
        Ok(Self {
            endpoint: endpoint.to_owned(),
            socket: None,
        })
    }

    fn close_socket(&mut self) {
        if let Some(mut socket) = self.socket.take() {
            let _ = socket.close(None);
            let _ = socket.flush();
        }
    }
}

impl Drop for NativeLiveTransport {
    fn drop(&mut self) {
        self.close_socket();
    }
}

impl LiveTransport for NativeLiveTransport {
    fn connect(&mut self, endpoint: &str, token: &str) -> Result<(), LiveTransportError> {
        if endpoint != self.endpoint {
            return Err(LiveTransportError::Network);
        }
        self.close_socket();
        let mut request = self
            .endpoint
            .as_str()
            .into_client_request()
            .map_err(|_| LiveTransportError::Network)?;
        request.headers_mut().insert(
            AUTHORIZATION,
            format!("Bearer {token}")
                .parse()
                .map_err(|_| LiveTransportError::Network)?,
        );
        let (socket, response) =
            connect_with_config(request, None, 0).map_err(|_| LiveTransportError::Network)?;
        if response.status() != StatusCode::SWITCHING_PROTOCOLS {
            return Err(LiveTransportError::Network);
        }
        set_timeouts(socket.get_ref());
        self.socket = Some(socket);
        Ok(())
    }

    fn send(&mut self, frame: &WsFrame) -> Result<(), LiveTransportError> {
        let socket = self.socket.as_mut().ok_or(LiveTransportError::Closed)?;
        let text = serde_json::to_string(frame).map_err(|_| LiveTransportError::MalformedFrame)?;
        socket
            .send(Message::Text(text.into()))
            .map_err(|_| LiveTransportError::Network)?;
        socket.flush().map_err(|_| LiveTransportError::Network)
    }

    fn receive(&mut self) -> Result<WsFrame, LiveTransportError> {
        let socket = self.socket.as_mut().ok_or(LiveTransportError::Closed)?;
        loop {
            let message = socket.read().map_err(|_| LiveTransportError::Network)?;
            match message {
                Message::Text(text) => {
                    return serde_json::from_str(text.as_str())
                        .map_err(|_| LiveTransportError::MalformedFrame)
                }
                Message::Ping(payload) => {
                    socket
                        .send(Message::Pong(payload))
                        .map_err(|_| LiveTransportError::Network)?;
                    socket.flush().map_err(|_| LiveTransportError::Network)?;
                }
                Message::Pong(_) => {}
                Message::Close(_) => return Err(LiveTransportError::Closed),
                Message::Binary(_) => return Err(LiveTransportError::MalformedFrame),
                Message::Frame(_) => {}
            }
        }
    }

    fn close(&mut self) {
        self.close_socket();
    }
}

fn set_timeouts(stream: &MaybeTlsStream<TcpStream>) {
    let timeout = Some(Duration::from_secs(60));
    match stream {
        MaybeTlsStream::Plain(socket) => {
            let _ = socket.set_read_timeout(timeout);
            let _ = socket.set_write_timeout(timeout);
        }
        MaybeTlsStream::Rustls(socket) => {
            let _ = socket.sock.set_read_timeout(timeout);
            let _ = socket.sock.set_write_timeout(timeout);
        }
        _ => {}
    }
}

pub struct NativeChatTransport {
    client: reqwest::blocking::Client,
    base: reqwest::Url,
}

impl NativeChatTransport {
    /// A production base must be a bare HTTPS origin ending in `/`.
    pub fn new(base: &str) -> Result<Self, LiveError> {
        Self::with_scheme(base, "https")
    }

    /// Plain `http` base for local integration tests. Enable the
    /// `local-smoke` feature; production uses `new`.
    #[cfg(any(test, feature = "local-smoke"))]
    pub fn new_plain(base: &str) -> Result<Self, LiveError> {
        Self::with_scheme(base, "http")
    }

    fn with_scheme(base: &str, scheme: &str) -> Result<Self, LiveError> {
        let base = reqwest::Url::parse(base).map_err(|_| LiveError::InvalidEndpoint)?;
        if base.scheme() != scheme
            || base.host_str().is_none()
            || !base.username().is_empty()
            || base.password().is_some()
            || base.query().is_some()
            || base.fragment().is_some()
            || base.path() != "/"
        {
            return Err(LiveError::InvalidEndpoint);
        }
        let mut builder = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10));
        if scheme == "https" {
            builder = builder.https_only(true);
        }
        let client = builder.build().map_err(|_| LiveError::InvalidEndpoint)?;
        Ok(Self { client, base })
    }

    fn url(&self, channel_id: &str) -> Result<reqwest::Url, ChatTransportError> {
        let channel_id =
            Uuid::parse_str(channel_id).map_err(|_| ChatTransportError::InvalidResponse)?;
        self.base
            .join(&format!("v1/channels/{channel_id}/messages"))
            .map_err(|_| ChatTransportError::Network)
    }
}

impl ChatTransport for NativeChatTransport {
    fn history(
        &self,
        bearer: &str,
        channel_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> Result<HistoryPage, ChatTransportError> {
        let response = self
            .client
            .get(self.url(channel_id)?)
            .query(&[
                ("after_seq", after_seq.to_string()),
                ("limit", limit.to_string()),
            ])
            .bearer_auth(bearer)
            .send()
            .map_err(|_| ChatTransportError::Network)?;
        let status = response.status().as_u16();
        let body = read_body(response)?;
        match status {
            200..=299 => {}
            401 => return Err(ChatTransportError::Unauthorized),
            403 => return Err(ChatTransportError::Forbidden),
            _ => return Err(api_error(status, &body)),
        }
        let payload: HistoryPayload =
            serde_json::from_slice(&body).map_err(|_| ChatTransportError::InvalidResponse)?;
        if payload.schema_version != 1 {
            return Err(ChatTransportError::InvalidResponse);
        }
        Ok(HistoryPage {
            channel_id: payload.channel_id,
            messages: payload.messages,
            has_more: payload.has_more,
        })
    }

    fn send(
        &self,
        bearer: &str,
        channel_id: &str,
        client_message_id: &str,
        body: &str,
    ) -> Result<MessageAck, ChatTransportError> {
        let response = self
            .client
            .post(self.url(channel_id)?)
            .bearer_auth(bearer)
            .json(&serde_json::json!({
                "client_message_id": client_message_id,
                "body": body,
            }))
            .send()
            .map_err(|_| ChatTransportError::Network)?;
        let status = response.status().as_u16();
        let body = read_body(response)?;
        match status {
            200..=299 => {}
            401 => return Err(ChatTransportError::Unauthorized),
            403 => return Err(ChatTransportError::Forbidden),
            _ => return Err(api_error(status, &body)),
        }
        let payload: AckPayload =
            serde_json::from_slice(&body).map_err(|_| ChatTransportError::InvalidResponse)?;
        if payload.schema_version != 1 {
            return Err(ChatTransportError::InvalidResponse);
        }
        Ok(MessageAck {
            channel_id: payload.channel_id,
            client_message_id: payload.client_message_id,
            seq: payload.seq,
        })
    }
}

#[derive(Deserialize)]
struct HistoryPayload {
    schema_version: u8,
    channel_id: String,
    messages: Vec<ChatEvent>,
    has_more: bool,
}

#[derive(Deserialize)]
struct AckPayload {
    schema_version: u8,
    channel_id: String,
    client_message_id: String,
    seq: u64,
}

fn read_body(response: reqwest::blocking::Response) -> Result<Vec<u8>, ChatTransportError> {
    let mut body = Vec::new();
    response
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut body)
        .map_err(|_| ChatTransportError::Network)?;
    if body.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(ChatTransportError::InvalidResponse);
    }
    Ok(body)
}

fn api_error(status: u16, body: &[u8]) -> ChatTransportError {
    let code = serde_json::from_slice::<cordel_protocol::ApiError>(body)
        .ok()
        .filter(|error| error.schema_version == 1)
        .map(|error| error.error.code.as_str().to_owned())
        .unwrap_or_else(|| "invalid_response".to_owned());
    ChatTransportError::Api { status, code }
}
