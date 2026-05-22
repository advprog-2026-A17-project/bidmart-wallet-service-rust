# Wallet Module — Evidence Screenshots

## Required

| File | Capture |
|------|---------|
| `llvm-cov-summary.png` | Terminal or CI log showing `cargo llvm-cov` ≥ 90% lines |
| `sonar-quality-gate.png` | `advprog-2026-A17-project_bidmart-wallet-service-rust` (once scan enabled) |
| `ci-workflow-success.png` | Green `.github/workflows/ci.yml` run |

## Module-specific

| File | Capture |
|------|---------|
| `hold-integration-test.png` | `tests/api_tests.rs` or auction `wallet_integration_tests` pass |
| `provisioning-consumer.png` | `tests/provisioning_tests.rs` success |
| `wallet-metrics.png` | Prometheus target `bidmart-wallet-service` UP |

Sonar: https://sonarcloud.io/project/overview?id=advprog-2026-A17-project_bidmart-wallet-service-rust

## Panduan capture manual

Langkah lengkap: [SCREENSHOT_CAPTURE_GUIDE.md](../../../SCREENSHOT_CAPTURE_GUIDE.md) (workspace root).
