use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::body::Body;
use axum::http::Request;
use axum::middleware::Next;
use axum::response::Response;

pub struct RequestMetrics {
    pub total_requests: AtomicU64,
    pub total_errors: AtomicU64,
    pub apdex_satisfied: AtomicU64,
    pub apdex_tolerating: AtomicU64,
    pub apdex_frustrated: AtomicU64,
    pub latency_le_5ms: AtomicU64,
    pub latency_le_25ms: AtomicU64,
    pub latency_le_50ms: AtomicU64,
    pub latency_le_100ms: AtomicU64,
    pub latency_le_250ms: AtomicU64,
    pub latency_le_500ms: AtomicU64,
    pub latency_le_1000ms: AtomicU64,
    pub latency_le_2500ms: AtomicU64,
    pub latency_le_inf: AtomicU64,
    pub latency_sum_us: AtomicU64,
    pub holds_total: AtomicU64,
    pub top_ups_total: AtomicU64,
    pub try_bids_total: AtomicU64,
    pub payouts_total: AtomicU64,
}

impl RequestMetrics {
    const fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            apdex_satisfied: AtomicU64::new(0),
            apdex_tolerating: AtomicU64::new(0),
            apdex_frustrated: AtomicU64::new(0),
            latency_le_5ms: AtomicU64::new(0),
            latency_le_25ms: AtomicU64::new(0),
            latency_le_50ms: AtomicU64::new(0),
            latency_le_100ms: AtomicU64::new(0),
            latency_le_250ms: AtomicU64::new(0),
            latency_le_500ms: AtomicU64::new(0),
            latency_le_1000ms: AtomicU64::new(0),
            latency_le_2500ms: AtomicU64::new(0),
            latency_le_inf: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            holds_total: AtomicU64::new(0),
            top_ups_total: AtomicU64::new(0),
            try_bids_total: AtomicU64::new(0),
            payouts_total: AtomicU64::new(0),
        }
    }

    pub fn record_request(&self, duration_us: u64, is_error: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        if is_error {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
        }
        let ms = duration_us / 1000;
        if ms <= 500 {
            self.apdex_satisfied.fetch_add(1, Ordering::Relaxed);
        } else if ms <= 2000 {
            self.apdex_tolerating.fetch_add(1, Ordering::Relaxed);
        } else {
            self.apdex_frustrated.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 5 {
            self.latency_le_5ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 25 {
            self.latency_le_25ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 50 {
            self.latency_le_50ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 100 {
            self.latency_le_100ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 250 {
            self.latency_le_250ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 500 {
            self.latency_le_500ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 1000 {
            self.latency_le_1000ms.fetch_add(1, Ordering::Relaxed);
        }
        if ms <= 2500 {
            self.latency_le_2500ms.fetch_add(1, Ordering::Relaxed);
        }
        self.latency_le_inf.fetch_add(1, Ordering::Relaxed);
        self.latency_sum_us.fetch_add(duration_us, Ordering::Relaxed);
    }

    pub fn record_operation(&self, op: &str) {
        match op {
            "hold" => {
                self.holds_total.fetch_add(1, Ordering::Relaxed);
            }
            "top_up" => {
                self.top_ups_total.fetch_add(1, Ordering::Relaxed);
            }
            "try_bid" => {
                self.try_bids_total.fetch_add(1, Ordering::Relaxed);
            }
            "payout" => {
                self.payouts_total.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

pub static METRICS: RequestMetrics = RequestMetrics::new();

pub async fn record_http_metrics(request: Request<Body>, next: Next) -> Response {
    let start = Instant::now();
    let response = next.run(request).await;
    let duration_us = start.elapsed().as_micros() as u64;
    let is_error = response.status().is_server_error() || response.status().is_client_error();
    METRICS.record_request(duration_us, is_error);
    response
}

pub fn render_prometheus_body(uptime_seconds: f64) -> String {
    let total = METRICS.total_requests.load(Ordering::Relaxed);
    let errors = METRICS.total_errors.load(Ordering::Relaxed);
    let satisfied = METRICS.apdex_satisfied.load(Ordering::Relaxed);
    let tolerating = METRICS.apdex_tolerating.load(Ordering::Relaxed);
    let holds = METRICS.holds_total.load(Ordering::Relaxed);
    let top_ups = METRICS.top_ups_total.load(Ordering::Relaxed);
    let try_bids = METRICS.try_bids_total.load(Ordering::Relaxed);
    let payouts = METRICS.payouts_total.load(Ordering::Relaxed);
    let sum_us = METRICS.latency_sum_us.load(Ordering::Relaxed);
    let sum_s = sum_us as f64 / 1_000_000.0;

    let apdex = if total > 0 {
        (satisfied as f64 + tolerating as f64 / 2.0) / total as f64
    } else {
        1.0
    };

    let le_5 = METRICS.latency_le_5ms.load(Ordering::Relaxed);
    let le_25 = METRICS.latency_le_25ms.load(Ordering::Relaxed);
    let le_50 = METRICS.latency_le_50ms.load(Ordering::Relaxed);
    let le_100 = METRICS.latency_le_100ms.load(Ordering::Relaxed);
    let le_250 = METRICS.latency_le_250ms.load(Ordering::Relaxed);
    let le_500 = METRICS.latency_le_500ms.load(Ordering::Relaxed);
    let le_1000 = METRICS.latency_le_1000ms.load(Ordering::Relaxed);
    let le_2500 = METRICS.latency_le_2500ms.load(Ordering::Relaxed);
    let le_inf = METRICS.latency_le_inf.load(Ordering::Relaxed);

    format!(
        "# HELP bidmart_service_up Service availability gauge\n\
         # TYPE bidmart_service_up gauge\n\
         bidmart_service_up{{service=\"wallet\"}} 1\n\
         # HELP bidmart_service_uptime_seconds Service uptime in seconds\n\
         # TYPE bidmart_service_uptime_seconds gauge\n\
         bidmart_service_uptime_seconds{{service=\"wallet\"}} {uptime_seconds}\n\
         # HELP bidmart_http_requests_total Total HTTP requests\n\
         # TYPE bidmart_http_requests_total counter\n\
         bidmart_http_requests_total{{service=\"wallet\"}} {total}\n\
         # HELP bidmart_http_errors_total Total HTTP error responses\n\
         # TYPE bidmart_http_errors_total counter\n\
         bidmart_http_errors_total{{service=\"wallet\"}} {errors}\n\
         # HELP bidmart_apdex_score APDEX score (500ms satisfied, 2000ms tolerating)\n\
         # TYPE bidmart_apdex_score gauge\n\
         bidmart_apdex_score{{service=\"wallet\"}} {apdex:.4}\n\
         # HELP bidmart_apdex_satisfied_total APDEX satisfied bucket\n\
         # TYPE bidmart_apdex_satisfied_total counter\n\
         bidmart_apdex_satisfied_total{{service=\"wallet\"}} {satisfied}\n\
         # HELP bidmart_apdex_tolerating_total APDEX tolerating bucket\n\
         # TYPE bidmart_apdex_tolerating_total counter\n\
         bidmart_apdex_tolerating_total{{service=\"wallet\"}} {tolerating}\n\
         # HELP bidmart_http_request_duration_seconds HTTP request latency histogram\n\
         # TYPE bidmart_http_request_duration_seconds histogram\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"0.005\"}} {le_5}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"0.025\"}} {le_25}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"0.05\"}} {le_50}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"0.1\"}} {le_100}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"0.25\"}} {le_250}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"0.5\"}} {le_500}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"1.0\"}} {le_1000}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"2.5\"}} {le_2500}\n\
         bidmart_http_request_duration_seconds_bucket{{service=\"wallet\",le=\"+Inf\"}} {le_inf}\n\
         bidmart_http_request_duration_seconds_sum{{service=\"wallet\"}} {sum_s}\n\
         bidmart_http_request_duration_seconds_count{{service=\"wallet\"}} {total}\n\
         # HELP bidmart_wallet_operations_total Wallet business operations\n\
         # TYPE bidmart_wallet_operations_total counter\n\
         bidmart_wallet_operations_total{{service=\"wallet\",operation=\"hold\"}} {holds}\n\
         bidmart_wallet_operations_total{{service=\"wallet\",operation=\"top_up\"}} {top_ups}\n\
         bidmart_wallet_operations_total{{service=\"wallet\",operation=\"try_bid\"}} {try_bids}\n\
         bidmart_wallet_operations_total{{service=\"wallet\",operation=\"payout\"}} {payouts}\n"
    )
}

static STARTED_AT: OnceLock<Instant> = OnceLock::new();

pub fn service_uptime_seconds() -> f64 {
    STARTED_AT
        .get_or_init(Instant::now)
        .elapsed()
        .as_secs_f64()
}
