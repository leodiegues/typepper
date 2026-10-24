mod app;
mod best;
mod fx;
mod lava;
mod menu;
mod pepper;
mod settings;
mod store;
mod theme;
mod ui;

use app::{App, Bot, Key};
// the rules live in their own crate, shared with the server
use typepper_engine as engine;
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::io::Result<()> {
    use std::time::Duration;

    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    // ask the terminal for its background before raw mode; unanswered → dark
    let light = flag("--light")
        || !flag("--dark")
            && terminal_colorsaurus::theme_mode(Default::default())
                .is_ok_and(|m| m == terminal_colorsaurus::ThemeMode::Light);
    theme::set_light(light);

    // --autopilot [wpm]: only types (for recording GIFs, see demo/)
    let autopilot = args.iter().position(|a| a == "--autopilot").map(|i| {
        let wpm = args.get(i + 1).and_then(|w| w.parse().ok());
        Bot::typist(wpm.unwrap_or(120.0))
    });
    let bot = autopilot.or(flag("--demo").then(Bot::tour));
    // --flawless: the bot never misses, so a run stays at reaper to watch its effects
    // --replay [FILE]: watch a round (FILE, or the last one played here)
    let watch = match args.iter().position(|a| a == "--replay") {
        Some(i) => match load_replay(args.get(i + 1).filter(|a| !a.starts_with("--"))) {
            Ok(w) => Some(w),
            Err(why) => {
                eprintln!("typepper: can't play that replay: {why}");
                std::process::exit(1);
            }
        },
        None => None,
    };
    let bot = bot.map(|b| if flag("--flawless") { b.flawless() } else { b });
    let mut app = App::new(bot, watch);
    // --fps N: frame cap. ~120 by default; recordings use less, since a
    // recording terminal can't draw full-screen flames that fast (see demo/)
    let fps: f64 = args
        .iter()
        .position(|a| a == "--fps")
        .and_then(|i| args.get(i + 1)?.parse().ok())
        .unwrap_or(120.0);
    let frame = Duration::from_secs_f64(1.0 / fps.max(1.0));
    // --script "2:right 3:left 4:enter": keys pressed at those seconds, so a
    // recording can drive the menus on its own (see demo/record.sh)
    let mut script: Vec<(f64, Key)> = args
        .iter()
        .position(|a| a == "--script")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.split_whitespace().filter_map(scripted).rev().collect())
        .unwrap_or_default();
    let began = Instant::now();
    ratatui::run(|terminal| {
        loop {
            while script
                .last()
                .is_some_and(|(at, _)| began.elapsed().as_secs_f64() >= *at)
            {
                let (_, key) = script.pop().unwrap();
                if app.key(key) {
                    return Ok(());
                }
            }
            terminal.draw(|f| app.draw(f))?;
            // one frame per `frame` (a frame draws in well under 1ms, and
            // terminals on 120Hz screens show it); a keypress wakes poll at once
            if event::poll(frame)?
                && let Event::Key(k) = event::read()?
                && k.kind == KeyEventKind::Press
            {
                let key = match k.code {
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Esc => Some(Key::Esc),
                    KeyCode::Tab => Some(Key::Tab),
                    KeyCode::Enter => Some(Key::Enter),
                    KeyCode::Left => Some(Key::Left),
                    KeyCode::Right => Some(Key::Right),
                    KeyCode::Up => Some(Key::Up),
                    KeyCode::Down => Some(Key::Down),
                    KeyCode::Backspace => Some(Key::Backspace),
                    KeyCode::F(2) => Some(Key::Settings),
                    KeyCode::Char(c) => Some(Key::Char(c)),
                    _ => None,
                };
                if key.is_some_and(|k| app.key(k)) {
                    return Ok(());
                }
            }
            app.tick(Instant::now());
        }
    })
}

/// The replay at `path`, or the last round played on this machine, checked
/// against these rules.
#[cfg(not(target_arch = "wasm32"))]
fn load_replay(path: Option<&String>) -> Result<(engine::Setup, engine::Replay), String> {
    let bytes = match path {
        Some(p) => std::fs::read(p).map_err(|e| format!("{p}: {e}"))?,
        None => store::read_bytes("last.replay").ok_or("no round played yet")?,
    };
    let replay = engine::Replay::decode(&bytes).map_err(|e| e.to_string())?;
    if replay.engine != engine::VERSION {
        return Err(format!(
            "it was recorded by other rules (engine {}, these are {})",
            replay.engine,
            engine::VERSION
        ));
    }
    let setup = replay
        .setup()
        .ok_or("its mode isn't one this typepper has")?;
    Ok((setup, replay))
}

/// One `--script` step, "seconds:key" (a key name, or a single character).
#[cfg(not(target_arch = "wasm32"))]
fn scripted(step: &str) -> Option<(f64, Key)> {
    let (at, name) = step.split_once(':')?;
    let key = match name {
        "esc" => Key::Esc,
        "tab" => Key::Tab,
        "enter" => Key::Enter,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "backspace" => Key::Backspace,
        "space" => Key::Char(' '),
        _ => Key::Char(name.chars().next()?),
    };
    Some((at.parse().ok()?, key))
}

/// Browser entry: waits for the chosen web font, then starts the game.
/// `?demo` in the URL turns on the autopilot (`?demo&flawless`: it never misses).
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen_futures::JsFuture;

    let font = settings::FONTS[settings::Settings::load().font];
    wasm_bindgen_futures::spawn_local(async move {
        // the atlas measures cells and rasterizes glyphs as soon as it exists:
        // load the web font first, or the fallback gets baked in for good
        let doc = ratzilla::web_sys::window().and_then(|w| w.document());
        if font != "system"
            && let Some(load) = doc.map(|d| d.fonts().load(&format!("16px \"{font}\"")))
        {
            let _ = JsFuture::from(load).await;
        }
        start(font).expect("typepper failed to start");
    });
}

#[cfg(target_arch = "wasm32")]
fn start(font: &'static str) -> Result<(), Box<dyn std::error::Error>> {
    use std::cell::RefCell;
    use std::rc::Rc;

    use ratzilla::backend::webgl2::WebGl2BackendOptions;
    use ratzilla::event::KeyCode;
    use ratzilla::ratatui::Terminal;
    use ratzilla::{FontAtlasConfig, WebGl2Backend, WebRenderer};

    let window = ratzilla::web_sys::window();
    let query = window
        .as_ref()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let demo = query.contains("demo");
    let prefers_light = window
        .and_then(|w| {
            w.match_media("(prefers-color-scheme: light)")
                .ok()
                .flatten()
        })
        .is_some_and(|m| m.matches());
    theme::set_light(query.contains("light") || prefers_light && !query.contains("dark"));
    let coarse = ratzilla::web_sys::window()
        .and_then(|w| w.match_media("(pointer: coarse)").ok().flatten())
        .is_some_and(|m| m.matches());
    settings::set_touch(coarse || query.contains("touch"));
    let bot = demo.then(Bot::tour);
    let bot = bot.map(|b| {
        if query.contains("flawless") {
            b.flawless()
        } else {
            b
        }
    });
    let app = Rc::new(RefCell::new(App::new(bot, None)));

    // dynamic atlas rasterizes any glyph (blocks, faces, sparks) from the font stack
    let system = ["Menlo", "Consolas", "DejaVu Sans Mono", "monospace"];
    let stack: Vec<&str> = std::iter::once(font)
        .filter(|&f| f != "system")
        .chain(system)
        .collect();
    let backend = WebGl2Backend::new_with_options(
        WebGl2BackendOptions::new()
            .grid_id("game") // sized by the page, leaving room for the footer
            .font_atlas_config(FontAtlasConfig::dynamic(&stack, 16.0))
            .canvas_padding_color(theme::page()),
    )?;
    let mut terminal = Terminal::new(backend)?;

    terminal.on_key_event({
        let app = app.clone();
        move |k| {
            let key = match k.code {
                KeyCode::Esc => Key::Esc,
                KeyCode::Tab => Key::Tab,
                KeyCode::Enter => Key::Enter,
                KeyCode::Left => Key::Left,
                KeyCode::Right => Key::Right,
                KeyCode::Up => Key::Up,
                KeyCode::Down => Key::Down,
                KeyCode::Backspace => Key::Backspace,
                KeyCode::F(2) => Key::Settings,
                KeyCode::Char(c) if !k.ctrl && !k.alt => Key::Char(c),
                _ => return,
            };
            // nothing to quit to in a browser tab
            let _ = app.borrow_mut().key(key);
        }
    })?;

    // the page around the grid follows the theme (also when settings flip it)
    let mut painted = None;
    terminal.draw_web(move |f| {
        let mut app = app.borrow_mut();
        app.tick(Instant::now());
        app.draw(f);
        let light = theme::is_light();
        if painted != Some(light) {
            painted = Some(light);
            if let Some(root) = ratzilla::web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.document_element())
            {
                let _ = root.set_attribute("data-theme", if light { "light" } else { "dark" });
            }
        }
    });
    Ok(())
}
