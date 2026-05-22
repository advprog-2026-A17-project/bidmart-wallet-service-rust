use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

pub async fn require_metrics_basic_auth(
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let (expected_user, expected_pass) = match metrics_credentials() {
        Some(values) => values,
        None => return Ok(next.run(request).await),
    };

    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_basic_credentials)
        .is_some_and(|(user, pass)| user == expected_user && pass == expected_pass);

    if authorized {
        Ok(next.run(request).await)
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"metrics\"")],
            "Unauthorized",
        )
            .into_response())
    }
}

fn metrics_credentials() -> Option<(String, String)> {
    let user = std::env::var("METRICS_BASIC_USER").ok()?;
    let pass = std::env::var("METRICS_BASIC_PASSWORD").ok()?;
    if user.is_empty() || pass.is_empty() {
        return None;
    }
    Some((user, pass))
}

fn parse_basic_credentials(header: &str) -> Option<(String, String)> {
    let encoded = header.strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(encoded.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (user, pass) = decoded.split_once(':')?;
    Some((user.to_string(), pass.to_string()))
}
