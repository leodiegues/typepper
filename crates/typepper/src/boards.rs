//! The leaderboards: one mode's top 20 for today, this week, this month,
//! this year or all time, fetched when it opens and on each period. Full
//! screen from `b`, or as a strip beside the results chart: there it opens
//! on ←/→, or by itself once a linked player's round is in.

use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Paragraph, Wrap};
use web_time::Instant;

use crate::engine::Mode;
use crate::online::{self, Call, Row};
use crate::{best, ui};

/// The periods, as the server names them and as the screen says them.
const PERIODS: [(&str, &str); 5] = [
    ("day", "today"),
    ("week", "this week"),
    ("month", "this month"),
    ("year", "this year"),
    ("all", "all time"),
];

pub struct Boards {
    pub mode: Mode,
    period: usize,
    call: Option<Call>,
    /// None while it loads.
    rows: Option<Result<Vec<Row>, String>>,
}

impl Boards {
    /// `mode`'s board, today first.
    pub fn open(mode: Mode) -> Boards {
        let mut b = Boards {
            mode,
            period: 0,
            call: None,
            rows: None,
        };
        b.fetch();
        b
    }

    fn fetch(&mut self) {
        self.rows = None;
        match Call::board(&self.mode.key(), PERIODS[self.period].0) {
            Ok(call) => self.call = Some(call),
            Err(why) => (self.call, self.rows) = (None, Some(Err(why))),
        }
    }

    /// The next period over (←/→; 0 fetches this one again).
    pub fn step(&mut self, dir: i64) {
        self.period = (self.period as i64 + dir).rem_euclid(PERIODS.len() as i64) as usize;
        self.fetch();
    }

    pub fn tick(&mut self) {
        if let Some(call) = &mut self.call
            && let Some(answer) = call.poll()
        {
            self.rows = Some(answer.and_then(|text| online::rows(&text)));
            self.call = None;
        }
    }
}

pub fn draw(f: &mut Frame, b: &Boards, me: Option<&str>, now: Instant) {
    let _ = now;
    let col = ui::column(f.area());
    let [_, title, periods, resets, _, table, _, help] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(24),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(col);
    let name = match b.mode.start_level() {
        Some(lv @ 2..) => format!("{} from lv{lv}", b.mode.name()),
        _ => b.mode.name(),
    };
    f.render_widget(Line::from(name.bold()).centered(), title);
    let chips: Vec<Span> = PERIODS
        .iter()
        .enumerate()
        .flat_map(|(i, (_, said))| {
            let chip = format!(" {said} ");
            let chip = match i == b.period {
                true => chip.black().on_yellow().bold(),
                false => chip.dark_gray(),
            };
            [chip, " ".into()]
        })
        .collect();
    f.render_widget(Line::from(chips).centered(), periods);
    let period = PERIODS[b.period].0;
    if let Some(left) = resets_in(period, unix_now()) {
        let line = format!("resets in {} (UTC)", span(left));
        f.render_widget(Line::from(line.dark_gray()).centered(), resets);
    }
    let lines: Vec<Line> = match &b.rows {
        None => vec![Line::from("…".dark_gray()).centered()],
        Some(Err(why)) => {
            vec![Line::from(format!("couldn't load the board: {why}").dark_gray()).centered()]
        }
        Some(Ok(rows)) if rows.is_empty() => {
            vec![Line::from("nobody yet: play one and be first".dark_gray()).centered()]
        }
        Some(Ok(rows)) => {
            let mut lines = Vec::new();
            for r in rows {
                if r.me {
                    lines.push(Line::from("⋯".dark_gray()).centered());
                }
                let mine = mine(r, me);
                let who = match mine {
                    true => format!("{} (you)", r.login),
                    false => r.login.clone(),
                };
                let text = format!(
                    "{:>4}  {:<24} {:>10}  {}",
                    format!("#{}", r.rank),
                    who,
                    best::show(b.mode, r.score),
                    r.day
                );
                lines.push(match (mine, r.rank) {
                    (true, _) => Line::from(text.yellow().bold()).centered(),
                    (_, 1) => Line::from(text.white().bold()).centered(),
                    _ => Line::from(text.gray()).centered(),
                });
            }
            lines
        }
    };
    f.render_widget(Paragraph::new(lines), table);
    let hint = match me {
        Some(_) => "←/→ period · esc menu",
        None => "←/→ period · esc menu · `typepper link <github-login>` to be on it",
    };
    f.render_widget(Line::from(hint.dark_gray()).centered(), help);
}

/// The player's row: the server's `me`, or their login in the top 20.
fn mine(r: &Row, me: Option<&str>) -> bool {
    r.me || me.is_some_and(|m| m.eq_ignore_ascii_case(&r.login))
}

/// Where `score` would land among `rows` (a later run goes under a tie), or
/// None past a full top 20.
fn would_be(rows: &[Row], score: f64) -> Option<usize> {
    let rank = 1 + rows.iter().filter(|r| !r.me && r.score >= score).count();
    (rank <= TOP).then_some(rank)
}

const TOP: usize = 20;
/// The strip's rows under its period line.
const STRIP_ROWS: usize = 7;

/// The board beside the results chart, `b` none until it's opened (it says
/// `prompt` then). `score`: this round's, placed among the rows for a player
/// who isn't on them (not linked).
pub fn strip(
    f: &mut Frame,
    area: Rect,
    b: Option<&Boards>,
    me: Option<&str>,
    score: f64,
    prompt: &str,
) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Color::DarkGray)
        .title_bottom(Line::from(" b more ").dark_gray().right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);
    let Some(b) = b else {
        let [_, mid, _] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(1),
            Constraint::Fill(1),
        ])
        .areas(inner);
        f.render_widget(Line::from(prompt.dark_gray()).centered(), mid);
        return;
    };
    let mut lines = vec![
        Line::from(vec![
            "◂ ".dark_gray(),
            PERIODS[b.period].1.yellow().bold(),
            " ▸".dark_gray(),
        ])
        .centered(),
    ];
    let rows = match &b.rows {
        Some(Ok(rows)) => rows,
        None => {
            lines.push(Line::from("…".dark_gray()).centered());
            return f.render_widget(Paragraph::new(lines), inner);
        }
        Some(Err(why)) => {
            lines.push(Line::from(format!("couldn't load: {why}").dark_gray()));
            return f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
        }
    };
    // the width left for a login: rank, score and the spaces between
    let room = (inner.width as usize).saturating_sub(4 + 1 + 1 + 9 + 2);
    let line = |rank: String, who: String, score: f64| {
        let who = match who.chars().count() > room {
            true => format!(
                "{}…",
                who.chars().take(room.saturating_sub(1)).collect::<String>()
            ),
            false => who,
        };
        format!(" {rank:>4} {who:<room$} {:>9} ", best::show(b.mode, score))
    };
    // every row as it shows, and which one is the player
    let linked = rows.iter().any(|r| mine(r, me));
    let mut shown: Vec<(Line, bool)> = rows
        .iter()
        .map(|r| {
            let you = mine(r, me);
            let who = if you {
                format!("{} (you)", r.login)
            } else {
                r.login.clone()
            };
            let text = line(format!("#{}", r.rank), who, r.score);
            let text = match (you, r.rank) {
                (true, _) => text.yellow().bold(),
                (_, 1) => text.white().bold(),
                _ => text.gray(),
            };
            (Line::from(text), you)
        })
        .collect();
    if !linked && score.is_finite() {
        let (at, rank) = match would_be(rows, score) {
            Some(rank) => (rank - 1, format!("#{rank}?")),
            None => (shown.len(), format!("#{}+", TOP + 1)),
        };
        // the web can't link yet
        let you = match me.is_some() || cfg!(target_arch = "wasm32") {
            true => "you".to_string(),
            false => "you, if linked".to_string(),
        };
        shown.insert(
            at.min(shown.len()),
            (Line::from(line(rank, you, score).yellow().italic()), true),
        );
    }
    if shown.is_empty() {
        lines.push(Line::from("nobody yet: be first".dark_gray()).centered());
    }
    // the top, and the player under a ⋯ when they're further down
    let you = shown.iter().position(|(_, you)| *you).unwrap_or(0);
    match you < STRIP_ROWS {
        true => lines.extend(shown.into_iter().take(STRIP_ROWS).map(|(l, _)| l)),
        false => {
            let mine = shown.remove(you).0;
            lines.extend(shown.into_iter().take(STRIP_ROWS - 2).map(|(l, _)| l));
            lines.push(Line::from("⋯".dark_gray()).centered());
            lines.push(mine);
        }
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn unix_now() -> u64 {
    // web_time's SystemTime is the browser's clock on the web
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Seconds until `period`'s board starts over (UTC; weeks start Monday).
fn resets_in(period: &str, now: u64) -> Option<u64> {
    let day = now / 86_400;
    let (y, m, _) = civil(day as i64);
    let next = match period {
        "day" => day + 1,
        // 1970-01-01 was a Thursday
        "week" => day - (day + 3) % 7 + 7,
        "month" if m == 12 => days(y + 1, 1) as u64,
        "month" => days(y, m + 1) as u64,
        "year" => days(y + 1, 1) as u64,
        _ => return None,
    };
    Some(next * 86_400 - now)
}

/// Days since 1970-01-01 → (year, month, day) (Howard Hinnant's algorithm).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// The first of month `m` of year `y`, in days since 1970-01-01.
fn days(y: i64, m: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "3d 4h", "5h 12m", "12m".
fn span(secs: u64) -> String {
    let (d, h, m) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60);
    match (d, h) {
        (1.., _) => format!("{d}d {h}h"),
        (0, 1..) => format!("{h}h {m}m"),
        _ => format!("{}m", m.max(1)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unranked_rounds_land_under_ties() {
        let row = |rank, score| Row {
            rank,
            score,
            login: "x".into(),
            day: "2026-10-25".into(),
            me: false,
        };
        let rows: Vec<Row> = (1..=3).map(|i| row(i, 100.0 - 10.0 * i as f64)).collect();
        assert_eq!(would_be(&rows, 95.0), Some(1));
        assert_eq!(would_be(&rows, 80.0), Some(3), "a tie goes under");
        assert_eq!(would_be(&rows, 10.0), Some(4));
        assert_eq!(would_be(&[], 10.0), Some(1), "an empty board");
        let full: Vec<Row> = (1..=20).map(|i| row(i, 200.0 - i as f64)).collect();
        assert_eq!(would_be(&full, 181.0), Some(20));
        assert_eq!(would_be(&full, 10.0), None, "past a full top 20");
    }

    #[test]
    fn boards_reset_on_utc_calendar_lines() {
        // Saturday 2026-10-24 22:00:00 UTC
        let t = 1_792_879_200;
        assert_eq!(civil((t / 86_400) as i64), (2026, 10, 24));
        assert_eq!(resets_in("day", t), Some(2 * 3600));
        assert_eq!(
            resets_in("week", t),
            Some(26 * 3600),
            "Monday 2026-10-26 00:00"
        );
        assert_eq!(
            resets_in("month", t),
            Some((7 * 24 + 2) * 3600),
            "2026-11-01"
        );
        assert_eq!(
            resets_in("year", t),
            Some(((31 + 30 + 7) * 24 + 2) * 3600),
            "2027-01-01"
        );
        assert_eq!(resets_in("all", t), None);
        // December rolls into the next year
        let dec = (days(2026, 12) * 86_400 + 5) as u64;
        assert_eq!(resets_in("month", dec), Some(31 * 86_400 - 5));
        assert_eq!(days(1970, 1), 0);
        assert_eq!(civil(days(2024, 3)), (2024, 3, 1), "after a leap day");
        assert_eq!(
            (span(2 * 86_400 + 3 * 3600), span(3 * 3600 + 120), span(30)),
            ("2d 3h".into(), "3h 2m".into(), "1m".into())
        );
    }
}
