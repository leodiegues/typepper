//! Players, their ssh keys and their web sessions: a key linked to a GitHub
//! account is who the terminal game is; a session's cookie is who a browser is.

use sqlx::{PgPool, Row};

use crate::id::{Id, with_new_id};

/// The player a key belongs to, if it's linked: `(id, login)`.
pub async fn player_of_key(db: &PgPool, fingerprint: &str) -> sqlx::Result<Option<(i64, String)>> {
    let row = sqlx::query(
        "select p.id, p.login from ssh_keys k join players p on p.id = k.player_id
         where k.fingerprint = $1",
    )
    .bind(fingerprint)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| (r.get("id"), r.get("login"))))
}

/// Ties `fingerprint` to the GitHub account `github_id`; returns the player.
pub async fn link(
    db: &PgPool,
    github_id: i64,
    login: &str,
    fingerprint: &str,
) -> sqlx::Result<i64> {
    let player = player(db, github_id, login).await?;
    with_new_id(|id: Id| {
        sqlx::query(
            "insert into ssh_keys (id, fingerprint, player_id) values ($1, $2, $3)
             on conflict (fingerprint) do update set player_id = excluded.player_id",
        )
        .bind(id.0)
        .bind(fingerprint)
        .bind(player)
        .execute(db)
    })
    .await?;
    Ok(player)
}

/// The GitHub account `github_id`'s player (made, or its login refreshed, as
/// needed).
pub async fn player(db: &PgPool, github_id: i64, login: &str) -> sqlx::Result<i64> {
    with_new_id(|id| {
        sqlx::query_scalar(
            "insert into players (id, github_id, login) values ($1, $2, $3)
             on conflict (github_id) do update set login = excluded.login
             returning id",
        )
        .bind(id.0)
        .bind(github_id)
        .bind(login)
        .fetch_one(db)
    })
    .await
}

/// Lets go of a key; true if it was linked.
pub async fn unlink(db: &PgPool, fingerprint: &str) -> sqlx::Result<bool> {
    let done = sqlx::query("delete from ssh_keys where fingerprint = $1")
        .bind(fingerprint)
        .execute(db)
        .await?;
    Ok(done.rows_affected() > 0)
}

/// A new session for `player`: the cookie's secret, as hex. Only its hash
/// is stored.
pub async fn sign_in(db: &PgPool, player: i64) -> sqlx::Result<String> {
    let secret: String = rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    sqlx::query("insert into sessions (hash, player_id) values ($1, $2)")
        .bind(hash(&secret))
        .bind(player)
        .execute(db)
        .await?;
    Ok(secret)
}

/// The player a session's secret belongs to, if it's still one: `(id, login)`.
pub async fn player_of_session(db: &PgPool, secret: &str) -> sqlx::Result<Option<(i64, String)>> {
    let row = sqlx::query(
        "select p.id, p.login from sessions s join players p on p.id = s.player_id
         where s.hash = $1",
    )
    .bind(hash(secret))
    .fetch_optional(db)
    .await?;
    Ok(row.map(|r| (r.get("id"), r.get("login"))))
}

/// Ends a session.
pub async fn sign_out(db: &PgPool, secret: &str) -> sqlx::Result<()> {
    sqlx::query("delete from sessions where hash = $1")
        .bind(hash(secret))
        .execute(db)
        .await
        .map(|_| ())
}

fn hash(secret: &str) -> Vec<u8> {
    ring::digest::digest(&ring::digest::SHA256, secret.as_bytes())
        .as_ref()
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::tests::test_db;

    #[tokio::test]
    async fn a_key_links_relinks_and_lets_go() {
        let Some(db) = test_db().await else {
            return eprintln!("skipped: no TYPEPPER_TEST_DATABASE_URL");
        };
        let fpr = format!("SHA256:test-{}", Id::random());
        let gh = Id::random().0;
        assert_eq!(player_of_key(&db, &fpr).await.unwrap(), None);
        let p = link(&db, gh, "old-name", &fpr).await.unwrap();
        assert_eq!(
            player_of_key(&db, &fpr).await.unwrap(),
            Some((p, "old-name".into()))
        );
        // renamed on GitHub: same player, new login
        assert_eq!(link(&db, gh, "new-name", &fpr).await.unwrap(), p);
        assert_eq!(
            player_of_key(&db, &fpr).await.unwrap(),
            Some((p, "new-name".into()))
        );
        assert!(unlink(&db, &fpr).await.unwrap());
        assert!(!unlink(&db, &fpr).await.unwrap());
        assert_eq!(player_of_key(&db, &fpr).await.unwrap(), None);
    }

    #[tokio::test]
    async fn a_session_signs_in_and_out() {
        let Some(db) = test_db().await else {
            return eprintln!("skipped: no TYPEPPER_TEST_DATABASE_URL");
        };
        let p = player(&db, Id::random().0, "web-player").await.unwrap();
        let secret = sign_in(&db, p).await.unwrap();
        assert_eq!(secret.len(), 64);
        assert_eq!(
            player_of_session(&db, &secret).await.unwrap(),
            Some((p, "web-player".into()))
        );
        assert_eq!(player_of_session(&db, "guess").await.unwrap(), None);
        sign_out(&db, &secret).await.unwrap();
        assert_eq!(player_of_session(&db, &secret).await.unwrap(), None);
    }
}
