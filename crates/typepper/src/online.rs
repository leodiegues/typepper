//! The game's one door to typepper.com: replays go up, links come back.
//! Only ever on the player's say-so (pressing u, or having linked their
//! account), and a failure is one line of text. The one call they don't
//! make, a logged-in browser fetching its bests, is never waited on, and its
//! failure changes nothing (docs/built/decisions.md, 1). Desktop runs the system `ssh`, where a linked key is who the player
//! is; the web uses fetch, where a session cookie from logging in with GitHub
//! is. See docs/built/game-client.md.

use crate::store;

/// Rounds a linked player's outbox keeps until they go up.
const OUTBOX: usize = 20;
/// What a room says once its connection is gone.
pub const GONE: &str = "lost typepper.com";

/// The server's public host key: ssh refuses anything else.
#[cfg(not(target_arch = "wasm32"))]
const HOST_KEY: &str = include_str!("../../../infra/ssh/typepper-server.pub");
/// Where the game connects over SSH (`TYPEPPER_SSH=host:port` overrides it,
/// `TYPEPPER_SSH_KNOWN_HOST` the key it expects there).
#[cfg(not(target_arch = "wasm32"))]
const SSH: &str = "ssh.typepper.com:36080";
/// The HTTP API, for the web.
#[cfg(target_arch = "wasm32")]
const API: &str = "https://api.typepper.com";

/// A replay as a one-item submission: `claimed: f64 LE | replay`.
pub fn submission(replay: &[u8], claimed: Option<f64>) -> Vec<u8> {
    let mut body = claimed.unwrap_or(f64::NAN).to_le_bytes().to_vec();
    body.extend_from_slice(replay);
    body
}

/// One line of the server's answer: the replay's link, or why not.
pub fn answer(line: &str) -> Result<String, String> {
    let words: Vec<&str> = line.trim().split(' ').collect();
    match words[..] {
        ["ok", _id, _score, rank, link] => {
            let link = link.trim_start_matches("https://");
            Ok(match rank {
                "-" => link.to_string(),
                rank => format!("{link} · #{rank} all time"),
            })
        }
        ["err", _code, ..] => Err(words[2..].join(" ")),
        _ => Err("typepper.com didn't answer".into()),
    }
}

/// One row of a board.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub rank: u32,
    pub score: f64,
    pub login: String,
    /// `YYYY-MM-DD`, UTC.
    pub day: String,
    /// The run's id, to watch it (servers before 0.6 don't say).
    pub run: Option<String>,
    /// The player's own row, below the top.
    pub me: bool,
}

/// A board's text (`<rank> <score> <login> <day> <run>` lines, then `me …`).
pub fn rows(text: &str) -> Result<Vec<Row>, String> {
    if let Some(why) = text.strip_prefix("err ") {
        return Err(why
            .split_once(' ')
            .map_or(why, |(_, w)| w)
            .trim()
            .to_string());
    }
    Ok(text
        .lines()
        .filter_map(|l| {
            let (me, l) = match l.strip_prefix("me ") {
                Some(l) => (true, l),
                None => (false, l),
            };
            let w: Vec<&str> = l.split(' ').collect();
            Some(Row {
                rank: w.first()?.parse().ok()?,
                score: w.get(1)?.parse().ok()?,
                login: w.get(2)?.to_string(),
                day: w.get(3)?.to_string(),
                run: w.get(4).map(|r| r.to_string()),
                me,
            })
        })
        .collect())
}

/// The GitHub login this player is linked to (`typepper link`) or logged in
/// as (the web), if they are.
pub fn account() -> Option<String> {
    let login = store::read("account")?;
    Some(login.trim().to_string()).filter(|l| !l.is_empty())
}

/// Keeps a finished round for upload (dropping the oldest past 20).
pub fn outbox_add(replay: &[u8], claimed: Option<f64>) {
    let ms = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    store::write_bytes(&format!("outbox/{ms}"), &submission(replay, claimed));
    let all = store::list("outbox");
    for old in &all[..all.len().saturating_sub(OUTBOX)] {
        store::remove(old);
    }
}

/// The outbox, oldest first: its names and, back to back, a submission.
pub fn outbox() -> (Vec<String>, Vec<u8>) {
    let mut body = Vec::new();
    let names = store::list("outbox")
        .into_iter()
        .filter(|n| store::read_bytes(n).map(|b| body.extend(b)).is_some())
        .collect();
    (names, body)
}

/// Takes out what the server's answer settles: kept, or never will be.
pub fn outbox_settle(names: &[String], answer: &str) {
    for (name, line) in names.iter().zip(answer.lines()) {
        if settled(line) {
            store::remove(name);
        }
    }
}

/// A query's `login=` (if it's one GitHub could have), and the address
/// without it: `/`, or `/?` and the rest.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
fn login_in(query: &str) -> (Option<String>, String) {
    let (logins, rest): (Vec<&str>, Vec<&str>) = query
        .trim_start_matches('?')
        .split('&')
        .filter(|kv| !kv.is_empty())
        .partition(|kv| kv.starts_with("login="));
    // only what a GitHub login can be: it shows on screen, nothing more
    let login = logins
        .first()
        .map(|kv| &kv["login=".len()..])
        .filter(|l| (1..=39).contains(&l.len()))
        .filter(|l| l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    let rest = match rest.is_empty() {
        true => "/".to_string(),
        false => format!("/?{}", rest.join("&")),
    };
    (login.map(str::to_string), rest)
}

/// Whether one line of an answer settles its replay: stored, or never will
/// be (too old, broken, too big). Anything else (busy, rate-limited, signed
/// out, no line at all) keeps it for the next try.
fn settled(line: &str) -> bool {
    let done = ["ok ", "err old-engine ", "err bad-replay ", "err too-big "];
    done.iter().any(|d| line.starts_with(d))
}

/// A `v1 replay` answer (`ok <hex>`): the replay's bytes, or why not.
pub fn replay(text: &str) -> Result<Vec<u8>, String> {
    match text.trim().split_once(' ') {
        Some(("ok", hex)) => crate::store::unhex(hex).ok_or("a garbled replay".into()),
        Some(("err", why)) => Err(why.split_once(' ').map_or(why, |(_, w)| w).into()),
        _ => Err("typepper.com didn't answer".into()),
    }
}

/// What ssh printed, once it's done: an answer, even an empty one (an empty
/// board) if it exited cleanly; nothing from a failed ssh (255: no
/// connection) is no answer at all.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn heard(out: String, clean: bool) -> Result<String, String> {
    match out.is_empty() && !clean {
        true => Err("couldn't reach typepper.com".into()),
        false => Ok(out),
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::io::{Read, Write};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use crate::store;

    use super::account;

    /// The game's user on the server; any other is a GitHub login linking.
    const GAME_USER: &str = "typepper";

    /// A call to the server under way (a submission, or a board).
    pub struct Call {
        child: Child,
        started: Instant,
    }

    impl Call {
        /// Sends `body` (a submission) as the linked player when there is
        /// one, else anonymously.
        pub fn submit(body: Vec<u8>) -> Result<Call, String> {
            // anonymous: no key offered (an agent could ask its owner to
            // approve one). Linked: keys are offered, the server takes only
            // the linked one, and a locked key goes to an askpass that isn't
            // there, so nothing ever prompts over the game.
            let pubkey = match account() {
                Some(_) => "PubkeyAuthentication=yes",
                None => "PubkeyAuthentication=no",
            };
            let mut ssh = ssh(GAME_USER, &["-o", pubkey], &["v1", "submit"])?;
            ssh.env("SSH_ASKPASS", "typepper-asks-no-passphrases")
                .env("SSH_ASKPASS_REQUIRE", "force")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            let mut child = ssh.spawn().map_err(|_| "ssh isn't installed".to_string())?;
            let mut stdin = child.stdin.take().unwrap();
            // a big outbox could fill the pipe before ssh connects: write aside
            std::thread::spawn(move || {
                let _ = stdin.write_all(&body);
            });
            Ok(Call {
                child,
                started: Instant::now(),
            })
        }

        /// `mode`'s board for `period` (with the player's own row, if linked).
        pub fn board(mode: &str, period: &str) -> Result<Call, String> {
            Call::ask(&["v1", "board", mode, period])
        }

        /// The run `id`'s replay (`ok <hex>`, see `online::replay`).
        pub fn replay(id: &str) -> Result<Call, String> {
            Call::ask(&["v1", "replay", id])
        }

        /// A command with nothing on stdin, as the linked player if there is one.
        fn ask(command: &[&str]) -> Result<Call, String> {
            let pubkey = match account() {
                Some(_) => "PubkeyAuthentication=yes",
                None => "PubkeyAuthentication=no",
            };
            let child = ssh(GAME_USER, &["-o", pubkey], command)?
                .env("SSH_ASKPASS", "typepper-asks-no-passphrases")
                .env("SSH_ASKPASS_REQUIRE", "force")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "ssh isn't installed".to_string())?;
            Ok(Call {
                child,
                started: Instant::now(),
            })
        }

        /// The server's answer, once there is one (never waits).
        pub fn poll(&mut self) -> Option<Result<String, String>> {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    let mut out = String::new();
                    let _ = self.child.stdout.take()?.read_to_string(&mut out);
                    Some(super::heard(out, status.success()))
                }
                Ok(None) if self.started.elapsed() > Duration::from_secs(20) => {
                    let _ = self.child.kill();
                    Some(Err("typepper.com took too long".into()))
                }
                Ok(None) => None,
                Err(e) => Some(Err(e.to_string())),
            }
        }
    }

    /// The web puts links on the clipboard; a terminal's text can be selected.
    pub fn copy(_: &str) {}

    /// A room for THE HOT ONE, open as long as its `ssh … v1 room` runs:
    /// lines go in on its stdin, the room's come out on its stdout (a thread
    /// reads them, so nothing waits). Hanging up is dropping it.
    pub struct Room {
        child: Child,
        stdin: Option<std::process::ChildStdin>,
        lines: std::sync::mpsc::Receiver<String>,
        closed: bool,
    }

    impl Room {
        /// Makes a room (no code) or joins one, as the linked player if
        /// there is one, else as a guest.
        pub fn open(code: Option<&str>) -> Result<Room, String> {
            let pubkey = match account() {
                Some(_) => "PubkeyAuthentication=yes",
                None => "PubkeyAuthentication=no",
            };
            let arg = code.unwrap_or("new").to_string();
            // ssh's own keepalives, so a quiet lobby isn't hung up on
            let options = ["-o", pubkey, "-o", "ServerAliveInterval=10"];
            let mut child = ssh(GAME_USER, &options, &["v1", "room", &arg])?
                .env("SSH_ASKPASS", "typepper-asks-no-passphrases")
                .env("SSH_ASKPASS_REQUIRE", "force")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "ssh isn't installed".to_string())?;
            let stdin = child.stdin.take();
            let stdout = child.stdout.take().ok_or("no stdout")?;
            let (tx, lines) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                use std::io::BufRead;
                for line in std::io::BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            });
            Ok(Room {
                child,
                stdin,
                lines,
                closed: false,
            })
        }

        /// One line to the room.
        pub fn send(&mut self, line: &str) {
            if let Some(stdin) = &mut self.stdin
                && (writeln!(stdin, "{line}").is_err() || stdin.flush().is_err())
            {
                self.closed = true;
            }
        }

        /// The lines that came since the last poll; `Err` once the
        /// connection is gone (after whatever it still said).
        pub fn poll(&mut self) -> Result<Vec<String>, String> {
            let mut lines = Vec::new();
            loop {
                match self.lines.try_recv() {
                    Ok(l) => lines.push(l),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.closed = true;
                        break;
                    }
                }
            }
            match (lines.is_empty(), self.closed) {
                (true, true) => Err(super::GONE.into()),
                _ => Ok(lines),
            }
        }
    }

    impl Drop for Room {
        fn drop(&mut self) {
            // closing stdin is hanging up; ssh goes once the server closes the channel
            self.stdin.take();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// `typepper link <login>`: ties this machine's ssh key to a GitHub
    /// account (the server checks the key is on github.com/<login>.keys).
    /// Runs in the terminal, not the game, so the agent may ask as it likes.
    pub fn link(login: &str) -> Result<String, String> {
        let out = ssh(login, &["-o", "PubkeyAuthentication=yes"], &["v1", "link"])?
            .stderr(Stdio::inherit())
            .output()
            .map_err(|_| "ssh isn't installed".to_string())?;
        let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
        match line.strip_prefix("ok linked ") {
            Some(login) => {
                store::write("account", login).map_err(|e| e.to_string())?;
                Ok(login.to_string())
            }
            // ssh says why just above; it exits the same whether it couldn't
            // connect or wasn't let in, so name both
            None if line.is_empty() => Err(format!(
                "ssh didn't get in (see above): either typepper.com is unreachable, \
                 or none of your ssh keys is on github.com/{login}.keys"
            )),
            None => Err(line),
        }
    }

    /// `typepper unlink`: this machine's key lets go of the account.
    pub fn unlink() -> Result<(), String> {
        let out = ssh(
            GAME_USER,
            &["-o", "PubkeyAuthentication=yes"],
            &["v1", "unlink"],
        )?
        .stderr(Stdio::inherit())
        .output()
        .map_err(|_| "ssh isn't installed".to_string())?;
        store::remove("account");
        match String::from_utf8_lossy(&out.stdout).starts_with("ok") {
            true => Ok(()),
            false => Err("forgot it here, but typepper.com didn't answer".into()),
        }
    }

    /// `ssh` to the server as `user` with `options`, running `command`, with
    /// no prompt about the host: its key is checked, never asked about.
    fn ssh(user: &str, options: &[&str], command: &[&str]) -> Result<Command, String> {
        let addr = std::env::var("TYPEPPER_SSH").unwrap_or(super::SSH.into());
        let (host, port) = addr.rsplit_once(':').ok_or("TYPEPPER_SSH is host:port")?;
        // the host key under a fixed alias, so any address works with it
        // (TYPEPPER_SSH_KNOWN_HOST: a local server's, while working on it)
        let key = std::env::var("TYPEPPER_SSH_KNOWN_HOST").unwrap_or(super::HOST_KEY.into());
        let key = key.split_whitespace().take(2).collect::<Vec<_>>();
        store::write("known_hosts", &format!("typepper {}\n", key.join(" ")))
            .map_err(|e| format!("can't keep the host key: {e}"))?;
        let known = store::path("known_hosts").ok_or("no data dir")?;
        let mut ssh = Command::new("ssh");
        ssh.args(["-T", "-p", port, "-l", user])
            .args(["-o", "ConnectTimeout=5", "-o", "LogLevel=ERROR"])
            .args([
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "HostKeyAlias=typepper",
            ])
            .arg("-o")
            .arg(format!("UserKnownHostsFile={}", known.display()))
            // no passwords; the server lets the game in without a key, and
            // without asking anything (BatchMode would refuse that)
            .args([
                "-o",
                "PasswordAuthentication=no",
                "-o",
                "NumberOfPasswordPrompts=1",
            ])
            .args(options)
            .arg(host)
            .args(command);
        Ok(ssh)
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::cell::RefCell;
    use std::rc::Rc;

    use ratzilla::web_sys::{self, js_sys};
    use wasm_bindgen_futures::JsFuture;

    use super::account;
    use crate::store;

    type Slot = Rc<RefCell<Option<Result<String, String>>>>;

    /// A fetch under way.
    pub struct Call(Slot);

    impl Call {
        pub fn submit(body: Vec<u8>) -> Result<Call, String> {
            let slot = Slot::default();
            let done = slot.clone();
            // logged in: as the player, or not at all (the outbox waits)
            let path = match account() {
                Some(_) => "/v1/me/runs",
                None => "/v1/runs",
            };
            wasm_bindgen_futures::spawn_local(async move {
                let answer = post(path, body).await;
                let first = answer.as_deref().map(|t| t.lines().next().unwrap_or(""));
                match first {
                    // the session's gone: logged out here too
                    Ok(l) if l.starts_with("err signed-out ") => store::remove("account"),
                    Ok(l) if path == "/v1/runs" => {
                        if let Ok(link) = super::answer(l) {
                            copy(&format!("https://{link}"));
                        }
                    }
                    _ => {}
                }
                *done.borrow_mut() = Some(answer);
            });
            Ok(Call(slot))
        }

        pub fn board(mode: &str, period: &str) -> Result<Call, String> {
            let slot = Slot::default();
            let done = slot.clone();
            let url = format!("{}/v1/boards/{mode}?period={period}", api());
            wasm_bindgen_futures::spawn_local(async move {
                *done.borrow_mut() = Some(get_text(&url).await);
            });
            Ok(Call(slot))
        }

        /// The logged-in player's bests on the server (`<mode> <score>`
        /// lines), the terminal's rounds included.
        pub fn bests() -> Call {
            let slot = Slot::default();
            let done = slot.clone();
            let url = format!("{}/v1/me/bests", api());
            wasm_bindgen_futures::spawn_local(async move {
                let answer = get_text(&url).await;
                // the session's gone: logged out here too
                if answer
                    .as_deref()
                    .is_ok_and(|t| t.starts_with("err signed-out "))
                {
                    store::remove("account");
                }
                *done.borrow_mut() = Some(answer);
            });
            Call(slot)
        }

        /// The run `id`'s replay, answered the way SSH does (`ok <hex>`).
        pub fn replay(id: &str) -> Result<Call, String> {
            let slot = Slot::default();
            let done = slot.clone();
            let id = id.to_string();
            wasm_bindgen_futures::spawn_local(async move {
                let answer = fetch(&id)
                    .await
                    .map(|b| format!("ok {}", crate::store::hex(&b)));
                *done.borrow_mut() = Some(answer);
            });
            Ok(Call(slot))
        }

        pub fn poll(&mut self) -> Option<Result<String, String>> {
            self.0.borrow_mut().take()
        }
    }

    /// A room for THE HOT ONE over a WebSocket (`/v1/room/new`, or
    /// `/v1/room/<code>`); a logged-in browser plays as its login. Lines
    /// land in a queue as they come; hanging up is dropping it.
    pub struct Room {
        socket: web_sys::WebSocket,
        lines: Rc<RefCell<Vec<String>>>,
        closed: Rc<RefCell<bool>>,
        /// The callbacks stay alive as long as the room does.
        _on: Vec<web_sys::wasm_bindgen::closure::Closure<dyn FnMut(web_sys::Event)>>,
    }

    impl Room {
        pub fn open(code: Option<&str>) -> Result<Room, String> {
            use web_sys::wasm_bindgen::JsCast;
            use web_sys::wasm_bindgen::closure::Closure;
            let api = api();
            let (scheme, host) = api.split_once("://").unwrap_or(("https", &api));
            let ws = if scheme == "http" { "ws" } else { "wss" };
            let url = format!("{ws}://{host}/v1/room/{}", code.unwrap_or("new"));
            let socket = web_sys::WebSocket::new(&url)
                .map_err(|_| "couldn't reach typepper.com".to_string())?;
            let lines = Rc::new(RefCell::new(Vec::new()));
            let closed = Rc::new(RefCell::new(false));
            let mut on = Vec::new();
            let got = lines.clone();
            let on_message = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
                if let Some(text) = e
                    .dyn_ref::<web_sys::MessageEvent>()
                    .and_then(|m| m.data().as_string())
                {
                    got.borrow_mut().extend(text.lines().map(str::to_string));
                }
            });
            socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on.push(on_message);
            for set in [
                web_sys::WebSocket::set_onclose,
                web_sys::WebSocket::set_onerror,
            ] {
                let gone = closed.clone();
                let f = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
                    *gone.borrow_mut() = true;
                });
                set(&socket, Some(f.as_ref().unchecked_ref()));
                on.push(f);
            }
            Ok(Room {
                socket,
                lines,
                closed,
                _on: on,
            })
        }

        /// One line to the room (lines before the socket opens are kept
        /// until it does; the browser queues them).
        pub fn send(&mut self, line: &str) {
            if self.socket.ready_state() == web_sys::WebSocket::CONNECTING {
                // not open yet: the first lines wait a frame or two
                self.pending_push(line);
                return;
            }
            self.flush_pending();
            if self.socket.send_with_str(&format!("{line}\n")).is_err() {
                *self.closed.borrow_mut() = true;
            }
        }

        fn pending_push(&mut self, line: &str) {
            PENDING.with(|p| p.borrow_mut().push(line.to_string()));
        }

        fn flush_pending(&mut self) {
            let pending: Vec<String> = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
            for line in pending {
                if self.socket.send_with_str(&format!("{line}\n")).is_err() {
                    *self.closed.borrow_mut() = true;
                }
            }
        }

        pub fn poll(&mut self) -> Result<Vec<String>, String> {
            if self.socket.ready_state() == web_sys::WebSocket::OPEN {
                self.flush_pending();
            }
            let lines = std::mem::take(&mut *self.lines.borrow_mut());
            match (lines.is_empty(), *self.closed.borrow()) {
                (true, true) => Err(super::GONE.into()),
                _ => Ok(lines),
            }
        }
    }

    impl Drop for Room {
        fn drop(&mut self) {
            PENDING.with(|p| p.borrow_mut().clear());
            let _ = self.socket.close();
        }
    }

    thread_local! {
        /// Lines sent before the socket opened (one room at a time).
        static PENDING: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    /// Off to GitHub to log in; the page comes back with `?login=`.
    pub fn log_in() {
        if let Some(w) = web_sys::window() {
            let _ = w.location().set_href(&format!("{}/auth/github", api()));
        }
    }

    /// Forgets the login here, and ends its session on the server.
    pub fn log_out() {
        store::remove("account");
        let init = with_cookies("POST");
        let url = format!("{}/auth/logout", api());
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(w) = web_sys::window() {
                let _ = JsFuture::from(w.fetch_with_str_and_init(&url, &init)).await;
            }
        });
    }

    /// `?login=<login>`, as the server sends a player back from GitHub:
    /// kept as the account, and taken out of the address.
    pub fn arrived() {
        let Some(w) = web_sys::window() else { return };
        let query = w.location().search().unwrap_or_default();
        let (Some(login), rest) = super::login_in(&query) else {
            return;
        };
        let _ = store::write("account", &login);
        // the rest (a local `?api=`) stays
        if let Ok(h) = w.history() {
            let _ =
                h.replace_state_with_url(&web_sys::wasm_bindgen::JsValue::NULL, "", Some(&rest));
        }
    }

    /// A request that brings the session cookie along.
    fn with_cookies(method: &str) -> web_sys::RequestInit {
        let init = web_sys::RequestInit::new();
        init.set_method(method);
        init.set_credentials(web_sys::RequestCredentials::Include);
        init
    }

    async fn get_text(url: &str) -> Result<String, String> {
        let window = web_sys::window().ok_or("no window")?;
        let init = with_cookies("GET");
        let response = JsFuture::from(window.fetch_with_str_and_init(url, &init))
            .await
            .map_err(|_| "couldn't reach typepper.com".to_string())?;
        let response: web_sys::Response = response.into();
        let text = JsFuture::from(response.text().map_err(|_| "no answer")?)
            .await
            .map_err(|_| "no answer".to_string())?;
        text.as_string().ok_or("no answer".into())
    }

    async fn post(path: &str, body: Vec<u8>) -> Result<String, String> {
        let window = web_sys::window().ok_or("no window")?;
        let init = with_cookies("POST");
        init.set_body(&js_sys::Uint8Array::from(&body[..]).into());
        let url = format!("{}{path}", api());
        let response = JsFuture::from(window.fetch_with_str_and_init(&url, &init))
            .await
            .map_err(|_| "couldn't reach typepper.com".to_string())?;
        let response: web_sys::Response = response.into();
        let text = JsFuture::from(response.text().map_err(|_| "no answer")?)
            .await
            .map_err(|_| "no answer".to_string())?;
        text.as_string().ok_or("no answer".into())
    }

    /// A replay's bytes, for `?replay=<id>`.
    pub async fn fetch(id: &str) -> Result<Vec<u8>, String> {
        let window = web_sys::window().ok_or("no window")?;
        let url = format!("{}/v1/runs/{id}", api());
        let response = JsFuture::from(window.fetch_with_str(&url))
            .await
            .map_err(|_| "couldn't reach typepper.com".to_string())?;
        let response: web_sys::Response = response.into();
        if !response.ok() {
            return Err("that replay is gone (they keep for a week)".into());
        }
        let buffer = JsFuture::from(response.array_buffer().map_err(|_| "no body")?)
            .await
            .map_err(|_| "no body".to_string())?;
        Ok(js_sys::Uint8Array::new(&buffer).to_vec())
    }

    /// The API, or `?api=` (for a local server while working on it).
    fn api() -> String {
        let query = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .unwrap_or_default();
        query
            .trim_start_matches('?')
            .split('&')
            .find_map(|kv| kv.strip_prefix("api="))
            .map_or(super::API.into(), |a| a.to_string())
    }

    /// The link on the clipboard: the canvas has no text to select.
    pub fn copy(text: &str) {
        if let Some(w) = web_sys::window() {
            let _ = w.navigator().clipboard().write_text(text);
        }
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boards_read_their_rows() {
        let text = "1 112 leodiegues 2026-10-24 7Xf9qyuC\n2 98.5 y0sif 2026-10-23\nme 14 61 OctoTest 2026-10-20\n";
        let rows = rows(text).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].run.as_deref(), Some("7Xf9qyuC"));
        assert_eq!(rows[1].run, None, "an older server's row");
        assert_eq!(
            (rows[0].rank, rows[0].score, rows[0].login.as_str()),
            (1, 112.0, "leodiegues")
        );
        assert!(rows[2].me && rows[2].rank == 14);
        assert_eq!(super::rows(""), Ok(vec![]), "an empty board");
        assert_eq!(
            super::rows("err bad-board no such mode\n"),
            Err("no such mode".into())
        );
        let saved = "ok x7Kp2QaZ 112 14 https://r.typepper.com/x7Kp2QaZ";
        assert_eq!(
            answer(saved),
            Ok("r.typepper.com/x7Kp2QaZ · #14 all time".into())
        );
    }

    #[test]
    fn coming_back_from_github_keeps_the_rest() {
        assert_eq!(
            login_in("?api=http://localhost:8080&login=Octo-Cat"),
            (
                Some("Octo-Cat".into()),
                "/?api=http://localhost:8080".into()
            )
        );
        assert_eq!(login_in("?login=a"), (Some("a".into()), "/".into()));
        assert_eq!(login_in("?login=<b>"), (None, "/".into()), "not a login");
        assert_eq!(login_in("?replay=x7Kp2QaZ").0, None);
    }

    #[test]
    fn replays_come_as_hex() {
        assert_eq!(replay("ok 545052ff\n"), Ok(vec![0x54, 0x50, 0x52, 0xff]));
        assert_eq!(
            replay("err gone that replay is gone\n"),
            Err("that replay is gone".into())
        );
        assert!(replay("ok zz").is_err());
        assert!(replay("").is_err());
    }

    #[test]
    fn an_empty_board_is_an_answer() {
        assert_eq!(heard(String::new(), true), Ok(String::new()));
        assert!(heard(String::new(), false).is_err(), "ssh couldn't connect");
        let busy = "err busy try later\n".to_string();
        assert_eq!(heard(busy.clone(), false), Ok(busy), "the server's own err");
    }

    #[test]
    fn only_final_answers_empty_the_outbox() {
        for done in [
            "ok x 1 - r",
            "err old-engine update",
            "err bad-replay truncated",
            "err too-big big",
        ] {
            assert!(settled(done), "{done}");
        }
        for keep in [
            "err busy try later",
            "err rate-limited later",
            "err too-many only 20",
            "",
            "ok",
        ] {
            assert!(!settled(keep), "{keep:?}");
        }
    }

    #[test]
    fn answers_become_links_or_reasons() {
        let ok = "ok 7Xf9qyuC 108 - https://r.typepper.com/7Xf9qyuC";
        assert_eq!(answer(ok), Ok("r.typepper.com/7Xf9qyuC".into()));
        let err = "err old-engine update typepper to submit";
        assert_eq!(answer(err), Err("update typepper to submit".into()));
        assert!(answer("").is_err());
        assert!(answer("<html>").is_err());
        let body = submission(&[1, 2], Some(108.0));
        assert_eq!(
            (&body[..8], &body[8..]),
            (&108f64.to_le_bytes()[..], &[1u8, 2][..])
        );
    }
}
