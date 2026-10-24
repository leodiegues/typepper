//! Title screen: a carousel of modes. ←/→ swipes, ↑/↓ picks a length.
//! Each mode animates the scenery around the cards, and a swipe cross-fades
//! one into the next. Every animation is a function of time since the last
//! swipe, so the state is just where the carousel is going, where it came
//! from, and when.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph, Widget, Wrap};
use web_time::Instant;

use crate::best::{self, Bests};
use crate::engine::{Aim, LEVELS, Mode};
use crate::fx::{POLICE, Rgb, SIREN_RED, WHITE, blur, fire, hash, lerp, mix, page_rgb, rank_color};
use crate::settings::{ROWS, Row, Settings};
use crate::{pepper, theme, ui};

pub struct Group {
    pub name: &'static str,
    blurb: &'static str,
    pub color: Rgb,
    /// The scenery around the cards while this mode is up; `w` (0..1) fades it.
    scene: fn(&mut Buffer, Rect, Rect, f64, f64),
    modes: &'static [Mode],
}

pub const MENU: [Group; 6] = [
    Group {
        name: "SPRINT",
        blurb: "type the words, fast as you can",
        color: (110, 215, 80),
        scene: sprint,
        modes: &[Mode::Words(10), Mode::Words(25), Mode::Words(50)],
    },
    Group {
        name: "BLITZ",
        blurb: "beat the clock",
        color: (255, 215, 0),
        scene: blitz,
        modes: &[Mode::Time(15), Mode::Time(30), Mode::Time(60)],
    },
    Group {
        name: "ZEN",
        blurb: "no clock, no pressure · stop whenever",
        color: (60, 200, 190),
        scene: zen,
        modes: &[Mode::Zen],
    },
    Group {
        name: "SURVIVAL",
        blurb: "feed the fuel · it drains faster every level",
        color: (255, 50, 30),
        scene: survival,
        modes: &[Mode::Survival { start_level: 1 }],
    },
    Group {
        name: "FLOOR IS LAVA",
        blurb: "type the words before they hit the lava",
        color: (255, 110, 20),
        scene: lava,
        modes: &[
            Mode::Lava {
                aim: Aim::Lock,
                start_level: 1,
            },
            Mode::Lava {
                aim: Aim::Free,
                start_level: 1,
            },
        ],
    },
    Group {
        name: "HOT PURSUIT",
        blurb: "outrun the cops · a wrong key stops you cold",
        // police blue: stays blue (not teal like ZEN) when the light theme deepens it
        color: POLICE,
        scene: pursuit,
        modes: &[Mode::Pursuit { start_level: 1 }],
    },
];

pub fn group(m: Mode) -> &'static Group {
    &MENU[slot(m)]
}

/// Which card a mode is on.
fn slot(m: Mode) -> usize {
    match m {
        Mode::Words(_) => 0,
        Mode::Time(_) => 1,
        Mode::Zen => 2,
        Mode::Survival { .. } => 3,
        Mode::Lava { .. } => 4,
        Mode::Pursuit { .. } => 5,
    }
}

/// "BLITZ 30s", "ZEN".
pub fn label(m: Mode) -> String {
    m.name()
}

/// Esc can't close a browser tab, so the web title screen doesn't offer it.
const QUIT: &str = if cfg!(target_arch = "wasm32") {
    ""
} else {
    " · esc quit"
};
const SLIDE: f64 = 0.26; // seconds per swipe
const WAIT_TITLE: f64 = 0.12; // card mostly arrived; text starts
const DECODE: f64 = 0.35; // title letters scrambling into place
const TYPE: f64 = 0.4; // blurb typing itself in
const CHIP: f64 = 0.18; // length highlight sliding between options
const PRESS: f64 = 0.2; // enter: the card squeezes and springs back
const NUDGE: f64 = 0.25; // a ◂ ▸ hint lighting up on its swipe

fn since(t: Option<Instant>, now: Instant) -> f64 {
    t.map_or(f64::MAX, |t| now.saturating_duration_since(t).as_secs_f64())
}

pub struct Menu {
    /// Carousel target; unbounded so wrapping around still slides one step.
    target: i64,
    /// Carousel position when the last swipe started.
    from: f64,
    /// When the last swipe started (or the menu was entered).
    at: Instant,
    /// Chosen length per group.
    len: [usize; MENU.len()],
    /// Last starting level picked per card (index into `LEVELS`).
    start: [usize; MENU.len()],
    /// Last length change on the selected card: from which option, when.
    chip: (usize, Option<Instant>),
    /// Enter was pressed: the card squeezes, then the test starts.
    pressed: Option<Instant>,
    /// Clock for idle animations.
    born: Instant,
}

impl Menu {
    pub fn new(now: Instant) -> Self {
        Menu {
            target: 1,
            from: 1.0,
            at: now,
            len: [1, 1, 0, 0, 0, 0],
            start: [0; MENU.len()],
            chip: (0, None),
            pressed: None,
            born: now,
        }
    }

    fn age(&self, now: Instant) -> f64 {
        now.saturating_duration_since(self.at).as_secs_f64()
    }

    /// Carousel position: card `i` is centered at `i` (mod 4).
    fn scroll(&self, now: Instant) -> f64 {
        let k = self.age(now) / SLIDE;
        if k >= 1.0 {
            return self.target as f64;
        }
        self.from + (self.target as f64 - self.from) * ease_out_back(k)
    }

    pub fn sel(&self) -> usize {
        self.target.rem_euclid(MENU.len() as i64) as usize
    }

    pub fn mode(&self) -> Mode {
        let s = self.sel();
        MENU[s].modes[self.len[s]]
    }

    /// Mashing keys restarts the slide from wherever the cards are now.
    pub fn swipe(&mut self, dir: i64, now: Instant) {
        self.from = self.scroll(now);
        self.target += dir;
        self.at = now;
    }

    pub fn length(&mut self, dir: i64, now: Instant) {
        let s = self.sel();
        let n = MENU[s].modes.len() as i64;
        self.chip = (self.len[s], Some(now));
        self.len[s] = (self.len[s] as i64 + dir).rem_euclid(n) as usize;
    }

    /// Where the selected card's length highlight is, in options (slides).
    fn chip_at(&self, now: Instant) -> f64 {
        let to = self.len[self.sel()] as f64;
        let k = since(self.chip.1, now) / CHIP;
        match k < 1.0 {
            true => self.chip.0 as f64 + (to - self.chip.0 as f64) * ease_out_back(k),
            false => to,
        }
    }

    pub fn press(&mut self, now: Instant) {
        self.pressed.get_or_insert(now);
    }

    pub fn pressing(&self) -> bool {
        self.pressed.is_some()
    }

    /// The press animation has played: time to start the test.
    pub fn press_done(&self, now: Instant) -> bool {
        self.pressed.is_some_and(|p| since(Some(p), now) >= PRESS)
    }

    /// Demo: swiped all the way round, back on the starting card.
    pub fn lap_done(&self) -> bool {
        self.target > MENU.len() as i64
    }

    /// Back from a test: same card, its title decodes in again.
    /// The starting level last picked for `m`'s card (index into `LEVELS`).
    pub fn start_of(&self, m: Mode) -> usize {
        self.start[slot(m)]
    }

    pub fn set_start(&mut self, m: Mode, i: usize) {
        self.start[slot(m)] = i;
    }

    pub fn back(&mut self, now: Instant) {
        self.from = self.target as f64;
        self.at = now;
        self.pressed = None;
    }
}

/// The settings dialog over the menu.
pub struct Dialog {
    pub row: usize,
    opened: Instant,
    /// When a value last changed (it flashes).
    pub changed: Option<Instant>,
}

impl Dialog {
    pub fn new(now: Instant) -> Self {
        Dialog {
            row: 0,
            opened: now,
            changed: None,
        }
    }
}

const POP: f64 = 0.18; // the dialog springing open

/// Settings over the menu: the menu fades back, the box springs open, and
/// the selected row's value flashes when it changes.
pub fn dialog(f: &mut Frame, d: &Dialog, s: &Settings, now: Instant) {
    let area = f.area();
    let k = (since(Some(d.opened), now) / POP).min(1.0);
    dim(f.buffer_mut(), 0.6 * k);
    let h = (ROWS.len() as u16 * 2 + 5).min(area.height);
    let w = (44.0 * (0.7 + 0.3 * ease_out_back(k))).round() as u16;
    let w = w.min(area.width);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Color::White)
        .title(Line::from(" SETTINGS ".bold()).centered());
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let flash = since(d.changed, now) < 0.15;
    for (j, &row) in ROWS.iter().enumerate() {
        let y = inner.y + 1 + 2 * j as u16;
        if y >= inner.bottom() {
            break;
        }
        let on = j == d.row;
        let label = match row {
            Row::Theme => "theme",
            Row::Effects => "effects",
            Row::Font => "font",
            Row::Online => "online",
        };
        let line = Rect::new(inner.x + 2, y, inner.width.saturating_sub(4), 1);
        let label = if on {
            label.white().bold()
        } else {
            label.dark_gray()
        };
        f.render_widget(Line::from(label), line);
        let value = format!("{:^16}", s.value(row));
        let value = match on {
            true if flash => value.black().on_white().bold(),
            true => value.white().bold(),
            false => value.gray(),
        };
        let arrow = |a: &'static str| if on { a.yellow().bold() } else { "  ".into() };
        let right = Line::from(vec![arrow("◂ "), value, arrow(" ▸")]);
        f.render_widget(right.right_aligned(), line);
    }
    let hint = match ROWS.get(d.row) {
        Some(Row::Font) => "new font loads when you close this",
        _ => "↑/↓ pick · ←/→ change · esc close",
    };
    let bottom = Rect::new(inner.x, inner.bottom() - 1, inner.width, 1);
    f.render_widget(Line::from(hint.dark_gray()).centered(), bottom);
}

/// Lines `text` takes when word-wrapped to `width` (like `Wrap { trim: true }`).
fn wrapped_rows(text: &str, width: usize) -> u16 {
    let (mut rows, mut line) = (1, 0);
    for word in text.split(' ') {
        let n = word.chars().count();
        if line > 0 && line + 1 + n > width {
            rows += 1;
            line = n;
        } else {
            line += n + usize::from(line > 0);
        }
    }
    rows
}

/// Fade everything already drawn toward the page, `k` 0..1 (behind a dialog).
fn dim(buf: &mut Buffer, k: f64) {
    let page = page_rgb();
    for cell in buf.content.iter_mut() {
        if let Color::Rgb(r, g, b) = cell.fg {
            cell.fg = lerp((r, g, b), page, k);
        }
    }
}

/// The starting-level picker over a leveled game (survival, lava, pursuit),
/// before its first key: Tetris-style, so experienced typers can skip the
/// warm-up.
pub struct Pick {
    pub choice: usize,
    /// The chip slides from here.
    from: usize,
    moved: Option<Instant>,
    opened: Instant,
}

impl Pick {
    pub fn new(choice: usize, now: Instant) -> Self {
        Pick {
            choice,
            from: choice,
            moved: None,
            opened: now,
        }
    }

    pub fn step(&mut self, dir: i64, now: Instant) {
        self.from = self.choice;
        self.choice = (self.choice as i64 + dir).rem_euclid(LEVELS.len() as i64) as usize;
        self.moved = Some(now);
    }

    /// Seconds since it opened (the demo autopilot confirms after a moment).
    pub fn age(&self, now: Instant) -> f64 {
        since(Some(self.opened), now)
    }

    fn chip_at(&self, now: Instant) -> f64 {
        let k = since(self.moved, now) / CHIP;
        let to = self.choice as f64;
        match k < 1.0 {
            true => self.from as f64 + (to - self.from as f64) * ease_out_back(k),
            false => to,
        }
    }
}

/// How each starting level feels, in heat ranks (see `engine::RANKS`).
const PICK_HEAT: [(&str, usize); 4] = [("mild", 1), ("medium", 3), ("hot", 4), ("extra hot", 6)];

pub fn pick(f: &mut Frame, p: &Pick, mode: Mode, bests: &Bests, now: Instant) {
    let area = f.area();
    let k = (p.age(now) / POP).min(1.0);
    dim(f.buffer_mut(), 0.6 * k);
    let (r, g, b) = group(mode).color;
    let color = Color::Rgb(r, g, b);
    let h = 9.min(area.height);
    let w = ((36.0 * (0.7 + 0.3 * ease_out_back(k))).round() as u16).min(area.width);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(color)
        .title(Line::from(" START LEVEL ".bold()).centered());
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let row = |dy: u16| Rect::new(inner.x, inner.y + dy, inner.width, 1);
    let labels: Vec<String> = LEVELS.iter().map(|l| format!("lv{l}")).collect();
    options(f.buffer_mut(), row(1), &labels, p.chip_at(now), color);
    let (heat, rank) = PICK_HEAT[p.choice];
    f.render_widget(
        Line::from(heat.fg(rank_color(rank)).bold()).centered(),
        row(3),
    );
    let best = match bests.get(mode.from_level(LEVELS[p.choice])) {
        Some(v) => format!("best {}", best::show(mode, v)),
        None => "no best from here yet".to_string(),
    };
    f.render_widget(Line::from(best.dark_gray()).centered(), row(4));
    let hint = match crate::settings::touch() {
        true => "swipe pick · tap go · ✕ back",
        false => "←/→ pick · enter go · esc back",
    };
    f.render_widget(Line::from(hint.dark_gray()).centered(), row(6));
}

/// Ease out with a slight overshoot (~6%) before settling.
fn ease_out_back(k: f64) -> f64 {
    let (c1, k) = (1.2, k - 1.0);
    1.0 + (c1 + 1.0) * k.powi(3) + c1 * k * k
}

pub fn draw(f: &mut Frame, m: &Menu, bests: &Bests, now: Instant) {
    let area = f.area();
    let col = ui::column(area);
    // logo a bit above the middle, the 3D pepper gets everything above it;
    // on short screens the menu below keeps its room and the pepper shrinks
    let below = (area.height.saturating_sub(5) as f64 * 0.6) as u16;
    let below = below.max(18);
    let [stage, logo, rest] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(5),
        Constraint::Length(below),
    ])
    .areas(area);
    let [_, tag, _, cards, _, dot_row, _, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(11),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(2), // cards ride a bit above the middle
        Constraint::Length(1),
    ])
    .areas(rest);
    let secs = now.saturating_duration_since(m.born).as_secs_f64();
    let s = m.scroll(now);
    // the scenes of the two cards passing through the middle, cross-faded
    let (i, k) = (s.floor() as i64, s - s.floor());
    let focus = Rect {
        x: area.x + area.width.saturating_sub(CARD_W) / 2,
        width: CARD_W.min(area.width),
        ..cards
    };
    for (j, w) in [(i, 1.0 - k), (i + 1, k)] {
        // calm: the scenery stays still (it's all screen-wide motion)
        if w > 0.01 && !crate::settings::calm() {
            (MENU[j.rem_euclid(MENU.len() as i64) as usize].scene)(
                f.buffer_mut(),
                area,
                focus,
                secs,
                w,
            );
        }
    }

    // spin, sway, and morph up the heat ladder: 4s per pepper, the last 1s morphing
    if stage.height >= 4 {
        let shapes = &pepper::RANK_SHAPES;
        let n = (secs / 4.0) as usize;
        let k = 0.5 - 0.5 * (((secs % 4.0) - 3.0).clamp(0.0, 1.0) * std::f64::consts::PI).cos();
        let shape = shapes[n % shapes.len()].lerp(&shapes[(n + 1) % shapes.len()], k);
        let pose = pepper::Pose {
            spin: secs * 1.1,
            tilt: 0.35 + 0.12 * (secs * 0.7).sin(),
        };
        pepper::draw(f.buffer_mut(), col.intersection(stage), &shape, &pose);
    }
    // heat ripples through the letters, bell pepper → reaper
    let rows: Vec<Line> = (0..5)
        .map(|row| {
            "TYPEPPER"
                .chars()
                .enumerate()
                .flat_map(|(i, c)| {
                    let k = (secs * 2.0 - i as f64 * 0.4).sin() * 0.5 + 0.5;
                    let mut cells = ui::solid(
                        ui::glyph(c).unwrap()[row],
                        rank_color(2 + (k * 4.99) as usize),
                    );
                    cells.push(" ".into());
                    cells
                })
                .collect()
        })
        .collect();
    ratatui::widgets::Paragraph::new(rows)
        .centered()
        .render(logo, f.buffer_mut());
    Line::from("a spicy little typing game".dark_gray())
        .centered()
        .render(tag, f.buffer_mut());

    carousel(f.buffer_mut(), cards, m, bests, now, secs);

    dots(f.buffer_mut(), dot_row, m.scroll(now));
    // padded so the scenery keeps a little distance from the text
    let hint = match crate::settings::touch() {
        true => "  swipe ←/→ mode · ↑/↓ length · tap to play  ".to_string(),
        false => {
            let boards = if crate::settings::offline() {
                ""
            } else {
                " · b boards"
            };
            format!("  ←/→ mode · ↑/↓ length · enter play{boards} · s settings{QUIT}  ")
        }
    };
    Line::from(hint.dark_gray())
        .centered()
        .render(help, f.buffer_mut());
}

/// Cards side by side, the selected one centered, neighbors peeking in dimmed.
/// Each card is drawn into its own buffer, then copied over clipped, so cards
/// can hang off either edge of the screen.
const CARD_W: u16 = 46;
/// Most focus a card one slot off the middle keeps (1 − this): shade blocks, no letters.
const SIDE_BLUR: f64 = 0.55;
/// Side cards' size relative to the middle one.
const SIDE: f64 = 0.7;
/// The middle card's border glints white as it lands, then cools to its color.
const GLINT: f64 = 0.35;

fn carousel(buf: &mut Buffer, area: Rect, m: &Menu, bests: &Bests, now: Instant, secs: f64) {
    let s = m.scroll(now);
    let before = now.checked_sub(Duration::from_millis(10)).unwrap_or(now);
    let v = (s - m.scroll(before)) / 0.01; // cards per second
    let big = CARD_W.min(area.width.saturating_sub(10)).max(24) as f64;
    let small = (big * SIDE).round();
    let gap = (big + small) / 2.0 + 4.0; // center to center, one slot
    let mid = area.x as f64 + area.width as f64 / 2.0;
    // gone within one slot either way, however wide the screen
    let falloff = (area.width as f64 / 2.0 - big / 2.0).clamp(8.0, gap);
    for (i, g) in MENU.iter().enumerate() {
        // offset from the middle, wrapped to ±n/2
        let n = MENU.len() as f64;
        let d = (i as f64 - s + n / 2.0).rem_euclid(n) - n / 2.0;
        // cards shrink away from the middle, so they grow as they slide in
        let k = d.abs().min(1.0);
        // enter squeezes the selected card in and lets it spring back
        let sq = match (i == m.sel(), m.pressed) {
            (true, Some(p)) => (since(Some(p), now) / PRESS * std::f64::consts::PI)
                .sin()
                .max(0.0),
            _ => 0.0,
        };
        let w = (big + (small - big) * k - 4.0 * sq).round() as u16;
        let h = (area.height as f64 - 2.0 * k - 2.0 * sq).round() as u16;
        let x = (mid + d * gap - w as f64 / 2.0).round() as i32;
        // only the middle card and one either side
        if d.abs() >= 1.75 || x + w as i32 <= area.x as i32 || x >= area.right() as i32 {
            continue;
        }
        let mut card = Buffer::empty(Rect::new(0, 0, w, h));
        let mode = g.modes[m.len[i]];
        // leveled modes show the best from the starting level last picked
        let start = if mode.leveled() {
            LEVELS[m.start[i]]
        } else {
            1
        };
        let best = bests.get(mode.from_level(start)).map(|v| match start {
            1 => best::show(mode, v),
            _ => format!("{} · from lv{start}", best::show(mode, v)),
        });
        draw_card(&mut card, m, i, best, now, secs);
        // sharp inside the middle slot, defocusing toward the screen edges;
        // a side card is capped as a whole, so its inner edge can't stay
        // readable while the rest is blocks (it sharpens as it slides in)
        let cap = 1.0 - SIDE_BLUR * d.abs();
        blit(buf, &card, x, area.y + (area.height - h) / 2, |sx| {
            let off = (sx as f64 + 0.5 - mid).abs() - big / 2.0 - 1.0;
            (1.0 - off.max(0.0) / falloff).min(cap)
        });
    }
    hints(buf, area, m, mid - big / 2.0, mid + big / 2.0, now, secs);
    // speed streaks through the empty space while the cards fly
    if v.abs() > 1.5 {
        let len = (v.abs() * 3.0).min(18.0) as i32;
        let dir = v.signum() as i32; // cards move left when v > 0: tails trail right
        let page = page_rgb();
        let color = MENU[m.sel()].color;
        for y in area.top() + 1..area.bottom() - 1 {
            let seed = hash(y as u64, 13, 0) as u64;
            if !seed.is_multiple_of(3) {
                continue;
            }
            let span = area.width as f64 + 40.0;
            let head = (seed as f64 * 7.0 - s * gap * 1.6).rem_euclid(span) as i32 - 20;
            for j in 0..len {
                let x = area.x as i32 + head + j * dir;
                if !(area.left() as i32..area.right() as i32).contains(&x) {
                    continue;
                }
                let cell = &mut buf[(x as u16, y)];
                if cell.symbol() == " " {
                    let k = j as f64 / len as f64;
                    cell.set_symbol(if j == 0 { "━" } else { "─" })
                        .set_fg(lerp(color, page, k));
                }
            }
        }
    }
}

fn draw_card(card: &mut Buffer, m: &Menu, i: usize, best: Option<String>, now: Instant, secs: f64) {
    let (g, sel, age) = (&MENU[i], i == m.sel(), m.age(now));
    let area = card.area;
    let (r, gr, b) = g.color;
    let color = Color::Rgb(r, gr, b);
    let block = Block::bordered()
        .border_type(if sel {
            BorderType::Thick
        } else {
            BorderType::Rounded
        })
        .border_style(match sel {
            _ if sel && m.pressing() => Color::White,
            true => lerp(WHITE, g.color, ((age - WAIT_TITLE) / GLINT).clamp(0.0, 1.0)),
            false => color,
        })
        // breathing room so text never touches the border
        .padding(Padding::horizontal(2));
    let inner = block.inner(area);
    block.render(area, card);
    // a blurb too long for the card (on a phone) wraps, taking the rows it
    // needs (up to three); a row of air always separates the parts, and the
    // leftover space centers them (it gives way first on the smaller side cards)
    let blurb_rows = wrapped_rows(g.blurb, inner.width as usize).min(3);
    let [_, title, _, blurb, _, len, best_row, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(blurb_rows),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ])
    .areas(inner);

    // the new card's title decodes: each letter scrambles, then locks in, left
    // to right; letter-spaced when it fits, plain when the card is narrow
    let n = g.name.chars().count() as f64;
    let gap = if 2 * g.name.chars().count() > inner.width as usize {
        ""
    } else {
        " "
    };
    let spaced: Vec<Span> = g
        .name
        .chars()
        .enumerate()
        .flat_map(|(i, c)| {
            let lock = WAIT_TITLE + DECODE * i as f64 / n;
            let c = match sel && age < lock {
                true => {
                    let h = hash(i as u64, (secs * 30.0) as u64, 2) as usize;
                    ['#', '%', '&', '*', '@', '$', '?', '!'][h % 8]
                }
                false => c,
            };
            [c.to_string().fg(color).bold(), gap.into()]
        })
        .collect();
    Line::from(spaced).centered().render(title, card);

    let k = if sel {
        ((age - WAIT_TITLE) / TYPE).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let n = (g.blurb.chars().count() as f64 * k) as usize;
    let mut typed: String = g.blurb.chars().take(n).collect();
    if k < 1.0 {
        typed.push('▌');
    }
    Paragraph::new(Line::from(typed.gray()).centered())
        .wrap(Wrap { trim: true })
        .render(blurb, card);

    match g.modes {
        [_] => Line::from("endless".dark_gray())
            .centered()
            .render(len, card),
        modes => {
            let chip = if sel { m.chip_at(now) } else { m.len[i] as f64 };
            let labels: Vec<String> = modes.iter().map(|m| m.to_string()).collect();
            options(card, len, &labels, chip, color);
        }
    }
    if let Some(b) = best {
        Line::from(format!("best {b}").dark_gray())
            .centered()
            .render(best_row, card);
    }
}

/// Length options in a row, with a highlight chip at option `chip`; a
/// fractional `chip` puts it partway between two options, sliding.
fn options(card: &mut Buffer, row: Rect, labels: &[String], chip: f64, color: Color) {
    let labels: Vec<String> = labels.iter().map(|l| format!(" {l} ")).collect();
    let total: usize = labels.iter().map(|l| l.chars().count() + 1).sum::<usize>() - 1;
    let x0 = row.x as f64 + (row.width as f64 - total as f64) / 2.0;
    let mut starts = Vec::new();
    let mut x = x0.floor();
    for l in &labels {
        starts.push(x);
        x += l.chars().count() as f64 + 1.0;
    }
    let mut cx = x0.floor() as u16;
    for (j, l) in labels.iter().enumerate() {
        if j > 0 {
            cx += 1;
        }
        Line::from(l.as_str().dark_gray())
            .render(Rect::new(cx, row.y, l.chars().count() as u16, 1), card);
        cx += l.chars().count() as u16;
    }
    let chip = chip.clamp(0.0, (labels.len() - 1) as f64);
    let (j, f) = (chip.floor() as usize, chip.fract());
    let k = (j + 1).min(labels.len() - 1);
    let width = |j: usize| labels[j].chars().count() as f64;
    let hx = starts[j] + (starts[k] - starts[j]) * f;
    let hw = width(j) + (width(k) - width(j)) * f;
    for x in hx.round() as u16..(hx + hw).round() as u16 {
        if card.area.contains(Position::new(x, row.y)) {
            card[(x, row.y)].set_style(Style::new().black().bg(color).bold());
        }
    }
}

/// ◂ ▸ either side of the middle card: breathing while idle, lit and nudged
/// outward on a swipe their way.
fn hints(buf: &mut Buffer, area: Rect, m: &Menu, left: f64, right: f64, now: Instant, secs: f64) {
    let y = area.y + area.height / 2;
    let dir = (m.target as f64 - m.from).signum() as i64;
    let lit = m.age(now) < NUDGE;
    let idle = lerp(
        page_rgb(),
        (200, 200, 200),
        0.35 + 0.25 * (secs * 3.0).sin(),
    );
    for (side, x, sym) in [(-1, left - 2.0, "◂"), (1, right + 1.0, "▸")] {
        let on = lit && dir == side;
        let x = x as i32 + if on { side as i32 } else { 0 };
        if (area.left() as i32..area.right() as i32).contains(&x) {
            let style = match on {
                true => Style::new().fg(Color::White).bold(),
                false => Style::new().fg(idle),
            };
            buf[(x as u16, y)].set_symbol(sym).set_style(style);
        }
    }
}

/// One dot per mode; while sliding, the active one stretches toward the
/// next like a worm, then catches up.
fn dots(buf: &mut Buffer, row: Rect, s: f64) {
    let n = MENU.len() as i64;
    let x0 = row.x as i32 + (row.width as i32 - (2 * n as i32 - 1)) / 2;
    let dot_x = |i: i64| x0 + 2 * i.rem_euclid(n) as i32;
    for i in 0..n {
        let x = dot_x(i);
        if (row.left() as i32..row.right() as i32).contains(&x) {
            buf[(x as u16, row.y)]
                .set_symbol("●")
                .set_fg(Color::DarkGray);
        }
    }
    let (i, k) = (s.floor() as i64, s - s.floor());
    let color = mix(
        MENU[i.rem_euclid(n) as usize].color,
        MENU[(i + 1).rem_euclid(n) as usize].color,
        k,
    );
    let style = Style::new().fg(Color::Rgb(color.0, color.1, color.2));
    let (a, b) = (dot_x(i), dot_x(i + 1));
    // wrapping around the ends just hops; neighbors stretch
    let (tail, head) = match b > a {
        true => (
            a + ((b - a) as f64 * (2.0 * k - 1.0).max(0.0)).round() as i32,
            a + ((b - a) as f64 * (2.0 * k).min(1.0)).round() as i32,
        ),
        false => {
            let at = if k < 0.5 { a } else { b };
            (at, at)
        }
    };
    for x in tail..=head {
        if (row.left() as i32..row.right() as i32).contains(&x) {
            let sym = if x == tail || x == head { "●" } else { "━" };
            buf[(x as u16, row.y)].set_symbol(sym).set_style(style);
        }
    }
}

/// Copy `card` onto `buf` with its left edge at `x` (may be off-screen),
/// defocused by `focus(screen x)` (1 = sharp): glyphs step down a density
/// ramp and fade toward the page, and cells that dissolve completely let the
/// scenery through.
fn blit(buf: &mut Buffer, card: &Buffer, x: i32, y: u16, focus: impl Fn(i32) -> f64) {
    let page = page_rgb();
    let a = buf.area;
    for cy in 0..card.area.height {
        for cx in 0..card.area.width {
            let (sx, sy) = (x + cx as i32, y + cy);
            if !(a.left() as i32..a.right() as i32).contains(&sx) || sy >= a.bottom() {
                continue;
            }
            let mut cell = card[(cx, cy)].clone();
            let f = focus(sx);
            if f < 1.0 {
                // a little noise dithers the steps of the ramp
                let noise = hash(sx as u64, sy as u64, 5) as f64 / 255.0 - 0.5;
                let f = (f + 0.15 * noise).clamp(0.0, 0.97);
                let sym = blur(cell.symbol(), f);
                if sym == Some(" ") {
                    continue;
                }
                // selected-length chips are colored backgrounds: the ink is the chip
                let ink = match (cell.bg, cell.fg) {
                    (Color::Rgb(..), _) => cell.bg,
                    (_, fg) => fg,
                };
                let ink = match ink {
                    Color::Rgb(r, g, b) => lerp(page, (r, g, b), 0.25 + 0.75 * f),
                    _ => Color::DarkGray,
                };
                // an explicit page bg makes `theme::apply` leave the fade alone
                if let Some(sym) = sym {
                    cell.set_symbol(sym);
                }
                cell.set_fg(ink).set_bg(theme::page());
            }
            buf[(sx as u16, sy)] = cell;
        }
    }
}

// ── scenery: one per mode, drawn behind everything ─────────────────────

/// Clipped write into an empty cell, colored `color` at strength `k` over the page.
fn dab(buf: &mut Buffer, x: i32, y: i32, sym: &str, color: Rgb, k: f64) {
    let a = buf.area;
    if (a.left() as i32..a.right() as i32).contains(&x)
        && (a.top() as i32..a.bottom() as i32).contains(&y)
    {
        buf[(x as u16, y as u16)].set_symbol(sym).set_fg(lerp(
            page_rgb(),
            color,
            k.clamp(0.0, 1.0),
        ));
    }
}

/// Lanes fade in and out by weight: lane `seed` shows once `w` passes it.
fn lane_on(seed: u8, w: f64) -> bool {
    (seed as f64) < w * 256.0
}

/// SPRINT: speed lines tearing across the whole screen.
fn sprint(buf: &mut Buffer, area: Rect, _focus: Rect, secs: f64, w: f64) {
    let width = area.width as f64;
    for y in area.top()..area.bottom() {
        let seed = hash(y as u64, 31, 0);
        if !seed.is_multiple_of(3) || !lane_on(hash(y as u64, 32, 0), w) {
            continue;
        }
        let len = 6 + seed as i32 % 16;
        let speed = 70.0 + (seed % 50) as f64;
        let head = width - (secs * speed + seed as f64 * 7.0) % (width + len as f64);
        for j in 0..len {
            let fade = 1.0 - j as f64 / len as f64; // bright head, dim tail
            let sym = if j == 0 { "━" } else { "─" };
            let c = mix((110, 215, 80), WHITE, 0.3 * fade);
            dab(
                buf,
                area.x as i32 + head as i32 + j,
                y as i32,
                sym,
                c,
                0.75 * fade * w,
            );
        }
    }
}

/// BLITZ: a clock face of ticks around the card; the second hand sweeps
/// round once a minute-in-12, and every second a pulse ripples out.
fn blitz(buf: &mut Buffer, _area: Rect, focus: Rect, secs: f64, w: f64) {
    let (cx, cy) = (
        focus.x as f64 + focus.width as f64 / 2.0,
        focus.y as f64 + focus.height as f64 / 2.0,
    );
    let (rx, ry) = (
        focus.width as f64 / 2.0 + 8.0,
        focus.height as f64 / 2.0 + 4.0,
    );
    let gold = (255, 215, 0);
    let hand = secs.floor() as i64;
    for t in 0..12i64 {
        let a = t as f64 / 12.0 * std::f64::consts::TAU - std::f64::consts::FRAC_PI_2;
        let (x, y) = (cx + rx * a.cos(), cy + ry * a.sin());
        let ago = (hand - t).rem_euclid(12) as f64; // 0 = the hand is here
        let k = if ago == 0.0 {
            1.0
        } else {
            0.25 + 0.45 * (1.0 - ago / 4.0).max(0.0)
        };
        let sym = if ago == 0.0 { "◆" } else { "◇" };
        dab(buf, x.round() as i32, y.round() as i32, sym, gold, k * w);
    }
    // the pulse: an ellipse growing out from the card each second
    let p = secs.fract();
    let (px, py) = (rx * (0.6 + 0.9 * p), ry * (0.6 + 0.9 * p));
    for step in 0..96 {
        let a = step as f64 / 96.0 * std::f64::consts::TAU;
        let (x, y) = (cx + px * a.cos(), cy + py * a.sin());
        if focus.contains(Position::new(x.max(0.0) as u16, y.max(0.0) as u16)) {
            continue;
        }
        dab(
            buf,
            x.round() as i32,
            y.round() as i32,
            "·",
            gold,
            (1.0 - p) * 0.6 * w,
        );
    }
}

/// ZEN: fireflies drifting slowly up, swaying, blinking softly.
fn zen(buf: &mut Buffer, area: Rect, _focus: Rect, secs: f64, w: f64) {
    let h = area.height as f64;
    for n in 0..(area.width / 4) as u64 {
        let seed = hash(n, 41, 0) as f64;
        if !lane_on(hash(n, 42, 0), w) {
            continue;
        }
        let up = (secs * (0.6 + seed / 255.0) + seed) % h;
        let x = (n * 4) as f64 + 2.0 * (secs * 0.7 + seed).sin();
        let glow = 0.5 + 0.5 * (secs * 1.8 + seed).sin();
        let sym = if glow > 0.7 { "•" } else { "·" };
        let y = area.bottom() as f64 - 1.0 - up;
        dab(
            buf,
            area.x as i32 + x as i32,
            y as i32,
            sym,
            (60, 200, 190),
            (0.25 + 0.75 * glow) * w,
        );
    }
}

/// SURVIVAL: flames along the bottom, sinking like a draining fuel gauge,
/// then roaring back.
fn survival(buf: &mut Buffer, area: Rect, _focus: Rect, secs: f64, w: f64) {
    let fuel = 1.0 - (secs % 5.0 / 5.0).powf(1.5); // drains, then refills
    let h = area.height as f64;
    let reach = h * (0.12 + 0.28 * fuel) * w;
    for x in area.left()..area.right() {
        let xf = x as f64;
        let wave = ((xf * 0.31 + secs * 4.0).sin() + (xf * 0.13 - secs * 2.3).sin()) / 2.0;
        let col_h = reach * (0.7 + 0.3 * wave);
        for up in 0..col_h.ceil() as u16 {
            let y = area.bottom() - 1 - up.min(area.height - 1);
            let noise = hash(x as u64, y as u64, (secs * 15.0) as u64) as f64 / 255.0;
            let v = 1.0 - up as f64 / col_h.max(0.01) + (noise - 0.5) * 0.35;
            if v > 0.05 {
                let v = v.min(1.0);
                buf[(x, y)]
                    .set_symbol([".", ":", "*", "▒", "▓", "█"][((v * 5.99) as usize).min(5)])
                    .set_fg(fire(v * w));
            }
        }
    }
}

/// FLOOR IS LAVA: words drift down into a bubbling pool along the bottom.
fn lava(buf: &mut Buffer, area: Rect, focus: Rect, secs: f64, w: f64) {
    const WORDS: [&str; 6] = ["chili", "heat", "salsa", "spice", "pepper", "sauce"];
    let h = area.height as f64;
    let pool = (h * 0.12 * w).round() as u16;
    for n in 0..(area.width / 9) as u64 {
        if !lane_on(hash(n, 52, 0), w) {
            continue;
        }
        let seed = hash(n, 51, 0) as f64;
        let word = WORDS[n as usize % WORDS.len()];
        let fall = h - pool as f64;
        let y = (secs * (1.0 + seed / 255.0) + seed) % fall.max(1.0);
        let x = area.x as i32 + (n * 9) as i32 + (hash(n, 53, 0) % 3) as i32;
        let row = area.y + y as u16;
        if (focus.top()..focus.bottom()).contains(&row) {
            continue; // behind the cards: letters there would read as loose card text
        }
        let near = y / fall; // words glow hotter as they near the lava
        let c = mix((150, 150, 150), (255, 110, 40), near * near);
        for (j, ch) in word.chars().enumerate() {
            let mut utf8 = [0; 4];
            let sym = ch.encode_utf8(&mut utf8);
            dab(
                buf,
                x + j as i32,
                area.y as i32 + y as i32,
                sym,
                c,
                0.55 * w,
            );
        }
    }
    for up in 0..pool {
        let y = area.bottom() - 1 - up;
        for x in area.left()..area.right() {
            let (sym, v) = crate::fx::lava_cell(x, y, up + 1 == pool, pool - 1 - up, secs);
            buf[(x, y)].set_symbol(sym).set_fg(fire(v * w));
        }
    }
}

/// HOT PURSUIT: a racer tearing along the road the cards stand on, a cop on
/// its tail, the siren flashing red and blue off the lane dashes streaming by.
fn pursuit(buf: &mut Buffer, area: Rect, focus: Rect, secs: f64, w: f64) {
    const RED: Rgb = SIREN_RED;
    const BLUE: Rgb = POLICE;
    // side view, facing right: a cabin on a body on two wheels
    const CAR: [&str; 2] = [" ▄███▄▄ ", "▀█▀▀▀▀█▀"];
    let (left, right) = (area.left() as i32, area.right() as i32);
    // the chase drives the lane just under the cards (behind them, the side
    // cards' blurred text would break the cars up)
    let (top, lane) = (focus.top() as i32 - 1, focus.bottom() as i32);
    // the racer laps the screen; the cop closes in and drops back, both gone
    // off the edge (siren glow too) before the racer wraps around
    let racer = left + ((secs * 34.0).rem_euclid(area.width as f64 + 60.0) - 30.0) as i32;
    let cop = racer - 8 - (3.0 + 3.0 * (1.0 + (secs * 0.9).sin())) as i32;
    // wig-wag: red, then blue, three flashes a second each
    let red_on = (secs * 6.0) as i64 % 2 == 0;
    let lit = if red_on { RED } else { BLUE };
    // lane dashes on both edges of the road; near the cop they catch the siren
    for y in [top, lane + 2] {
        for x in left..right {
            let u = x as f64 + secs * 40.0;
            let dash = hash((u / 8.0) as u64, y as u64, 61);
            if u % 8.0 >= 4.0 || !lane_on(dash, w) {
                continue;
            }
            let near = (1.0 - (x - cop - 3).abs() as f64 / 12.0).max(0.0);
            let c = mix((130, 130, 140), lit, near);
            dab(buf, x, y, "━", c, (0.4 + 0.5 * near) * w);
        }
    }
    for (x, color) in [(cop, BLUE), (racer, (255, 215, 0))] {
        for (dy, row) in CAR.iter().enumerate() {
            for (j, ch) in row.chars().enumerate().filter(|&(_, c)| c != ' ') {
                let mut utf8 = [0; 4];
                let sym = ch.encode_utf8(&mut utf8);
                dab(buf, x + j as i32, lane + dy as i32, sym, color, 0.9 * w);
            }
        }
    }
    // the light bar on the cop's roof
    for (j, color, on) in [
        (2, RED, red_on),
        (3, (150, 150, 150), false),
        (4, BLUE, !red_on),
    ] {
        let k = if on { 1.0 } else { 0.3 };
        dab(buf, cop + j, lane - 1, "▄", color, k * w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blurbs_wrap_like_the_paragraph() {
        assert_eq!(wrapped_rows("beat the clock", 40), 1);
        assert_eq!(
            wrapped_rows("type the words before they hit the lava", 26),
            2
        );
        assert_eq!(
            wrapped_rows("feed the fuel · it drains faster every level", 22),
            3
        );
        assert_eq!(
            wrapped_rows("outrun the cops · a wrong key stops you cold", 40),
            2
        );
    }

    #[test]
    fn blur_steps_down_the_ramp() {
        assert_eq!(blur("Z", 0.9), None, "sharp enough: untouched");
        assert_eq!(blur("Z", 0.5), Some("▒"));
        assert_eq!(blur("━", 0.3), Some("╌"), "lines break up along themselves");
        assert_eq!(blur("┃", 0.3), Some("╎"));
        assert_eq!(blur("Z", 0.0), Some(" "), "gone");
    }

    #[test]
    fn swipes_wrap_and_settle() {
        let t0 = Instant::now();
        let mut m = Menu::new(t0);
        assert_eq!((m.sel(), m.mode()), (1, Mode::Time(30)));
        m.swipe(-1, t0);
        m.swipe(-1, t0);
        assert_eq!(m.sel(), 5, "wraps past the first card");
        m.length(1, t0);
        assert_eq!(
            m.mode(),
            Mode::Pursuit { start_level: 1 },
            "single-length modes stay put"
        );
        m.swipe(-1, t0);
        assert_eq!(
            m.mode(),
            Mode::Lava {
                aim: Aim::Lock,
                start_level: 1
            }
        );
        m.length(1, t0);
        assert_eq!(
            m.mode(),
            Mode::Lava {
                aim: Aim::Free,
                start_level: 1
            }
        );
        let done = t0 + Duration::from_secs(1);
        assert_eq!(m.scroll(done), -2.0);
        assert!(
            m.scroll(t0 + Duration::from_millis(200)) < -2.0,
            "overshoots"
        );
        m.swipe(1, done);
        m.swipe(1, done);
        m.length(-1, done);
        assert_eq!(m.mode(), Mode::Words(10), "back on sprint, shortest");
        assert_eq!(label(m.mode()), "SPRINT 10w");
    }

    #[test]
    fn every_mode_lands_on_its_own_card() {
        for (i, g) in MENU.iter().enumerate() {
            for &m in g.modes {
                assert_eq!(slot(m), i, "{m}");
            }
        }
        assert_eq!(label(Mode::Pursuit { start_level: 1 }), "HOT PURSUIT");
    }
}
