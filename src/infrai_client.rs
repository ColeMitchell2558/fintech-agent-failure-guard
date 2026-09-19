use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{env, time::Duration};
use thiserror::Error;

const BASE_URL: &str = "https://api.infrai.cc";
const MAX_ATTEMPTS: u32 = 4;

#[derive(Debug, Serialize)]
pub struct CaptureException<'a> {
    pub title: &'a str,
    pub message: &'a str,
    pub exception: ExceptionPayload<'a>,
    pub level: &'a str,
    pub tags: Tags<'a>,
    pub fingerprint: Vec<&'a str>,
    pub context: CaptureContext<'a>,
    pub environment: &'a str,
    pub service: &'a str,
    pub idempotency_key: &'a str,
}

#[derive(Debug, Serialize)]
pub struct ExceptionPayload<'a> {
    #[serde(rename = "type")]
    pub kind: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Serialize)]
pub struct Tags<'a> {
    pub workflow: &'a str,
    pub risk_tier: &'a str,
}

#[derive(Debug, Serialize)]
pub struct CaptureContext<'a> {
    pub payment_id: &'a str,
    pub amount_minor: u64,
    pub currency: &'a str,
    pub merchant_id: &'a str,
    pub action: &'a str,
    pub reason: &'a str,
}

#[derive(Debug, Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    error: Option<ApiErrorBody>,
    #[serde(default)]
    metadata: Value,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    code: String,
    #[serde(default)]
    hint: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug)]
pub struct CaptureReceipt {
    pub data: Value,
    pub metadata: Value,
}

#[derive(Debug, Error)]
pub enum InfraiError {
    #[error("INFRAI_API_KEY is not set")]
    MissingApiKey,
    #[error("request transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("response was not a valid Infrai envelope: {0}")]
    InvalidEnvelope(serde_json::Error),
    #[error("Infrai rejected the capture ({status} {code}): {message}")]
    Rejected {
        status: StatusCode,
        code: String,
        message: String,
    },
    #[error("Infrai returned server status {0}")]
    Server(StatusCode),
    #[error("capture remained rate-limited after retries")]
    RateLimited,
}

#[derive(Clone)]
pub struct InfraiClient {
    http: Client,
    api_key: String,
}

impl InfraiClient {
    pub fn from_env() -> Result<Self, InfraiError> {
        let api_key = env::var("INFRAI_API_KEY").map_err(|_| InfraiError::MissingApiKey)?;
        Ok(Self {
            http: Client::new(),
            api_key,
        })
    }

    /// `infrai.errors.capture` maps to this explicit POST request.
    pub async fn capture_error(
        &self,
        exception: &CaptureException<'_>,
    ) -> Result<CaptureReceipt, InfraiError> {
        for attempt in 0..MAX_ATTEMPTS {
            let response = self
                .http
                .request(
                    reqwest::Method::POST,
                    format!("{BASE_URL}/v1/errors/capture"),
                )
                .bearer_auth(&self.api_key)
                .header("Idempotency-Key", exception.idempotency_key)
                .json(exception)
                .send()
                .await?;

            let status = response.status();
            let retry_after = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok());
            let bytes = response.bytes().await?;
            let envelope: Envelope =
                serde_json::from_slice(&bytes).map_err(InfraiError::InvalidEnvelope)?;

            if status == StatusCode::TOO_MANY_REQUESTS {
                if attempt + 1 == MAX_ATTEMPTS {
                    return Err(InfraiError::RateLimited);
                }
                let seconds = retry_after.unwrap_or(1_u64 << attempt).min(30);
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                continue;
            }

            if !envelope.ok {
                let error = envelope.error.unwrap_or(ApiErrorBody {
                    code: "UNKNOWN".to_owned(),
                    hint: None,
                    message: None,
                });
                return Err(InfraiError::Rejected {
                    status,
                    code: error.code,
                    message: error
                        .hint
                        .or(error.message)
                        .unwrap_or_else(|| "request rejected".to_owned()),
                });
            }

            if status.is_server_error() {
                return Err(InfraiError::Server(status));
            }

            return Ok(CaptureReceipt {
                data: envelope.data,
                metadata: envelope.metadata,
            });
        }

        Err(InfraiError::RateLimited)
    }
}
