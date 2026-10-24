//! Platform-neutral app: what a key does, what a frame shows.
//! `main.rs` feeds it from the terminal (crossterm) or the browser (ratzilla).

use std::time::Duration;

use rand::RngExt;
use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::style::Stylize;
use ratatui::text::Line;
use web_time::Instant;

use crate::best::{Bests, Outcome};
use crate::boards::{self, Boards};
use crate::engine::{Game, Input, LEVELS, Mode, Replay, Round, Setup, Test};
use crate::lava;
use crate::menu::{self, Dialog, Menu, Pick};
use crate::online::{self, Call};
use crate::settings::{ROWS, Settings, touch};
use crate::ui;

pub enum Key {
    Esc,
    Tab,
    Enter,
    Left,
    Right,
    Up,
    Down,
    Backspace,
    /// F2, or the web page’s ⚙ button on phones (no s key to press there).
    Settings,
    Char(char),
}

#[allow(clippy::large_enum_variant)] // only one Screen is ever alive
enum Screen {
    Title,
    Play(Round),
    Watch(Watch),
    Boards(Boards),
}

/// Rounds going up: a shared one (anonymous), or a linked player's outbox.
/// One at a time; it carries on if the player moves on, and only the
/// results screen it started on talks about it.
struct Share {
    /// The screen it started on (`App::screens`).
    screen: u64,
    /// The outbox entries it carries (none: an anonymous share).
    outbox: Vec<String>,
    /// Under way, or how it went for this round: its link, or why not.
    state: Result<Call, Result<String, String>>,
}

/// A replay playing back: its inputs land on a fresh round as their time comes.
struct Watch {
    round: Round,
    inputs: Vec<(u32, Input)>,
    next: usize,
}

impl Screen {
    /// The round on screen, played or watched (or just finished).
    fn round(&self) -> Option<&Round> {
        match self {
            Screen::Play(r) => Some(r),
            Screen::Watch(w) => Some(&w.round),
            Screen::Title | Screen::Boards(_) => None,
        }
    }

    fn test(&self) -> Option<&Test> {
        self.round().map(Round::test)
    }
}

pub struct App {
    screen: Screen,
    /// Kept across tests so esc lands back on the mode you played.
    menu: Menu,
    bests: Bests,
    settings: Settings,
    /// The theme the terminal (or browser) asked for, for `auto`.
    detected_light: bool,
    /// The font and theme this page started with: the web renderer takes
    /// both only at startup, so a different pick reloads the page.
    loaded: (usize, bool),
    /// The settings dialog, open over the menu.
    dialog: Option<Dialog>,
    /// The starting-level picker, over a leveled game before it begins.
    pick: Option<Pick>,
    /// The finished test's result against your best (once it's been scored).
    outcome: Option<Outcome>,
    scored: bool,
    since: Instant,
    bot: Option<Bot>,
    /// The last frame drawn, and the one crumbling away since a screen change.
    frame: Option<Buffer>,
    fade: Option<(Buffer, Instant)>,
    share: Option<Share>,
    /// Screens shown so far (tells a share's results screen from the next).
    screens: u64,
    /// The GitHub login this machine is linked to (`typepper link`): its
    /// rounds go up on their own.
    account: Option<String>,
}

impl App {
    /// `bot`: the demo tour or the autopilot typist, if any (see `Bot`).
    /// `watch`: a replay to play back first (its setup already checked).
    pub fn new(bot: Option<Bot>, watch: Option<(Setup, Replay)>) -> Self {
        let now = Instant::now();
        let detected_light = crate::theme::is_light();
        let settings = Settings::load();
        settings.apply(detected_light);
        let screen = match watch {
            Some((setup, replay)) => Screen::Watch(Watch {
                round: Round::new(setup),
                inputs: replay.inputs,
                next: 0,
            }),
            None => Screen::Title,
        };
        App {
            screen,
            menu: Menu::new(now),
            bests: Bests::load(),
            settings,
            detected_light,
            loaded: (settings.font, crate::theme::is_light()),
            dialog: None,
            pick: None,
            outcome: None,
            scored: false,
            since: now,
            bot,
            frame: None,
            fade: None,
            share: None,
            screens: 0,
            account: online::account(),
        }
    }

    /// Returns true when the player wants out (Esc on the title screen).
    pub fn key(&mut self, key: Key) -> bool {
        let now = Instant::now();
        let mut next = None;
        if let (Screen::Title, Some(d)) = (&self.screen, &mut self.dialog) {
            match key {
                Key::Up => d.row = (d.row + ROWS.len() - 1) % ROWS.len(),
                Key::Down => d.row = (d.row + 1) % ROWS.len(),
                Key::Left | Key::Right => {
                    let dir = if matches!(key, Key::Left) { -1 } else { 1 };
                    self.settings.change(ROWS[d.row], dir);
                    self.settings.apply(self.detected_light);
                    self.settings.save();
                    d.changed = Some(now);
                }
                Key::Esc | Key::Enter | Key::Char('s') | Key::Settings => {
                    self.dialog = None;
                    if (self.settings.font, crate::theme::is_light()) != self.loaded {
                        reload();
                    }
                }
                _ => {}
            }
            return false;
        }
        if let Some(p) = &mut self.pick {
            match key {
                Key::Left | Key::Up => p.step(-1, now),
                Key::Right | Key::Down => p.step(1, now),
                Key::Enter | Key::Char(' ') => self.start_from(),
                Key::Esc => {
                    self.menu.back(now);
                    self.go(Screen::Title, now);
                }
                _ => {}
            }
            return false;
        }
        // in any test: tab restarts (at once mid-test; every other screen change
        // dissolves), enter on the results plays again, from the same level
        let test = self.screen.test();
        if let Some((mode, done)) = test.map(|t| (t.mode, t.finished.is_some()))
            && (matches!(key, Key::Tab) || matches!(key, Key::Enter) && done)
        {
            let instant = matches!(key, Key::Tab) && !done;
            self.go(practice(mode), now);
            if instant {
                self.fade = None;
            }
            return false;
        }
        // the results screen: u shares the round (a linked player's goes up
        // on its own: u tries again if it couldn't)
        if let (Screen::Play(r), Key::Char('u')) = (&self.screen, &key)
            && r.test().finished.is_some()
            && !self.settings.offline
            && self.may_send()
        {
            let score = r.result().and_then(|o| o.score);
            let body = online::submission(&Replay::of(r).encode(), score);
            match self.account {
                Some(_) => self.send_outbox(),
                None => self.send(body, Vec::new()),
            }
            return false;
        }
        match (&mut self.screen, key) {
            (Screen::Title, Key::Esc) => return true,
            // zen has no clock: esc is how it ends
            (Screen::Play(r), Key::Esc)
                if r.test().mode == Mode::Zen
                    && r.test().started.is_some()
                    && r.test().finished.is_none() =>
            {
                r.input(r.ms(now), Input::End)
            }
            (_, Key::Esc) => {
                self.menu.back(now);
                next = Some(Screen::Title);
            }
            // enter was pressed: the card is springing, the test is coming
            (Screen::Title, _) if self.menu.pressing() => {}
            (Screen::Title, Key::Char('s') | Key::Settings) => {
                self.dialog = Some(Dialog::new(now));
            }
            (Screen::Title, Key::Left) => self.menu.swipe(-1, now),
            (Screen::Title, Key::Right) => self.menu.swipe(1, now),
            (Screen::Title, Key::Up) => self.menu.length(-1, now),
            (Screen::Title, Key::Down) => self.menu.length(1, now),
            (Screen::Title, Key::Enter | Key::Char(' ')) => self.menu.press(now),
            // the boards of the card's mode (from its starting level)
            (Screen::Title, Key::Char('b')) if !self.settings.offline => {
                let mode = self.menu.mode();
                let mode = mode.from_level(LEVELS[self.menu.start_of(mode)]);
                next = Some(Screen::Boards(Boards::open(mode)));
            }
            (Screen::Boards(b), Key::Left) => b.step(-1),
            (Screen::Boards(b), Key::Right) => b.step(1),
            (Screen::Boards(_), _) => {}
            (Screen::Title, _) => {}
            (Screen::Play(r), key) => match key {
                Key::Backspace => r.input(r.ms(now), Input::Backspace),
                Key::Char(c) => r.input(r.ms(now), Input::Char(c)),
                _ => {}
            },
            // watching: hands off (esc leaves, tab or enter plays the mode yourself)
            (Screen::Watch(_), _) => {}
        }
        if let Some(s) = next {
            self.go(s, now);
        }
        false
    }

    pub fn tick(&mut self, now: Instant) {
        if let Some(share) = &mut self.share
            && let Ok(up) = &mut share.state
            && let Some(answer) = up.poll()
        {
            share.state = Err(answer.and_then(|text| {
                online::outbox_settle(&share.outbox, &text);
                // this round is the last one sent
                online::answer(text.lines().last().unwrap_or(""))
            }));
        }
        let mut next = None;
        // demo: take the suggested starting level after a moment
        let touring = self.bot.as_ref().is_some_and(|b| b.tour);
        if touring && self.pick.as_ref().is_some_and(|p| p.age(now) > 0.8) {
            self.start_from();
        }
        let idle = now.saturating_duration_since(self.since);
        let picking = self.pick.is_some();
        // leveled modes ask for a starting level before they begin
        let mut ask = None;
        match &mut self.screen {
            Screen::Title if self.menu.press_done(now) => {
                let mode = self.menu.mode();
                next = Some(practice(mode));
                ask = mode.leveled().then_some(mode);
            }
            // demo: show off the carousel for a lap, then play
            Screen::Title if touring && idle > Duration::from_millis(1200) => {
                if self.menu.lap_done() {
                    self.menu.press(now);
                } else {
                    self.menu.swipe(1, now);
                    self.since = now;
                }
            }
            Screen::Title => {}
            Screen::Boards(b) => b.tick(),
            Screen::Watch(w) => {
                let ms = w.round.ms(now);
                while let Some(&(at, input)) = w.inputs.get(w.next).filter(|(at, _)| *at <= ms) {
                    w.round.input(at, input);
                    w.next += 1;
                }
                w.round.advance(ms);
            }
            Screen::Play(r) => {
                if let Some(bot) = self.bot.as_mut().filter(|_| !picking) {
                    bot.drive(r, now);
                    let over = r.test().finished;
                    if touring && over.is_some_and(|f| now - f > Duration::from_secs(4)) {
                        next = Some(practice(r.setup.mode));
                    }
                }
                r.advance(r.ms(now));
            }
        }
        // finished: dissolve into results, and score it (demo runs don't count)
        if let Some(r) = self.screen.round()
            && let Some(round) = r.result()
            && !self.scored
        {
            self.scored = true;
            self.fade = self.frame.take().map(|b| (b, now));
            // a watched replay is someone's past round: nothing to keep
            if let Screen::Play(r) = &self.screen {
                // the last round, for `--replay` (and, later, sharing)
                crate::store::write_bytes("last.replay", &Replay::of(r).encode());
                if self.bot.is_none() {
                    self.outcome = self.bests.record(r.setup.mode, &round);
                    // a linked player's rounds go up on their own
                    if self.account.is_some() && !self.settings.offline {
                        online::outbox_add(&Replay::of(r).encode(), round.score);
                        if self.may_send() {
                            self.send_outbox();
                        }
                    }
                }
            }
        }
        if let Some(s) = next {
            self.go(s, now);
        }
        if let Some(mode) = ask {
            self.pick = Some(Pick::new(self.menu.start_of(mode), now));
        }
    }

    /// Close the level picker and start the game from its choice.
    fn start_from(&mut self) {
        let Some(p) = self.pick.take() else { return };
        if let Screen::Play(r) = &mut self.screen {
            let mode = r.setup.mode.from_level(LEVELS[p.choice]);
            self.menu.set_start(mode, p.choice);
            *r = Round::new(Setup { mode, ..r.setup });
        }
    }

    pub fn draw(&mut self, f: &mut Frame) {
        let now = Instant::now();
        match &self.screen {
            Screen::Title => {
                menu::draw(f, &self.menu, &self.bests, now);
                if let Some(d) = &self.dialog {
                    menu::dialog(f, d, &self.settings, now);
                }
            }
            Screen::Boards(b) => boards::draw(f, b, self.account.as_deref(), now),
            Screen::Play(r) | Screen::Watch(Watch { round: r, .. }) => match &r.game {
                Game::Lava(l) if l.test.finished.is_none() => lava::draw(f, l, now),
                _ => ui::draw(f, r.test(), self.outcome, self.share_hint().as_deref()),
            },
        }
        if let (Some(p), Some(t)) = (&self.pick, self.screen.test()) {
            menu::pick(f, p, t.mode, &self.bests, now);
        }
        if self.bot.as_ref().is_some_and(|b| b.tour) {
            f.render_widget(Line::from(" DEMO ").black().on_magenta().bold(), f.area());
        }
        if matches!(self.screen, Screen::Watch(_)) {
            f.render_widget(Line::from(" REPLAY ").black().on_cyan().bold(), f.area());
        }
        crate::theme::apply(f.buffer_mut());
        if let Some((old, at)) = &self.fade {
            let k = now.saturating_duration_since(*at).as_secs_f64() / crate::fx::DISSOLVE;
            match k < 1.0 {
                true => crate::fx::dissolve(f.buffer_mut(), old, k),
                false => self.fade = None,
            }
        }
        // keep the frame for the next dissolve, reusing last frame's memory
        let buf = f.buffer_mut();
        match &mut self.frame {
            Some(old) if old.area == buf.area => old.content.clone_from(&buf.content),
            _ => self.frame = Some(buf.clone()),
        }
    }

    /// Nothing under way, and this screen's round hasn't gone up yet.
    fn may_send(&self) -> bool {
        match &self.share {
            Some(s) if s.state.is_ok() => false,
            Some(s) => s.screen != self.screens || s.state.as_ref().is_err_and(|r| r.is_err()),
            None => true,
        }
    }

    fn send(&mut self, body: Vec<u8>, outbox: Vec<String>) {
        self.share = Some(Share {
            screen: self.screens,
            outbox,
            state: Call::submit(body).map_err(Err),
        });
    }

    fn send_outbox(&mut self) {
        let (names, body) = online::outbox();
        if !names.is_empty() {
            self.send(body, names);
        }
    }

    /// The results screen's word on sharing (none offline, on touch screens
    /// with no u to press, or under someone else's replay).
    fn share_hint(&self) -> Option<String> {
        if self.settings.offline || touch() || !matches!(self.screen, Screen::Play(_)) {
            return None;
        }
        let share = self.share.as_ref().filter(|s| s.screen == self.screens);
        let (going, done, again) = match self.account {
            Some(_) => ("saving…", "saved", "it'll go up with the next one · u now"),
            None if cfg!(target_arch = "wasm32") => ("sharing…", "copied", "u again"),
            None => ("sharing…", "shared", "u again"),
        };
        Some(match share.map(|s| &s.state) {
            None if self.account.is_some() => "saved for later".into(),
            None => "u share".into(),
            Some(Ok(_)) => going.into(),
            Some(Err(Ok(link))) => format!("{done}: {link}"),
            Some(Err(Err(why))) => format!("couldn't send: {why} · {again}"),
        })
    }

    fn go(&mut self, screen: Screen, now: Instant) {
        // an upload carries on; whatever it says won't be about the next screen
        self.screens += 1;
        if self.share.as_ref().is_some_and(|s| s.state.is_err()) {
            self.share = None;
        }
        self.screen = screen;
        self.since = now;
        (self.outcome, self.scored) = (None, false);
        self.pick = None;
        self.fade = self.frame.take().map(|b| (b, now));
    }
}

/// A new font (or the canvas edge color for a new theme) only takes after a
/// fresh start on the web (see the web `main`); the terminal needs nothing.
fn reload() {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = ratzilla::web_sys::window() {
        let _ = w.location().reload();
    }
}

/// A fresh round of `mode`, on new words.
fn practice(mode: Mode) -> Screen {
    let seed = rand::random();
    Screen::Play(Round::new(Setup { mode, seed }))
}

/// The autopilot: types whatever test is on screen with occasional fixed
/// typos, speeding up as it goes so every rank and effect shows up.
/// `--demo` / `?demo` is the full tour (40 → 180 wpm, it also swipes the menu,
/// picks levels and restarts); `--autopilot [wpm]` only types, at `wpm`,
/// leaving the menus to you, or to a recording script (see `demo/`).
/// `--flawless` on either one never misses.
pub struct Bot {
    next: Instant,
    fixing: bool,
    /// Typing speed at the start of each test.
    wpm: f64,
    /// Also drive the menus (the self-running demo).
    tour: bool,
    /// Chance of a (fixed) typo per key.
    typos: f64,
}

impl Bot {
    pub fn tour() -> Self {
        Bot {
            typos: 0.015,
            ..Bot::typist(40.0).touring()
        }
    }

    pub fn typist(wpm: f64) -> Self {
        Bot {
            next: Instant::now(),
            fixing: false,
            wpm,
            tour: false,
            // rare enough that a recording's run climbs to reaper, but the
            // odd combo shatter still shows
            typos: 0.004,
        }
    }

    fn touring(self) -> Self {
        Bot { tour: true, ..self }
    }

    pub fn flawless(self) -> Self {
        Bot { typos: 0.0, ..self }
    }

    /// Types into the round on screen, through the same inputs as a player.
    fn drive(&mut self, r: &mut Round, now: Instant) {
        // after a menu or a new round, resync instead of bursting the backlog
        if r.test().started.is_none() && self.next + Duration::from_millis(500) < now {
            self.next = now;
        }
        let mut rng = rand::rng();
        while self.next <= now && r.test().finished.is_none() {
            let at = self.next;
            let input = match &r.game {
                // FLOOR IS LAVA: finishes the word it's on, or starts the lowest one
                Game::Lava(l) => l.next_char().map(Input::Char),
                Game::Test(_) if self.fixing => {
                    self.fixing = false;
                    Some(Input::Backspace)
                }
                Game::Test(t) if rng.random_bool(self.typos) => {
                    // pursuit: a miss stays put, so there's nothing to take back
                    self.fixing = !matches!(t.mode, Mode::Pursuit { .. });
                    Some(Input::Char('#'))
                }
                Game::Test(t) => t.target.get(t.typed.len()).map(|&c| Input::Char(c)),
            };
            if let Some(input) = input {
                r.input(r.ms(at), input);
            }
            self.next = self.after(r.test(), at);
        }
    }

    /// When the next key falls, jittered: the tour speeds up from `wpm` (+8 a
    /// second, to 140 more, at most 200) so every rank shows; the typist keeps
    /// to `wpm`, so a recording gets the pace it asked for.
    fn after(&self, t: &Test, at: Instant) -> Instant {
        let ramp =
            (self.wpm + 8.0 * t.elapsed(at).as_secs_f64()).min((self.wpm + 140.0).min(200.0));
        let wpm = if self.tour { ramp } else { self.wpm };
        at + Duration::from_secs_f64(12.0 / wpm * rand::rng().random_range(0.7..1.3))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bot that fumbles its first key, then types on clean for one more.
    fn fumble(mode: Mode) -> Round {
        let t0 = Instant::now();
        let mut r = Round::new(Setup { mode, seed: 1 });
        let mut bot = Bot {
            next: t0,
            typos: 1.0,
            ..Bot::typist(60.0)
        };
        bot.drive(&mut r, t0);
        bot.typos = 0.0;
        bot.drive(&mut r, bot.next);
        r
    }

    #[test]
    fn the_bot_fixes_its_typos_except_in_pursuit() {
        let r = fumble(Mode::Words(10));
        let t = r.test();
        assert_eq!((t.typed.len(), t.erased.len()), (0, 1), "took the # back");
        let r = fumble(Mode::Pursuit { start_level: 1 });
        let t = r.test();
        assert_eq!(
            t.typed,
            [t.target[0]],
            "the miss stayed put, then it went on"
        );
        assert_eq!((t.errors, t.erased.len()), (1, 0));
        assert_eq!(r.log.len(), 2, "both keys went through the round's inputs");
    }
}
