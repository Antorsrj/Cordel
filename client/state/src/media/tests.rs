use std::cell::RefCell;

use cordel_protocol::MediaAdmission;

use super::*;

struct FakeTransport {
    status: u16,
    body: String,
    path: RefCell<String>,
    bearer: RefCell<String>,
}

impl FakeTransport {
    fn new(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_owned(),
            path: RefCell::new(String::new()),
            bearer: RefCell::new(String::new()),
        }
    }
}

impl AdmissionTransport for FakeTransport {
    fn post(&self, path: &str, bearer: &str, _body: &[u8]) -> Result<HttpReply, TransportFailure> {
        *self.path.borrow_mut() = path.to_owned();
        *self.bearer.borrow_mut() = bearer.to_owned();
        Ok(HttpReply {
            status: self.status,
            body: self.body.clone().into_bytes(),
        })
    }
}

fn admission_body() -> String {
    r#"{"schema_version":1,"voice_channel_id":"0700eaa6-8204-4f0b-a45e-57d929289818","room":"voice:0700eaa6-8204-4f0b-a45e-57d929289818:3","media_generation":3,"url":"wss://media.example","token":"room-token","expires_at":"2026-10-21T12:05:00Z","expires_in_secs":300}"#
        .to_owned()
}

#[test]
fn an_admission_names_one_channel_one_room_and_one_generation() {
    let transport = FakeTransport::new(200, &admission_body());
    let client = MediaAdmissionClient::new(&transport);
    let admission: MediaAdmission = client
        .admission("access-token", "0700eaa6-8204-4f0b-a45e-57d929289818")
        .expect("the server returned an admission");
    assert_eq!(admission.schema_version, 1);
    assert_eq!(
        admission.voice_channel_id,
        "0700eaa6-8204-4f0b-a45e-57d929289818"
    );
    assert_eq!(
        admission.room,
        "voice:0700eaa6-8204-4f0b-a45e-57d929289818:3"
    );
    assert_eq!(admission.media_generation, 3);
    assert_eq!(admission.url, "wss://media.example");
    assert_eq!(admission.token, "room-token");
    assert_eq!(admission.expires_in_secs, 300);
    assert_eq!(
        *transport.path.borrow(),
        "/v1/voice-channels/0700eaa6-8204-4f0b-a45e-57d929289818/admission"
    );
    assert_eq!(*transport.bearer.borrow(), "access-token");
}

#[test]
fn a_refused_admission_names_the_reason() {
    for (status, expected) in [
        (401u16, AdmissionError::Unauthorized),
        (403, AdmissionError::Forbidden),
        (404, AdmissionError::NotFound),
        (500, AdmissionError::InvalidResponse),
    ] {
        let transport = FakeTransport::new(status, "{}");
        let client = MediaAdmissionClient::new(&transport);
        assert_eq!(
            client.admission("access-token", "channel").unwrap_err(),
            expected,
            "status {status} must map to a stable reason"
        );
    }
}

#[test]
fn a_response_without_the_agreed_fields_is_refused() {
    for body in [
        r#"{"schema_version":2,"voice_channel_id":"c","room":"r","media_generation":1,"url":"u","token":"t","expires_at":"e","expires_in_secs":1}"#,
        r#"{"schema_version":1,"room":"r","media_generation":1,"url":"u","token":"t","expires_at":"e","expires_in_secs":1}"#,
        r#"{"schema_version":1,"voice_channel_id":"c","room":"r","media_generation":"one","url":"u","token":"t","expires_at":"e","expires_in_secs":1}"#,
        r#"not json"#,
    ] {
        let transport = FakeTransport::new(200, body);
        let client = MediaAdmissionClient::new(&transport);
        assert_eq!(
            client.admission("access-token", "channel").unwrap_err(),
            AdmissionError::InvalidResponse,
            "an incomplete admission must not be used"
        );
    }
}
