use fintech_agent_failure_guard::{
    infrai_client::InfraiClient,
    payment_guard::{decide, run_payment, PaymentEvent, RiskAction},
};
use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    match command().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn command() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let simulate_failure = args.iter().any(|arg| arg == "--simulate-provider-failure");
    let risk_score = option(&args, "--risk-score")
        .unwrap_or("24")
        .parse::<u8>()?;
    let payment_id = option(&args, "--payment-id").unwrap_or("pay_2026_0913_001");

    let event = PaymentEvent {
        payment_id: payment_id.to_owned(),
        merchant_id: "merchant_cli_demo".to_owned(),
        amount_minor: 12_500,
        currency: "USD".to_owned(),
        risk_score,
    };
    let decision = decide(&event);

    if decision.action != RiskAction::Execute {
        println!(
            "payment_id={} action={} reason=\"{}\"",
            event.payment_id,
            decision.action.as_str(),
            decision.reason
        );
        return Ok(());
    }

    let client = InfraiClient::from_env()?;
    match run_payment(&client, &event, simulate_failure).await {
        Ok(result) => println!(
            "payment_id={} action={} reason=\"{}\"",
            event.payment_id,
            result.action.as_str(),
            result.reason
        ),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}
