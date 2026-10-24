//! Player settings: theme, effects and (on the web) font, saved via `store`.
//!
//! Calm effects follow one rule: nothing screen-wide moves or flickers,
//! nothing shakes, and no color pulses faster than about once a second.
//! Small local feedback (sparks, key pops, word sweeps) stays.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::{store, theme};

// ponytail: one global switch like `theme`; pass Settings through draw if more knobs land
static CALM: AtomicBool = AtomicBool::new(false);

pub fn calm() -> bool {
    CALM.load(Ordering::Relaxed)
}

/// Played by touch (a phone in the browser): hints talk about taps, not keys.
static OFFLINE: AtomicBool = AtomicBool::new(false);

/// settings → online: off (no sharing, no boards, no hints about them).
pub fn offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

static TOUCH: AtomicBool = AtomicBool::new(false);

pub fn touch() -> bool {
    TOUCH.load(Ordering::Relaxed)
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn set_touch(on: bool) {
    TOUCH.store(on, Ordering::Relaxed);
}

/// Web font choices, the default first; "system" is the built-in stack.
/// Glyphs a font lacks (blocks, braille…) fall back to the system stack.
pub const FONTS: [&str; 5] = [
    "Departure Mono",
    "JetBrains Mono",
    "Fira Code",
    "IBM Plex Mono",
    "system",
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Row {
    Theme,
    Effects,
    Font,
    Online,
}

/// The dialog's rows; fonts only exist in the browser.
pub const ROWS: &[Row] = if cfg!(target_arch = "wasm32") {
    &[Row::Theme, Row::Effects, Row::Font, Row::Online]
} else {
    &[Row::Theme, Row::Effects, Row::Online]
};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Settings {
    /// `None` follows the terminal (or browser) theme.
    pub light: Option<bool>,
    pub calm: bool,
    /// Index into `FONTS`.
    pub font: usize,
    /// Off: no sharing, and no hint about it (the game never needs typepper.com).
    pub offline: bool,
}

const THEMES: [Option<bool>; 3] = [None, Some(false), Some(true)];

impl Settings {
    pub fn load() -> Self {
        Self::parse(&store::read("settings").unwrap_or_default())
    }

    fn parse(text: &str) -> Self {
        let mut s = Settings::default();
        for (k, v) in text.lines().filter_map(|l| l.split_once('=')) {
            match (k, v) {
                ("theme", "dark") => s.light = Some(false),
                ("theme", "light") => s.light = Some(true),
                ("effects", "calm") => s.calm = true,
                ("font", f) => s.font = FONTS.iter().position(|&x| x == f).unwrap_or(0),
                ("online", "off") => s.offline = true,
                _ => {}
            }
        }
        s
    }

    fn to_text(self) -> String {
        format!(
            "theme={}\neffects={}\nfont={}\nonline={}\n",
            self.value(Row::Theme),
            self.value(Row::Effects),
            FONTS[self.font],
            self.value(Row::Online)
        )
    }

    pub fn save(&self) {
        // ponytail: a failed save only forgets the setting; no error UI for it
        let _ = store::write("settings", &self.to_text());
    }

    /// Make the settings live; `detected` is the theme to follow on auto.
    pub fn apply(&self, detected: bool) {
        theme::set_light(self.light.unwrap_or(detected));
        CALM.store(self.calm, Ordering::Relaxed);
        OFFLINE.store(self.offline, Ordering::Relaxed);
    }

    pub fn value(&self, row: Row) -> &'static str {
        match row {
            Row::Theme => match self.light {
                None => "auto",
                Some(false) => "dark",
                Some(true) => "light",
            },
            Row::Effects if self.calm => "calm",
            Row::Effects => "full",
            Row::Font => FONTS[self.font],
            Row::Online if self.offline => "off",
            Row::Online => "on",
        }
    }

    pub fn change(&mut self, row: Row, dir: i64) {
        let step = |i: usize, n: usize| (i as i64 + dir).rem_euclid(n as i64) as usize;
        match row {
            Row::Theme => {
                let i = THEMES.iter().position(|&t| t == self.light).unwrap_or(0);
                self.light = THEMES[step(i, THEMES.len())];
            }
            Row::Effects => self.calm = !self.calm,
            Row::Font => self.font = step(self.font, FONTS.len()),
            Row::Online => self.offline = !self.offline,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_cycles() {
        let mut s = Settings::default();
        assert_eq!(s.value(Row::Theme), "auto");
        s.change(Row::Theme, 1);
        s.change(Row::Theme, 1);
        s.change(Row::Effects, 1);
        s.change(Row::Font, -1);
        s.change(Row::Online, 1);
        assert_eq!(
            (
                s.value(Row::Theme),
                s.value(Row::Effects),
                s.value(Row::Font),
                s.value(Row::Online)
            ),
            ("light", "calm", "system", "off")
        );
        assert_eq!(Settings::parse(&s.to_text()), s);
        assert_eq!(
            Settings::parse("junk\nfont=Comic Sans"),
            Settings::default()
        );
    }
}
