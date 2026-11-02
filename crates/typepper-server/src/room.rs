//! Rooms: THE HOT ONE played together. A room is a four-letter code, the
//! players in it, and one seed per game, so everyone types the same words;
//! each player's inputs are relayed to the others, who play them through the
//! same engine. The server never simulates live: when a player's round ends
//! they send its replay, the server plays it (as `runs::submit` would), and
//! that outcome, never the client's word, says when they went out and with
//! how many words. Last one standing wins. See docs/reference/protocol.md.
//!
//! One line protocol over both doors (a WebSocket, or a long-lived `v1 room`
//! SSH channel); `http.rs` and `ssh.rs` only move lines.
//!
//! ```text
//! in:  start | in <ms> <op> | done <hex> | ping
//! out: ok <code> <slot> | peer <slot> <name> | left <slot> | go <ms> <seed>
//!      | in <slot> <ms> <op> | out <slot> <words> | win <slot>
//!      | place <n> <slot> <words> <ms> | over | pong | err <code> <why>
//! ```

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use typepper_engine::{Mode, Replay, VERSION, simulate};

/// Players a room holds.
pub const MAX_PLAYERS: usize = 8;
/// Rooms the server holds at once.
const MAX_ROOMS: usize = 1000;
/// The countdown after `start`.
const COUNTDOWN: Duration = Duration::from_secs(3);
/// A room nobody has touched for this long is forgotten by the sweep.
const IDLE: Duration = Duration::from_secs(30 * 60);
/// Letters a code is made of: no vowels, so a code is never a word, and
/// none that read as each other.
const LETTERS: &[u8] = b"BCDFGHJKLMNPRSTVWXZ";
pub const CODE_LEN: usize = 4;

/// Lines on their way to one player.
pub type Outbox = mpsc::UnboundedSender<String>;

struct Player {
    name: String,
    out: Outbox,
    /// Their round's verified end, this game.
    finish: Option<Finish>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Finish {
    /// When their round ended, in its own milliseconds.
    ms: u32,
    words: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Lobby,
    Playing,
}

struct Room {
    /// Slots stay put when someone leaves (the others know them by number).
    players: Vec<Option<Player>>,
    state: State,
    seed: u64,
    touched: Instant,
}

/// Every room, in memory: one server, and a room outlives nobody.
#[derive(Default)]
pub struct Rooms(Mutex<HashMap<String, Room>>);

/// Where a player sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seat {
    pub code: String,
    pub slot: usize,
}

impl Rooms {
    /// Makes a room (no code) or joins one, as `name`; the lines for this
    /// player come out of the receiver, starting with `ok` and the roster.
    pub fn join(
        &self,
        code: Option<&str>,
        name: &str,
    ) -> Result<(Seat, mpsc::UnboundedReceiver<String>), String> {
        let mut rooms = self.0.lock().unwrap();
        let code = match code {
            Some(c) => {
                let c = c.to_ascii_uppercase();
                if !rooms.contains_key(&c) {
                    return Err("err no-room no room has that code".into());
                }
                c
            }
            None => {
                if rooms.len() >= MAX_ROOMS {
                    return Err("err busy too many rooms right now".into());
                }
                let code = loop {
                    let c = new_code();
                    if !rooms.contains_key(&c) {
                        break c;
                    }
                };
                rooms.insert(
                    code.clone(),
                    Room {
                        players: Vec::new(),
                        state: State::Lobby,
                        seed: 0,
                        touched: Instant::now(),
                    },
                );
                code
            }
        };
        let room = rooms.get_mut(&code).unwrap();
        if room.state != State::Lobby {
            return Err("err playing that room is mid-game, try again in a minute".into());
        }
        let slot = match room.players.iter().position(Option::is_none) {
            Some(free) => free,
            None if room.players.len() < MAX_PLAYERS => {
                room.players.push(None);
                room.players.len() - 1
            }
            None => return Err("err full that room has eight players".into()),
        };
        let (out, lines) = mpsc::unbounded_channel();
        let name = match name.is_empty() {
            true => format!("guest-{}", slot + 1),
            false => name.to_string(),
        };
        let _ = out.send(format!("ok {code} {slot}"));
        for (i, p) in room.players.iter().enumerate() {
            if let Some(p) = p {
                let _ = out.send(format!("peer {i} {}", p.name));
            }
        }
        room.players[slot] = Some(Player {
            name: name.clone(),
            out,
            finish: None,
        });
        room.touched = Instant::now();
        room.tell(format!("peer {slot} {name}"), None);
        Ok((Seat { code, slot }, lines))
    }

    /// A player's connection is gone: out of the room, and the room's gone
    /// too once it's empty.
    pub fn leave(&self, seat: &Seat) {
        let mut rooms = self.0.lock().unwrap();
        let Some(room) = rooms.get_mut(&seat.code) else {
            return;
        };
        if seat.slot < room.players.len() {
            room.players[seat.slot] = None;
        }
        room.tell(format!("left {}", seat.slot), None);
        room.touched = Instant::now();
        if room.players.iter().all(Option::is_none) {
            rooms.remove(&seat.code);
        } else {
            room.settle();
        }
    }

    /// One line from a player. `done` plays the replay first (off the async
    /// threads); everything else is answered at once.
    pub async fn line(&self, seat: &Seat, line: &str) -> Result<(), String> {
        let words: Vec<&str> = line.trim().split(' ').collect();
        match words[..] {
            ["ping"] => self.tell_one(seat, "pong".into()),
            ["start"] => self.start(seat),
            ["in", ms, op] => {
                let (Ok(ms), Ok(op)) = (ms.parse::<u32>(), op.parse::<u64>()) else {
                    return Err("err bad-line in <ms> <op>".into());
                };
                let mut rooms = self.0.lock().unwrap();
                let Some(room) = rooms.get_mut(&seat.code) else {
                    return Err("err no-room that room is gone".into());
                };
                if room.state == State::Playing {
                    room.tell(format!("in {} {ms} {op}", seat.slot), Some(seat.slot));
                }
                Ok(())
            }
            ["done", hex] => {
                let bytes = unhex(hex).ok_or("err bad-replay not hex")?;
                let seed = {
                    let rooms = self.0.lock().unwrap();
                    let room = rooms
                        .get(&seat.code)
                        .ok_or("err no-room that room is gone")?;
                    if room.state != State::Playing {
                        return Err("err not-playing no game is on".into());
                    }
                    room.seed
                };
                let finish = verify(&bytes, seed).await?;
                self.finished(seat, finish);
                Ok(())
            }
            _ => Err(format!("err unknown-command {}", line.trim())),
        }
    }

    fn tell_one(&self, seat: &Seat, line: String) -> Result<(), String> {
        let rooms = self.0.lock().unwrap();
        if let Some(Some(p)) = rooms.get(&seat.code).map(|r| r.players.get(seat.slot)) {
            let _ = p.as_ref().map(|p| p.out.send(line));
        }
        Ok(())
    }

    /// `start`, by the host (the lowest seat): a new seed, and the countdown.
    fn start(&self, seat: &Seat) -> Result<(), String> {
        let mut rooms = self.0.lock().unwrap();
        let room = rooms
            .get_mut(&seat.code)
            .ok_or("err no-room that room is gone")?;
        if room.host() != Some(seat.slot) {
            return Err("err not-host only the host starts the game".into());
        }
        if room.state != State::Lobby {
            return Err("err playing a game is already on".into());
        }
        room.state = State::Playing;
        room.seed = rand::random();
        room.touched = Instant::now();
        for p in room.players.iter_mut().flatten() {
            p.finish = None;
        }
        room.tell(format!("go {} {}", COUNTDOWN.as_millis(), room.seed), None);
        Ok(())
    }

    /// A player's round ended (verified): everyone hears, and the game may be over.
    fn finished(&self, seat: &Seat, finish: Finish) {
        let mut rooms = self.0.lock().unwrap();
        let Some(room) = rooms.get_mut(&seat.code) else {
            return;
        };
        let Some(Some(p)) = room.players.get_mut(seat.slot) else {
            return;
        };
        if p.finish.is_some() {
            return;
        }
        p.finish = Some(finish);
        room.tell(format!("out {} {}", seat.slot, finish.words), None);
        room.touched = Instant::now();
        room.settle();
    }

    /// Forgets rooms nobody has touched in half an hour (a lobby left open).
    pub fn sweep(&self) -> usize {
        let mut rooms = self.0.lock().unwrap();
        let before = rooms.len();
        rooms.retain(|_, r| r.touched.elapsed() < IDLE);
        before - rooms.len()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

impl Room {
    /// The lowest seat taken.
    fn host(&self) -> Option<usize> {
        self.players.iter().position(Option::is_some)
    }

    /// A line to everyone (but `except`).
    fn tell(&self, line: String, except: Option<usize>) {
        for (i, p) in self.players.iter().enumerate() {
            if let Some(p) = p
                && except != Some(i)
            {
                let _ = p.out.send(line.clone());
            }
        }
    }

    /// Mid-game, after someone's out or gone: one left standing wins (and
    /// is told to wrap up); none left, the game's over and the places go out.
    fn settle(&mut self) {
        if self.state != State::Playing {
            return;
        }
        let standing: Vec<usize> = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.as_ref().filter(|p| p.finish.is_none()).map(|_| i))
            .collect();
        match standing[..] {
            [] => self.over(),
            [winner] => self.tell(format!("win {winner}"), None),
            _ => {}
        }
    }

    fn over(&mut self) {
        let mut places: Vec<(usize, Finish)> = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(i, p)| Some((i, p.as_ref()?.finish?)))
            .collect();
        // the last to go out wins; out at the same time, more words wins
        places.sort_by(|a, b| b.1.ms.cmp(&a.1.ms).then(b.1.words.cmp(&a.1.words)));
        for (n, (slot, f)) in places.iter().enumerate() {
            self.tell(format!("place {} {slot} {} {}", n + 1, f.words, f.ms), None);
        }
        self.tell("over".into(), None);
        self.state = State::Lobby;
    }
}

/// Plays a `done` replay: it must be this game (the hot one, this seed, these
/// rules) and end; then when, and with how many words.
async fn verify(bytes: &[u8], seed: u64) -> Result<Finish, String> {
    let replay = Replay::decode(bytes).map_err(|e| format!("err bad-replay {e}"))?;
    if replay.engine != VERSION {
        return Err("err old-engine update typepper to play along".into());
    }
    let setup = replay.setup().ok_or("err bad-replay bad mode")?;
    if setup.mode != Mode::HotOne || setup.seed != seed {
        return Err("err bad-replay that isn't this game's round".into());
    }
    let inputs = replay.inputs;
    let outcome = tokio::task::spawn_blocking(move || simulate(setup, &inputs))
        .await
        .map_err(|_| "err busy try later".to_string())?
        .ok_or("err bad-replay the round never ends")?;
    Ok(Finish {
        ms: outcome.ms,
        words: outcome.words,
    })
}

fn new_code() -> String {
    (0..CODE_LEN)
        .map(|_| LETTERS[rand::random_range(0..LETTERS.len())] as char)
        .collect()
}

/// A code as typed: four of the letters, any case.
pub fn valid_code(s: &str) -> bool {
    s.len() == CODE_LEN && s.bytes().all(|b| LETTERS.contains(&b.to_ascii_uppercase()))
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use typepper_engine::{Input, Round, Setup};

    fn drain(rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<String> {
        let mut lines = Vec::new();
        while let Ok(l) = rx.try_recv() {
            lines.push(l);
        }
        lines
    }

    #[tokio::test]
    async fn a_room_is_made_joined_and_left() {
        let rooms = Rooms::default();
        let (host, mut h) = rooms.join(None, "leo").unwrap();
        assert!(valid_code(&host.code) && host.slot == 0);
        assert_eq!(
            drain(&mut h),
            [format!("ok {} 0", host.code), "peer 0 leo".into()]
        );
        let (guest, mut g) = rooms.join(Some(&host.code.to_lowercase()), "").unwrap();
        assert_eq!(guest.slot, 1);
        assert_eq!(
            drain(&mut g),
            [
                format!("ok {} 1", host.code),
                "peer 0 leo".into(),
                "peer 1 guest-2".into()
            ]
        );
        assert_eq!(drain(&mut h), ["peer 1 guest-2"]);
        assert_eq!(
            rooms.join(Some("ZZZZ"), "x").err().as_deref(),
            Some("err no-room no room has that code")
        );
        // only the host starts
        assert!(rooms.line(&guest, "start").await.is_err());
        rooms.line(&host, "ping").await.unwrap();
        assert_eq!(drain(&mut h), ["pong"]);
        rooms.leave(&host);
        assert_eq!(drain(&mut g), ["left 0"]);
        // the guest is host now; the room's gone once they go too
        rooms.line(&guest, "start").await.unwrap();
        assert!(drain(&mut g)[0].starts_with("go 3000 "));
        rooms.leave(&guest);
        assert_eq!(rooms.len(), 0);
    }

    #[tokio::test]
    async fn a_game_relays_inputs_verifies_ends_and_places_everyone() {
        let rooms = Rooms::default();
        let (a, mut ra) = rooms.join(None, "a").unwrap();
        let (b, mut rb) = rooms.join(Some(&a.code), "b").unwrap();
        let (c, mut rc) = rooms.join(Some(&a.code), "c").unwrap();
        assert!(
            rooms.line(&a, "in 5 104").await.is_ok(),
            "ignored in the lobby"
        );
        rooms.line(&a, "start").await.unwrap();
        let go = drain(&mut ra).pop().unwrap();
        let seed: u64 = go.split(' ').nth(2).unwrap().parse().unwrap();
        drain(&mut rb);
        drain(&mut rc);
        assert!(rooms.join(Some(&a.code), "late").is_err(), "mid-game");
        rooms.line(&a, "in 120 104").await.unwrap();
        assert_eq!(
            drain(&mut ra),
            Vec::<String>::new(),
            "not back to the sender"
        );
        assert_eq!(drain(&mut rb), ["in 0 120 104"]);
        // a round of this game, played to its end, goes up as its replay
        let round = |keys: u32| {
            let setup = Setup {
                mode: Mode::HotOne,
                seed,
            };
            let mut r = Round::new(setup);
            for k in 0..keys {
                let c = r.test().target[r.test().typed.len()];
                r.input(100 + k * 100, Input::Char(c));
            }
            // bite every plate until the hot one
            let mut ms = 100 + keys * 100;
            while r.test().finished.is_none() {
                ms += 1000;
                r.advance(ms);
                if let Some(p) = r.test().plate {
                    r.input(ms, Input::Pick(p.hot));
                }
            }
            let hex: String = Replay::of(&r)
                .encode()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            (hex, r.result().unwrap())
        };
        let (hex_a, out_a) = round(30);
        rooms.line(&a, &format!("done {hex_a}")).await.unwrap();
        assert_eq!(drain(&mut rb), [format!("out 0 {}", out_a.words)]);
        // the wrong game is refused
        let other = {
            let mut r = Round::new(Setup {
                mode: Mode::HotOne,
                seed: seed ^ 1,
            });
            r.input(100, Input::Char('x'));
            Replay::of(&r)
                .encode()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        assert!(rooms.line(&b, &format!("done {other}")).await.is_err());
        assert!(rooms.line(&b, "done zz").await.is_err());
        let (hex_b, out_b) = round(60);
        rooms.line(&b, &format!("done {hex_b}")).await.unwrap();
        // one standing: c wins, and is told to wrap up
        assert_eq!(drain(&mut rc).last().map(String::as_str), Some("win 2"));
        let (hex_c, out_c) = round(10);
        rooms.line(&c, &format!("done {hex_c}")).await.unwrap();
        let lines = drain(&mut ra);
        let mut ranked = [(2usize, out_c), (0, out_a), (1, out_b)];
        ranked.sort_by(|x, y| y.1.ms.cmp(&x.1.ms).then(y.1.words.cmp(&x.1.words)));
        let want: Vec<String> = ranked
            .iter()
            .enumerate()
            .map(|(n, (slot, o))| format!("place {} {slot} {} {}", n + 1, o.words, o.ms))
            .chain(["over".to_string()])
            .collect();
        assert_eq!(lines[lines.len() - 4..], want[..]);
        // back in the lobby: another game can start, with a new seed
        rooms.line(&a, "start").await.unwrap();
        let again = drain(&mut ra).pop().unwrap();
        assert!(again.starts_with("go 3000 ") && again != go);
    }

    #[tokio::test]
    async fn leaving_mid_game_ends_it() {
        let rooms = Rooms::default();
        let (a, mut ra) = rooms.join(None, "a").unwrap();
        let (b, _rb) = rooms.join(Some(&a.code), "b").unwrap();
        rooms.line(&a, "start").await.unwrap();
        drain(&mut ra);
        rooms.leave(&b);
        assert_eq!(drain(&mut ra), ["left 1", "win 0"]);
        rooms.leave(&a);
        assert_eq!(rooms.len(), 0);
    }

    #[test]
    fn codes_are_four_safe_letters() {
        for _ in 0..50 {
            let c = new_code();
            assert!(valid_code(&c) && valid_code(&c.to_lowercase()), "{c}");
        }
        assert!(!valid_code("ABCD"), "A is a vowel");
        assert!(!valid_code("BCD") && !valid_code("BCDFG"));
        let rooms = Rooms::default();
        rooms.join(None, "x").unwrap();
        assert_eq!(rooms.sweep(), 0);
        rooms.0.lock().unwrap().values_mut().for_each(|r| {
            r.touched = Instant::now() - IDLE - Duration::from_secs(1);
        });
        assert_eq!(rooms.sweep(), 1);
    }
}
