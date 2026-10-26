//! A room for THE HOT ONE: friends at one table, typing the same words, each
//! on their own plate. The server (`online::Room`) deals the seed and relays
//! every input; the others' rounds are played here, through the same engine,
//! as their keys arrive, so you watch them race along the text live. Who's
//! out, and in what place, is the server's word (it plays each finished
//! round's replay). See docs/built/game-client.md.

use std::collections::BTreeMap;
use std::time::Duration;

use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use web_time::Instant;

use crate::engine::{Input, Mode, Replay, Round, STRIKES, Setup, replay};
use crate::fx::{Rgb, rgb};
use crate::online;
use crate::settings::touch;
use crate::{menu, ui};

/// A quiet line to the server this often, so nothing hangs up on a lobby.
const PING: Duration = Duration::from_secs(10);

/// Someone at the table.
pub struct Peer {
    pub name: String,
    /// Their round this game, played from the inputs the room relays.
    round: Option<Round>,
    /// Their words when the server said they were out.
    out: Option<u32>,
    /// They left mid-game.
    gone: bool,
}

/// A place in the standings: `(place, slot, words, when they went out)`.
type Place = (usize, usize, u32, u32);

enum Phase {
    /// Waiting for the server's `ok`.
    Joining,
    Lobby,
    /// `go` came: the round starts at `at`.
    Countdown {
        at: Instant,
        seed: u64,
    },
    Playing,
    Over(Vec<Place>),
    /// The connection's gone (why).
    Gone(String),
}

pub struct Room {
    link: Option<online::Room>,
    pub code: Option<String>,
    /// Your seat.
    pub me: Option<usize>,
    peers: BTreeMap<usize, Peer>,
    phase: Phase,
    /// Your round this game.
    pub round: Option<Round>,
    /// How much of your round's log has gone up.
    sent: usize,
    /// Your finished round has gone up.
    done: bool,
    pinged: Instant,
    /// The last one standing, once the server said.
    won: Option<usize>,
    /// Games played in this room (the screen scores each of yours once).
    pub games: u32,
    pub note: Option<String>,
}

impl Room {
    /// Makes a room (no code) or joins one.
    pub fn open(code: Option<&str>) -> Room {
        let now = Instant::now();
        let (link, phase) = match online::Room::open(code) {
            Ok(link) => (Some(link), Phase::Joining),
            Err(why) => (None, Phase::Gone(why)),
        };
        Room {
            link,
            code: None,
            me: None,
            peers: BTreeMap::new(),
            phase,
            round: None,
            sent: 0,
            done: false,
            pinged: now,
            won: None,
            games: 0,
            note: None,
        }
    }

    /// The lowest seat taken: whoever starts the games.
    pub fn host(&self) -> Option<usize> {
        self.peers.keys().next().copied()
    }

    pub fn playing(&self) -> bool {
        matches!(self.phase, Phase::Playing)
    }

    fn send(&mut self, line: &str) {
        if let Some(link) = &mut self.link {
            link.send(line);
        }
    }

    /// Enter: the host starts a game from the lobby; after one, back to the
    /// lobby for another.
    pub fn enter(&mut self) {
        match self.phase {
            Phase::Lobby if self.host() == self.me => self.send("start"),
            Phase::Lobby => self.note = Some("the host starts the game".into()),
            Phase::Over(_) => {
                self.phase = Phase::Lobby;
                self.round = None;
                for p in self.peers.values_mut() {
                    p.round = None;
                    p.out = None;
                }
            }
            _ => {}
        }
    }

    /// Reads what the room said, plays everyone's inputs, and sends yours.
    /// True when a new game just began: your round is fresh.
    pub fn tick(&mut self, now: Instant) -> bool {
        let lines = match self.link.as_mut().map(online::Room::poll) {
            Some(Ok(lines)) => lines,
            Some(Err(why)) => {
                self.link = None;
                if !matches!(self.phase, Phase::Gone(_)) {
                    self.phase = Phase::Gone(why);
                }
                Vec::new()
            }
            None => Vec::new(),
        };
        for line in lines {
            self.heard(&line, now);
        }
        let mut began = false;
        if let Phase::Countdown { at, seed } = self.phase
            && now >= at
        {
            let setup = Setup {
                mode: Mode::HotOne,
                seed,
            };
            self.round = Some(Round::new(setup));
            for p in self.peers.values_mut() {
                p.round = Some(Round::new(setup));
                p.out = None;
            }
            (self.sent, self.done, self.won) = (0, false, None);
            self.games += 1;
            self.phase = Phase::Playing;
            began = true;
        }
        if self.playing() {
            // yours: the clock runs, and every input goes up as it's logged
            let mut up = Vec::new();
            if let Some(r) = &mut self.round {
                r.advance(r.ms(now));
                for &(ms, input) in &r.log[self.sent..] {
                    up.push(format!("in {ms} {}", replay::op(input)));
                }
                self.sent = r.log.len();
                if r.test().finished.is_some() && !self.done {
                    self.done = true;
                    let hex: String = Replay::of(r)
                        .encode()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect();
                    up.push(format!("done {hex}"));
                }
            }
            for line in up {
                self.send(&line);
            }
            // theirs: the clock runs between their keys
            for p in self.peers.values_mut() {
                if let Some(r) = &mut p.round {
                    r.advance(r.ms(now));
                }
            }
        }
        if now.saturating_duration_since(self.pinged) >= PING {
            self.pinged = now;
            self.send("ping");
        }
        began
    }

    /// One line from the room.
    fn heard(&mut self, line: &str, now: Instant) {
        let w: Vec<&str> = line.trim().split(' ').collect();
        let num = |i: usize| w.get(i).and_then(|s| s.parse::<u64>().ok());
        match w[..] {
            ["ok", code, _] => {
                self.code = Some(code.to_string());
                self.me = num(2).map(|s| s as usize);
                self.phase = Phase::Lobby;
            }
            ["peer", _, name] => {
                if let Some(slot) = num(1) {
                    self.peers.insert(
                        slot as usize,
                        Peer {
                            name: name.to_string(),
                            round: None,
                            out: None,
                            gone: false,
                        },
                    );
                }
            }
            ["left", _] => {
                if let Some(slot) = num(1) {
                    match self.playing() {
                        true => {
                            if let Some(p) = self.peers.get_mut(&(slot as usize)) {
                                p.gone = true;
                            }
                        }
                        false => {
                            self.peers.remove(&(slot as usize));
                        }
                    }
                }
            }
            ["go", _, _] => {
                if let (Some(ms), Some(seed)) = (num(1), num(2)) {
                    self.phase = Phase::Countdown {
                        at: now + Duration::from_millis(ms),
                        seed,
                    };
                    self.note = None;
                    self.peers.retain(|_, p| !p.gone);
                }
            }
            ["in", _, _, _] => {
                if let (Some(slot), Some(ms), Some(op)) = (num(1), num(2), num(3))
                    && let Some(input) = replay::input(op)
                    && let Some(r) = self
                        .peers
                        .get_mut(&(slot as usize))
                        .and_then(|p| p.round.as_mut())
                {
                    r.input(ms as u32, input);
                }
            }
            ["out", _, _] => {
                if let (Some(slot), Some(words)) = (num(1), num(2))
                    && let Some(p) = self.peers.get_mut(&(slot as usize))
                {
                    p.out = Some(words as u32);
                }
            }
            // the last one standing: your round ends here, a winner
            ["win", _] => {
                self.won = num(1).map(|s| s as usize);
                if self.won == self.me
                    && let Some(r) = &mut self.round
                {
                    r.input(r.ms(now), Input::End);
                }
            }
            ["place", ..] => {
                if let (Some(n), Some(slot), Some(words), Some(ms)) =
                    (num(1), num(2), num(3), num(4))
                {
                    let place = (n as usize, slot as usize, words as u32, ms as u32);
                    match &mut self.phase {
                        Phase::Over(places) => places.push(place),
                        _ => self.phase = Phase::Over(vec![place]),
                    }
                }
            }
            ["over"] => {
                if !matches!(self.phase, Phase::Over(_)) {
                    self.phase = Phase::Over(Vec::new());
                }
                self.peers.retain(|_, p| !p.gone);
            }
            ["pong"] => {}
            ["err", code, ..] => {
                let why = w[2..].join(" ");
                match code {
                    // before a seat: there's no room to be in
                    "no-room" | "full" | "playing" | "busy" if self.me.is_none() => {
                        self.link = None;
                        self.phase = Phase::Gone(why);
                    }
                    _ => self.note = Some(why),
                }
            }
            _ => {}
        }
    }

    /// Everyone's color, by seat.
    fn color(slot: usize) -> Rgb {
        ui::PLATE_COLORS[slot % ui::PLATE_COLORS.len()]
    }

    /// The others' carets on your text.
    fn ghosts(&self) -> Vec<ui::Ghost> {
        self.peers
            .iter()
            .filter(|(slot, p)| Some(**slot) != self.me && p.out.is_none() && !p.gone)
            .filter_map(|(&slot, p)| {
                let r = p.round.as_ref()?;
                Some(ui::Ghost {
                    at: r.test().typed.len(),
                    color: rgb(Self::color(slot)),
                    initial: p.name.chars().next().unwrap_or('?'),
                })
            })
            .collect()
    }

    fn name_of(&self, slot: usize) -> String {
        self.peers
            .get(&slot)
            .map_or(format!("seat {}", slot + 1), |p| p.name.clone())
    }
}

/// `hover`: the pepper the player is on, when a plate's up (see `ui::draw`).
pub fn draw(f: &mut Frame, room: &Room, hover: usize, now: Instant) {
    let col = ui::column(f.area());
    let (r, g, b) = menu::HOT_PINK;
    let pink = Color::Rgb(r, g, b);
    let leave = if touch() { "✕ leave" } else { "esc leave" };
    match &room.phase {
        Phase::Joining => centered(f, col, "finding a table…", leave),
        Phase::Gone(why) => centered(f, col, why, leave),
        Phase::Lobby | Phase::Countdown { .. } => {
            let [_, title, how, _, table, _, help] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(12),
                Constraint::Fill(1),
                Constraint::Length(1),
            ])
            .areas(col);
            let code = room.code.clone().unwrap_or_default();
            let spaced: String = code.chars().flat_map(|c| [c, ' ']).collect();
            f.render_widget(
                Line::from(vec![
                    "ROOM  ".dark_gray(),
                    spaced.trim_end().fg(pink).bold(),
                ])
                .centered(),
                title,
            );
            f.render_widget(
                Line::from("friends join from THE HOT ONE's card with that code".dark_gray())
                    .centered(),
                how,
            );
            roster(f, room, table, now);
            let host = room.host();
            let hint = match (&room.phase, host == room.me, host) {
                (Phase::Countdown { .. }, ..) => "get ready…".to_string(),
                (_, true, _) if touch() => format!("tap start · {leave}"),
                (_, true, _) => format!("enter start · {leave}"),
                (_, false, Some(h)) => format!("{} starts the game · {leave}", room.name_of(h)),
                (_, false, None) => leave.to_string(),
            };
            let hint = room.note.clone().unwrap_or(hint);
            f.render_widget(Line::from(hint.dark_gray()).centered(), help);
            if let Phase::Countdown { at, .. } = room.phase {
                countdown(f, col, at, now);
            }
        }
        Phase::Playing => {
            let Some(round) = &room.round else { return };
            let t = round.test();
            let [_, _, below, _] = ui::play_layout(f.area());
            if t.finished.is_none() {
                let hint = format!("{leave} the table");
                ui::play(
                    f,
                    t,
                    None,
                    None,
                    false,
                    Some(hover),
                    &room.ghosts(),
                    Some(&hint),
                );
            } else {
                let [_, out, words, _] = Layout::vertical([
                    Constraint::Fill(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Fill(1),
                ])
                .areas(ui::play_layout(f.area())[1]);
                let say = match room.won == room.me && room.me.is_some() {
                    true => "LAST ONE STANDING".fg(pink).bold(),
                    false => "YOU BIT THE HOT ONE".fg(pink).bold(),
                };
                f.render_widget(Line::from(say).centered(), out);
                f.render_widget(
                    Line::from(format!(
                        "{} words · the others are still at the table",
                        t.words
                    ))
                    .dark_gray()
                    .centered(),
                    words,
                );
            }
            race(f, room, below, now);
        }
        Phase::Over(places) => {
            let [_, title, _, table, _, help] = Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(10),
                Constraint::Fill(1),
                Constraint::Length(1),
            ])
            .areas(col);
            let winner = places.first().map(|p| p.1);
            let say = match winner {
                Some(w) if Some(w) == room.me => "YOU'RE THE LAST ONE STANDING".to_string(),
                Some(w) => format!("{} IS THE LAST ONE STANDING", room.name_of(w)),
                None => "NOBODY LEFT STANDING".to_string(),
            };
            f.render_widget(Line::from(say.fg(pink).bold()).centered(), title);
            let rows: Vec<Line> = places
                .iter()
                .map(|&(n, slot, words, ms)| {
                    let (cr, cg, cb) = Room::color(slot);
                    let you = if Some(slot) == room.me { "  (you)" } else { "" };
                    Line::from(vec![
                        format!("{n:>2}. ").dark_gray(),
                        format!("{:<16}", room.name_of(slot))
                            .fg(Color::Rgb(cr, cg, cb))
                            .bold(),
                        format!("{words:>4} words").into(),
                        format!("   out at {}", clock(ms)).dark_gray(),
                        you.dark_gray(),
                    ])
                })
                .collect();
            let w = 58.min(table.width);
            let at = Rect::new(table.x + (table.width - w) / 2, table.y, w, table.height);
            f.render_widget(Paragraph::new(rows), at);
            let again = match (room.host() == room.me, touch()) {
                (true, true) => "tap another",
                (true, false) => "enter another",
                (false, _) => "enter back to the table",
            };
            f.render_widget(
                Line::from(format!("{again} · {leave}").dark_gray()).centered(),
                help,
            );
        }
    }
}

fn centered(f: &mut Frame, col: Rect, say: &str, hint: &str) {
    let [_, mid, _, help] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(col);
    f.render_widget(Line::from(say.to_string()).centered(), mid);
    f.render_widget(Line::from(hint.to_string().dark_gray()).centered(), help);
}

/// Who's at the table, the host and you marked.
fn roster(f: &mut Frame, room: &Room, area: Rect, now: Instant) {
    let host = room.host();
    let blink = (now.elapsed().as_millis() / 600).is_multiple_of(2);
    let mut rows: Vec<Line> = room
        .peers
        .iter()
        .map(|(&slot, p)| {
            let (cr, cg, cb) = Room::color(slot);
            let mut spans = vec![
                "● ".fg(Color::Rgb(cr, cg, cb)),
                format!("{:<16}", p.name).into(),
            ];
            if Some(slot) == host {
                spans.push("host".dark_gray());
            }
            if Some(slot) == room.me {
                spans.push("  you".fg(Color::Rgb(cr, cg, cb)));
            }
            Line::from(spans)
        })
        .collect();
    if room.peers.len() < 2 && blink {
        rows.push(Line::from("○ waiting for a friend…".dark_gray()));
    }
    let w = 36.min(area.width);
    let at = Rect::new(area.x + (area.width - w) / 2, area.y, w, area.height);
    f.render_widget(Paragraph::new(rows), at);
}

/// 3, 2, 1 in big digits over the lobby.
fn countdown(f: &mut Frame, col: Rect, at: Instant, now: Instant) {
    let left = at.saturating_duration_since(now).as_secs_f64();
    let n = left.ceil().clamp(1.0, 9.0) as u64;
    let rows: Vec<Line> = ui::big(n)
        .iter()
        .map(|r| Line::from(ui::solid(r, Color::White)))
        .collect();
    let w = rows[0].width() as u16 + 4;
    let rect = Rect::new(
        col.x + col.width.saturating_sub(w) / 2,
        col.y + (col.height / 2).saturating_sub(4),
        w.min(col.width),
        7,
    );
    f.render_widget(Clear, rect);
    f.render_widget(
        Paragraph::new(rows).centered().block(
            Block::bordered()
                .border_type(BorderType::Thick)
                .border_style(Color::White),
        ),
        rect,
    );
}

/// Everyone's row: a bar to the leader's word count, their words, and how
/// they're doing (strikes, a plate, chewing, out).
fn race(f: &mut Frame, room: &Room, area: Rect, now: Instant) {
    if area.height < 2 {
        return;
    }
    let area = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    let mut seats: Vec<Seat> = room
        .peers
        .iter()
        .map(|(&slot, p)| (slot, p.name.as_str(), p.round.as_ref(), p.out, p.gone))
        .collect();
    if let (Some(me), Some(r)) = (room.me, &room.round) {
        seats.retain(|s| s.0 != me);
        seats.push((me, "you", Some(r), None, false));
        seats.sort_by_key(|s| s.0);
    }
    let words = |r: Option<&Round>| r.map_or(0, |r| r.test().words);
    let lead = seats
        .iter()
        .map(|s| s.3.unwrap_or(words(s.2)))
        .max()
        .unwrap_or(0)
        .max(10);
    let narrow = area.width < 60;
    let bar_w = if narrow { 12 } else { 24 };
    for (i, (slot, name, round, out, gone)) in seats.iter().enumerate() {
        if i as u16 >= area.height {
            break;
        }
        let (cr, cg, cb) = Room::color(*slot);
        let c = Color::Rgb(cr, cg, cb);
        let n = out.unwrap_or(words(*round));
        let fill = ((n as f64 / lead as f64) * bar_w as f64).round() as usize;
        let state = match (gone, out, round.map(Round::test)) {
            (true, ..) => "left".to_string(),
            (_, Some(_), _) => "OUT".to_string(),
            (_, _, Some(t)) if t.chewing.is_some() => "chewing…".to_string(),
            (_, _, Some(t)) if t.plate.is_some() => "picking a pepper".to_string(),
            (_, _, Some(t)) if t.finished.is_some() => "done".to_string(),
            (_, _, Some(t)) => format!(
                "{}{}",
                "●".repeat(t.strikes.min(STRIKES) as usize),
                "○".repeat((STRIKES - t.strikes.min(STRIKES)) as usize)
            ),
            _ => String::new(),
        };
        let name = format!("{:<10}", name.chars().take(10).collect::<String>());
        let line = Line::from(vec![
            name.fg(c).bold(),
            Span::styled("▰".repeat(fill.min(bar_w)), c),
            Span::styled("▱".repeat(bar_w - fill.min(bar_w)), Color::DarkGray),
            format!(" {n:>3}w  ").into(),
            state.dark_gray(),
        ]);
        let w = line.width() as u16;
        let at = Rect::new(
            area.x + area.width.saturating_sub(w) / 2,
            area.y + i as u16,
            w.min(area.width),
            1,
        );
        f.render_widget(line, at);
    }
    let _ = now;
}

/// A row of the race: seat, name, their round, their words once out, gone.
type Seat<'a> = (usize, &'a str, Option<&'a Round>, Option<u32>, bool);

/// Round milliseconds as m:ss.
fn clock(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A room that never connected: it still plays the lines a server would say.
    fn table() -> Room {
        let mut room = Room::open(Some("ZZZZ"));
        room.link = None;
        room.phase = Phase::Joining;
        room
    }

    #[test]
    fn a_room_plays_the_servers_lines() {
        let t0 = Instant::now();
        let mut room = table();
        room.heard("ok KXQP 1", t0);
        room.heard("peer 0 leo", t0);
        room.heard("peer 1 guest-2", t0);
        assert_eq!(
            (room.code.as_deref(), room.me, room.host()),
            (Some("KXQP"), Some(1), Some(0))
        );
        assert!(matches!(room.phase, Phase::Lobby));
        room.heard("go 0 42", t0);
        assert!(room.tick(t0 + Duration::from_millis(1)), "a game began");
        assert!(room.playing() && room.round.is_some() && room.games == 1);
        let setup = room.round.as_ref().unwrap().setup;
        assert_eq!(
            setup,
            Setup {
                mode: Mode::HotOne,
                seed: 42
            }
        );
        // the host types the first letter: their caret ghosts along your text
        let first = room.round.as_ref().unwrap().test().target[0];
        room.heard(&format!("in 0 300 {}", replay::op(Input::Char(first))), t0);
        let ghosts = room.ghosts();
        assert_eq!((ghosts.len(), ghosts[0].at, ghosts[0].initial), (1, 1, 'l'));
        room.heard("out 0 7", t0);
        assert!(room.ghosts().is_empty(), "out: off the text");
        // the last one standing: your round ends, and the places come
        room.heard("win 1", t0);
        assert!(room.round.as_ref().unwrap().test().finished.is_some());
        room.heard("place 1 1 0 1200", t0);
        room.heard("place 2 0 7 900", t0);
        room.heard("over", t0);
        assert!(matches!(&room.phase, Phase::Over(p) if p.len() == 2 && p[0].1 == 1));
        room.enter();
        assert!(matches!(room.phase, Phase::Lobby) && room.round.is_none());
        room.heard("err no-room nope", t0);
        assert_eq!(room.note.as_deref(), Some("nope"), "seated: just a note");
        assert_eq!(clock(61_000), "1:01");
    }

    #[test]
    fn a_refused_join_is_gone() {
        let mut room = table();
        room.heard("err no-room no room has that code", Instant::now());
        assert!(matches!(&room.phase, Phase::Gone(w) if w == "no room has that code"));
    }
}
