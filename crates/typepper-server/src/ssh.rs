//! The SSH side: a command in (and its stdin), response lines and an exit
//! status out. The game runs the system `ssh`, so nothing here prompts.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh::keys::{HashAlg, PrivateKey, PublicKey};
use russh::server::{Auth, ChannelOpenHandle, Handler, Msg, Response, Server, Session};
use russh::{Channel, ChannelId};
use sqlx::PgPool;

use crate::id::Id;
use crate::room::{self, Rooms, Seat};
use crate::runs::{self, Quota};
use crate::{accounts, board, github};

/// The user the game logs in as. Any other user is a GitHub login, linking a
/// key to that account (`ssh -l <login> … v1 link`).
const GAME_USER: &str = "typepper";

pub async fn serve(
    key: PrivateKey,
    db: PgPool,
    quota: Arc<Quota>,
    rooms: Arc<Rooms>,
    port: u16,
) -> std::io::Result<()> {
    let config = russh::server::Config {
        keys: vec![key],
        inactivity_timeout: Some(Duration::from_secs(30)),
        auth_rejection_time: Duration::from_secs(1),
        // "none" is every client's first try: turn it down at once
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    };
    let mut door = Door {
        db,
        quota,
        rooms,
        who: String::new(),
        uploads: HashMap::new(),
        key: None,
        github: None,
        seat: None,
    };
    door.run_on_address(Arc::new(config), ("::", port)).await
}

/// One connection.
#[derive(Clone)]
struct Door {
    db: PgPool,
    quota: Arc<Quota>,
    rooms: Arc<Rooms>,
    /// Whose quota this is: their key's fingerprint, else where they came from.
    who: String,
    /// `v1 submit` channels still reading their stdin.
    uploads: HashMap<ChannelId, Vec<u8>>,
    /// The key that logged in (its fingerprint), and what it vouched for.
    key: Option<(String, Vouch)>,
    /// A linking login's keys on GitHub, asked once per connection.
    github: Option<(String, Vec<String>)>,
    /// `v1 room`: the channel the room's lines go over, and the seat (left
    /// when the connection goes).
    seat: Option<(ChannelId, Arc<Seated>, Vec<u8>)>,
}

/// A seat in a room, left when dropped (however the connection ended).
struct Seated {
    rooms: Arc<Rooms>,
    seat: Seat,
}

impl Drop for Seated {
    fn drop(&mut self) {
        self.rooms.leave(&self.seat);
    }
}

/// What a key that got in stands for.
#[derive(Clone, Debug)]
enum Vouch {
    /// A linked key: this player, `(id, login)`.
    Player(i64, String),
    /// A key on this login's GitHub, logging in to link to it.
    Linking(String),
}

impl Door {
    /// Whether `key` may log in as `user` (None turns it down, and the
    /// client tries its next key, or none: anonymous).
    async fn vouch(&mut self, user: &str, key: &PublicKey) -> Option<Vouch> {
        if user == GAME_USER {
            let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
            return match accounts::player_of_key(&self.db, &fingerprint).await {
                Ok(player) => player.map(|(id, login)| Vouch::Player(id, login)),
                Err(e) => {
                    eprintln!("ssh: player_of_key: {e}");
                    None
                }
            };
        }
        if !github::valid_login(user) {
            return None;
        }
        let keys = match &self.github {
            Some((login, keys)) if login == user => keys.clone(),
            _ => {
                let keys = github::keys(user)
                    .await
                    .inspect_err(|e| eprintln!("ssh: {e}"))
                    .ok()?;
                self.github = Some((user.to_string(), keys.clone()));
                keys
            }
        };
        let line = key.to_openssh().ok()?;
        let line: Vec<&str> = line.split_whitespace().take(2).collect();
        keys.contains(&line.join(" "))
            .then(|| Vouch::Linking(user.to_string()))
    }

    fn player(&self) -> Option<(i64, String)> {
        match &self.key {
            Some((_, Vouch::Player(id, login))) => Some((*id, login.clone())),
            _ => None,
        }
    }
}

impl Server for Door {
    type Handler = Self;
    fn new_client(&mut self, addr: Option<SocketAddr>) -> Self {
        Door {
            who: addr.map_or("?".into(), |a| a.ip().to_string()),
            uploads: HashMap::new(),
            key: None,
            github: None,
            seat: None,
            ..self.clone()
        }
    }
}

impl Handler for Door {
    type Error = russh::Error;

    // turning "none" down makes clients offer their key, which is who they are
    async fn auth_none(&mut self, _: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::reject())
    }

    // asked before the client signs, so a key that won't do never costs its
    // owner an agent prompt
    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        Ok(match self.vouch(user, key).await {
            Some(_) => Auth::Accept,
            None => Auth::reject(),
        })
    }

    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        let Some(vouch) = self.vouch(user, key).await else {
            return Ok(Auth::reject());
        };
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        self.who = fingerprint.clone();
        self.key = Some((fingerprint, vouch));
        Ok(Auth::Accept)
    }

    // no key that's linked (or none at all): the game gets in anyway, with
    // nothing asked, anonymously. Linking needs a key.
    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _: &str,
        _: Option<Response<'a>>,
    ) -> Result<Auth, Self::Error> {
        Ok(match user == GAME_USER {
            true => Auth::Accept,
            false => Auth::reject(),
        })
    }

    async fn channel_open_session(
        &mut self,
        _: Channel<Msg>,
        reply: ChannelOpenHandle,
        _: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let (out, status) = match command {
            b"v1 submit" => {
                // the submission comes on stdin; it's answered at its end
                session.channel_success(channel)?;
                self.uploads.insert(channel, Vec::new());
                return Ok(());
            }
            b"v1 link" => self.link().await,
            b"v1 whoami" => match self.player() {
                Some((_, login)) => (format!("ok {login}\n"), 0),
                None => ("ok anonymous\n".into(), 0),
            },
            b"v1 unlink" => self.unlink().await,
            c if c.starts_with(b"v1 board ") => self.board(c).await,
            c if c.starts_with(b"v1 replay ") => self.replay(c).await,
            c if c.starts_with(b"v1 room ") => match self.room(channel, c, session) {
                // the channel stays open: the room's lines go over it
                Ok(()) => return session.channel_success(channel),
                Err(why) => (why, 1),
            },
            _ => run(command),
        };
        reply(channel, out, status, session)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // a room's lines, as they come (a line may come in pieces)
        if let Some((ch, seated, buf)) = &mut self.seat
            && *ch == channel
        {
            buf.extend_from_slice(data);
            while let Some(end) = buf.iter().position(|&b| b == b'\n') {
                let line = String::from_utf8_lossy(&buf[..end]).into_owned();
                buf.drain(..=end);
                if let Err(why) = seated.rooms.line(&seated.seat, &line).await {
                    session.data(channel, format!("{why}\n").into_bytes())?;
                }
            }
            return Ok(());
        }
        let Some(body) = self.uploads.get_mut(&channel) else {
            return Ok(());
        };
        if body.len() + data.len() > runs::MAX_BODY {
            self.uploads.remove(&channel);
            let out = "err too-big that's more than a submission\n".into();
            return reply(channel, out, 1, session);
        }
        body.extend_from_slice(data);
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        // the game hung up on its room
        if self.seat.as_ref().is_some_and(|(ch, ..)| *ch == channel) {
            self.seat = None;
            session.exit_status_request(channel, 0)?;
            session.eof(channel)?;
            return session.close(channel);
        }
        let Some(body) = self.uploads.remove(&channel) else {
            return Ok(());
        };
        if !self.quota.take(&self.who) {
            let out = "err rate-limited try again in an hour\n".into();
            return reply(channel, out, 1, session);
        }
        let player = self.player().map(|(id, _)| id);
        let out = runs::submit(&self.db, player, &body).await;
        reply(channel, out, 0, session)
    }

    // a plain `ssh ssh.typepper.com`: no shell here, say so instead of hanging
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let out = "typepper: no shell here; the game runs its commands (try `ping`)\r\n";
        reply(channel, out.into(), 1, session)
    }
}

impl Door {
    /// `v1 link`, logged in as a GitHub login with one of its keys.
    async fn link(&self) -> (String, u32) {
        let Some((fingerprint, Vouch::Linking(login))) = &self.key else {
            return (
                "err not-linking log in as your GitHub login: ssh -l <login> … v1 link\n".into(),
                1,
            );
        };
        let linked = match github::user(login).await {
            Ok((github_id, login)) => accounts::link(&self.db, github_id, &login, fingerprint)
                .await
                .map(|_| login)
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        match linked {
            Ok(login) => (format!("ok linked {login}\n"), 0),
            Err(e) => {
                eprintln!("ssh: link: {e}");
                ("err busy try later\n".into(), 1)
            }
        }
    }

    /// `v1 board <mode> <period>`: the board, and the caller's own row.
    async fn board(&self, command: &[u8]) -> (String, u32) {
        let words: Vec<&str> = std::str::from_utf8(command)
            .unwrap_or("")
            .split(' ')
            .collect();
        let (Some(mode), Some(period)) = (
            words.get(2).and_then(|m| typepper_engine::Mode::parse(m)),
            words.get(3).and_then(|p| board::Period::parse(p)),
        ) else {
            return (
                "err bad-board v1 board <mode> <day|week|month|year|all>\n".into(),
                1,
            );
        };
        let me = self.player().map(|(id, _)| id);
        match board::board(&self.db, mode, period, me).await {
            Ok((top, mine)) => (board::text(&top, mine.as_ref()), 0),
            Err(e) => {
                eprintln!("ssh: board: {e}");
                ("err busy try later\n".into(), 1)
            }
        }
    }

    /// `v1 replay <id>`: a stored replay, as `ok <hex>` (the game reads text).
    async fn replay(&self, command: &[u8]) -> (String, u32) {
        let id = std::str::from_utf8(&command[b"v1 replay ".len()..]).unwrap_or("");
        let Ok(id) = id.parse::<Id>() else {
            return ("err gone no replay has that id\n".into(), 1);
        };
        match runs::replay(&self.db, id).await {
            Ok(Some(bytes)) => {
                let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                (format!("ok {hex}\n"), 0)
            }
            Ok(None) => ("err gone that replay is gone\n".into(), 1),
            Err(e) => {
                eprintln!("ssh: replay: {e}");
                ("err busy try later\n".into(), 1)
            }
        }
    }

    /// `v1 room new` / `v1 room <code>`: into a room for THE HOT ONE, as the
    /// linked player or a guest. The channel stays open: lines in on stdin,
    /// the room's lines out on stdout, until the game hangs up.
    fn room(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut Session,
    ) -> Result<(), String> {
        let arg = std::str::from_utf8(&command[b"v1 room ".len()..]).unwrap_or("");
        let code = match arg {
            "new" => None,
            c if room::valid_code(c) => Some(c),
            _ => return Err("err no-room v1 room <new|code>\n".into()),
        };
        if self.seat.is_some() {
            return Err("err busy this connection is in a room already\n".into());
        }
        let name = self.player().map(|(_, login)| login).unwrap_or_default();
        let (seat, mut lines) = self
            .rooms
            .join(code, &name)
            .map_err(|why| format!("{why}\n"))?;
        let handle = session.handle();
        tokio::spawn(async move {
            while let Some(line) = lines.recv().await {
                if handle.data(channel, format!("{line}\n")).await.is_err() {
                    break;
                }
            }
        });
        let seated = Seated {
            rooms: self.rooms.clone(),
            seat,
        };
        self.seat = Some((channel, Arc::new(seated), Vec::new()));
        Ok(())
    }

    /// `v1 unlink`: the key that logged in lets go of its player.
    async fn unlink(&self) -> (String, u32) {
        let Some((fingerprint, Vouch::Player(..))) = &self.key else {
            return ("ok not-linked\n".into(), 0);
        };
        match accounts::unlink(&self.db, fingerprint).await {
            Ok(_) => ("ok unlinked\n".into(), 0),
            Err(e) => {
                eprintln!("ssh: unlink: {e}");
                ("err busy try later\n".into(), 1)
            }
        }
    }
}

/// Answer a command: its output, its exit status, then hang up.
fn reply(
    channel: ChannelId,
    out: String,
    status: u32,
    session: &mut Session,
) -> Result<(), russh::Error> {
    session.channel_success(channel)?;
    session.data(channel, out.into_bytes())?;
    session.exit_status_request(channel, status)?;
    session.eof(channel)?;
    session.close(channel)
}

/// One command → what to print, and the exit status.
fn run(command: &[u8]) -> (String, u32) {
    match command {
        b"ping" => ("pong\n".into(), 0),
        _ => {
            let c = String::from_utf8_lossy(command);
            (format!("err unknown-command {c}\n"), 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_pongs_and_the_rest_is_an_error() {
        assert_eq!(run(b"ping"), ("pong\n".into(), 0));
        assert_eq!(run(b"v9 nope"), ("err unknown-command v9 nope\n".into(), 1));
    }
}
