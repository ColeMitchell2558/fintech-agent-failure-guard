#!/usr/bin/env sh
set -eu

cargo run --quiet --bin payment-agent -- --payment-id pay_demo_high_risk --risk-score 91

