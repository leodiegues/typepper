//! Runs: a submission's replays are each played again here, and stored with
//! the server's own numbers (docs/reference/protocol.md). The answer is one
//! line per replay, the same over SSH and HTTP:
//!
//! ```text
//! ok  <id> <score|-> <rank|-> <link>
//! err <old-engine|bad-replay|too-big|too-many|busy|rate-limited> <why>
//! ```

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sqlx::{PgPool, Row};
use typepper_engine::replay::{self, Bad};
use typepper_engine::{Mode, Replay, VERSION, simulate};

use crate::board;
use crate::id::{Id, with_new_id};

/// Replays in one submission (the game's outbox holds 20).
pub const MAX_ITEMS: usize = 20;
/// The biggest submission read: 20 replays and their claimed scores.
pub const MAX_BODY: usize = MAX_ITEMS * (replay::MAX_BYTES + 8);
/// Where share links point.
pub const SHARE: &str = "https://r.typepper.com";

/// Stores every replay in `body` (each `claimed: f64 LE | replay`) for
/// `player` (none yet: anonymous), and answers one line per replay.
pub async fn submit(db: &PgPool, player: Option<i64>, body: &[u8]) -> String {
    let mut lines = Vec::new();
    let mut rest = body;
    while !rest.is_empty() {
        if lines.len() == MAX_ITEMS {
            lines.push(format!("err too-many only {MAX_ITEMS} at a time"));
            break;
        }
        let Some((claimed, after)) = rest.split_first_chunk::<8>() else {
            lines.push(format!("err bad-replay {}", Bad::Truncated));
            break;
        };
        let claimed = f64::from_le_bytes(*claimed);
        // a broken replay leaves no way to find the next one: stop there
        let (replay, after) = match Replay::decode_prefix(after) {
            Ok(r) => r,
            Err(why) => {
                lines.push(format!("err bad-replay {why}"));
                break;
            }
        };
        let bytes = &rest[8..rest.len() - after.len()];
        rest = after;
        lines.push(match one(db, player, claimed, replay, bytes).await {
            Ok(line) | Err(line) => line,
        });
    }
    lines.push(String::new());
    lines.join("\n")
}

async fn one(
    db: &PgPool,
    player: Option<i64>,
    claimed: f64,
    replay: Replay,
    bytes: &[u8],
) -> Result<String, String> {
    if bytes.len() > replay::MAX_BYTES {
        return Err(format!("err too-big {}", Bad::TooBig));
    }
    if replay.engine != VERSION {
        return Err("err old-engine update typepper to submit".into());
    }
    let setup = replay
        .setup()
        .ok_or(format!("err bad-replay {}", Bad::Mode))?;
    // a long zen replay is millions of steps: keep it off the async threads
    let inputs = replay.inputs;
    let outcome = tokio::task::spawn_blocking(move || simulate(setup, &inputs))
        .await
        .map_err(|_| "err busy try later".to_string())?
        .ok_or("err bad-replay the round never ends")?;
    if claimed.is_finite() && outcome.score.is_some_and(|s| s != claimed) {
        // the client saw another number: the rules ran differently somewhere
        eprintln!(
            "runs: {} scored {:?} here, {claimed} on the player's side",
            setup.mode.key(),
            outcome.score
        );
    }
    let id = with_new_id(|id| async move {
        sqlx::query(
            "insert into runs (id, player_id, replay, engine, mode, score, wpm, accuracy, claimed)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(id.0)
        .bind(player)
        .bind(bytes)
        .bind(VERSION as i32)
        .bind(setup.mode.key())
        .bind(outcome.score)
        .bind(outcome.wpm)
        .bind(outcome.accuracy)
        .bind(claimed.is_finite().then_some(claimed))
        .execute(db)
        .await
        .map(|_| id)
    })
    .await
    .map_err(|e| {
        eprintln!("runs: insert: {e}");
        "err busy try later".to_string()
    })?;
    let score = outcome.score.map_or("-".into(), |s| s.to_string());
    // a player's run: where they stand on the mode's all-time board now
    let rank = match (player, outcome.score) {
        (Some(p), Some(_)) => board::rank(db, setup.mode, p).await.ok().flatten(),
        _ => None,
    };
    let rank = rank.map_or("-".into(), |r| r.to_string());
    Ok(format!("ok {id} {score} {rank} {SHARE}/{id}"))
}

/// A stored replay's bytes.
pub async fn replay(db: &PgPool, id: Id) -> sqlx::Result<Option<Vec<u8>>> {
    sqlx::query_scalar("select replay from runs where id = $1")
        .bind(id.0)
        .fetch_optional(db)
        .await
}

/// The share page: link previews read its tags, people get sent on to the
/// game, which plays the replay.
pub async fn page(db: &PgPool, id: Id) -> sqlx::Result<Option<String>> {
    let row = sqlx::query("select mode, score, wpm from runs where id = $1")
        .bind(id.0)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|r| {
        let key: String = r.get("mode");
        let score: Option<f64> = r.get("score");
        let wpm: f64 = r.get("wpm");
        share_page(id, &key, score, wpm)
    }))
}

fn share_page(id: Id, key: &str, score: Option<f64>, wpm: f64) -> String {
    let mode = Mode::parse(key);
    let name = mode.map_or(key.to_string(), |m| match m.start_level() {
        Some(lv @ 2..) => format!("{} from lv{lv}", m.name()),
        _ => m.name(),
    });
    let result = match (mode, score) {
        (Some(m), Some(s)) if m.leveled() => format!("{s:.0} words"),
        (_, Some(s)) => format!("{s:.0} wpm"),
        _ => format!("{wpm:.0} wpm"),
    };
    let title = format!("{result} · {name} · typepper");
    let game = format!("https://typepper.com/?replay={id}");
    // nothing here comes from the player but the id, which is base62
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>{title}</title>
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="Watch this round of typepper, a spicy little typing game.">
<meta property="og:type" content="website">
<meta property="og:site_name" content="typepper">
<meta property="og:title" content="{title}">
<meta property="og:description" content="Watch the round, key by key, then try to beat it.">
<meta property="og:url" content="{SHARE}/{id}">
<meta property="og:image" content="https://typepper.com/og.png">
<meta name="twitter:card" content="summary_large_image">
<meta http-equiv="refresh" content="0; url={game}">
<link rel="canonical" href="{game}">
</head>
<body><p><a href="{game}">{title}: watch it</a></p></body>
</html>
"#
    )
}

/// How many submissions a key (an ssh key, an address) may make an hour.
const PER_HOUR: u32 = 60;

/// An hourly allowance per caller, in memory (one server, no table needed).
#[derive(Default)]
pub struct Quota(Mutex<HashMap<String, (Instant, u32)>>);

impl Quota {
    /// Counts one submission for `who`; false once the hour's used up.
    pub fn take(&self, who: &str) -> bool {
        let mut seen = self.0.lock().unwrap();
        let now = Instant::now();
        // ponytail: forgets everyone past their hour at each call; fine for
        // thousands of callers, a timer if it ever isn't
        seen.retain(|_, (since, _)| now - *since < Duration::from_secs(3600));
        let (_, n) = seen.entry(who.to_string()).or_insert((now, 0));
        *n += 1;
        *n <= PER_HOUR
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_share_page_names_the_round() {
        let page = share_page(Id(123), "pursuit:7", Some(31.0), 64.2);
        assert!(page.contains("<title>31 words · HOT PURSUIT from lv7 · typepper</title>"));
        assert!(page.contains(&format!("https://typepper.com/?replay={}", Id(123))));
        let page = share_page(Id(1), "time:30", Some(112.0), 112.4);
        assert!(page.contains("112 wpm · BLITZ 30s"));
        let page = share_page(Id(1), "zen", None, 71.6);
        assert!(page.contains("72 wpm · ZEN"));
    }

    /// A database to test against (`TYPEPPER_TEST_DATABASE_URL`; CI has
    /// one), migrated. None: the test says so and passes.
    pub async fn test_db() -> Option<PgPool> {
        let url = std::env::var("TYPEPPER_TEST_DATABASE_URL").ok()?;
        let db = PgPool::connect(&url)
            .await
            .expect("the test database answers");
        sqlx::migrate!().run(&db).await.expect("migrations run");
        Some(db)
    }

    /// A finished BLITZ round typed clean at 120 ms a key (101 wpm), as bytes.
    fn a_round() -> Vec<u8> {
        use typepper_engine::{Input, Round, Setup};
        let mut r = Round::new(Setup {
            mode: Mode::Time(15),
            seed: 3,
        });
        let mut ms = 0;
        while r.test().finished.is_none() {
            ms += 120;
            let c = r.test().target[r.test().typed.len()];
            r.input(ms, Input::Char(c));
        }
        Replay::of(&r).encode()
    }

    #[tokio::test]
    async fn submissions_are_scored_here_and_stored() {
        let Some(db) = test_db().await else {
            return eprintln!("skipped: no TYPEPPER_TEST_DATABASE_URL");
        };
        let replay = a_round();
        let mut body = f64::NAN.to_le_bytes().to_vec();
        body.extend(&replay);
        body.extend(99.0f64.to_le_bytes()); // a wrong claim: logged, not believed
        body.extend(&replay);
        body.extend(b"junk");
        let answer = submit(&db, None, &body).await;
        let lines: Vec<&str> = answer.lines().collect();
        assert_eq!(lines.len(), 3, "{answer}");
        let ok: Vec<&str> = lines[0].split(' ').collect();
        assert_eq!((ok[0], ok[2], ok[3]), ("ok", "101", "-"), "{answer}");
        assert!(lines[1].starts_with("ok "));
        assert_eq!(lines[2], "err bad-replay truncated");
        let id: Id = ok[1].parse().unwrap();
        assert_eq!(replay_bytes(&db, id).await, replay);
        assert!(
            page(&db, id)
                .await
                .unwrap()
                .unwrap()
                .contains("101 wpm · BLITZ 15s")
        );

        let mut old = Replay::decode(&replay).unwrap();
        old.engine = VERSION + 1;
        let mut body = f64::NAN.to_le_bytes().to_vec();
        body.extend(old.encode());
        assert_eq!(
            submit(&db, None, &body).await,
            "err old-engine update typepper to submit\n"
        );
    }

    async fn replay_bytes(db: &PgPool, id: Id) -> Vec<u8> {
        replay(db, id).await.unwrap().unwrap()
    }

    #[test]
    fn an_hour_allows_sixty() {
        let q = Quota::default();
        assert!((0..PER_HOUR).all(|_| q.take("a")));
        assert!(!q.take("a"), "the 61st waits");
        assert!(q.take("b"), "others have their own");
    }
}
