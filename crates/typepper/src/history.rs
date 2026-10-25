//! Your rounds, kept here (the data dir, or localStorage on the web) so you
//! can watch them again, offline, and find the link of one you shared. One
//! store entry, `history`, one line per round, the newest last, at most 50:
//! `<unix ms> <mode key> <score|-> <link|-> <replay hex>`.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::engine::Mode;
use crate::{best, boards, store, ui};

/// Rounds kept.
const KEEP: usize = 50;
/// A bigger replay (hours of zen) is only kept as `last.replay`: localStorage
/// holds about 5 MB, and hex doubles it.
// ponytail: 50 rounds at the cap would still pass 5 MB (saves then fail and the
// history stops changing); typical rounds are ~3 KB of hex. Cap the total if it bites.
const MAX_BYTES: usize = 64 * 1024;
/// Rows on screen at once.
const SHOWN: usize = 20;

#[derive(Clone, Debug, PartialEq)]
pub struct Round {
    /// When it ended, in unix ms; also how a share finds its round again.
    pub at: u64,
    /// `Mode::key`.
    pub mode: String,
    /// None in zen.
    pub score: Option<f64>,
    /// Its share link (`r.typepper.com/<id>`), once it went up.
    pub link: Option<String>,
    pub replay: Vec<u8>,
}

impl Round {
    fn line(&self) -> String {
        let score = self.score.map_or("-".into(), |s| s.to_string());
        let link = self.link.as_deref().unwrap_or("-");
        let replay = store::hex(&self.replay);
        format!("{} {} {score} {link} {replay}", self.at, self.mode)
    }

    fn parse(line: &str) -> Option<Round> {
        let w: Vec<&str> = line.split(' ').collect();
        let [at, mode, score, link, replay] = w[..] else {
            return None;
        };
        Some(Round {
            at: at.parse().ok()?,
            mode: mode.into(),
            score: (score != "-").then(|| score.parse().ok()).flatten(),
            link: (link != "-").then(|| link.into()),
            replay: store::unhex(replay)?,
        })
    }
}

fn load() -> Vec<Round> {
    let text = store::read("history").unwrap_or_default();
    text.lines().filter_map(Round::parse).collect()
}

fn save(rounds: &[Round]) {
    let lines: Vec<String> = rounds.iter().map(Round::line).collect();
    // ponytail: a failed save only loses this round's copy; no error UI for it
    let _ = store::write("history", &lines.join("\n"));
}

/// `rounds` with `round` at the end, the oldest gone past `KEEP`.
fn pushed(mut rounds: Vec<Round>, round: Round) -> Vec<Round> {
    if round.replay.len() <= MAX_BYTES {
        rounds.push(round);
    }
    let over = rounds.len().saturating_sub(KEEP);
    rounds.drain(..over);
    rounds
}

/// Keeps a finished round; returns its `at`.
pub fn add(mode: Mode, score: Option<f64>, replay: Vec<u8>) -> u64 {
    let at = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let round = Round {
        at,
        mode: mode.key(),
        score,
        link: None,
        replay,
    };
    save(&pushed(load(), round));
    at
}

/// Writes down the link the round that ended `at` was shared under.
pub fn set_link(at: u64, link: &str) {
    let mut rounds = load();
    if let Some(r) = rounds.iter_mut().find(|r| r.at == at) {
        r.link = Some(link.into());
        save(&rounds);
    }
}

/// The history screen: your rounds, newest first.
pub struct History {
    pub rounds: Vec<Round>,
    sel: usize,
    /// Why the last enter didn't play (other rules, a mode this build lacks).
    pub note: Option<String>,
}

impl History {
    pub fn open() -> History {
        let mut rounds = load();
        rounds.reverse();
        History {
            rounds,
            sel: 0,
            note: None,
        }
    }

    /// Reads the rounds again (a link came in), keeping the same one picked.
    pub fn reload(&mut self) {
        let at = self.selected().map(|r| r.at);
        *self = History {
            note: self.note.take(),
            ..History::open()
        };
        self.sel = self
            .rounds
            .iter()
            .position(|r| Some(r.at) == at)
            .unwrap_or(0);
    }

    /// ↑/↓.
    pub fn step(&mut self, dir: i64) {
        let last = self.rounds.len().saturating_sub(1) as i64;
        self.sel = (self.sel as i64 + dir).clamp(0, last) as usize;
        self.note = None;
    }

    pub fn selected(&self) -> Option<&Round> {
        self.rounds.get(self.sel)
    }
}

/// `status`: how sharing the picked round is going, if it is.
pub fn draw(f: &mut Frame, h: &History, status: Option<&str>, online: bool) {
    let col = ui::column(f.area());
    let [_, title, _, note, above, table, below, help] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(SHOWN as u16),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(col);
    f.render_widget(Line::from("YOUR ROUNDS".bold()).centered(), title);
    if let Some(say) = h.note.as_deref().or(status) {
        f.render_widget(Line::from(say.dark_gray()).centered(), note);
    }
    if h.rounds.is_empty() {
        let say = "nothing yet: your rounds show up here once you've played";
        f.render_widget(Line::from(say.dark_gray()).centered(), table);
    }
    // a window around the picked round
    let from = h.sel.saturating_sub(SHOWN - 1);
    // ⋯ where rounds are out of sight
    let mut more = |area| f.render_widget(Line::from("⋯".dark_gray()).centered(), area);
    if from > 0 {
        more(above);
    }
    if from + SHOWN < h.rounds.len() {
        more(Rect { height: 1, ..below });
    }
    let now = boards::unix_now();
    let lines: Vec<Line> = (h.rounds.iter().enumerate().skip(from).take(SHOWN))
        .map(|(i, r)| {
            let mode = Mode::parse(&r.mode);
            let name = match mode {
                Some(m) => match m.start_level() {
                    Some(lv @ 2..) => format!("{} from lv{lv}", m.name()),
                    _ => m.name(),
                },
                None => r.mode.clone(),
            };
            let score = match (mode, r.score) {
                (Some(m), Some(s)) => best::show(m, s),
                _ => String::new(),
            };
            let ago = format!("{} ago", boards::span(now.saturating_sub(r.at / 1000)));
            let link = r.link.as_deref().unwrap_or("");
            let mark = if i == h.sel { "▸" } else { " " };
            let text = format!("{mark} {name:<26} {score:>10}  {ago:>10}  {link:<24}");
            match i == h.sel {
                true => Line::from(text.yellow().bold()).centered(),
                false => Line::from(text.gray()).centered(),
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), table);
    let hint = match online && !crate::settings::touch() {
        true if cfg!(target_arch = "wasm32") => {
            "↑/↓ pick · enter watch · u share or copy its link · esc menu"
        }
        true => "↑/↓ pick · enter watch · u share · esc menu",
        false => "↑/↓ pick · enter watch · esc menu",
    };
    f.render_widget(Line::from(hint.dark_gray()).centered(), help);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(at: u64, bytes: usize) -> Round {
        Round {
            at,
            mode: "time:15".into(),
            score: Some(108.5),
            link: None,
            replay: vec![0x54; bytes],
        }
    }

    #[test]
    fn rounds_survive_as_lines() {
        let shared = Round {
            link: Some("r.typepper.com/7Xf9qyuC".into()),
            ..round(1_792_879_200_123, 3)
        };
        let zen = Round {
            mode: "zen".into(),
            score: None,
            ..round(5, 0)
        };
        for r in [shared, zen] {
            assert_eq!(Round::parse(&r.line()), Some(r.clone()), "{}", r.line());
        }
        assert_eq!(Round::parse("garbage"), None);
        assert_eq!(Round::parse("1 zen - - zz"), None, "bad hex");
    }

    #[test]
    fn the_newest_fifty_stay() {
        let mut rounds = Vec::new();
        for at in 0..60 {
            rounds = pushed(rounds, round(at, 2));
        }
        assert_eq!(rounds.len(), KEEP);
        assert_eq!((rounds[0].at, rounds[KEEP - 1].at), (10, 59));
        let rounds = pushed(rounds, round(60, MAX_BYTES + 1));
        assert_eq!(rounds.last().unwrap().at, 59, "too big to keep");
    }
}
