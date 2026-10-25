//! The leaderboards: each player's best per mode, all time or since the
//! current day, week, month or year began (UTC, weeks from Monday). Only
//! linked players' runs count, so names are GitHub logins.

use sqlx::{PgPool, Row};
use typepper_engine::Mode;

use crate::id::Id;

/// Rows a board shows.
pub const TOP: i64 = 20;

/// A board's span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Day,
    Week,
    Month,
    Year,
    All,
}

impl Period {
    pub fn parse(s: &str) -> Option<Period> {
        Some(match s {
            "day" => Period::Day,
            "week" => Period::Week,
            "month" => Period::Month,
            "year" => Period::Year,
            "all" => Period::All,
            _ => return None,
        })
    }

    /// `date_trunc`'s field, or none for all time.
    fn field(self) -> Option<&'static str> {
        match self {
            Period::Day => Some("day"),
            Period::Week => Some("week"),
            Period::Month => Some("month"),
            Period::Year => Some("year"),
            Period::All => None,
        }
    }
}

/// One row: where, who, what, when (`YYYY-MM-DD`, UTC), and the run, to
/// watch it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub rank: i64,
    pub login: String,
    pub score: f64,
    pub day: String,
    pub run: Id,
}

// every player's best in `mode` since `$2` began (or ever, for null), ranked;
// ties go to whoever got there first (a macro, so queries stay literals)
macro_rules! ranked {
    () => {
        "
    with best as (
        select distinct on (r.player_id) r.id, r.player_id, r.score, r.created_at
        from runs r
        where r.mode = $1 and r.player_id is not null and r.score is not null
          and ($2::text is null or r.created_at >= date_trunc($2, now(), 'UTC'))
        order by r.player_id, r.score desc, r.created_at
    )
    select rank() over (order by b.score desc, b.created_at) as rank,
           b.id as run, p.id as player, p.login, b.score,
           to_char(b.created_at at time zone 'UTC', 'YYYY-MM-DD') as day
    from best b join players p on p.id = b.player_id"
    };
}

/// The top of `mode`'s board for `period`, and `player`'s own row (if they
/// have one and it's below the top).
pub async fn board(
    db: &PgPool,
    mode: Mode,
    period: Period,
    player: Option<i64>,
) -> sqlx::Result<(Vec<Entry>, Option<Entry>)> {
    let rows = sqlx::query(concat!(
        "select * from (",
        ranked!(),
        ") ranked where rank <= $3 or player = $4 order by rank, login"
    ))
    .bind(mode.key())
    .bind(period.field())
    .bind(TOP)
    .bind(player)
    .fetch_all(db)
    .await?;
    let (mut top, mut me) = (Vec::new(), None);
    for r in rows {
        let entry = Entry {
            rank: r.get("rank"),
            login: r.get("login"),
            score: r.get("score"),
            day: r.get("day"),
            run: Id(r.get("run")),
        };
        match entry.rank <= TOP {
            true => top.push(entry),
            false => me = Some(entry),
        }
    }
    Ok((top, me))
}

/// `player`'s place on `mode`'s all-time board.
pub async fn rank(db: &PgPool, mode: Mode, player: i64) -> sqlx::Result<Option<i64>> {
    sqlx::query_scalar(concat!(
        "select rank from (",
        ranked!(),
        ") ranked where player = $3"
    ))
    .bind(mode.key())
    .bind(None::<String>)
    .bind(player)
    .fetch_optional(db)
    .await
}

/// `player`'s best in each mode, as `<mode> <score>` lines (the web merges
/// them into its own bests, so rounds played in the terminal count there too).
pub async fn bests(db: &PgPool, player: i64) -> sqlx::Result<String> {
    // ponytail: no index leads with player_id (the sweep keeps runs few); add
    // one if this shows up as slow
    let rows: Vec<(String, f64)> = sqlx::query_as(
        "select mode, max(score) from runs
         where player_id = $1 and score is not null
         group by mode order by mode",
    )
    .bind(player)
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(|(m, s)| format!("{m} {s}\n")).collect())
}

/// A board as text, one line per row (`<rank> <score> <login> <day> <run>`),
/// then the caller's own row as `me …` when it's below the top. Over SSH and
/// HTTP. `<run>` came last, so games that read four words still read it.
pub fn text(top: &[Entry], me: Option<&Entry>) -> String {
    let line = |e: &Entry| format!("{} {} {} {} {}", e.rank, e.score, e.login, e.day, e.run);
    let mut out: Vec<String> = top.iter().map(line).collect();
    if let Some(e) = me {
        out.push(format!("me {}", line(e)));
    }
    out.push(String::new());
    out.join("\n")
}

/// Forgets what no board needs any more: runs older than 8 days (the day and
/// week boards, with a day's margin for Sunday night) unless they're a
/// player's best for their mode in their calendar month (which also keeps
/// the year's and all time's). Anonymous runs go after the 8 days too.
pub async fn sweep(db: &PgPool) -> sqlx::Result<u64> {
    let done = sqlx::query(
        "delete from runs
         where created_at < now() - interval '8 days'
           and id not in (
             select distinct on (player_id, mode, date_trunc('month', created_at, 'UTC')) id
             from runs
             where player_id is not null and score is not null
             order by player_id, mode, date_trunc('month', created_at, 'UTC'), score desc, created_at
           )",
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts;
    use crate::id::Id;
    use crate::runs::tests::test_db;

    /// A run for `player` in `mode`, `days_ago`.
    async fn run(db: &PgPool, player: Option<i64>, mode: &str, score: f64, days_ago: i32) -> Id {
        let id = Id::random();
        sqlx::query(
            "insert into runs (id, player_id, replay, engine, mode, score, wpm, accuracy, created_at)
             values ($1, $2, '', 1, $3, $4, $4, 100, now() - make_interval(days => $5))",
        )
        .bind(id.0)
        .bind(player)
        .bind(mode)
        .bind(score)
        .bind(days_ago)
        .execute(db)
        .await
        .unwrap();
        id
    }

    async fn player(db: &PgPool, login: &str) -> i64 {
        let fpr = format!("SHA256:{login}-{}", Id::random());
        accounts::link(db, Id::random().0, login, &fpr)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn boards_rank_each_players_best() {
        let Some(db) = test_db().await else {
            return eprintln!("skipped: no TYPEPPER_TEST_DATABASE_URL");
        };
        // a mode no other test touches: a fresh board
        let mode = Mode::Survival { start_level: 10 };
        sqlx::query("delete from runs where mode = $1")
            .bind(mode.key())
            .execute(&db)
            .await
            .unwrap();
        let (a, b, c) = (
            player(&db, "ann").await,
            player(&db, "bob").await,
            player(&db, "cy").await,
        );
        run(&db, Some(a), &mode.key(), 30.0, 0).await;
        let best = run(&db, Some(a), &mode.key(), 50.0, 400).await; // ann's best, but last year
        run(&db, Some(b), &mode.key(), 40.0, 0).await;
        run(&db, Some(c), &mode.key(), 40.0, 0).await; // ties bob, later: below him
        run(&db, None, &mode.key(), 99.0, 0).await; // anonymous: never ranked

        let (top, _) = board(&db, mode, Period::All, None).await.unwrap();
        let names: Vec<(i64, &str, f64)> = top
            .iter()
            .map(|e| (e.rank, e.login.as_str(), e.score))
            .collect();
        assert_eq!(names, [(1, "ann", 50.0), (2, "bob", 40.0), (3, "cy", 40.0)]);
        assert_eq!(top[0].run, best, "a row is its best run, to watch");
        let (top, _) = board(&db, mode, Period::Week, None).await.unwrap();
        let names: Vec<&str> = top.iter().map(|e| e.login.as_str()).collect();
        assert_eq!(names, ["bob", "cy", "ann"], "this week ann only has 30");
        assert_eq!(rank(&db, mode, c).await.unwrap(), Some(3));
        assert_eq!(rank(&db, Mode::Zen, c).await.unwrap(), None);
        assert!(text(&top, None).starts_with("1 40 bob "));
        assert_eq!(bests(&db, a).await.unwrap(), format!("{} 50\n", mode.key()));
    }

    #[tokio::test]
    async fn the_sweep_keeps_what_boards_need() {
        let Some(db) = test_db().await else {
            return eprintln!("skipped: no TYPEPPER_TEST_DATABASE_URL");
        };
        let mode = "pursuit:10";
        sqlx::query("delete from runs where mode = $1")
            .bind(mode)
            .execute(&db)
            .await
            .unwrap();
        let p = player(&db, "dee").await;
        let recent = run(&db, Some(p), mode, 10.0, 2).await;
        let old_best = run(&db, Some(p), mode, 20.0, 100).await;
        let old_worse = run(&db, Some(p), mode, 15.0, 100).await;
        let old_anon = run(&db, None, mode, 30.0, 100).await;
        sweep(&db).await.unwrap();
        let left: Vec<i64> = sqlx::query_scalar("select id from runs where mode = $1")
            .bind(mode)
            .fetch_all(&db)
            .await
            .unwrap();
        assert!(left.contains(&recent.0), "the last 8 days stay");
        assert!(left.contains(&old_best.0), "a month's best stays");
        assert!(
            !left.contains(&old_worse.0),
            "an old run that isn't a best goes"
        );
        assert!(!left.contains(&old_anon.0), "old anonymous runs go");
    }
}
