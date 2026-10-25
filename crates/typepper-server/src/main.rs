//! typepper's server: the HTTP API and the SSH door, over one Postgres.
//! See docs/built/server.md.

mod accounts;
mod board;
mod github;
mod http;
mod id;
mod runs;
mod ssh;

use std::env;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let var = |k: &str| env::var(k).map_err(|_| format!("{k} is not set"));
    let port = |k: &str, default| {
        env::var(k)
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(default)
    };
    // lazy: the server boots without Postgres, and /health says (quickly)
    // whether it answers
    let db = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(3))
        .connect_lazy(&var("TYPEPPER_DATABASE_URL")?)?;
    // a fixed key from the environment: a new one per deploy would make every
    // player's ssh refuse the server (the game ships the public half)
    let host_key = russh::keys::PrivateKey::from_openssh(var("TYPEPPER_SSH_HOST_KEY")?)?;
    migrate(&db).await?;
    let quota = Arc::new(runs::Quota::default());
    // hourly: forget runs no board needs (docs/built/server.md, "The sweep")
    tokio::spawn({
        let db = db.clone();
        async move {
            let mut hourly = tokio::time::interval(Duration::from_secs(3600));
            loop {
                hourly.tick().await;
                match board::sweep(&db).await {
                    Ok(0) => {}
                    Ok(n) => eprintln!("sweep: forgot {n} runs"),
                    Err(e) => eprintln!("sweep: {e}"),
                }
            }
        }
    });
    tokio::try_join!(
        http::serve(db.clone(), quota.clone(), port("PORT", 8080)),
        ssh::serve(host_key, db, quota, port("TYPEPPER_SSH_PORT", 2222)),
    )?;
    Ok(())
}

/// Brings the schema up to date before serving (one server, so no race),
/// giving Postgres half a minute to come up.
async fn migrate(db: &sqlx::PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    let mut tries = 0;
    loop {
        match sqlx::migrate!().run(db).await {
            Err(sqlx::migrate::MigrateError::Execute(e)) if tries < 10 && is_down(&e) => {
                eprintln!("migrate: waiting for postgres: {e}");
                tries += 1;
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            r => return r,
        }
    }
}

fn is_down(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::PoolTimedOut | sqlx::Error::Io(_))
}
