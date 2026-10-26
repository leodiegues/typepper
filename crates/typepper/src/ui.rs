use std::time::Duration;

use web_time::Instant;

use ratatui::prelude::*;
use ratatui::symbols::Marker;
use ratatui::widgets::{
    Axis, Block, BorderType, Chart, Clear, Dataset, GraphType, Padding, Paragraph,
};

use crate::best::{self, Outcome};
use crate::engine::{FUEL, HOT, LEAD_MAX, Mode, PICK_SECS, RANKS, STRIKES, Served, Test, scoville};
use crate::fx::{
    self, POLICE, RANK_RGB, Rgb, SIREN_RED, WHITE, anim_clock, hash, hsv, lerp, rank_color, rgb,
    since,
};
use crate::settings::{calm, touch};
use crate::{menu, pepper};

const FLASH: Duration = Duration::from_millis(120);
const SHAKE: Duration = Duration::from_millis(250);
const CALLOUT: Duration = Duration::from_millis(1200);
const MISS_RED: Color = Color::Rgb(200, 30, 30);
/// THE HOT ONE: a strike flashes the edges this long.
const STRUCK: f64 = 0.35;

const DIGITS: [[&str; 5]; 10] = [
    ["███", "█ █", "█ █", "█ █", "███"],
    [" █ ", "██ ", " █ ", " █ ", "███"],
    ["███", "  █", "███", "█  ", "███"],
    ["███", "  █", "███", "  █", "███"],
    ["█ █", "█ █", "███", "  █", "  █"],
    ["███", "█  ", "███", "  █", "███"],
    ["███", "█  ", "███", "█ █", "███"],
    ["███", "  █", "  █", "  █", "  █"],
    ["███", "█ █", "███", "█ █", "███"],
    ["███", "█ █", "███", "  █", "███"],
];

pub fn column(area: Rect) -> Rect {
    let [_, col, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Max(86),
        Constraint::Fill(1),
    ])
    .areas(area);
    col
}

/// Typing screen or its results.
/// `share`: the results screen's word on sharing the round, if any.
/// `strip`: room for the boards beside the results chart; where it went
/// (once the chart's in, on a screen wide enough).
/// `pepper`: THE HOT ONE, the pepper the player is hovering on a plate
/// (none while watching someone else's round).
pub fn draw(
    f: &mut Frame,
    t: &Test,
    outcome: Option<Outcome>,
    share: Option<&str>,
    strip: bool,
    pepper: Option<usize>,
) -> Option<Rect> {
    play(f, t, outcome, share, strip, pepper, &[], None)
}

/// Someone else typing the same text (a room): where their caret is, in
/// their color, with their initial.
pub struct Ghost {
    pub at: usize,
    pub color: Color,
    pub initial: char,
}

/// The screen (above the typing panel, the panel, below it, the help line).
pub fn play_layout(area: Rect) -> [Rect; 4] {
    Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(18),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(column(area))
}

/// `draw`, with the others in the room ghosting along the text, and the
/// room's own help line (`hint`) instead of the usual one.
#[allow(clippy::too_many_arguments)]
pub fn play(
    f: &mut Frame,
    t: &Test,
    outcome: Option<Outcome>,
    share: Option<&str>,
    strip: bool,
    pepper: Option<usize>,
    ghosts: &[Ghost],
    hint: Option<&str>,
) -> Option<Rect> {
    let now = Instant::now();
    let col = column(f.area());
    if t.finished.is_some() {
        return results(f, col, t, outcome, share, strip, now);
    }
    let [above, panel, _, help] = play_layout(f.area());
    fx::speed_lines(f.buffer_mut(), t, now);
    fx::flames(f.buffer_mut(), t, now);
    fx::knocked(f.buffer_mut(), t, now, panel);
    let (text, combo) = typing(f, panel, help, t, now);
    if let Some(hint) = hint {
        f.render_widget(Clear, help);
        f.render_widget(
            Line::from(format!("  {hint}  ").dark_gray()).centered(),
            help,
        );
    }
    // the others' carets ride the text, behind your own
    for g in ghosts {
        if let Some(p) = text.pos(g.at).filter(|&p| p != text.caret) {
            let cell = &mut f.buffer_mut()[p];
            if cell.symbol() == " " {
                cell.set_symbol(&g.initial.to_ascii_uppercase().to_string());
            }
            cell.set_fg(Color::Black).set_bg(g.color);
        }
    }
    fx::crowd(f.buffer_mut(), t, now, panel);
    // milestones slam into the space over the crowd
    let sky = Rect {
        height: above.height.saturating_sub(3),
        ..above
    };
    fx::disco(f.buffer_mut(), t, now, panel, sky);
    fx::slam(f.buffer_mut(), t, now, sky);
    fx::caret_flame(f.buffer_mut(), t, now, text.caret);
    fx::shatter(f.buffer_mut(), t, now, combo);
    fx::tumblers(f.buffer_mut(), t, now, panel, help);
    // the cop rides over the effects that pass behind the text, and under
    // the box's left border and padding
    if let Some(hood) = text.cop {
        let x = text.inner.x as i32;
        fx::cop(f.buffer_mut(), t, now, hood, x - 2..x);
    }
    // survival nearly dry: the edges throb red with the heartbeat
    if t.fuel < 2.0 {
        let beat = fx::fuel_beat(t, now);
        fx::vignette(f.buffer_mut(), (1.0 - t.fuel / 2.0) * (0.4 + 0.6 * beat));
    }
    // pursuit, under 2s from a bust: the siren lights up the edges
    let bust = fx::bust_in(t);
    if matches!(t.mode, Mode::Pursuit { .. }) && t.started.is_some() && bust < 2.0 {
        let beat = fx::siren_beat(t, now);
        fx::siren_wash(
            f.buffer_mut(),
            t,
            now,
            (1.0 - bust / 2.0) * (0.5 + 0.5 * beat),
        );
    }
    // the hot one: a strike flashes the edges, harder the closer to a plate
    if t.mode == Mode::HotOne {
        let ago = since(t.struck, now).as_secs_f64();
        if ago < STRUCK {
            let k = t.strikes.max(1) as f64 / STRIKES as f64;
            fx::vignette(f.buffer_mut(), (1.0 - ago / STRUCK) * (0.3 + 0.5 * k));
        }
        if t.plate.is_some() || t.chewing.is_some() {
            plate(f, t, pepper, now);
        }
    }
    None
}

/// THE HOT ONE's plate: one pepper at a time, big, each its own color;
/// ←/→ turns the plate to the next, enter (or its number) bites it. Then
/// it's chewed, bite by bite, and only once it's down do you know.
/// `pepper` is the one hovered (none while watching someone else's round).
fn plate(f: &mut Frame, t: &Test, pepper: Option<usize>, now: Instant) {
    let chewing = t.chewed(now);
    let (n, shown, age) = match (t.plate, chewing, t.bites.last()) {
        (Some(p), _, _) => (
            p.peppers as usize,
            pepper.unwrap_or(0),
            t.plate_age(now).unwrap_or(0.0),
        ),
        (None, Some(_), Some(bite)) => (
            bite.peppers as usize,
            bite.pepper as usize,
            since(Some(bite.at), now).as_secs_f64() + PICK_SECS,
        ),
        _ => return,
    };
    let area = f.area();
    let k = (age / 0.18).min(1.0);
    menu::dim(f.buffer_mut(), 0.6 * k);
    let (r, g, b) = menu::HOT_PINK;
    let pink = Color::Rgb(r, g, b);
    let title = match (t.plate.map(|p| p.why), chewing) {
        (_, Some(_)) => " CHEWING… ".to_string(),
        (Some(Served::Strikes), _) => format!(" {STRIKES} STRIKES · PICK A PEPPER "),
        _ => format!(" COURSE {} · PICK A PEPPER ", t.plates + 1),
    };
    let hint = match (chewing.is_some(), touch(), pepper.is_some(), n) {
        (true, ..) => "nom nom nom".to_string(),
        (_, _, false, _) => "picking…".to_string(),
        (_, true, true, _) => format!("type 1-{n} to bite"),
        (_, false, true, 1) => "one left · enter bites it".to_string(),
        (_, false, true, _) => format!("←/→ turn the plate · enter bite · or 1-{n}"),
    };
    let w = 56u16
        .max(title.chars().count() as u16 + 2)
        .max(hint.chars().count() as u16 + 4)
        .min(area.width);
    let h = 18.min(area.height);
    let rect = Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    );
    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(pink)
        .title(Line::from(title.bold()).centered());
    let inner = block.inner(rect);
    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    let buf = f.buffer_mut();
    let [stage, labels, _, fuse_row, hint_row] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);
    // the pepper on show: the menu's jalapeño, in this pepper's color; a
    // bite takes a piece off it, and the stem sinks with it
    let mut shape = pepper::JALAPENO;
    let (cr, cg, cb) = PLATE_COLORS[shown % PLATE_COLORS.len()];
    shape.color = (cr as f64, cg as f64, cb as f64);
    if let Some(c) = chewing {
        let bites = (c * 4.0).floor() / 4.0;
        shape.len *= 1.0 - 0.8 * bites;
        shape.girth *= 1.0 - 0.3 * bites;
    }
    let pose = pepper::Pose {
        spin: age * 1.2 + shown as f64 * 2.0,
        tilt: 0.35 + 0.12 * (age * 0.9).sin(),
    };
    pepper::draw(buf, stage, &shape, &pose);
    // the plate: one dot per pepper, each its color, the shown one named
    let mut spans: Vec<Span> = Vec::new();
    for i in 0..n {
        let (cr, cg, cb) = PLATE_COLORS[i % PLATE_COLORS.len()];
        let c = Color::Rgb(cr, cg, cb);
        if i == shown {
            spans.push(format!(" [{}] ", i + 1).fg(c).bold());
        } else {
            spans.push(format!("  {}  ", i + 1).fg(c));
        }
    }
    Line::from(spans).centered().render(labels, buf);
    // the plate waits only so long: a fuse burning down; chewing fills it back
    let fuse = match chewing {
        Some(c) => c,
        None => (1.0 - age / PICK_SECS).clamp(0.0, 1.0),
    };
    let lit = (fuse_row.width as f64 * fuse).round() as usize;
    Line::from(vec![
        Span::styled("━".repeat(lit), fx::fire(0.5 + 0.5 * fuse)),
        Span::styled("╌".repeat(fuse_row.width as usize - lit), Color::DarkGray),
    ])
    .render(fuse_row, buf);
    Line::from(hint.dark_gray())
        .centered()
        .render(hint_row, buf);
}

/// The peppers on a plate, by their place on it: all innocent colors, no
/// hotter-looking than the next.
pub const PLATE_COLORS: [Rgb; 6] = [
    (60, 190, 70),
    (245, 200, 40),
    (255, 120, 40),
    (220, 50, 50),
    (150, 80, 220),
    (40, 190, 200),
];

/// Blitz's last 5 seconds: time since the clock last ticked over.
fn last_seconds(t: &Test, now: Instant) -> Option<f64> {
    let Mode::Time(s) = t.mode else { return None };
    t.started?;
    let left = s as f64 - t.elapsed(now).as_secs_f64();
    (left > 0.0 && left <= 5.0).then(|| left.ceil() - left)
}

/// Where the typing text sits on screen, so effects can start from a letter.
struct TextGeom {
    inner: Rect,
    shown: Vec<(usize, usize)>,
    caret: (u16, u16),
    /// Pursuit: where the cop car's hood is (see `cop_at`).
    cop: Option<(i32, u16)>,
}

impl TextGeom {
    fn pos(&self, i: usize) -> Option<(u16, u16)> {
        let row = self.shown.iter().position(|&(s, e)| (s..e).contains(&i))?;
        Some((
            self.inner.x + (i - self.shown[row].0) as u16,
            self.inner.y + row as u16,
        ))
    }

    /// Pursuit: the cell for the cop car's hood, the last letter it ate (the
    /// car reaches back two cells from there). Off the top of the window, or
    /// before the text starts, it waits by the top row's left edge, out past
    /// the border but no further left than `lo`, and drives in as it closes.
    fn cop_at(&self, t: &Test, lo: u16) -> Option<(i32, u16)> {
        if !matches!(t.mode, Mode::Pursuit { .. }) {
            return None;
        }
        let hood = t.cop.floor() as i64 - 1;
        let start = self.shown.first()?.0 as i64;
        if hood >= start {
            return self.pos(hood as usize).map(|(x, y)| (x as i32, y));
        }
        let x = (self.inner.x as i64 + hood - start).max(lo as i64);
        Some((x as i32, self.inner.y))
    }
}

/// Draws the HUD panel; returns where the text and the combo counter are.
fn typing(f: &mut Frame, panel: Rect, help: Rect, t: &Test, now: Instant) -> (TextGeom, Rect) {
    let clock = anim_clock(t, now);
    let color = rank_color(t.rank);

    // flames frame the panel instead of burning through it
    f.render_widget(Clear, panel);
    let panel_block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg({
            // blitz's last seconds: the border ticks red with the clock
            let rest = fx::mix((0, 0, 0), RANK_RGB[t.rank], fx::PANEL_DIM);
            let tick = last_seconds(t, now).filter(|_| !calm()).unwrap_or(1.0);
            lerp((255, 40, 30), rest, tick / 0.5)
        }))
        .padding(Padding::new(2, 2, 1, 1));
    let inner = panel_block.inner(panel);
    f.render_widget(panel_block, panel);
    fx::rank_up_sweep(f.buffer_mut(), panel, t, now);

    let [header, _, hud, _, text, heat] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length(1),
        Constraint::Length(5),
        Constraint::Length(1),
    ])
    .areas(inner);

    if t.started.is_none() {
        let (r, g, b) = menu::group(t.mode).color;
        let label = format!(" {} ", menu::label(t.mode));
        f.render_widget(
            Line::from(label.black().bg(Color::Rgb(r, g, b)).bold()),
            header,
        );
    } else {
        let progress = match t.mode {
            Mode::Time(s) => {
                let left = format!("{}", s.saturating_sub(t.elapsed(now).as_secs()));
                match last_seconds(t, now) {
                    Some(tick) if tick < 0.25 => left.white(),
                    Some(_) => left.red(),
                    None => left.yellow(),
                }
            }
            Mode::Words(n) => {
                format!("{}/{n}", t.typed.iter().filter(|c| **c == ' ').count()).yellow()
            }
            Mode::Zen | Mode::Lava { .. } => format!("{} words", t.words).yellow(),
            Mode::Survival { .. } => fuel_gauge(t, fx::fuel_beat(t, now)),
            Mode::Pursuit { .. } => lead_gauge(t, fx::siren_beat(t, now)),
            Mode::HotOne => strikes_gauge(t, now),
        };
        let stats = Line::from(vec![
            progress.bold(),
            format!("  {:.0} wpm", t.wpm(now)).into(),
            format!("  {:.0}%", t.accuracy()).dark_gray(),
        ]);
        f.render_widget(stats, header);
    }
    // odometer: the digits a keystroke changed flash as they tick over
    let digits = t.score.to_string();
    let prev = (t.score - t.last_points).to_string();
    let pad = digits.len() - prev.len();
    let ticking = since(t.last_key, now) < Duration::from_millis(100);
    let mut score = vec!["SCORE ".dark_gray()];
    score.extend(digits.char_indices().map(|(j, c)| {
        let changed = ticking && (j < pad || prev.as_bytes()[j - pad] != digits.as_bytes()[j]);
        c.to_string()
            .fg(if changed { Color::White } else { color })
            .bold()
    }));
    f.render_widget(Line::from(score).right_aligned(), header);

    // HUD: fighting-game combo counter left, pepper rank right
    let [combo_area, rank_area] = Layout::horizontal([Constraint::Fill(1); 2]).areas(hud);
    let pop = t.combo > 0
        && t.combo.is_multiple_of(25)
        && since(t.last_key, now) < Duration::from_millis(200);
    let combo_color = match t.combo {
        0 => Color::DarkGray,
        _ if pop => Color::White,
        _ => color,
    };
    let mut lines: Vec<Line> = big(t.combo)
        .iter()
        .map(|r| Line::from(solid(r, combo_color)))
        .collect();
    lines[1].push_span("  HITS".dark_gray());
    let punch = since(t.mult_up, now).as_secs_f64() < fx::PUNCH;
    let mult_x = combo_area.x + lines[3].width() as u16 + 3; // the N in "  ×N"
    lines[3].push_span(
        format!("  ×{}", t.multiplier())
            .fg(if punch { Color::White } else { color })
            .bold(),
    );
    if t.fever() {
        lines[3].push_span(
            " FEVER"
                .fg(fx::fire(match calm() {
                    true => 0.8,
                    false => 0.6 + 0.4 * (clock * 9.0).sin(),
                }))
                .bold(),
        );
    }
    f.render_widget(Paragraph::new(lines), combo_area);
    fx::mult_punch(f.buffer_mut(), t, now, (mult_x, combo_area.y + 3));

    let mut rank_lines = vec![
        Line::from(""),
        Line::from(RANKS[t.rank].name).fg(color).bold(),
    ];
    let bite = t.bites.last().filter(|b| since(Some(b.at), now) < CALLOUT);
    if let Some(b) = bite.filter(|b| !b.hot) {
        let fresh = since(Some(b.at), now) < Duration::from_millis(150);
        let c = if fresh { Color::White } else { rank_color(2) };
        rank_lines.push(Line::from("MILD · PHEW").fg(c).bold());
    } else if let Some((n, at)) = t.broke
        && since(Some(at), now) < CALLOUT
    {
        rank_lines.push(Line::from(format!("COMBO BROKEN ×{n}")).red().bold());
    } else if since(t.rank_up, now) < CALLOUT && !RANKS[t.rank].callout.is_empty() {
        let p = since(t.rank_up, now).as_secs_f64() / CALLOUT.as_secs_f64();
        let c = if p < 0.15 {
            Color::White
        } else if p < 0.75 {
            color
        } else {
            Color::DarkGray
        };
        rank_lines.push(Line::from(RANKS[t.rank].callout).fg(c).bold());
    }
    let rank_lines: Vec<Line> = rank_lines.into_iter().map(Line::right_aligned).collect();
    f.render_widget(Paragraph::new(rank_lines), rank_area);

    // text box: shakes on a broken combo, border heats up with rank
    let shake = !calm() && t.broke.is_some_and(|(_, at)| since(Some(at), now) < SHAKE);
    // centered with 1 spare column each side; shaking jitters it left/right
    let dx = match shake {
        true if ((clock * 30.0) as u64).is_multiple_of(2) => 0,
        true => 2,
        false => 1,
    };
    let lo = text.x; // the cop waits out here, clear of the box's border
    let text = Rect {
        x: text.x + dx,
        width: text.width.saturating_sub(2),
        ..text
    };
    let flashing = !calm() && since(t.last_error, now) < FLASH;
    let border = match t.rank {
        _ if flashing => Color::Red,
        4.. if calm() => color,
        0..=2 => Color::DarkGray,
        3 => color,
        4 => lerp((90, 50, 0), RANK_RGB[4], 0.5 + 0.5 * (clock * 5.0).sin()),
        5 => hsv(20.0 + 15.0 * (clock * 8.0).sin(), 1.0, 1.0),
        _ => hsv(clock * 120.0, 0.9, 1.0),
    };
    let block = Block::bordered()
        .border_type(if t.rank >= 5 {
            BorderType::Thick
        } else {
            BorderType::Rounded
        })
        .border_style(border)
        .padding(Padding::horizontal(1));
    let text_inner = block.inner(text);
    f.render_widget(block, text);
    let (wrapped, first) = visible(t, text_inner.width as usize);
    let shown = &wrapped[first..(first + text_inner.height as usize).min(wrapped.len())];
    // pursuit: what the cop drove over is tire tracks
    let tracks = fx::tracks();
    let lines: Vec<Line> = shown
        .iter()
        .map(|&(start, end)| {
            (start..end)
                .map(|i| match (eaten(t, i), t.target[i]) {
                    (true, ' ') => Span::raw(" "),
                    (true, _) => Span::styled(fx::TRACKS, tracks),
                    _ => Span::styled(t.target[i].to_string(), letter(t, i, clock)),
                })
                .collect()
        })
        .collect();
    f.render_widget(Paragraph::new(lines), text_inner);
    let geom = TextGeom {
        inner: text_inner,
        shown: shown.to_vec(),
        caret: (text_inner.x, text_inner.y),
        cop: None,
    };
    let geom = TextGeom {
        caret: geom.pos(t.typed.len()).unwrap_or(geom.caret),
        cop: geom.cop_at(t, lo),
        ..geom
    };
    let pos = |i| geom.pos(i);
    let buf = f.buffer_mut();
    fx::key_pop(buf, t, now, pos);
    fx::word_sweep(buf, t, now, pos);
    fx::miss_shake(buf, t, now, pos, geom.caret);
    fx::crumbs(buf, t, now, pos);
    fx::sparks(buf, t, now, pos);
    fx::points(buf, t, now, pos);
    fx::go_burst(buf, t, now, pos);

    f.render_widget(heat_bar(t.heat, t.rank, heat.width as usize), heat);

    let zen_on = t.mode == Mode::Zen && t.started.is_some();
    let hint = match (touch(), zen_on) {
        (true, true) => "✕ finish",
        (true, false) => "✕ menu",
        (false, true) => "esc finish · tab restart",
        (false, false) => "tab restart · esc menu",
    };
    // style the span, not the line: a styled Line recolors the flames across the whole row
    // padded so the flames keep a little distance from the text
    f.render_widget(
        Line::from(format!("  {hint}  ").dark_gray()).centered(),
        help,
    );
    (geom, combo_area)
}

/// Survival fuel: a draining gauge, red when nearly dry, plus the level.
/// `beat` (0..1) flashes it like a heartbeat when nearly dry.
fn fuel_gauge(t: &Test, beat: f64) -> Span<'static> {
    let k = (t.fuel / FUEL).clamp(0.0, 1.0);
    let c = match beat {
        0.0 => lerp((255, 30, 30), (255, 215, 0), k * 2.0),
        _ => lerp((255, 30, 30), WHITE, 0.7 * beat),
    };
    gauge(t.level(), k, c)
}

/// Pursuit: your lead on the cop, police blue while it's far back, siren red
/// as it closes in, plus the level. `beat` (0..1) flashes it with the siren.
fn lead_gauge(t: &Test, beat: f64) -> Span<'static> {
    let k = t.lead() / LEAD_MAX;
    let c = match beat {
        0.0 => lerp(SIREN_RED, POLICE, k * 2.0),
        _ => lerp(SIREN_RED, WHITE, 0.7 * beat),
    };
    gauge(t.level(), k, c)
}

/// THE HOT ONE: the strikes so far ("●●○"), hotter each one, and when the
/// next course is served; plates survived beside it.
fn strikes_gauge(t: &Test, now: Instant) -> Span<'static> {
    let n = (t.strikes as usize).min(STRIKES as usize);
    let dots = format!("{}{}", "●".repeat(n), "○".repeat(STRIKES as usize - n));
    let text = match t.course_in(now) {
        Some(secs) => format!("{dots}  course in {}s", secs.ceil() as u32),
        None => format!("{dots}  a plate's on the table"),
    };
    let text = match t.plates {
        0 => text,
        1 => format!("{text}  ·  1 plate down"),
        p => format!("{text}  ·  {p} plates down"),
    };
    let c = match n {
        0 => Color::Yellow,
        1 => rank_color(3),
        2 => rank_color(4),
        _ => rank_color(5),
    };
    Span::styled(text, c)
}

/// "LV3 ▰▰▰▰▱▱▱▱▱▱": the level and ten ticks filled to `k` (0..1).
fn gauge(level: u32, k: f64, c: Color) -> Span<'static> {
    let ticks = (k * 10.0).ceil() as usize;
    let s = format!("LV{level} {}{}", "▰".repeat(ticks), "▱".repeat(10 - ticks));
    Span::styled(s, c)
}

/// Heat gauge colored by the rank each segment belongs to, plus a Scoville readout.
pub fn heat_bar(heat: f64, rank: usize, width: usize) -> Line<'static> {
    let w = width.saturating_sub(16).max(1);
    let mut spans: Vec<Span> = (0..w)
        .map(|j| {
            let v = (j as f64 + 0.5) / w as f64 * 100.0;
            if v <= heat {
                let r = RANKS.iter().rposition(|r| v >= r.at).unwrap_or(0);
                Span::styled("█", rank_color(r))
            } else {
                "░".dark_gray()
            }
        })
        .collect();
    let shu = scoville(heat).to_string();
    let shu = shu
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join(",");
    spans.push(format!(" {shu:>11}").fg(rank_color(rank)).bold());
    spans.push(" SHU".dark_gray());
    Line::from(spans)
}

/// Word-wrapped target and the first visible line (keeps the caret on line two).
fn visible(t: &Test, width: usize) -> (Vec<(usize, usize)>, usize) {
    let lines = wrap(&t.target, width.max(1));
    let caret = t.typed.len();
    let caret_line = lines
        .iter()
        .position(|&(_, end)| caret < end)
        .unwrap_or(lines.len() - 1);
    (lines, caret_line.saturating_sub(1))
}

/// Pursuit: the cop has driven over target index `i`.
fn eaten(t: &Test, i: usize) -> bool {
    matches!(t.mode, Mode::Pursuit { .. }) && (i as f64) < t.cop.floor()
}

fn letter(t: &Test, i: usize, clock: f64) -> Style {
    let c = t.target[i];
    let caret = t.typed.len();
    match t.typed.get(i) {
        Some(&typed) if typed != c => {
            return if c == ' ' {
                Style::new().on_red()
            } else {
                Style::new().red().underlined()
            };
        }
        Some(_) => {}
        // pursuit: a wrong key leaves the caret red until you hit the right one
        None if i == caret && t.stuck => return Style::new().white().bold().bg(MISS_RED),
        None if i == caret => return Style::new().black().bg(rank_color(t.rank.max(3))),
        // at reaper the word you're about to type glows
        None if t.rank == 6 && !t.target[caret..i].contains(&' ') => return Style::new().gray(),
        None => return Style::new().dark_gray(),
    }
    let age = caret - 1 - i; // 0 = just typed
    // fever: the trail behind the caret smoulders down to ash (reaper keeps its rainbow)
    if t.fever() && t.rank < 6 {
        let noise = hash(i as u64, 1, (clock * 10.0) as u64) as f64 / 255.0;
        return match age {
            0..=1 => Style::new().fg(Color::Rgb(255, 244, 192)).bold(),
            2..=15 => Style::new().fg(fx::fire(1.0 - age as f64 / 16.0 + (noise - 0.5) * 0.25)),
            _ if noise < 0.04 => Style::new().fg(fx::fire(0.5)),
            _ => Style::new().fg(Color::Rgb(95, 85, 80)),
        };
    }
    let rgb = RANK_RGB[t.rank];
    let bold = |s: Style, on: bool| if on { s.bold() } else { s };
    // calm: the top ranks keep the steady trail instead of waves and rainbows
    let tier = if calm() { t.rank.min(4) } else { t.rank };
    match tier {
        0 | 1 => Style::new().white(),
        2 => bold(
            Style::new().fg(if age == 0 {
                Color::White
            } else {
                rank_color(2)
            }),
            age == 0,
        ),
        3 | 4 if age < 8 => bold(Style::new().fg(lerp(WHITE, rgb, age as f64 / 8.0)), age < 2),
        3 | 4 => Style::new().fg(lerp((0, 0, 0), rgb, 0.7)),
        5 if age < 3 => Style::new().fg(Color::Rgb(255, 244, 192)).bold(),
        5 => Style::new().fg(hsv(
            25.0 + 25.0 * (i as f64 * 0.35 - clock * 6.0).sin(),
            1.0,
            1.0,
        )),
        _ if hash(i as u64, 0, (clock * 12.0) as u64) < 8 => Style::new().white().bold(),
        _ => bold(
            Style::new().fg(hsv(i as f64 * 12.0 - clock * 240.0, 1.0, 1.0)),
            age < 3,
        ),
    }
}

/// Line ranges `[start, end)` breaking after spaces, each at most `width` chars.
fn wrap(text: &[char], width: usize) -> Vec<(usize, usize)> {
    let mut lines = Vec::new();
    let (mut start, mut last_space) = (0, None);
    for (i, &c) in text.iter().enumerate() {
        if i - start >= width {
            let brk = last_space.filter(|&s| s >= start).map_or(i, |s| s + 1);
            lines.push((start, brk));
            start = brk;
        }
        if c == ' ' {
            last_space = Some(i);
        }
    }
    lines.push((start, text.len()));
    lines
}

// results choreography, seconds after the test ends
const PEAK_AT: f64 = 0.6;
const CARDS_AT: f64 = 0.25;
const CHART_AT: f64 = 0.5;
const BEST_AT: f64 = 0.85;
/// The boards strip's width, and the narrowest column that leaves the chart
/// beside it enough room.
const STRIP: u16 = 36;
const STRIP_FROM: u16 = 76;

fn results(
    f: &mut Frame,
    area: Rect,
    t: &Test,
    outcome: Option<Outcome>,
    share: Option<&str>,
    strip: bool,
    now: Instant,
) -> Option<Rect> {
    let [_, top, _, cards, _, chart, _, details, _, help] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(14), // 3D pepper + score
        Constraint::Length(1),
        Constraint::Length(3), // cards
        Constraint::Length(1),
        Constraint::Length(10), // chart
        Constraint::Length(1),
        Constraint::Length(1), // details
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(area);
    // phones: no room for the 3D pepper beside the score, and shorter lines
    let narrow = area.width < 60;
    let [stage, info] = match narrow {
        true => [Rect::default(), top],
        false => Layout::horizontal([Constraint::Length(30), Constraint::Fill(1)]).areas(top),
    };
    // a busted chase says so on the SCORE row: short screens have none to spare
    let [_, label, _, score, _, peak, best_row, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1), // SCORE
        Constraint::Length(1),
        Constraint::Length(5), // digits
        Constraint::Length(1),
        Constraint::Length(1), // peak
        Constraint::Length(1), // personal best
        Constraint::Fill(1),
    ])
    .areas(info);
    let color = rank_color(t.peak_rank);
    // everything counts up and settles in over the first second, in order
    let secs = since(t.finished, now).as_secs_f64();
    let ease = |delay: f64, len: f64| 1.0 - (1.0 - ((secs - delay) / len).clamp(0.0, 1.0)).powi(3);

    // your peak pepper in 3D, spinning faster the hotter you got
    let pose = pepper::Pose {
        spin: secs * (0.6 + 0.25 * t.peak_rank as f64),
        tilt: 0.35 + 0.12 * (secs * 0.7).sin(),
    };
    pepper::draw(
        f.buffer_mut(),
        stage,
        &pepper::RANK_SHAPES[t.peak_rank],
        &pose,
    );

    let label_line = match (t.busted, t.bitten) {
        (Some(_), _) => busted_line(t, now, secs),
        (_, Some(_)) => bitten_line(secs),
        _ => Line::from("SCORE").dark_gray(),
    };
    f.render_widget(label_line.centered(), label);
    let shown = (t.score as f64 * ease(0.0, 0.8)).round() as u64;
    let digits: Vec<Line> = big(shown)
        .iter()
        .map(|r| Line::from(solid(r, color)))
        .collect();
    f.render_widget(Paragraph::new(digits).centered(), score);
    let mut peak_line = Line::from(vec![
        "peak ".dark_gray(),
        RANKS[t.peak_rank].name.fg(color).bold(),
    ]);
    if !narrow {
        let hot = format!(
            " · {:.1}s at {}+",
            t.hot_secs,
            RANKS[HOT].name.to_lowercase()
        );
        peak_line.push_span(hot.dark_gray());
    }
    if secs >= PEAK_AT {
        f.render_widget(peak_line.centered(), peak);
    }
    if let Some(o) = outcome.filter(|_| secs >= BEST_AT) {
        let best_line = if o.is_new() {
            // a new best lands white-hot, then shimmers through the heat colors
            let c = match secs - BEST_AT < 0.15 {
                true => Color::White,
                false if calm() => rank_color(5),
                false => hsv(20.0 + 40.0 * (secs * 4.0).sin(), 1.0, 1.0),
            };
            let mut spans = vec![
                format!("★ NEW BEST {}", best::show(t.mode, o.value))
                    .fg(c)
                    .bold(),
            ];
            if let Some(w) = o.was {
                spans.push(format!("  was {}", best::show(t.mode, w)).dark_gray());
            }
            Line::from(spans)
        } else {
            Line::from(format!("best {}", best::show(t.mode, o.was.unwrap_or(o.value))).dark_gray())
        };
        let mid = best_row.x + best_row.width / 2;
        f.render_widget(best_line.centered(), best_row);
        if o.is_new() {
            fx::confetti(f.buffer_mut(), (mid, best_row.y), secs - BEST_AT);
        }
    }

    let card_areas: [Rect; 4] = Layout::horizontal([Constraint::Fill(1); 4])
        .spacing(1)
        .areas(cards);
    let stats = [
        ("wpm", t.wpm(now), "", Color::Yellow),
        ("acc", t.accuracy(), "%", Color::Green),
        ("raw", t.raw_wpm(now), "", Color::Cyan),
        (
            if narrow { "combo" } else { "best combo" },
            t.best_combo as f64,
            "×",
            Color::Magenta,
        ),
    ];
    // cards drop in one after another, their numbers counting up
    for (j, ((label, value, unit, c), a)) in stats.into_iter().zip(card_areas).enumerate() {
        let delay = CARDS_AT + 0.1 * j as f64;
        if secs < delay {
            continue;
        }
        let k = ease(delay, 0.5);
        let n = format!("{:.0}", value * k);
        let value = match unit {
            "×" => format!("×{n}"),
            u => format!("{n}{u}"),
        };
        let fresh = secs - delay < 0.12; // lands with a white flash
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .title(label)
            .border_style(if fresh { Color::White } else { Color::DarkGray })
            .title_style(Color::DarkGray);
        let a = Rect {
            y: a.y
                .saturating_sub(((1.0 - ease(delay, 0.2)) * 2.0).round() as u16),
            ..a
        };
        f.render_widget(
            Paragraph::new(value.fg(c).bold()).centered().block(block),
            a,
        );
    }

    let (chart, board) = match strip && area.width >= STRIP_FROM {
        true => {
            let [chart, _, board] = Layout::horizontal([
                Constraint::Fill(1),
                Constraint::Length(1),
                Constraint::Length(STRIP),
            ])
            .areas(chart);
            (chart, Some(board))
        }
        false => (chart, None),
    };
    let took = t.elapsed(now).as_secs_f64();
    let max_x = took.max(2.0);
    // the line draws itself left to right
    let reach = 1.0 + (max_x - 1.0) * ease(CHART_AT, 0.8);
    let pts: Vec<(f64, f64)> = t
        .samples
        .iter()
        .enumerate()
        .map(|(i, w)| ((i as f64 + 1.0).min(took), *w))
        .filter(|&(x, _)| x <= reach)
        .collect();
    let max_y = (t.samples.iter().cloned().fold(0.0, f64::max) * 1.2).max(10.0);
    let dataset = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style(color)
        .data(&pts);
    let chart_widget = Chart::new(vec![dataset])
        .x_axis(
            Axis::default()
                .bounds([1.0, max_x])
                .labels(["1s".to_string(), format!("{max_x:.1}s")])
                .dark_gray(),
        )
        .y_axis(
            Axis::default()
                .bounds([0.0, max_y])
                .labels(["0".to_string(), format!("{max_y:.0}")])
                .dark_gray(),
        );
    f.render_widget(chart_widget, chart);

    let mode = match t.mode {
        m if m.leveled() && t.start_level() > 1 => {
            format!(
                "{} lv{} (from lv{})",
                menu::label(m),
                t.level(),
                t.start_level()
            )
        }
        m if m.leveled() => format!("{} lv{}", menu::label(m), t.level()),
        m => menu::label(m),
    };
    let line = match (narrow, t.mode) {
        (true, _) => format!("{mode} · {took:.1}s · {} slips", t.errors),
        // the hot one: wrong keys never land either, and the plates count
        (false, Mode::HotOne) => format!(
            "{mode} · {took:.1}s · {} correct · {} slips · {} plate{} survived",
            t.correct(),
            t.errors,
            t.plates,
            if t.plates == 1 { "" } else { "s" }
        ),
        // pursuit: wrong keys never land, they're all slips
        (false, Mode::Pursuit { .. }) => format!(
            "{mode} · {took:.1}s · {} correct · {} slips",
            t.correct(),
            t.errors
        ),
        (false, _) => format!(
            "{mode} · {:.1}s · {} correct · {} wrong · {} slips",
            took,
            t.correct(),
            t.typed.len() - t.correct(),
            t.errors
        ),
    };
    f.render_widget(Line::from(line).dark_gray().centered(), details);
    let again = match touch() {
        true => "tap to play again · ✕ menu",
        false => "tab/enter again · esc menu",
    };
    let help_line = match share {
        Some(share) => format!("  {share} · {again}  "),
        None => format!("  {again}  "),
    };
    f.render_widget(Line::from(help_line).dark_gray().centered(), help);
    board.filter(|_| secs >= CHART_AT)
}

/// "▄▄▄  BUSTED! · SCORE  ▄▄▄" between the cop's lights: BUSTED! lands
/// white-hot, then takes the color of whichever light is lit (calm: both
/// lit, steady red).
fn busted_line(t: &Test, now: Instant, secs: f64) -> Line<'static> {
    let red_on = fx::siren_red(t, now);
    let light = |c: Rgb, on: bool| {
        let c = match on || calm() {
            true => rgb(c),
            false => lerp(fx::page_rgb(), c, 0.3),
        };
        // on paper: the page behind it, so the dim light isn't repainted dark
        let style = Style::new().fg(c);
        let style = match crate::theme::is_light() {
            true => style.bg(crate::theme::page()),
            false => style,
        };
        Span::styled("▄▄▄", style)
    };
    let word = match secs < 0.15 {
        true => WHITE,
        false if red_on || calm() => SIREN_RED,
        false => POLICE,
    };
    Line::from(vec![
        light(SIREN_RED, red_on),
        Span::styled("  BUSTED!", Style::new().fg(rgb(word)).bold()),
        " · SCORE  ".dark_gray(),
        light(POLICE, !red_on),
    ])
}

/// "◗  THE HOT ONE · SCORE  ◗": it lands white-hot, then throbs hot pink.
fn bitten_line(secs: f64) -> Line<'static> {
    let (r, g, b) = menu::HOT_PINK;
    let pink = Color::Rgb(r, g, b);
    let word = match secs < 0.15 {
        true => Color::White,
        false if calm() => pink,
        false => lerp(
            (255, 255, 255),
            menu::HOT_PINK,
            0.6 + 0.4 * (secs * 6.0).sin(),
        ),
    };
    Line::from(vec![
        Span::styled("◗", Style::new().fg(pink)),
        Span::styled("  THE HOT ONE", Style::new().fg(word).bold()),
        " · SCORE  ".dark_gray(),
        Span::styled("◗", Style::new().fg(pink)),
    ])
}

/// 5-row block glyphs: digits plus the letters of the logo.
pub fn glyph(c: char) -> Option<[&'static str; 5]> {
    Some(match c {
        '0'..='9' => DIGITS[c as usize - '0' as usize],
        'T' => ["███", " █ ", " █ ", " █ ", " █ "],
        'Y' => ["█ █", "█ █", "▀█▀", " █ ", " █ "],
        'P' => ["██▄", "█ █", "██▀", "█  ", "█  "],
        'E' => ["███", "█  ", "██ ", "█  ", "███"],
        'R' => ["██▄", "█ █", "██▀", "█ █", "█ █"],
        _ => return None,
    })
}

/// One row of a block glyph as cells. Full blocks also paint their
/// background: fonts rarely fill the whole cell with `█`, which leaves seams
/// between stacked rows (very visible on the web grid).
pub fn solid(row: &str, color: Color) -> Vec<Span<'static>> {
    row.chars()
        .map(|c| match c {
            '█' => Span::styled("█", Style::new().fg(color).bg(color)),
            c => Span::styled(c.to_string(), Style::new().fg(color).bold()),
        })
        .collect()
}

/// A number in 5-row block digits.
pub fn big(n: impl ToString) -> Vec<String> {
    let s = n.to_string();
    (0..5)
        .map(|row| {
            let parts: Vec<&str> = s.chars().filter_map(glyph).map(|g| g[row]).collect();
            parts.join(" ")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn wraps_after_spaces() {
        let text: Vec<char> = "aa bb cc dddddddd".chars().collect();
        assert_eq!(wrap(&text, 6), vec![(0, 6), (6, 9), (9, 15), (15, 17)]);
    }

    /// THE HOT ONE's plate draws at phone and desktop widths, every pepper
    /// numbered, the hovered one marked; the gauge counts the strikes.
    #[test]
    fn the_plate_fits_any_width_and_numbers_its_peppers() {
        let now = Instant::now();
        for w in [36u16, 60, 100] {
            for peppers in [1u8, 5, 6] {
                let mut t = Test::new(Mode::HotOne, &["hot", "one"].repeat(40));
                t.started = Some(now);
                t.plate = Some(crate::engine::Plate {
                    served: now,
                    why: Served::Course,
                    peppers,
                    hot: 0,
                });
                let mut term = Terminal::new(TestBackend::new(w, 30)).unwrap();
                term.draw(|f| {
                    draw(f, &t, None, None, false, Some(peppers as usize - 1));
                })
                .unwrap();
                let screen = term.backend().to_string();
                if std::env::var_os("TYPEPPER_SHOW").is_some() {
                    println!("{screen}");
                }
                for i in 1..=peppers {
                    assert!(
                        screen.contains(&i.to_string()),
                        "{w} wide, {peppers} peppers: {i}"
                    );
                }
                assert!(screen.contains("PICK A PEPPER"), "{w} wide");
                t.pick(0, now);
                term.draw(|f| {
                    draw(f, &t, None, None, false, Some(0));
                })
                .unwrap();
                let screen = term.backend().to_string();
                assert!(
                    screen.contains("CHEWING") && screen.contains("[1]"),
                    "{w} wide"
                );
            }
        }
        let mut t = Test::new(Mode::HotOne, &["hot"; 50]);
        t.started = Some(now);
        t.strikes = 2;
        assert!(strikes_gauge(&t, now).content.starts_with("●●○"));
        t.plates = 2;
        assert!(strikes_gauge(&t, now).content.ends_with("2 plates down"));
    }

    /// Pursuit, phone and desktop widths: wherever the cop is, its car rides
    /// a row of the text box, never past the text on the right, never off the
    /// panel on the left (it waits by the box's edge), inside the text once
    /// it's driving on it, and never over the caret or what's left to type.
    #[test]
    fn the_cop_keeps_to_the_text_box_behind_the_caret() {
        let now = Instant::now();
        let words = ["hot", "pursuit", "on", "the", "open", "road"].repeat(8);
        for w in [40, 80] {
            for caret in [0, 3, 12, 25, 40, 50, 61] {
                for lead in [LEAD_MAX, 20.0, 6.5, 1.0, 0.2] {
                    let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &words);
                    t.typed = t.target[..caret].to_vec();
                    t.started = Some(now);
                    t.cop = caret as f64 - lead;
                    let mut geom = None;
                    let mut term = Terminal::new(TestBackend::new(w, 19)).unwrap();
                    term.draw(|f| {
                        let (panel, help) = (Rect::new(0, 0, w, 18), Rect::new(0, 18, w, 1));
                        geom = Some(typing(f, panel, help, &t, now).0);
                    })
                    .unwrap();
                    let g = geom.unwrap();
                    let case = format!("{w} wide, caret {caret}, lead {lead}");
                    let hood = g.cop.expect(&case);
                    // as `draw` paints it: the box's border and padding hide it
                    let x0 = g.inner.x as i32;
                    let cells: Vec<i32> = (hood.0 - 2..=hood.0)
                        .filter(|x| !(x0 - 2..x0).contains(x))
                        .collect();
                    assert!(!cells.is_empty(), "{case}: the cop is out of sight");
                    let on_text = t.cop.floor() as usize > g.shown[0].0;
                    let ahead: Vec<_> = (caret..g.shown.last().unwrap().1)
                        .filter_map(|i| g.pos(i))
                        .collect();
                    for &x in &cells {
                        let at = (x as u16, hood.1);
                        assert!(g.inner.y <= at.1 && at.1 < g.inner.bottom(), "{case}");
                        assert!(0 < x && x < g.inner.right() as i32, "{case}: x {x}");
                        assert!(!on_text || x >= x0, "{case}: x {x} left of the text");
                        assert!(at != g.caret && !ahead.contains(&at), "{case}: {at:?}");
                    }
                }
            }
        }
    }
}
