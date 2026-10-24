//! The HTTP side: thin handlers over the core.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, RawQuery, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::board::{self, Period};
use crate::id::Id;
use crate::runs::{self, Quota};

#[derive(Clone)]
struct Ctx {
    db: PgPool,
    quota: Arc<Quota>,
}

pub async fn serve(db: PgPool, quota: Arc<Quota>, port: u16) -> std::io::Result<()> {
    // the web game (and a local `trunk serve`) calls in from the browser
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            let o = origin.as_bytes();
            o == b"https://typepper.com"
                || o.starts_with(b"http://localhost:")
                || o.starts_with(b"http://127.0.0.1:")
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([CONTENT_TYPE]);
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/runs", post(submit))
        .route("/v1/runs/{id}", get(replay))
        .route("/v1/boards/{mode}", get(boards))
        .route("/{id}", get(page))
        .layer(DefaultBodyLimit::max(runs::MAX_BODY))
        .layer(cors)
        .with_state(Ctx { db, quota });
    // "::" takes IPv4 too, and Railway's private network is IPv6
    let listener = tokio::net::TcpListener::bind(("::", port)).await?;
    axum::serve(listener, app).await
}

/// `POST /v1/runs`: a submission in, one line per replay out.
async fn submit(State(cx): State<Ctx>, headers: HeaderMap, body: Bytes) -> Response {
    // behind Railway's edge, the caller is the first forwarded address
    let who = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .unwrap_or("?")
        .trim()
        .to_string();
    if !cx.quota.take(&who) {
        let line = "err rate-limited try again in an hour\n";
        return (StatusCode::TOO_MANY_REQUESTS, line).into_response();
    }
    // anonymous until accounts arrive
    let lines = runs::submit(&cx.db, None, &body).await;
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], lines).into_response()
}

/// `GET /v1/boards/{mode}?period=day|week|month|year|all`: a board, one
/// line per row (as `v1 board` over SSH).
async fn boards(
    State(cx): State<Ctx>,
    Path(mode): Path<String>,
    RawQuery(q): RawQuery,
) -> Response {
    let period = q
        .as_deref()
        .unwrap_or("")
        .split('&')
        .find_map(|kv| kv.strip_prefix("period="))
        .unwrap_or("all");
    let (Some(mode), Some(period)) = (typepper_engine::Mode::parse(&mode), Period::parse(period))
    else {
        return (
            StatusCode::NOT_FOUND,
            "err bad-board no such mode or period\n",
        )
            .into_response();
    };
    match board::board(&cx.db, mode, period, None).await {
        Ok((top, _)) => (
            [
                (CONTENT_TYPE, "text/plain; charset=utf-8"),
                (CACHE_CONTROL, "public, max-age=30"),
            ],
            board::text(&top, None),
        )
            .into_response(),
        Err(e) => {
            eprintln!("boards: {e}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `GET /v1/runs/{id}`: the replay, for the web player. Never changes.
async fn replay(State(cx): State<Ctx>, Path(id): Path<String>) -> Response {
    let Ok(id) = id.parse::<Id>() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match runs::replay(&cx.db, id).await {
        Ok(Some(bytes)) => (
            [
                (CONTENT_TYPE, "application/octet-stream"),
                (CACHE_CONTROL, "public, max-age=604800, immutable"),
            ],
            bytes,
        )
            .into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            eprintln!("replay: {e}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `r.typepper.com/{id}`: the share page.
async fn page(State(cx): State<Ctx>, Path(id): Path<String>) -> Response {
    let gone = || {
        let html = "<!doctype html><title>typepper</title><p>This replay is gone (they keep for a week). <a href=\"https://typepper.com/\">Play typepper</a></p>";
        (
            StatusCode::NOT_FOUND,
            [(CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response()
    };
    let Ok(id) = id.parse::<Id>() else {
        return gone();
    };
    match runs::page(&cx.db, id).await {
        Ok(Some(html)) => ([(CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response(),
        Ok(None) => gone(),
        Err(e) => {
            eprintln!("page: {e}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// Ready once Postgres answers: 200 "pass", else 503 "fail" (Railway only
/// moves traffic to a deploy once it's 200).
async fn health(State(cx): State<Ctx>) -> impl IntoResponse {
    let t0 = Instant::now();
    let up = sqlx::query("select 1")
        .execute(&cx.db)
        .await
        // the reason stays in the logs; the response only says pass or fail
        .inspect_err(|e| eprintln!("health: postgres: {e}"))
        .is_ok();
    let deployment = std::env::var("RAILWAY_DEPLOYMENT_ID").unwrap_or_default();
    let (code, body) = report(up, t0.elapsed().as_millis() as u64, &deployment);
    (
        code,
        [(CONTENT_TYPE, "application/health+json")],
        body.to_string(),
    )
}

/// The response, in the IETF health check format
/// (draft-inadarei-api-health-check, `application/health+json`).
fn report(postgres_up: bool, postgres_ms: u64, deployment: &str) -> (StatusCode, Value) {
    let (code, status) = match postgres_up {
        true => (StatusCode::OK, "pass"),
        false => (StatusCode::SERVICE_UNAVAILABLE, "fail"),
    };
    let mut body = json!({
        "status": status,
        "version": "1",                         // the API: /v1
        "releaseId": env!("CARGO_PKG_VERSION"), // this server
        "serviceId": "typepper-server",
        "checks": {
            "postgres:responseTime": [{
                "componentType": "datastore",
                "status": status,
                "observedValue": postgres_ms,
                "observedUnit": "ms",
            }],
        },
    });
    if !deployment.is_empty() {
        let short = deployment.get(..8).unwrap_or(deployment);
        body["description"] = json!(format!("deployment {short}"));
    }
    (code, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_with_postgres_and_fails_without() {
        let (code, body) = report(true, 2, "d0aa815e-a00e-471e-917c-592e4935b63b");
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["status"], "pass");
        assert_eq!(body["releaseId"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["description"], "deployment d0aa815e");
        let check = &body["checks"]["postgres:responseTime"][0];
        assert_eq!(
            (&check["status"], &check["observedValue"]),
            (&json!("pass"), &json!(2))
        );

        let (code, body) = report(false, 3000, "");
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["status"], "fail");
        assert_eq!(body["checks"]["postgres:responseTime"][0]["status"], "fail");
        assert!(
            body.get("description").is_none(),
            "no deployment, no description"
        );
    }
}
