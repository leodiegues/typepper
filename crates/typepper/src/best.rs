//! Personal bests: one number per mode (wpm, or words survived), kept as
//! `label=value` lines (see `store`).

use std::collections::BTreeMap;

use crate::engine::{self, Mode};
use crate::menu::label;
use crate::store;

/// What a finished test scored against your best.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outcome {
    pub value: f64,
    pub was: Option<f64>,
}

impl Outcome {
    pub fn is_new(&self) -> bool {
        self.was.is_none_or(|w| self.value > w)
    }
}

#[derive(Default)]
pub struct Bests(BTreeMap<String, f64>);

impl Bests {
    pub fn load() -> Self {
        Self::parse(&store::read("bests").unwrap_or_default())
    }

    fn parse(text: &str) -> Self {
        Bests(
            text.lines()
                .filter_map(|l| {
                    let (k, v) = l.rsplit_once('=')?;
                    Some((k.to_string(), v.trim().parse().ok()?))
                })
                .collect(),
        )
    }

    fn to_text(&self) -> String {
        self.0.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }

    /// The best for `m` (from its starting level, in leveled modes).
    pub fn get(&self, m: Mode) -> Option<f64> {
        self.0.get(&key(m)).copied()
    }

    /// Score a finished round; a new best is kept (and saved). Zen has none.
    pub fn record(&mut self, m: Mode, round: &engine::Outcome) -> Option<Outcome> {
        let value = round.score?;
        let out = Outcome {
            value,
            was: self.get(m),
        };
        if out.is_new() {
            self.0.insert(key(m), value);
            // ponytail: a failed save only loses this best; no error UI for it
            let _ = store::write("bests", &self.to_text());
        }
        Some(out)
    }

    /// Takes in bests from elsewhere (`<mode key> <value>` lines, as the
    /// server sends a logged-in player's), keeping the higher of each.
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    pub fn merge(&mut self, text: &str) {
        if self.merged(text) {
            // ponytail: a failed save only loses them until the next load
            let _ = store::write("bests", &self.to_text());
        }
    }

    /// `merge`, without saving: whether anything changed.
    fn merged(&mut self, text: &str) -> bool {
        let mut changed = false;
        for line in text.lines() {
            let Some((k, v)) = line.split_once(' ') else {
                continue;
            };
            let (Some(m), Ok(v)) = (Mode::parse(k), v.trim().parse::<f64>()) else {
                continue;
            };
            if v.is_finite() && self.get(m).is_none_or(|was| v > was) {
                self.0.insert(key(m), v);
                changed = true;
            }
        }
        changed
    }
}

/// Bests are kept per mode, and per starting level when it isn't 1 (so a
/// run from level 10 isn't compared with one from level 1, and the keys
/// saved before levels existed still mean level 1).
fn key(m: Mode) -> String {
    match m.start_level() {
        Some(start @ 2..) => format!("{} from lv{start}", label(m)),
        _ => label(m),
    }
}

/// "87 wpm", "42 words".
pub fn show(m: Mode, v: f64) -> String {
    match m {
        m if m.words_scored() => format!("{v:.0} words"),
        _ => format!("{v:.0} wpm"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SURVIVAL_4: Mode = Mode::Survival { start_level: 4 };

    #[test]
    fn parses_what_it_writes_and_skips_junk() {
        let b = Bests::parse("BLITZ 30s=87\njunk\nSURVIVAL=nope\nSPRINT 10w=102.5\n");
        assert_eq!(b.get(Mode::Time(30)), Some(87.0));
        assert_eq!(b.get(Mode::Words(10)), Some(102.5));
        assert_eq!(b.get(Mode::Survival { start_level: 1 }), None);
        assert_eq!(key(SURVIVAL_4), "SURVIVAL from lv4");
        assert_eq!(Bests::parse(&b.to_text()).0, b.0);
    }

    #[test]
    fn pursuit_keeps_words_per_start_level() {
        let from = |start_level| Mode::Pursuit { start_level };
        assert_eq!(key(from(1)), "HOT PURSUIT");
        assert_eq!(key(from(7)), "HOT PURSUIT from lv7");
        let b = Bests::parse("HOT PURSUIT from lv7=31\n");
        assert_eq!(b.get(from(7)), Some(31.0));
        assert_eq!(b.get(from(1)), None);
        assert_eq!(show(from(1), 3.0), "3 words");
    }

    #[test]
    fn merging_keeps_the_higher_of_each() {
        let mut b = Bests::parse("BLITZ 30s=87\nSPRINT 10w=102.5\n");
        assert!(b.merged("time:30 90\nwords:10 80\npursuit:7 31\nnope 5\nzen x\n"));
        assert_eq!(b.get(Mode::Time(30)), Some(90.0));
        assert_eq!(b.get(Mode::Words(10)), Some(102.5));
        assert_eq!(b.get(Mode::Pursuit { start_level: 7 }), Some(31.0));
        assert!(!b.merged("time:30 90\nerr signed-out log in again\n"));
    }

    #[test]
    fn only_a_higher_number_is_new() {
        let o = |value, was| Outcome { value, was };
        assert!(o(50.0, None).is_new());
        assert!(o(51.0, Some(50.0)).is_new());
        assert!(!o(50.0, Some(50.0)).is_new());
    }
}
