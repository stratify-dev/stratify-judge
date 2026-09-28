use crate::{SystemOneRequest, SystemOneResponse};
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("invalid or missing API key")]
    Auth,
    #[error("request rejected: {0}")]
    Validation(String),
    #[error("service overloaded or rate limited after every retry")]
    Overloaded,
    #[error("transport failure: {0}")]
    Transport(String),
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(500),
        }
    }
}

/// The delay before retry `attempt`. Doubles each attempt with the factor
/// capped at `1 << 6`, and saturates rather than overflowing.
///
/// Pure and separate from the sleep so the arithmetic is testable. Testing
/// it through `ask` is not possible: reaching a retry means the test would
/// then sleep for the very duration under test.
///
/// Saturating matters because `ask` must never panic, `Duration`'s
/// `Mul<u32>` panics on overflow, and `base_delay` is a public field a
/// caller can set to anything.
fn backoff_delay(base: Duration, attempt: u32) -> Duration {
    let factor = 1u32 << (attempt.saturating_sub(1)).min(6);
    base.saturating_mul(factor)
}

/// The key rule, separated from the env read so it is testable without
/// mutating process-global state. Rust runs tests in one process across
/// threads, so a test that sets an env var races every other test that
/// reads one. An empty key is worse than no key: it would send `Bearer `
/// and earn a 401 on every request instead of cleanly running without
/// judgment.
pub fn usable_key(raw: Option<String>) -> Option<String> {
    let key = raw?;
    if key.trim().is_empty() {
        return None;
    }
    Some(key)
}

pub struct Client {
    http: reqwest::Client,
    base: String,
    /// None for an endpoint that needs no auth, such as a local
    /// laya-serve without LAYA_API_KEY set.
    key: Option<String>,
    retry: RetryPolicy,
}

impl Client {
    pub fn new(base: String, key: Option<String>) -> Client {
        Client {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("reqwest client builds with default TLS"),
            base,
            key,
            retry: RetryPolicy::default(),
        }
    }

    pub fn with_retry(mut self, retry: RetryPolicy) -> Client {
        self.retry = retry;
        self
    }

    /// One evaluation. 429 and 529 back off exponentially; 401 and 422 fail
    /// immediately, since retrying either one cannot help.
    pub async fn ask(&self, req: &SystemOneRequest) -> Result<SystemOneResponse, ClientError> {
        let url = format!("{}/v1/systemone", self.base.trim_end_matches('/'));
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut request = self.http.post(&url).json(req);
            if let Some(k) = &self.key {
                request = request.bearer_auth(k);
            }
            let sent = request.send().await;

            let resp = match sent {
                Ok(r) => r,
                Err(e) => {
                    if attempt >= self.retry.max_attempts {
                        return Err(ClientError::Transport(e.to_string()));
                    }
                    self.backoff(attempt).await;
                    continue;
                }
            };

            let status = resp.status().as_u16();
            match status {
                200 => {
                    return resp
                        .json::<SystemOneResponse>()
                        .await
                        .map_err(|e| ClientError::Transport(e.to_string()))
                }
                401 | 403 => return Err(ClientError::Auth),
                422 => {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ClientError::Validation(body));
                }
                429 | 529 | 500..=599 => {
                    if attempt >= self.retry.max_attempts {
                        return Err(ClientError::Overloaded);
                    }
                    self.backoff(attempt).await;
                }
                other => {
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ClientError::Transport(format!("HTTP {other}: {body}")));
                }
            }
        }
    }

    async fn backoff(&self, attempt: u32) {
        tokio::time::sleep(backoff_delay(self.retry.base_delay, attempt)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Question;
    use serde_json::json;
    use std::time::Duration;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn req() -> SystemOneRequest {
        SystemOneRequest {
            state: json!({ "finding_0": { "name": "helper" } }),
            model: Some("jev-latest".into()),
            questions: [("framework_invoked".to_string(), Question::noul("q", None))]
                .into_iter()
                .collect(),
        }
    }

    fn ok_body() -> serde_json::Value {
        json!({
            "model": "jev-1.13.0",
            "answers": { "framework_invoked": { "noul": 0.9 } },
            "usage": { "input_tokens": 100, "output_tokens": 0 }
        })
    }

    fn fast(server: &MockServer) -> Client {
        Client::new(server.uri(), Some("test-key".into())).with_retry(RetryPolicy {
            max_attempts: 4,
            base_delay: Duration::from_millis(1),
        })
    }

    #[tokio::test]
    async fn sends_bearer_auth_to_the_systemone_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/systemone"))
            .and(header("authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let got = fast(&server).ask(&req()).await.unwrap();
        assert_eq!(got.answers["framework_invoked"].as_noul(), Some(0.9));
    }

    #[tokio::test]
    async fn retries_a_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;

        let got = fast(&server).ask(&req()).await.unwrap();
        assert_eq!(got.model, "jev-1.13.0");
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts_on_persistent_overload() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(529))
            .mount(&server)
            .await;

        let err = fast(&server).ask(&req()).await.unwrap_err();
        assert!(matches!(err, ClientError::Overloaded), "got {err:?}");
    }

    #[tokio::test]
    async fn a_401_is_a_hard_error_and_is_not_retried() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&server)
            .await;

        let err = fast(&server).ask(&req()).await.unwrap_err();
        assert!(matches!(err, ClientError::Auth), "got {err:?}");
    }

    #[tokio::test]
    async fn a_422_carries_the_body_for_diagnosis() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(422).set_body_string("score needs 2-10 levels"))
            .mount(&server)
            .await;

        let err = fast(&server).ask(&req()).await.unwrap_err();
        match err {
            ClientError::Validation(body) => assert!(body.contains("2-10 levels")),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn a_key_that_is_absent_empty_or_whitespace_is_not_usable() {
        assert_eq!(usable_key(None), None);
        assert_eq!(usable_key(Some(String::new())), None);
        assert_eq!(usable_key(Some("   \t\n  ".into())), None);
    }

    #[test]
    fn a_real_key_is_usable_and_kept_verbatim() {
        assert_eq!(
            usable_key(Some("  sk-abc123  ".into())),
            Some("  sk-abc123  ".to_string()),
            "trimming is a usability test, not a transformation: the key is sent as given"
        );
    }

    #[test]
    fn backoff_delay_doubles_each_attempt_and_caps_the_factor() {
        let base = Duration::from_millis(10);
        assert_eq!(backoff_delay(base, 1), Duration::from_millis(10));
        assert_eq!(backoff_delay(base, 2), Duration::from_millis(20));
        assert_eq!(backoff_delay(base, 3), Duration::from_millis(40));
        // The shift is capped at 6, so the factor stops doubling at 64.
        assert_eq!(backoff_delay(base, 7), Duration::from_millis(640));
        assert_eq!(backoff_delay(base, 20), Duration::from_millis(640));
    }

    #[test]
    fn backoff_delay_saturates_instead_of_overflowing() {
        // This is the case that would panic inside ask()'s call graph.
        assert_eq!(backoff_delay(Duration::MAX, 4), Duration::MAX);
        assert_eq!(
            backoff_delay(Duration::from_secs(u64::MAX / 2), 7),
            Duration::MAX
        );
    }

    #[tokio::test]
    async fn a_client_without_a_key_sends_no_authorization_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;
        let c = Client::new(server.uri(), None);
        assert!(
            c.ask(&req()).await.is_ok(),
            "a local server with no auth is normal"
        );

        // wiremock 0.6 has no negation matcher, so assert on what was
        // actually sent. An absent header and an empty bearer are
        // different things on the wire, and only one of them is correct.
        let sent = server.received_requests().await.unwrap();
        assert_eq!(sent.len(), 1);
        assert!(
            sent[0].headers.get("authorization").is_none(),
            "no key means no header at all"
        );
    }

    #[tokio::test]
    async fn a_client_with_a_key_still_sends_the_bearer_header() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("authorization", "Bearer k"))
            .respond_with(ResponseTemplate::new(200).set_body_json(ok_body()))
            .mount(&server)
            .await;
        let c = Client::new(server.uri(), Some("k".into()));
        assert!(c.ask(&req()).await.is_ok());
    }
}
