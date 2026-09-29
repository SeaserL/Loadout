// SPDX-License-Identifier: Apache-2.0

//! Masking of credential-shaped text, for logs and for upstream error messages that may echo
//! part of a key (ADR-004 §5).

use std::borrow::Cow;

const MASK: &[u8] = b"[REDACTED]";

/// Masks credential-shaped substrings:
/// - `lo-` + ≥16 hex chars (Loadout client keys)
/// - `sk-` + ≥16 key chars (common provider API keys)
/// - `Bearer ` + ≥8 token chars
pub fn redact(input: &[u8]) -> Cow<'_, [u8]> {
    let mut out: Option<Vec<u8>> = None;
    let mut i = 0;
    let mut copied = 0;
    while i < input.len() {
        let rest = &input[i..];
        let hit = if rest.starts_with(b"lo-") {
            run(&rest[3..], |b| b.is_ascii_hexdigit())
                .filter(|&n| n >= 16)
                .map(|n| (3, n))
        } else if rest.starts_with(b"sk-") {
            run(&rest[3..], is_key_char)
                .filter(|&n| n >= 16)
                .map(|n| (3, n))
        } else if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case(b"bearer ") {
            run(&rest[7..], is_token_char)
                .filter(|&n| n >= 8)
                .map(|n| (7, n))
        } else {
            None
        };
        match hit {
            Some((keep, secret_len)) => {
                let buf = out.get_or_insert_with(|| Vec::with_capacity(input.len()));
                buf.extend_from_slice(&input[copied..i + keep]);
                buf.extend_from_slice(MASK);
                i += keep + secret_len;
                copied = i;
            }
            None => i += 1,
        }
    }
    match out {
        None => Cow::Borrowed(input),
        Some(mut buf) => {
            buf.extend_from_slice(&input[copied..]);
            Cow::Owned(buf)
        }
    }
}

/// [`redact`] for strings.
pub fn redact_str(input: &str) -> Cow<'_, str> {
    match redact(input.as_bytes()) {
        Cow::Borrowed(_) => Cow::Borrowed(input),
        // Masking only replaces ASCII runs with ASCII, so UTF-8 stays valid.
        Cow::Owned(bytes) => Cow::Owned(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

fn run(bytes: &[u8], accept: impl Fn(u8) -> bool) -> Option<usize> {
    Some(bytes.iter().take_while(|&&b| accept(b)).count())
}

fn is_key_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

fn is_token_char(b: u8) -> bool {
    !b.is_ascii_whitespace() && b != b'"' && b != b'\'' && b != b','
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(s: &str) -> String {
        redact_str(s).into_owned()
    }

    #[test]
    fn masks_credentials() {
        let client = format!("lo-{}", "ab12".repeat(16));
        assert_eq!(r(&format!("key={client} ok")), "key=lo-[REDACTED] ok");
        assert_eq!(
            r("upstream key sk-proj-AbCdEf0123456789xyz rejected"),
            "upstream key sk-[REDACTED] rejected"
        );
        assert_eq!(
            r(r#"header="Bearer abc.def-123456" next"#),
            r#"header="Bearer [REDACTED]" next"#
        );
        assert_eq!(
            r("authorization: bearer tok_1234567890"),
            "authorization: bearer [REDACTED]"
        );
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for s in [
            "loadout-core started",
            "lo-abc too short",
            "risk-free",
            "sk-short",
            "Bearer x",
            "gateway listening on 127.0.0.1:8317",
            "",
        ] {
            assert!(matches!(redact_str(s), Cow::Borrowed(_)), "{s}");
        }
    }

    #[test]
    fn masks_multiple_occurrences_and_keeps_unicode() {
        let k = format!("lo-{}", "f".repeat(64));
        assert_eq!(r(&format!("{k} 和 {k}")), "lo-[REDACTED] 和 lo-[REDACTED]");
    }
}
