pub struct ChatMessage {
    pub author: &'static str,
    pub body: String,
}

pub const CHAT_LINE_COUNT: usize = 10_000;

pub fn phase0_messages() -> Vec<ChatMessage> {
    (1..=CHAT_LINE_COUNT)
        .map(|n| ChatMessage {
            author: if n % 2 == 0 { "ada" } else { "kai" },
            body: format!("phase 0 line {n}: readable sample Aa0"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_sample_has_ten_thousand_lines() {
        let lines = phase0_messages();
        assert_eq!(lines.len(), CHAT_LINE_COUNT);
        assert_eq!(lines[0].author, "kai");
        assert!(lines[0].body.contains("line 1"));
        assert_eq!(lines[CHAT_LINE_COUNT - 1].author, "ada");
        assert!(lines[CHAT_LINE_COUNT - 1].body.contains("line 10000"));
    }
}
