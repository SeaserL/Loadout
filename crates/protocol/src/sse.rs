// SPDX-License-Identifier: Apache-2.0

//! Incremental Server-Sent Events parsing (WHATWG rules: `\n`, `\r\n` and `\r` line endings,
//! comments, multi-line `data`) and serialization.
//!
//! Each parsed event keeps its exact raw bytes so the passthrough path can forward upstream
//! bytes unchanged while still observing (or dropping) individual events.

use bytes::{Bytes, BytesMut};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    /// The exact bytes of this event, including any comment lines before it and the blank
    /// line that terminated it.
    pub raw: Bytes,
}

/// Feeds arbitrary byte chunks, yields complete events.
#[derive(Debug, Default)]
pub struct SseParser {
    buf: BytesMut,
    /// Offset into `buf` where the next unparsed line starts.
    pos: usize,
    event: Option<String>,
    data: String,
    has_data: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some((line_end, next)) = find_line(&self.buf[self.pos..], false) {
            let line_start = self.pos;
            let line = self.buf[line_start..line_start + line_end].to_vec();
            self.pos += next;
            if let Some(event) = self.process_line(&line) {
                out.push(event);
            }
        }
        out
    }

    /// Flushes a final event not followed by a blank line (lenient toward upstreams that
    /// close the connection early).
    pub fn finish(&mut self) -> Option<SseEvent> {
        if self.pos < self.buf.len() {
            let rest = self.buf[self.pos..].to_vec();
            let trimmed = rest.strip_suffix(b"\r").unwrap_or(&rest).to_vec();
            self.pos = self.buf.len();
            if let Some(event) = self.process_line(&trimmed) {
                return Some(event);
            }
        }
        self.process_line(b"")
    }

    fn process_line(&mut self, line: &[u8]) -> Option<SseEvent> {
        if line.is_empty() {
            let event = self.event.take();
            if !self.has_data {
                // Nothing to dispatch (comment or empty block): keep its bytes so they are
                // forwarded together with the next event.
                self.data.clear();
                return None;
            }
            let raw = self.buf.split_to(self.pos).freeze();
            self.pos = 0;
            self.has_data = false;
            let data = std::mem::take(&mut self.data);
            return Some(SseEvent { event, data, raw });
        }
        if line[0] == b':' {
            return None;
        }
        let (field, value) = match line.iter().position(|&b| b == b':') {
            Some(i) => {
                let v = &line[i + 1..];
                (&line[..i], v.strip_prefix(b" ").unwrap_or(v))
            }
            None => (line, &b""[..]),
        };
        let value = String::from_utf8_lossy(value);
        match field {
            b"data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(&value);
                self.has_data = true;
            }
            b"event" => self.event = Some(value.into_owned()),
            _ => {}
        }
        None
    }
}

/// Finds the first line terminator. Returns (line length, bytes to consume). A trailing `\r`
/// is not treated as complete because a `\n` may follow in the next chunk.
fn find_line(buf: &[u8], at_eof: bool) -> Option<(usize, usize)> {
    for (i, &b) in buf.iter().enumerate() {
        match b {
            b'\n' => return Some((i, i + 1)),
            b'\r' => {
                return match buf.get(i + 1) {
                    Some(b'\n') => Some((i, i + 2)),
                    Some(_) => Some((i, i + 1)),
                    None if at_eof => Some((i, i + 1)),
                    None => None,
                };
            }
            _ => {}
        }
    }
    None
}

/// Serializes one event. Multi-line data is split into several `data:` lines.
pub fn encode(event: Option<&str>, data: &str) -> Bytes {
    let mut out = String::with_capacity(data.len() + 32);
    if let Some(name) = event {
        out.push_str("event: ");
        out.push_str(name);
        out.push('\n');
    }
    for line in data.split('\n') {
        out.push_str("data: ");
        out.push_str(line);
        out.push('\n');
    }
    out.push('\n');
    Bytes::from(out)
}

/// Serializes a JSON value as an event.
pub fn encode_json(event: Option<&str>, value: &serde_json::Value) -> Bytes {
    encode(event, &value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(p.feed(c));
        }
        out.extend(p.finish());
        out
    }

    #[test]
    fn parses_events_across_chunk_boundaries() {
        let events = all(&[b"event: a\nda", b"ta: 1\n", b"\ndata: 2\n\n"]);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("a"));
        assert_eq!(events[0].data, "1");
        assert_eq!(events[0].raw, Bytes::from_static(b"event: a\ndata: 1\n\n"));
        assert_eq!(events[1].event, None);
        assert_eq!(events[1].data, "2");
    }

    #[test]
    fn handles_crlf_and_cr_line_endings_split_mid_terminator() {
        let events = all(&[b"data: x\r", b"\n\r\n", b"data: y\r\r"]);
        assert_eq!(
            events.iter().map(|e| e.data.as_str()).collect::<Vec<_>>(),
            vec!["x", "y"]
        );
    }

    #[test]
    fn joins_multiline_data_and_ignores_comments() {
        let events = all(&[b": keep-alive\n\ndata: a\ndata: b\nid: 7\nretry: 5\n\n"]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "a\nb");
        // The comment-only block produced no event but its bytes stay with the next one.
        assert!(events[0].raw.starts_with(b": keep-alive"));
    }

    #[test]
    fn field_without_colon_and_value_without_space() {
        let events = all(&[b"data\n\ndata:tight\n\n"]);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "");
        assert_eq!(events[1].data, "tight");
    }

    #[test]
    fn flushes_unterminated_final_event() {
        let events = all(&[b"data: [DONE]"]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "[DONE]");
    }

    #[test]
    fn raw_bytes_reassemble_the_stream() {
        let input: &[u8] = b"event: x\r\ndata: 1\r\n\r\n: c\n\ndata: 2\n\n";
        let mut p = SseParser::new();
        let mut rebuilt = Vec::new();
        for byte in input.chunks(3) {
            for e in p.feed(byte) {
                rebuilt.extend_from_slice(&e.raw);
            }
        }
        assert_eq!(rebuilt, input);
    }

    #[test]
    fn encode_round_trips() {
        let bytes = encode(Some("message_start"), "{\"a\":1}\nsecond");
        let events = all(&[&bytes]);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[0].data, "{\"a\":1}\nsecond");
    }
}
