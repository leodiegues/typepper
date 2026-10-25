//! The HTTP side: thin handlers over the core.

use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, RawQuery, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE, COOKIE, SET_COOKIE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::accounts;
use crate::board::{self, Period};
use crate::github;
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
        .allow_headers([CONTENT_TYPE])
        // a logged-in browser's session cookie comes along
        .allow_credentials(true);
    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/runs", post(submit))
        .route("/v1/me/runs", post(submit_mine))
        .route("/v1/me/bests", get(my_bests))
        .route("/auth/github", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
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

/// The cookies: a session's secret, and a login's `state` on its way through
/// GitHub. Host-only (no `Domain`): only api.typepper.com reads them.
const SESSION: &str = "typepper_session";
const STATE: &str = "typepper_state";

/// `POST /v1/runs`: an anonymous submission in, one line per replay out.
async fn submit(State(cx): State<Ctx>, headers: HeaderMap, body: Bytes) -> Response {
    // behind Railway's edge, the caller is the first forwarded address
    let who = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .unwrap_or("?")
        .trim()
        .to_string();
    store(&cx, &who, None, &body).await
}

/// `POST /v1/me/runs`: a logged-in browser's submission. Without a session
/// it's refused, not stored anonymously: the game keeps the rounds and asks
/// the player to log in again.
async fn submit_mine(State(cx): State<Ctx>, headers: HeaderMap, body: Bytes) -> Response {
    match session(&cx, &headers).await {
        Ok(Some((player, _))) => store(&cx, &format!("player {player}"), Some(player), &body).await,
        Ok(None) => (StatusCode::UNAUTHORIZED, "err signed-out log in again\n").into_response(),
        Err(code) => code.into_response(),
    }
}

/// `GET /v1/me/bests`: a logged-in browser's best in each mode, from every
/// round of theirs on the server (the terminal's too).
async fn my_bests(State(cx): State<Ctx>, headers: HeaderMap) -> Response {
    let player = match session(&cx, &headers).await {
        Ok(Some((player, _))) => player,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "err signed-out log in again\n").into_response();
        }
        Err(code) => return code.into_response(),
    };
    match board::bests(&cx.db, player).await {
        Ok(text) => (
            [
                (CONTENT_TYPE, "text/plain; charset=utf-8"),
                (CACHE_CONTROL, "private, no-store"),
            ],
            text,
        )
            .into_response(),
        Err(e) => {
            eprintln!("bests: {e}");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

async fn store(cx: &Ctx, who: &str, player: Option<i64>, body: &[u8]) -> Response {
    if !cx.quota.take(who) {
        let line = "err rate-limited try again in an hour\n";
        return (StatusCode::TOO_MANY_REQUESTS, line).into_response();
    }
    let lines = runs::submit(&cx.db, player, body).await;
    ([(CONTENT_TYPE, "text/plain; charset=utf-8")], lines).into_response()
}

/// The browser's player, if its session cookie is one: `(id, login)`.
async fn session(cx: &Ctx, headers: &HeaderMap) -> Result<Option<(i64, String)>, StatusCode> {
    let Some(secret) = cookie(headers, SESSION) else {
        return Ok(None);
    };
    accounts::player_of_session(&cx.db, secret)
        .await
        .map_err(|e| {
            eprintln!("session: {e}");
            StatusCode::SERVICE_UNAVAILABLE
        })
}

/// A cookie's value, from however many `Cookie` headers came.
fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(name)?.strip_prefix('='))
        .filter(|v| !v.is_empty())
}

/// The GitHub App's client id and secret, set in Railway.
fn github_app() -> Option<(String, String)> {
    Some((
        var("TYPEPPER_GITHUB_CLIENT_ID")?,
        var("TYPEPPER_GITHUB_CLIENT_SECRET")?,
    ))
}

/// Where the web game is, logins go back to: typepper.com, or
/// `TYPEPPER_WEB` (a local one, with its `?api=`).
fn web() -> String {
    var("TYPEPPER_WEB").unwrap_or("https://typepper.com/".into())
}

/// Where GitHub sends players back: the app's first callback URL, or
/// `TYPEPPER_GITHUB_CALLBACK` (a local server's, also registered on the app).
fn callback_url() -> Option<String> {
    var("TYPEPPER_GITHUB_CALLBACK")
}

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// The game's address with `login=<login>` added to its query.
fn with_login(web: &str, login: &str) -> String {
    let sep = if web.contains('?') { '&' } else { '?' };
    format!("{web}{sep}login={login}")
}

/// `GET /auth/github`: off to GitHub to log in, with a `state` to check on
/// the way back.
async fn login() -> Response {
    let Some((client_id, _)) = github_app() else {
        return (StatusCode::SERVICE_UNAVAILABLE, "login isn't set up\n").into_response();
    };
    let state: String = rand::random::<[u8; 16]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    // Lax, not Strict: GitHub sending the player back is a cross-site visit
    let cookie =
        format!("{STATE}={state}; Path=/auth; Max-Age=600; HttpOnly; Secure; SameSite=Lax");
    let github = std::env::var("TYPEPPER_GITHUB").unwrap_or("https://github.com".into());
    let mut to = format!("{github}/login/oauth/authorize?client_id={client_id}&state={state}");
    if let Some(back) = callback_url() {
        to += &format!("&redirect_uri={back}");
    }
    (AppendHeaders([(SET_COOKIE, cookie)]), Redirect::to(&to)).into_response()
}

/// `GET /auth/callback?code=…&state=…`: GitHub sends the player back here.
/// Their player is made or found, the browser gets a session cookie, and
/// the game learns the login from `?login=`.
async fn callback(State(cx): State<Ctx>, headers: HeaderMap, RawQuery(q): RawQuery) -> Response {
    let fail = |why: &str| {
        let text = format!("couldn't log in: {why}\n\nback to the game: {}\n", web());
        (StatusCode::BAD_REQUEST, text).into_response()
    };
    let q = q.unwrap_or_default();
    let param = |k: &str| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='))
    };
    // turned down at GitHub: back to the game, as they were
    if param("error").is_some() {
        return Redirect::to(&web()).into_response();
    }
    let Some(code) = param("code") else {
        return fail("GitHub sent no code");
    };
    if !state_matches(param("state"), cookie(&headers, STATE)) {
        return fail("that login didn't start here (or took over ten minutes)");
    }
    let Some((client_id, secret)) = github_app() else {
        return fail("login isn't set up");
    };
    let (github_id, login) =
        match github::signed_in(&client_id, &secret, code, callback_url().as_deref()).await {
            Ok(who) => who,
            Err(e) => {
                eprintln!("callback: {e}");
                return fail("GitHub didn't say who you are");
            }
        };
    let secret = match accounts::player(&cx.db, github_id, &login).await {
        Ok(player) => accounts::sign_in(&cx.db, player).await,
        Err(e) => Err(e),
    };
    let Ok(secret) = secret.inspect_err(|e| eprintln!("callback: {e}")) else {
        return fail("typepper.com is busy, try again");
    };
    // 400 days, the most browsers keep a cookie
    let session =
        format!("{SESSION}={secret}; Path=/; Max-Age=34560000; HttpOnly; Secure; SameSite=Lax");
    let state = format!("{STATE}=; Path=/auth; Max-Age=0");
    let to = with_login(&web(), &login);
    (
        AppendHeaders([(SET_COOKIE, session), (SET_COOKIE, state)]),
        Redirect::to(&to),
    )
        .into_response()
}

/// The `state` GitHub handed back is the one this browser set off with.
fn state_matches(sent: Option<&str>, kept: Option<&str>) -> bool {
    matches!((sent, kept), (Some(a), Some(b)) if a == b)
}

/// `POST /auth/logout`: ends the browser's session.
async fn logout(State(cx): State<Ctx>, headers: HeaderMap) -> Response {
    if let Some(secret) = cookie(&headers, SESSION)
        && let Err(e) = accounts::sign_out(&cx.db, secret).await
    {
        eprintln!("logout: {e}");
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let gone = format!("{SESSION}=; Path=/; Max-Age=0");
    (AppendHeaders([(SET_COOKIE, gone)]), "ok\n").into_response()
}

/// `GET /v1/boards/{mode}?period=day|week|month|year|all`: a board, one
/// line per row (as `v1 board` over SSH).
async fn boards(
    State(cx): State<Ctx>,
    Path(mode): Path<String>,
    headers: HeaderMap,
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
    // a logged-in browser's own row comes too (no session: just the top)
    let player = session(&cx, &headers).await.ok().flatten().map(|(p, _)| p);
    match board::board(&cx.db, mode, period, player).await {
        Ok((top, me)) => (
            [
                (CONTENT_TYPE, "text/plain; charset=utf-8"),
                // with a `me` row it's this player's alone
                match player {
                    Some(_) => (CACHE_CONTROL, "private, max-age=30"),
                    None => (CACHE_CONTROL, "public, max-age=30"),
                },
            ],
            board::text(&top, me.as_ref()),
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
    fn cookies_and_state_are_read_strictly() {
        let mut h = HeaderMap::new();
        h.append(COOKIE, "a=1; typepper_state=abc".parse().unwrap());
        h.append(
            COOKIE,
            "typepper_session=s3cret; typepper_sessionx=no"
                .parse()
                .unwrap(),
        );
        assert_eq!(cookie(&h, SESSION), Some("s3cret"));
        assert_eq!(cookie(&h, STATE), Some("abc"));
        assert_eq!(cookie(&h, "typepper"), None, "a prefix isn't a name");
        h.insert(COOKIE, "typepper_session=".parse().unwrap());
        assert_eq!(cookie(&h, SESSION), None, "empty is none");

        assert!(state_matches(Some("abc"), Some("abc")));
        assert!(!state_matches(Some("abc"), Some("abd")));
        assert!(
            !state_matches(Some("abc"), None),
            "no cookie: not this browser's"
        );
        assert!(!state_matches(None, None));

        assert_eq!(
            with_login("https://typepper.com/", "Octo-Cat"),
            "https://typepper.com/?login=Octo-Cat"
        );
        let local = "http://localhost:8090/?api=http://localhost:8080";
        assert_eq!(
            with_login(local, "a"),
            "http://localhost:8090/?api=http://localhost:8080&login=a"
        );
    }

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
