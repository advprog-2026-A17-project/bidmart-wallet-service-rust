use std::sync::atomic::Ordering;

use bidmart_wallet_service_rust::http::metrics::METRICS;

#[test]
fn record_request_increments_total() {
    let before = METRICS.total_requests.load(Ordering::Relaxed);
    METRICS.record_request(100_000, false);
    let after = METRICS.total_requests.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn record_operation_hold() {
    let before = METRICS.holds_total.load(Ordering::Relaxed);
    METRICS.record_operation("hold");
    let after = METRICS.holds_total.load(Ordering::Relaxed);
    assert_eq!(after - before, 1);
}

#[test]
fn render_prometheus_includes_apdex_and_operations() {
    METRICS.record_request(50_000, false);
    METRICS.record_operation("top_up");
    let body = bidmart_wallet_service_rust::http::metrics::render_prometheus_body(1.0);
    assert!(body.contains("bidmart_apdex_score"));
    assert!(body.contains("bidmart_wallet_operations_total"));
    assert!(body.contains("service=\"wallet\""));
}
