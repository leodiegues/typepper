//! FLOOR IS LAVA on screen (the rules are `engine::lava`). Field positions
//! are fractions (x across, y down, 1 = the bottom); `draw` maps them to cells.

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};
use web_time::Instant;

use crate::engine::lava::{LANDINGS, Lava, RISE};
use crate::engine::{Aim, RANKS};
use crate::fx::{self, fire, hash, lerp, mix, page_rgb, put, put_behind, rank_color, since};
use crate::settings::{calm, touch};
use crate::{menu, ui};

const BURST: f64 = 0.6; // a cleared word flying apart
const SPLASH: f64 = 0.7; // a landed word's spatter
const RISING: f64 = 0.4; // the lava swelling up after a landing

/// Where a word of `len` letters at field position `(x, y)` starts on screen.
fn cell(field: Rect, x: f64, y: f64, len: usize) -> (i32, i32) {
    let span = field.height.saturating_sub(1) as f64;
    let cx = field.x as f64 + field.width.saturating_sub(len as u16) as f64 * x;
    (
        cx.round() as i32,
        field.y as i32 + (y * span).round() as i32,
    )
}

pub fn draw(f: &mut Frame, l: &Lava, now: Instant) {
    let col = ui::column(f.area());
    let [hud1, hud2, _, field, heat, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(col);
    // narrow screens (phones) get a compact HUD so its two sides don't collide
    let narrow = col.width < 64;
    fx::speed_lines(f.buffer_mut(), &l.test, now);
    hud(f, l, now, [hud1, hud2], narrow);
    let buf = f.buffer_mut();
    let surface = pool(buf, l, field, now);
    words(buf, l, field);
    if l.test.started.is_none() {
        let [a, b] = menu::rules(l.test.mode);
        let lines: Vec<Line> = [a, b, "", "type a falling word to start"]
            .map(|s| s.dark_gray().into())
            .into();
        let middle = Rect::new(field.x, field.y + field.height / 2, field.width, 5);
        Paragraph::new(lines)
            .centered()
            .wrap(Wrap { trim: true })
            .render(middle.intersection(field), buf);
    }
    bursts(buf, l, field, now);
    splashes(buf, l, field, surface, now);

    let t = &l.test;
    f.render_widget(ui::heat_bar(t.heat, t.rank, heat.width as usize), heat);
    let esc = if touch() { "✕" } else { "esc" };
    let hint = match (l.aim, narrow || touch()) {
        (Aim::Lock, false) => format!("backspace lets go of a word · tab restart · {esc} menu"),
        (Aim::Lock, true) => format!("backspace let go · {esc} menu"),
        (Aim::Free, false) => format!("space starts over · tab restart · {esc} menu"),
        (Aim::Free, true) => format!("space starts over · {esc} menu"),
    };
    f.render_widget(
        Line::from(format!("  {hint}  ").dark_gray()).centered(),
        help,
    );
}

/// Mode, level, words; score on the right. Lava meter, speed; combo below.
fn hud(f: &mut Frame, l: &Lava, now: Instant, [top, bottom]: [Rect; 2], narrow: bool) {
    let t = &l.test;
    let color = rank_color(t.rank);
    let (r, g, b) = menu::group(t.mode).color;
    let name = match narrow {
        true => format!("LAVA {}", t.mode),
        false => menu::label(t.mode),
    };
    let left = Line::from(vec![
        name.fg(Color::Rgb(r, g, b)).bold(),
        format!("  LV{}", t.level()).yellow().bold(),
        format!("  {} word{}", t.words, if t.words == 1 { "" } else { "s" }).into(),
    ]);
    f.render_widget(left, top);
    let score = Line::from(vec![
        "SCORE ".dark_gray(),
        t.score.to_string().fg(color).bold(),
    ]);
    f.render_widget(score.right_aligned(), top);
    let landed = l.splashes.len().min(LANDINGS);
    let mut meter = vec![
        "LAVA ".dark_gray(),
        Span::styled("▰".repeat(landed), Color::Rgb(255, 70, 20)),
        "▱".repeat(LANDINGS - landed).dark_gray(),
    ];
    if t.started.is_some() {
        meter.push(format!("  {:.0} wpm", t.wpm(now)).into());
        meter.push(format!("  {:.0}%", t.accuracy()).dark_gray());
    }
    f.render_widget(Line::from(meter), bottom);
    let mut combo = vec![
        format!("×{}  ", t.multiplier()).fg(color).bold(),
        format!("{} ", t.combo).into(),
    ];
    if !narrow {
        // on a phone the heat bar's color already says the rank
        combo.push("combo  ".dark_gray());
        combo.push(RANKS[t.rank].name.fg(color).bold());
    }
    f.render_widget(Line::from(combo).right_aligned(), bottom);
}

/// The lava, swelling up for a moment after each landing. Returns its surface row.
fn pool(buf: &mut Buffer, l: &Lava, field: Rect, now: Instant) -> u16 {
    let secs = fx::anim_clock(&l.test, now);
    let swell = l.splashes.last().map_or(1.0, |s| {
        (since(Some(s.0), now).as_secs_f64() / RISING).min(1.0)
    });
    let shown = (l.lava() - RISE * (1.0 - swell)).max(0.0);
    let span = field.height.saturating_sub(1) as f64;
    let surface = field.y + ((1.0 - shown) * span).round() as u16;
    for y in surface..field.bottom() {
        for x in field.left()..field.right() {
            let (sym, v) = fx::lava_cell(x, y, y == surface, y - surface, secs);
            buf[(x, y)].set_symbol(sym).set_fg(fire(v));
        }
    }
    // bubbles popping just above the surface
    if !calm() && surface > field.y {
        for x in field.left()..field.right() {
            if hash(x as u64, 7, (secs * 2.0) as u64) < 5 {
                put(
                    buf,
                    x as i32,
                    surface as i32 - 1,
                    "°",
                    Style::new().fg(fire(0.9)),
                );
            }
        }
    }
    surface
}

/// The falling words: the typed part white-hot, the rest heating up near the lava.
fn words(buf: &mut Buffer, l: &Lava, field: Rect) {
    let page = page_rgb();
    let span = field.height.saturating_sub(1) as f64;
    for d in &l.drops {
        let (x, y) = cell(field, d.x, d.y, d.word.len());
        let n = l.typed_in(d);
        let danger = ((d.y - (1.0 - l.lava() - 0.35)) / 0.35).clamp(0.0, 1.0);
        let heat = mix((215, 215, 215), (255, 90, 40), danger);
        // between rows, a faint echo on the row it's moving into (or out of),
        // strongest halfway: the eye reads the step-by-row fall as smooth
        let fy = d.y * span;
        let frac = fy - fy.floor();
        let echo = 0.45 * (1.0 - (2.0 * frac - 1.0).abs());
        if echo > 0.03 {
            let ey = if frac < 0.5 { y + 1 } else { y - 1 };
            let style = Style::new().fg(lerp(page, heat, echo));
            for (i, ch) in d.word.iter().enumerate() {
                put_behind(buf, x + i as i32, ey, ch.encode_utf8(&mut [0; 4]), style);
            }
        }
        let rest = Style::new().fg(Color::Rgb(heat.0, heat.1, heat.2));
        let locked = n > 0 && l.aim == Aim::Lock;
        for (i, ch) in d.word.iter().enumerate() {
            let mut style = match i < n {
                true => Style::new().fg(fire(0.98)).bold(),
                false => rest,
            };
            if locked {
                style = style.underlined();
            }
            put(buf, x + i as i32, y, ch.encode_utf8(&mut [0; 4]), style);
        }
    }
}

/// Cleared words fly apart letter by letter, their points floating up.
fn bursts(buf: &mut Buffer, l: &Lava, field: Rect, now: Instant) {
    for (k, c) in l.cleared.iter().enumerate().rev() {
        let age = since(Some(c.at), now).as_secs_f64();
        if age > fx::POPUP {
            break;
        }
        let (x0, y0) = cell(field, c.x, c.y, c.word.len());
        if age < BURST {
            let style = Style::new().fg(fire(1.0 - age / BURST)).bold();
            for (i, ch) in c.word.iter().enumerate() {
                let seed = (k * 31 + i) as u64;
                let vx = (hash(seed, 1, 3) as f64 / 255.0 - 0.5) * 24.0;
                let vy = -(2.0 + hash(seed, 2, 3) as f64 / 255.0 * 6.0);
                let x = x0 as f64 + i as f64 + vx * age;
                let y = y0 as f64 + vy * age + 14.0 * age * age;
                put(
                    buf,
                    x.round() as i32,
                    y.round() as i32,
                    ch.encode_utf8(&mut [0; 4]),
                    style,
                );
            }
        }
        let mid = x0 + c.word.len() as i32 / 2;
        fx::popup(buf, mid, y0, c.points, l.test.rank, age / fx::POPUP);
    }
}

/// Landed words spatter up out of the lava.
fn splashes(buf: &mut Buffer, l: &Lava, field: Rect, surface: u16, now: Instant) {
    for (k, &(at, x, len)) in l.splashes.iter().enumerate().rev() {
        let age = since(Some(at), now).as_secs_f64();
        if age > SPLASH {
            break;
        }
        let x0 = cell(field, x, 0.0, len).0 as f64;
        let style = Style::new().fg(fire(1.0 - age / SPLASH));
        for p in 0..10u64 {
            let seed = k as u64 * 17 + p;
            let sx = x0 + (hash(seed, 4, 4) as f64 / 255.0) * len as f64;
            let vx = (hash(seed, 5, 4) as f64 / 255.0 - 0.5) * 10.0;
            let vy = 4.0 + hash(seed, 6, 4) as f64 / 255.0 * 6.0;
            let y = surface as f64 - vy * age + 12.0 * age * age;
            let sym = if p % 3 == 0 { "*" } else { "·" };
            put(
                buf,
                (sx + vx * age).round() as i32,
                y.round() as i32,
                sym,
                style,
            );
        }
    }
}
