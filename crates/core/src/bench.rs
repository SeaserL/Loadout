// SPDX-License-Identifier: Apache-2.0

//! Active latency probes (ExecPlan 004 §3.7, Harness §15): a tiny fixed prompt sent through
//! a route's own endpoint and connection pool. Probes never carry user data.

use std::time::{Duration, Instant};

use futures_util::StreamExt;
use loadout_protocol::sse::SseParser;
use loadout_protocol::{
    CanonicalEvent, CanonicalRequest, Message, Part, Protocol, Role, chat, decoder_for, messages,
    responses,
};
use loadout_provider_sdk::{FailureKind, HttpClient, UpstreamFailure};
use loadout_router::{RouteTarget, Sample, SampleSource, now_ms};
use reqwest::header::{self, HeaderValue};

const PROBE_PROMPT: &str = "Reply with exactly: OK";
const PROBE_MAX_TOKENS: u32 = 16;
const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(60);
const TOTAL_TIMEOUT: Duration = Duration::from_secs(90);
const ERROR_BODY_LIMIT: usize = 16 * 1024;

/// How a probe's response stream ended (ExecPlan 014).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamHealth {
    /// A finish event or the protocol's end marker arrived.
    Clean,
    /// Events arrived, but the stream ended without a finish event.
    Truncated,
    /// An event could not be decoded.
    Malformed,
    /// A success status without any stream event.
    Empty,
}

impl StreamHealth {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Truncated => "truncated",
            Self::Malformed => "malformed",
            Self::Empty => "empty",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProbeOutcome {
    pub ts_ms: i64,
    pub ok: bool,
    pub ttft_ms: Option<u64>,
    pub duration_ms: u64,
    pub output_tokens: Option<u64>,
    pub error: Option<String>,
    /// `None` when no stream was received (HTTP error, timeout before the response).
    pub stream: Option<StreamHealth>,
}

impl ProbeOutcome {
    /// The latency sample this probe contributes, if it produced output.
    pub fn sample(&self) -> Option<Sample> {
        if !self.ok {
            return None;
        }
        Some(Sample {
            ts_ms: self.ts_ms,
            source: SampleSource::Probe,
            ttft_ms: u32::try_from(self.ttft_ms?).unwrap_or(u32::MAX),
            // One or two tokens say nothing about throughput.
            tps: None,
        })
    }
}

pub(crate) fn probe_request() -> CanonicalRequest {
    CanonicalRequest {
        messages: vec![Message::new(
            Role::User,
            vec![Part::Text(PROBE_PROMPT.to_owned())],
        )],
        max_output_tokens: Some(PROBE_MAX_TOKENS),
        stream: true,
        ..CanonicalRequest::default()
    }
}

fn encode(target: &RouteTarget) -> serde_json::Value {
    let c = probe_request();
    let model = &target.upstream_model;
    match target.protocol {
        Protocol::Chat => {
            chat::encode_request(&c, model, target.module.chat_options(&target.config))
        }
        Protocol::Messages => {
            messages::encode_request(&c, model, target.module.messages_options(&target.config))
        }
        Protocol::Responses => responses::encode_request(&c, model),
    }
}

/// Sends one probe and updates the target's circuit breaker with the result.
pub(crate) async fn probe(http: &HttpClient, target: &RouteTarget) -> ProbeOutcome {
    let ts_ms = now_ms();
    let started = Instant::now();
    let mut stream = None;
    let result = tokio::time::timeout(TOTAL_TIMEOUT, run(http, target, started, &mut stream)).await;
    let duration_ms = started.elapsed().as_millis() as u64;
    let outcome = |ok, ttft_ms, output_tokens, error| ProbeOutcome {
        ts_ms,
        ok,
        ttft_ms,
        duration_ms,
        output_tokens,
        error,
        stream,
    };
    match result {
        Ok(Ok(Measured {
            ttft_ms: Some(ttft),
            output_tokens,
        })) => {
            target.health.record_success(now_ms());
            outcome(true, Some(ttft), output_tokens, None)
        }
        Ok(Ok(Measured { output_tokens, .. })) => {
            // The upstream works but gave no visible output within the token budget (e.g. a
            // reasoning model): not a routing failure, just no measurement.
            target.health.record_success(now_ms());
            let error = if stream == Some(StreamHealth::Empty) {
                "empty response stream".to_owned()
            } else {
                format!("no output within {PROBE_MAX_TOKENS} tokens")
            };
            outcome(false, None, output_tokens, Some(error))
        }
        Ok(Err(failure)) => {
            target
                .health
                .record_failure(failure.kind, &failure.message, now_ms());
            outcome(
                false,
                None,
                None,
                Some(format!("{}: {}", failure.kind.code(), failure)),
            )
        }
        Err(_) => {
            target
                .health
                .record_failure(FailureKind::Timeout, "probe timed out", now_ms());
            outcome(false, None, None, Some("timed out".to_owned()))
        }
    }
}

struct Measured {
    ttft_ms: Option<u64>,
    output_tokens: Option<u64>,
}

async fn run(
    http: &HttpClient,
    target: &RouteTarget,
    started: Instant,
    health: &mut Option<StreamHealth>,
) -> Result<Measured, UpstreamFailure> {
    let body = serde_json::to_vec(&encode(target))
        .map_err(|e| UpstreamFailure::new(FailureKind::Other, None, e.to_string()))?;
    let endpoint = &target.endpoint;
    let mut headers = endpoint.headers.clone();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(
        header::ACCEPT,
        HeaderValue::from_static("text/event-stream"),
    );
    // A probe is not a conversation: a fresh id each time (ADR-013 §3).
    if let Some(name) = target.module.session_header()
        && let Ok(v) = HeaderValue::from_str(&uuid::Uuid::new_v4().to_string())
    {
        headers.insert(name, v);
    }
    let deadline = tokio::time::Instant::now() + FIRST_TOKEN_TIMEOUT;
    let send = http
        .for_url(&endpoint.url)
        .post(&endpoint.url)
        .headers(headers)
        .body(body)
        .send();
    let resp = tokio::time::timeout_at(deadline, send)
        .await
        .map_err(|_| UpstreamFailure::new(FailureKind::Timeout, None, "no response in time"))?
        .map_err(|e| UpstreamFailure::from_reqwest(&e))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        let headers = resp.headers().clone();
        let mut text = Vec::new();
        let mut stream = resp.bytes_stream();
        while let Some(Ok(chunk)) = stream.next().await {
            text.extend_from_slice(&chunk);
            if text.len() >= ERROR_BODY_LIMIT {
                break;
            }
        }
        return Err(target
            .module
            .classify(status, &headers, &String::from_utf8_lossy(&text)));
    }

    let mut stream = resp.bytes_stream();
    let mut parser = SseParser::new();
    let mut decoder = decoder_for(target.protocol);
    let mut ttft_ms = None;
    let mut output_tokens = None;
    let mut finished = false;
    *health = Some(StreamHealth::Empty);
    loop {
        let next = if ttft_ms.is_none() {
            tokio::time::timeout_at(deadline, stream.next())
                .await
                .map_err(|_| {
                    UpstreamFailure::new(FailureKind::Timeout, None, "no output in time")
                })?
        } else {
            stream.next().await
        };
        let (events, ended) = match next {
            None => (parser.finish().into_iter().collect::<Vec<_>>(), true),
            Some(Ok(bytes)) => (parser.feed(&bytes), false),
            Some(Err(e)) => return Err(UpstreamFailure::from_reqwest(&e)),
        };
        for event in &events {
            if *health == Some(StreamHealth::Empty) {
                *health = Some(StreamHealth::Truncated);
            }
            let canonical = decoder.feed(event).map_err(|e| {
                *health = Some(StreamHealth::Malformed);
                UpstreamFailure::new(FailureKind::Server, None, e.to_string())
            })?;
            for c in &canonical {
                if c.is_content() && ttft_ms.is_none() {
                    ttft_ms = Some(started.elapsed().as_millis() as u64);
                }
                match c {
                    CanonicalEvent::Usage(u) if u.output_tokens.is_some() => {
                        output_tokens = u.output_tokens;
                    }
                    CanonicalEvent::Finish(_) => finished = true,
                    CanonicalEvent::Error { message, .. } => {
                        return Err(UpstreamFailure::new(
                            FailureKind::Server,
                            None,
                            message.clone(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if ended || decoder.is_done() {
            break;
        }
    }
    if finished || decoder.is_done() {
        *health = Some(StreamHealth::Clean);
    }
    Ok(Measured {
        ttft_ms,
        output_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_request_is_tiny_and_streaming() {
        let r = probe_request();
        assert!(r.stream);
        assert_eq!(r.max_output_tokens, Some(PROBE_MAX_TOKENS));
        assert!(r.instructions.is_empty() && r.tools.is_empty());
        let body = chat::encode_request(&r, "m", chat::EncodeOptions::default());
        assert_eq!(body["messages"][0]["content"], PROBE_PROMPT);
    }

    #[test]
    fn failed_probes_give_no_sample() {
        let ok = ProbeOutcome {
            ts_ms: 5,
            ok: true,
            ttft_ms: Some(120),
            duration_ms: 200,
            output_tokens: Some(1),
            error: None,
            stream: Some(StreamHealth::Clean),
        };
        let s = ok.sample().expect("sample");
        assert_eq!((s.ttft_ms, s.source), (120, SampleSource::Probe));
        let failed = ProbeOutcome {
            ok: false,
            ttft_ms: None,
            ..ok
        };
        assert!(failed.sample().is_none());
    }
}
