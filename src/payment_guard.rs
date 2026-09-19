use crate::infrai_client::{
    CaptureContext, CaptureException, ExceptionPayload, InfraiClient, InfraiError, Tags,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentEvent {
    pub payment_id: String,
    pub merchant_id: String,
    pub amount_minor: u64,
    pub currency: String,
    pub risk_score: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskAction {
    Execute,
    ManualReview,
    Block,
}

impl RiskAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Execute => "execute",
            Self::ManualReview => "manual_review",
            Self::Block => "block",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentDecision {
    pub action: RiskAction,
    pub reason: &'static str,
}

#[derive(Debug, Error)]
pub enum AgentLoopError {
    #[error("payment {payment_id} requires {action}: {reason}")]
    Guarded {
        payment_id: String,
        action: &'static str,
        reason: &'static str,
    },
    #[error("payment provider failed: {0}")]
    Provider(String),
    #[error("failure capture failed: {0}")]
    Capture(#[from] InfraiError),
}

pub fn decide(event: &PaymentEvent) -> PaymentDecision {
    match event.risk_score {
        85..=u8::MAX => PaymentDecision {
            action: RiskAction::Block,
            reason: "risk score reached the block threshold",
        },
        60..=84 => PaymentDecision {
            action: RiskAction::ManualReview,
            reason: "risk score requires analyst review",
        },
        _ => PaymentDecision {
            action: RiskAction::Execute,
            reason: "risk score is inside the execution threshold",
        },
    }
}

pub async fn run_payment(
    client: &InfraiClient,
    event: &PaymentEvent,
    simulate_provider_failure: bool,
) -> Result<PaymentDecision, AgentLoopError> {
    let decision = decide(event);
    if decision.action != RiskAction::Execute {
        return Err(AgentLoopError::Guarded {
            payment_id: event.payment_id.clone(),
            action: decision.action.as_str(),
            reason: decision.reason,
        });
    }

    if simulate_provider_failure {
        let provider_error = "authorization connection closed";
        let idempotency_key = format!("payment-failure:{}:authorize", event.payment_id);
        let amount = event.amount_minor;
        let action = decision.action.as_str();
        client
            .capture_error(&CaptureException {
                title: "payment agent authorization failed",
                message: provider_error,
                exception: ExceptionPayload {
                    kind: "PaymentProviderError",
                    value: provider_error,
                },
                level: "error",
                tags: Tags {
                    workflow: "payment_authorization",
                    risk_tier: "low",
                },
                fingerprint: vec!["payment-agent", "authorize", "provider-connection"],
                context: CaptureContext {
                    payment_id: &event.payment_id,
                    amount_minor: amount,
                    currency: &event.currency,
                    merchant_id: &event.merchant_id,
                    action,
                    reason: decision.reason,
                },
                environment: "example",
                service: "payment-agent",
                idempotency_key: &idempotency_key,
            })
            .await?;
        return Err(AgentLoopError::Provider(provider_error.to_owned()));
    }

    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(risk_score: u8) -> PaymentEvent {
        PaymentEvent {
            payment_id: "pay_2026_0913_001".to_owned(),
            merchant_id: "merchant_cli_demo".to_owned(),
            amount_minor: 12_500,
            currency: "USD".to_owned(),
            risk_score,
        }
    }

    #[test]
    fn high_risk_payment_is_blocked_before_execution() {
        assert_eq!(decide(&event(91)).action, RiskAction::Block);
    }

    #[test]
    fn middle_band_payment_routes_to_manual_review() {
        assert_eq!(decide(&event(72)).action, RiskAction::ManualReview);
    }

    #[test]
    fn low_risk_payment_can_execute() {
        assert_eq!(decide(&event(24)).action, RiskAction::Execute);
    }
}
