use crate::{SystemOneRequest, SystemOneResponse};
use std::time::Duration;

pub const DEFAULT_BASE_URL: &str = "https://api.typesafe.ai";
pub const ENV_API_KEY: &str = "TYPESAFE_API_KEY";

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

pub struct Client {
    http: reqwest::Client,
    base: String,
    key: String,
    retry: RetryPolicy,
}

impl Client {
    pub fn new(base: String, key: String) -> Client {
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

    /// None when TYPESAFE_API_KEY is unset or empty. The caller treats that
    /// as "run without judgment", never as an error.
    pub fn from_env() -> Option<Client> {
        let key = std::env::var(ENV_API_KEY).ok()?;
        if key.trim().is_empty() {
            return None;
        }
        Some(Client::new(DEFAULT_BASE_URL.to_string(), key))
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
            let sent = self
                .http
                .post(&url)
                .bearer_auth(&self.key)
                .json(req)
                .send()
                .await;

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
        let factor = 1u32 << (attempt.saturating_sub(1)).min(6);
        tokio::time::sleep(self.retry.base_delay * factor).await;
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
            model: "jev-latest".into(),
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
        Client::new(server.uri(), "test-key".into()).with_retry(RetryPolicy {
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
    fn from_env_is_none_without_a_key() {
        std::env::remove_var("TYPESAFE_API_KEY");
        assert!(Client::from_env().is_none());
    }
}
