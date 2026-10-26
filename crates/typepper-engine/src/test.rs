//! One typing test: heat, combo, score, levels, fuel, the cop. Pure state:
//! every time-dependent call takes `now`, and the only randomness is `rng`.

use std::time::Duration;

use web_time::Instant;

use crate::Mode;
use crate::rng::Rng;

pub struct Rank {
    pub name: &'static str,
    /// Real-world Scoville heat units at the bottom of this rank.
    pub shu: u32,
    /// Heat needed to reach this rank.
    pub at: f64,
    pub callout: &'static str,
}

pub const RANKS: [Rank; 7] = [
    Rank {
        name: "BELL PEPPER",
        shu: 0,
        at: 0.0,
        callout: "",
    },
    Rank {
        name: "POBLANO",
        shu: 1_000,
        at: 8.0,
        callout: "",
    },
    Rank {
        name: "JALAPEÑO",
        shu: 2_500,
        at: 16.0,
        callout: "warming up",
    },
    Rank {
        name: "SERRANO",
        shu: 10_000,
        at: 30.0,
        callout: "NICE!",
    },
    Rank {
        name: "HABANERO",
        shu: 100_000,
        at: 48.0,
        callout: "SPICY!",
    },
    Rank {
        name: "GHOST PEPPER",
        shu: 855_000,
        at: 68.0,
        callout: "ON FIRE!!",
    },
    Rank {
        name: "CAROLINA REAPER",
        shu: 1_641_000,
        at: 88.0,
        callout: "REAPER MODE!!!",
    },
];

/// Heat → Scoville units, linear between rank anchors, 2.2M at max heat.
pub fn scoville(heat: f64) -> u32 {
    let r = RANKS.iter().rposition(|r| heat >= r.at).unwrap_or(0);
    let (next_at, next_shu) = RANKS
        .get(r + 1)
        .map_or((100.0, 2_200_000), |n| (n.at, n.shu));
    let k = ((heat - RANKS[r].at) / (next_at - RANKS[r].at)).clamp(0.0, 1.0);
    RANKS[r].shu + ((next_shu - RANKS[r].shu) as f64 * k) as u32
}

const WORDS: &str = include_str!("words.txt");

/// `n` random common English words, drawn from `rng`.
pub fn random_words(rng: &mut Rng, n: usize) -> Vec<&'static str> {
    static LIST: std::sync::LazyLock<Vec<&str>> =
        std::sync::LazyLock::new(|| WORDS.split_whitespace().collect());
    (0..n).map(|_| LIST[rng.below(LIST.len())]).collect()
}

/// Rank from which time counts as "hot" on the results screen.
pub const HOT: usize = 4;

const RANK_HYSTERESIS: f64 = 4.0;

/// Steady state heat = (wpm² / 600) · gain/1.2 / decay, so
/// 40wpm ≈ poblano, 60 ≈ jalapeño, 90 ≈ serrano, 110 ≈ habanero, 135+ ≈ reaper.
const HEAT_GAIN: f64 = 1.2;
/// Per second, exponential.
const HEAT_DECAY: f64 = 0.33;
/// Heat kept after a mistake.
const MISS_KEEP: f64 = 0.4;
/// Multiplier steps above ×1, one per 20 combo.
const MULT_STEPS: u32 = 3;

// ponytail: survival tuned on paper (lv5 ≈ 70wpm, lv10 ≈ 120wpm to break even); playtest
/// Survival: starting (and maximum) fuel in seconds.
pub const FUEL: f64 = 10.0;
/// Survival: fuel each correct word adds.
const REFILL: f64 = 1.5;
/// Levels a leveled mode (survival, lava, pursuit) can start from, Tetris-style.
pub const LEVELS: [u32; 4] = [1, 4, 7, 10];
/// Survival: words per level; each level drains this much faster.
const LEVEL_WORDS: u32 = 10;
const DRAIN_STEP: f64 = 1.12;

// ponytail: pursuit tuned on paper (cop from 25wpm at lv1 to ~69wpm at lv10); playtest
/// Pursuit: the furthest the cop falls behind the caret, in chars.
pub const LEAD_MAX: f64 = 30.0;
/// Pursuit: the cop's starting speed at level 1, ×1.12 per start level above it.
const COP_WPM: f64 = 25.0;
const COP_START_STEP: f64 = 1.12;
/// Pursuit: the cop speeds up once per `LEVEL_WORDS` words and per this many seconds.
const COP_STEP_SECS: f64 = 20.0;
// ponytail: a ceiling no human reaches, only there to keep the cop finite
// after a huge frame gap (a suspended laptop), where the ramp overflows to inf
/// Pursuit: the cop's top speed.
const COP_WPM_MAX: f64 = 1000.0;

// ponytail: the hot one tuned on paper (a clean typer sees 5 courses, 2.5 min); playtest
/// THE HOT ONE: wrong keys in a row that bring a plate round.
pub const STRIKES: u32 = 3;
/// THE HOT ONE: peppers on the first plate; every plate survived takes one away.
pub const PLATE: u8 = 5;
/// THE HOT ONE: a course (a plate for everyone) this many seconds apart.
pub const COURSE_SECS: f64 = 30.0;
/// THE HOT ONE: a plate left on the table this long picks for you.
pub const PICK_SECS: f64 = 8.0;
/// THE HOT ONE: this long without a key (once you've typed) is a strike.
pub const STALL_SECS: f64 = 3.0;
/// THE HOT ONE: chewing a pepper takes this long, and nothing types meanwhile;
/// only then do you find out which one it was.
pub const BITE_SECS: f64 = 1.6;
/// THE HOT ONE: one more pepper on the plate while you're hot (rank `HOT`+).
const HOT_BONUS: u8 = 1;

/// THE HOT ONE: why a plate came round.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Served {
    /// `STRIKES` wrong keys (or stalls) in a row.
    Strikes,
    /// The clock: every `COURSE_SECS`.
    Course,
}

/// THE HOT ONE: a plate of peppers on the table, waiting for a pick.
#[derive(Clone, Copy, Debug)]
pub struct Plate {
    pub served: Instant,
    pub why: Served,
    /// How many are on it; picks are `0..peppers`.
    pub peppers: u8,
    /// Which one is the hot one. Drawn when served, so a replay serves the same.
    pub hot: u8,
}

/// THE HOT ONE: a pepper bitten off a plate.
#[derive(Clone, Copy, Debug)]
pub struct Bite {
    pub at: Instant,
    pub pepper: u8,
    pub peppers: u8,
    /// It was the hot one: the round ended here.
    pub hot: bool,
}

/// A word finished with a correct space (drives the word sweep and +points).
pub struct WordDone {
    pub at: Instant,
    /// Target range `[start, end)`, without the space.
    pub start: usize,
    pub end: usize,
    /// No misses while typing it.
    pub clean: bool,
    /// Score earned on it, space included.
    pub points: u64,
}

/// One typing test. Pure state: every time-dependent call takes `now`.
#[derive(Default)]
pub struct Test {
    pub mode: Mode,
    pub target: Vec<char>,
    pub typed: Vec<char>,
    pub started: Option<Instant>,
    pub finished: Option<Instant>,
    pub keystrokes: u32,
    pub errors: u32,
    pub combo: u32,
    pub best_combo: u32,
    pub last_key: Option<Instant>,
    pub last_error: Option<Instant>,
    /// Combo that was broken (if worth mourning) and when.
    pub broke: Option<(u32, Instant)>,
    /// Cumulative wpm, one sample per elapsed second (plus a final one).
    pub samples: Vec<f64>,
    pub heat: f64,
    pub rank: usize,
    pub peak_rank: usize,
    pub rank_up: Option<Instant>,
    pub score: u64,
    /// Seconds spent at rank `HOT` or above.
    pub hot_secs: f64,
    /// Launch times of flying jalapeños (hot rank-ups and every 50 combo).
    pub jalapenos: Vec<Instant>,
    /// Correct keystrokes: when, and which target index (for caret sparks).
    pub hits: Vec<(Instant, usize)>,
    /// Last combo milestone (every 50) and when.
    pub milestone: Option<(u32, Instant)>,
    /// Words finished with a correct space (each counted once).
    pub words: u32,
    /// Survival seconds left.
    pub fuel: f64,
    /// Pursuit: the cop's nose, as a target index; chars before it are eaten.
    /// Negative until it reaches the start of the text.
    pub cop: f64,
    /// Pursuit: when the cop caught you.
    pub busted: Option<Instant>,
    /// Pursuit (and the hot one): the last key was wrong, so the caret waits
    /// for the right one.
    pub stuck: bool,
    /// THE HOT ONE: wrong keys (or stalls) in a row; a right key clears them.
    pub strikes: u32,
    /// THE HOT ONE: when the last strike landed.
    pub struck: Option<Instant>,
    /// THE HOT ONE: the plate on the table, while there is one.
    pub plate: Option<Plate>,
    /// THE HOT ONE: plates survived so far.
    pub plates: u32,
    /// THE HOT ONE: every bite so far (the last one may be the hot one).
    pub bites: Vec<Bite>,
    /// THE HOT ONE: still chewing the last bite (since when).
    pub chewing: Option<Instant>,
    /// THE HOT ONE: when the hot one was bitten.
    pub bitten: Option<Instant>,
    /// THE HOT ONE: the round second the next course is served at.
    next_course: f64,
    /// THE HOT ONE: when the stall clock last reset (none before the first key).
    stall_from: Option<Instant>,
    pub words_done: Vec<WordDone>,
    /// Backspaced chars: when, where, and what was typed there.
    pub erased: Vec<(Instant, usize, char)>,
    /// When the multiplier last stepped up.
    pub mult_up: Option<Instant>,
    /// Score the last keystroke earned (0 for a miss).
    pub last_points: u64,
    /// Where the current word starts, and the score and errors when it did.
    word_from: (usize, u64, u32),
    /// Furthest the caret has been, so retyping a space doesn't refill twice.
    frontier: usize,
    last_tick: Option<Instant>,
    /// Where the text streamed in mid-run comes from (seeded by the round).
    pub rng: Rng,
}

impl Test {
    /// The level a leveled mode started from (1 for the rest).
    pub fn start_level(&self) -> u32 {
        self.mode.start_level().unwrap_or(1).max(1)
    }

    fn pursuit(&self) -> bool {
        matches!(self.mode, Mode::Pursuit { .. })
    }

    fn hot_one(&self) -> bool {
        self.mode == Mode::HotOne
    }

    /// Pursuit and the hot one: a wrong key never lands, the caret waits.
    fn strict(&self) -> bool {
        self.pursuit() || self.hot_one()
    }

    pub fn new(mode: Mode, words: &[&str]) -> Self {
        Self {
            mode,
            target: words.join(" ").chars().collect(),
            fuel: FUEL,
            // a full lead; the cop waits there for the first key
            cop: -LEAD_MAX,
            next_course: COURSE_SECS,
            ..Default::default()
        }
    }

    pub fn type_char(&mut self, c: char, now: Instant) {
        if self.finished.is_some() || self.typed.len() >= self.target.len() {
            return;
        }
        // a plate on the table, or a mouthful: nothing types
        if self.plate.is_some() || self.chewing.is_some() {
            return;
        }
        // pursuit, the hot one: a wrong key doesn't move the caret, you have
        // to hit the right one
        if self.strict() && self.target[self.typed.len()] != c {
            return self.miss(now);
        }
        if !self.key_down(now) {
            return;
        }
        if self.target[self.typed.len()] == c {
            self.stuck = false;
            if self.hot_one() {
                self.strikes = 0;
                self.stall_from = Some(now);
            }
            // instant wpm / 60: 12 / gap / 60
            let speed = self.last_key.map_or(1.0, |t| {
                (0.2 / now.saturating_duration_since(t).as_secs_f64().max(0.01)).clamp(0.5, 3.0)
            });
            self.heat = (self.heat + HEAT_GAIN * speed).min(100.0);
            let mult = self.multiplier();
            self.combo += 1;
            if self.multiplier() > mult {
                self.mult_up = Some(now);
            }
            self.best_combo = self.best_combo.max(self.combo);
            self.hits.push((now, self.typed.len()));
            if self.combo.is_multiple_of(50) {
                self.milestone = Some((self.combo, now));
                self.launch(now);
            }
            self.last_points = 10 * self.multiplier() as u64 * (1 + self.rank as u64);
            self.score += self.last_points;
            let i = self.typed.len();
            if c == ' ' && i >= self.frontier {
                self.words += 1;
                self.fuel = (self.fuel + REFILL).min(FUEL);
                let (start, score, errors) = self.word_from;
                self.words_done.push(WordDone {
                    at: now,
                    start,
                    end: i,
                    clean: self.errors == errors,
                    points: self.score - score,
                });
                self.word_from = (i + 1, self.score, self.errors);
            }
        } else {
            self.mistake(now);
        }
        self.key_up(now);
        self.typed.push(c);
        self.frontier = self.frontier.max(self.typed.len());
        if self.pursuit() {
            // the cop never falls more than LEAD_MAX behind
            self.cop = self.cop.max(self.typed.len() as f64 - LEAD_MAX);
        }
        let low = self.target.len() - self.typed.len() < 400;
        match self.mode {
            Mode::Words(_) if self.typed.len() == self.target.len() => self.finish(now),
            // endless modes keep a few lines queued (lava feeds its own text)
            Mode::Time(_)
            | Mode::Zen
            | Mode::Survival { .. }
            | Mode::Pursuit { .. }
            | Mode::HotOne
                if low =>
            {
                self.target.push(' ');
                let more = random_words(&mut self.rng, 50).join(" ");
                self.target.extend(more.chars());
            }
            _ => {}
        }
    }

    /// Append `c` to the text and type it (lava: the text is what you hit).
    pub fn feed(&mut self, c: char, now: Instant) {
        self.target.push(c);
        self.type_char(c, now);
    }

    /// A wrong key that doesn't move the cursor (lava: no word to type it into).
    pub fn miss(&mut self, now: Instant) {
        if self.finished.is_some()
            || self.plate.is_some()
            || self.chewing.is_some()
            || !self.key_down(now)
        {
            return;
        }
        self.stuck = self.strict();
        self.mistake(now);
        self.key_up(now);
        if self.hot_one() {
            self.strike(now);
        }
    }

    /// THE HOT ONE: one strike; the third brings a plate round.
    fn strike(&mut self, now: Instant) {
        self.strikes += 1;
        self.struck = Some(now);
        self.stall_from = Some(now);
        if self.strikes >= STRIKES {
            self.serve(now, Served::Strikes);
        }
    }

    /// THE HOT ONE: a plate comes round. Each plate survived takes a pepper
    /// away (down to one: you know what it is), and being hot adds one.
    fn serve(&mut self, now: Instant, why: Served) {
        let peppers = (PLATE as u32).saturating_sub(self.plates).max(1) as u8
            + if self.rank >= HOT { HOT_BONUS } else { 0 };
        let hot = self.rng.below(peppers as usize) as u8;
        self.plate = Some(Plate {
            served: now,
            why,
            peppers,
            hot,
        });
        self.stuck = false;
        if why == Served::Course {
            self.next_course += COURSE_SECS;
        }
    }

    /// THE HOT ONE: whether pepper `n` can be picked right now.
    pub fn can_pick(&self, n: u8) -> bool {
        self.finished.is_none() && self.plate.is_some_and(|p| n < p.peppers)
    }

    /// THE HOT ONE: bite pepper `n` off the plate, and chew for `BITE_SECS`
    /// (see `swallow`): the plate's gone, the strikes with it.
    pub fn pick(&mut self, n: u8, now: Instant) {
        let Some(p) = self.plate.take().filter(|p| n < p.peppers) else {
            return;
        };
        self.bites.push(Bite {
            at: now,
            pepper: n,
            peppers: p.peppers,
            hot: n == p.hot,
        });
        self.chewing = Some(now);
        self.strikes = 0;
    }

    /// THE HOT ONE: the bite goes down. The hot one ends the round here; any
    /// other counts as a plate survived, and the typing goes on.
    fn swallow(&mut self, now: Instant) {
        self.chewing = None;
        self.stall_from = Some(now);
        match self.bites.last().is_some_and(|b| b.hot) {
            true => {
                self.bitten = Some(now);
                self.finish(now);
            }
            false => self.plates += 1,
        }
    }

    /// THE HOT ONE: how far through chewing the last bite, 0..1.
    pub fn chewed(&self, now: Instant) -> Option<f64> {
        let since = self.chewing?;
        Some((now.saturating_duration_since(since).as_secs_f64() / BITE_SECS).min(1.0))
    }

    /// THE HOT ONE: seconds until the next course (none while a plate's up).
    pub fn course_in(&self, now: Instant) -> Option<f64> {
        if !self.hot_one()
            || self.plate.is_some()
            || self.chewing.is_some()
            || self.finished.is_some()
        {
            return None;
        }
        Some((self.next_course - self.elapsed(now).as_secs_f64()).max(0.0))
    }

    /// THE HOT ONE: seconds a plate has been on the table.
    pub fn plate_age(&self, now: Instant) -> Option<f64> {
        let p = self.plate?;
        Some(now.saturating_duration_since(p.served).as_secs_f64())
    }

    /// What every keystroke does first: start the clock, cool, count it.
    /// False if the cop got to the caret first: the run is over, the key lost.
    fn key_down(&mut self, now: Instant) -> bool {
        self.started.get_or_insert(now);
        self.cool(now);
        if self.caught(now) {
            self.finish(now);
            return false;
        }
        self.keystrokes += 1;
        true
    }

    /// ...and last.
    fn key_up(&mut self, now: Instant) {
        self.last_key = Some(now);
        self.update_rank(now);
    }

    /// Lose the combo and some heat without a keystroke (lava: a word landed).
    pub fn burn(&mut self, now: Instant) {
        self.break_combo(now);
        self.update_rank(now);
    }

    fn mistake(&mut self, now: Instant) {
        self.last_points = 0;
        self.break_combo(now);
        self.errors += 1;
        self.last_error = Some(now);
    }

    fn break_combo(&mut self, now: Instant) {
        if self.combo >= 20 {
            self.broke = Some((self.combo, now));
        }
        self.heat *= MISS_KEEP;
        self.combo = 0;
    }

    /// Pursuit and the hot one have no backspace: there's never a wrong char
    /// to take back.
    pub fn backspace(&mut self, now: Instant) {
        if self.finished.is_none()
            && !self.strict()
            && let Some(c) = self.typed.pop()
        {
            self.erased.push((now, self.typed.len(), c));
        }
    }

    /// Advance the clock: cool heat, record per-second samples, end timed tests.
    pub fn tick(&mut self, now: Instant) {
        if self.started.is_none() || self.finished.is_some() {
            return;
        }
        self.cool(now);
        let over = match self.mode {
            Mode::Time(s) => self.elapsed(now) >= Duration::from_secs(s),
            Mode::Survival { .. } => self.fuel <= 0.0,
            Mode::Pursuit { .. } => self.caught(now),
            _ => false,
        };
        if over {
            return self.finish(now);
        }
        if self.hot_one() {
            self.dinner(now);
        }
        while self.samples.len() < self.elapsed(now).as_secs() as usize {
            self.samples.push(self.wpm(now));
        }
    }

    /// THE HOT ONE's clock: a plate left alone picks itself, the courses come
    /// round, and standing still is a strike.
    fn dinner(&mut self, now: Instant) {
        if let Some(since) = self.chewing {
            if now.saturating_duration_since(since).as_secs_f64() >= BITE_SECS {
                self.swallow(now);
            }
            return;
        }
        match self.plate {
            Some(p) => {
                if now.saturating_duration_since(p.served).as_secs_f64() >= PICK_SECS {
                    let n = self.rng.below(p.peppers as usize) as u8;
                    self.pick(n, now);
                }
            }
            None if self.elapsed(now).as_secs_f64() >= self.next_course => {
                self.serve(now, Served::Course);
            }
            None => {
                let stalled = self
                    .stall_from
                    .is_some_and(|f| now.saturating_duration_since(f).as_secs_f64() >= STALL_SECS);
                if stalled {
                    self.strike(now);
                }
            }
        }
    }

    /// Guitar Hero style: ×1 → ×4, one step per 20 combo.
    pub fn multiplier(&self) -> u32 {
        1 + (self.combo / 20).min(MULT_STEPS)
    }

    /// Maxed-out multiplier.
    pub fn fever(&self) -> bool {
        self.combo >= 20 * MULT_STEPS
    }

    /// Level in the endless word modes (survival, lava, pursuit), from 1.
    pub fn level(&self) -> u32 {
        match self.mode {
            // the cop's steps, so stalling levels up too, up to the step the
            // cop hits top speed (a week's frame gap would read lv30241)
            Mode::Pursuit { .. } => self.start_level() + self.steps().min(self.top_steps()) as u32,
            _ => self.start_level() + self.words / LEVEL_WORDS,
        }
    }

    /// Pursuit: chars between the cop and the caret, 0..=`LEAD_MAX`.
    pub fn lead(&self) -> f64 {
        (self.typed.len() as f64 - self.cop).clamp(0.0, LEAD_MAX)
    }

    /// Pursuit: the cop has reached the caret (and when, for BUSTED!).
    fn caught(&mut self, now: Instant) -> bool {
        let lead = self.lead();
        // NaN too: a cop that broke the maths can't hide behind `<=`
        let caught = self.pursuit() && (lead <= 0.0 || lead.is_nan());
        if caught {
            self.busted = Some(now);
        }
        caught
    }

    /// Pursuit: the cop's speed. Each step ramps it 4% plus 1% per start level.
    pub fn cop_wpm(&self) -> f64 {
        let (wpm, ramp) = self.cop_curve();
        (wpm * libm::pow(1.0 + ramp, self.steps())).min(COP_WPM_MAX)
    }

    /// Pursuit: the steps it takes the cop to reach `COP_WPM_MAX`.
    fn top_steps(&self) -> f64 {
        let (wpm, ramp) = self.cop_curve();
        libm::log(COP_WPM_MAX / wpm) / libm::log(1.0 + ramp)
    }

    /// Pursuit: the cop's speed before any steps, and its ramp per step.
    fn cop_curve(&self) -> (f64, f64) {
        let start = self.start_level() as f64;
        let wpm = COP_WPM * libm::pow(COP_START_STEP, start - 1.0);
        (wpm, 0.04 + start / 100.0)
    }

    fn cop_cps(&self) -> f64 {
        self.cop_wpm() * 5.0 / 60.0
    }

    /// Pursuit: how far the cop has ramped up, by words and by time on the
    /// clock (as of the last tick or key), fractional.
    fn steps(&self) -> f64 {
        let secs = match (self.started, self.last_tick) {
            (Some(s), Some(t)) => t.saturating_duration_since(s).as_secs_f64(),
            _ => 0.0,
        };
        self.words as f64 / LEVEL_WORDS as f64 + secs / COP_STEP_SECS
    }

    fn cool(&mut self, now: Instant) {
        let dt = self
            .last_tick
            .map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f64());
        self.last_tick = Some(now);
        if self.rank >= HOT {
            self.hot_secs += dt;
        }
        self.heat *= libm::exp(-HEAT_DECAY * dt);
        if matches!(self.mode, Mode::Survival { .. }) {
            self.fuel -= dt * libm::pow(DRAIN_STEP, self.level() as f64 - 1.0);
        } else if self.pursuit() {
            self.cop += dt * self.cop_cps();
        }
        self.update_rank(now);
    }

    fn update_rank(&mut self, now: Instant) {
        while self.rank + 1 < RANKS.len() && self.heat >= RANKS[self.rank + 1].at {
            self.rank += 1;
            self.rank_up = Some(now);
            if self.rank >= HOT {
                self.launch(now);
            }
        }
        // hysteresis so hovering on a threshold doesn't spam rank-ups
        while self.rank > 0 && self.heat < RANKS[self.rank].at - RANK_HYSTERESIS {
            self.rank -= 1;
        }
        self.peak_rank = self.peak_rank.max(self.rank);
    }

    /// Queue a jalapeño, spaced out so simultaneous triggers don't stack.
    fn launch(&mut self, now: Instant) {
        let at = self
            .jalapenos
            .last()
            .map_or(now, |&l| now.max(l + Duration::from_millis(400)));
        self.jalapenos.push(at);
    }

    pub fn finish(&mut self, now: Instant) {
        self.finished = Some(now);
        self.samples.push(self.wpm(now));
    }

    pub fn elapsed(&self, now: Instant) -> Duration {
        let Some(start) = self.started else {
            return Duration::ZERO;
        };
        let d = self
            .finished
            .unwrap_or(now)
            .saturating_duration_since(start);
        match self.mode {
            Mode::Time(s) => d.min(Duration::from_secs(s)),
            _ => d,
        }
    }

    pub fn correct(&self) -> usize {
        self.typed
            .iter()
            .zip(&self.target)
            .filter(|(a, b)| a == b)
            .count()
    }

    pub fn wpm(&self, now: Instant) -> f64 {
        self.per_minute(self.correct(), now)
    }

    pub fn raw_wpm(&self, now: Instant) -> f64 {
        self.per_minute(self.typed.len(), now)
    }

    fn per_minute(&self, chars: usize, now: Instant) -> f64 {
        let secs = self.elapsed(now).as_secs_f64();
        // first second is pure noise (1 char in 50ms = 240wpm)
        if secs < 1.0 {
            0.0
        } else {
            chars as f64 / 5.0 / (secs / 60.0)
        }
    }

    pub fn accuracy(&self) -> f64 {
        if self.keystrokes == 0 {
            return 100.0;
        }
        100.0 * (self.keystrokes - self.errors) as f64 / self.keystrokes as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(t: &mut Test, s: &str, now: Instant) {
        s.chars().for_each(|c| t.type_char(c, now));
    }

    /// Types the next `n` target chars, one every `gap_ms`. Returns the clock.
    fn type_steady(t: &mut Test, n: usize, gap_ms: u64, mut now: Instant) -> Instant {
        for _ in 0..n {
            now += Duration::from_millis(gap_ms);
            let c = t.target[t.typed.len()];
            t.type_char(c, now);
        }
        now
    }

    #[test]
    fn words_mode_scores_and_finishes() {
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Words(2), &["hello", "world"]);
        type_str(&mut t, "hellp", t0);
        assert_eq!((t.errors, t.combo, t.best_combo), (1, 0, 4));
        t.backspace(t0);
        type_str(&mut t, "o world", t0 + Duration::from_secs(6));
        let end = t.finished.expect("finished on last char");
        assert_eq!(t.correct(), 11);
        assert_eq!(t.best_combo, 7);
        assert_eq!(t.hits.len(), 11);
        assert_eq!(t.hits.last().map(|h| h.1), Some(10));
        // 11 chars / 5 / 0.1 min = 22 wpm
        assert!((t.wpm(end) - 22.0).abs() < 1e-9);
        assert!((t.accuracy() - 100.0 * 11.0 / 12.0).abs() < 1e-9);
        type_str(&mut t, "x", end);
        assert_eq!(t.typed.len(), 11, "input ignored after finish");
    }

    #[test]
    fn time_mode_samples_and_caps() {
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Time(15), &["a"; 100]);
        t.type_char('a', t0);
        t.tick(t0 + Duration::from_millis(3500));
        assert_eq!(t.samples.len(), 3);
        t.tick(t0 + Duration::from_secs(20));
        assert!(t.finished.is_some());
        assert_eq!(
            t.elapsed(t0 + Duration::from_secs(99)),
            Duration::from_secs(15)
        );
        assert_eq!(t.samples.len(), 4);
    }

    #[test]
    fn words_backspaces_and_steps_are_recorded() {
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Words(3), &["ab", "cd", "ef"]);
        type_str(&mut t, "ab cx", t0);
        t.backspace(t0);
        assert_eq!(t.erased.last().map(|e| (e.1, e.2)), Some((4, 'x')));
        type_str(&mut t, "d ", t0);
        let [first, second] = &t.words_done[..] else {
            panic!("two words done")
        };
        assert_eq!((first.start, first.end, first.clean), (0, 2, true));
        assert_eq!((second.start, second.end, second.clean), (3, 5, false));
        assert_eq!(first.points, 30, "a, b, space at x1");
        assert!(t.mult_up.is_none());
        let mut t = Test::new(Mode::Time(60), &["a"; 30]);
        type_steady(&mut t, 20, 200, t0);
        assert!(t.mult_up.is_some(), "combo 20 steps to x2");
    }

    #[test]
    fn levels_count_up_from_the_start_level() {
        let mut t = Test::new(Mode::Survival { start_level: 1 }, &["a"]);
        assert_eq!(t.level(), 1);
        t.mode = Mode::Survival { start_level: 7 };
        t.words = 23;
        assert_eq!(t.level(), 9, "7 plus two levels of ten words");
    }

    #[test]
    fn survival_refills_streams_and_runs_dry() {
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Survival { start_level: 1 }, &["ab"; 3]);
        type_str(&mut t, "ab ", t0);
        assert_eq!((t.words, t.fuel), (1, FUEL), "refill caps at full");
        t.backspace(t0);
        t.type_char(' ', t0);
        assert_eq!(t.words, 1, "retyping a space doesn't count again");
        assert!(t.target.len() > 100, "more words streamed in");
        t.tick(t0 + Duration::from_secs(4));
        let next: String = t.target[3..6].iter().collect();
        type_str(&mut t, &next, t0 + Duration::from_secs(4));
        assert!((t.fuel - (FUEL - 4.0 + REFILL)).abs() < 1e-9);
        t.tick(t0 + Duration::from_secs(11));
        assert!(t.finished.is_none());
        t.tick(t0 + Duration::from_secs(12));
        assert!(t.finished.is_some(), "out of fuel");
    }

    #[test]
    fn pursuit_stops_on_errors_and_has_no_backspace() {
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &["ab"; 100]);
        type_str(&mut t, "ab", t0);
        let t1 = t0 + Duration::from_secs(1);
        t.type_char('x', t1);
        assert_eq!(t.typed.len(), 2, "a wrong key stays put");
        assert_eq!(
            (t.keystrokes, t.errors, t.combo, t.best_combo),
            (3, 1, 0, 2)
        );
        assert_eq!((t.last_key, t.last_error), (Some(t1), Some(t1)));
        assert!(t.stuck, "the caret waits for the right key");
        assert!(t.cop > 2.0 - LEAD_MAX, "the clock runs on a miss");
        t.backspace(t1);
        assert_eq!(t.typed.len(), 2, "backspace does nothing");
        assert!(t.erased.is_empty());
        // the right key in the same batch (same instant) still frees it
        type_str(&mut t, " a", t1);
        assert!(!t.stuck);
        assert_eq!(t.typed, ['a', 'b', ' ', 'a']);
        assert_eq!((t.keystrokes, t.errors, t.combo, t.words), (5, 1, 2, 1));
        assert!(!t.words_done[0].clean);
        assert!((t.accuracy() - 80.0).abs() < 1e-9);
    }

    #[test]
    fn pursuit_cop_waits_then_trails_within_the_lead() {
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &["ab"; 100]);
        let mut now = t0 + Duration::from_secs(5);
        t.tick(now);
        assert_eq!(
            (t.cop, t.lead()),
            (-LEAD_MAX, LEAD_MAX),
            "waits for the first key"
        );
        t.type_char('a', now);
        assert_eq!(t.cop, 1.0 - LEAD_MAX, "starts a full lead behind");
        // 600wpm: the cop can't keep up, so the cap drags it along
        for _ in 0..600 {
            now = type_steady(&mut t, 1, 20, now);
            assert!(t.typed.len() as f64 - t.cop <= LEAD_MAX);
        }
        assert_eq!(t.lead(), LEAD_MAX);
        assert!(
            t.target.len() > t.typed.len() + 100,
            "more words streamed in"
        );
        assert!(t.finished.is_none());
    }

    #[test]
    fn pursuit_cop_ramps_with_start_level_words_and_time() {
        // ponytail: the paper numbers, 25wpm ×1.12 per start level
        for (lv, wpm) in [(1, 25.0), (4, 35.123), (7, 49.346), (10, 69.327)] {
            let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &["a"]);
            t.mode = Mode::Pursuit { start_level: lv };
            assert!((t.cop_wpm() - wpm).abs() < 1e-3, "lv{lv}: {}", t.cop_wpm());
            assert_eq!(t.level(), lv);
            // ten words is a step: 4% plus 1% per start level faster
            t.words = 10;
            let ramp = 1.04 + lv as f64 / 100.0;
            assert!((t.cop_wpm() - wpm * ramp).abs() < 1e-3);
            assert_eq!(t.level(), lv + 1);
        }
        // one long word at 40wpm keeps ahead of the cop but finishes no words,
        // so only the clock ramps it (20.7s since the first key)
        let long = "a".repeat(200);
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &[&long]);
        type_steady(&mut t, 70, 300, t0);
        assert!(t.finished.is_none());
        assert_eq!((t.words, t.level()), (0, 2), "time alone levels up");
        assert!((t.cop_wpm() - 25.0 * 1.05f64.powf(20.7 / 20.0)).abs() < 1e-9);
    }

    #[test]
    fn pursuit_busts_when_the_cop_catches_up() {
        let t0 = Instant::now();
        let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &["ab"; 100]);
        let mut now = type_steady(&mut t, 5, 100, t0);
        let (stall, caret) = (now, t.typed.len() as f64);
        let mut lead = t.lead();
        assert_eq!(lead, LEAD_MAX);
        while t.finished.is_none() {
            assert!(
                lead > 0.0 && t.busted.is_none(),
                "runs while there's a lead"
            );
            assert!(now - stall < Duration::from_secs(60), "never caught");
            now += Duration::from_millis(16);
            t.tick(now);
            lead = caret - t.cop;
        }
        assert!(lead <= 0.0, "ends on the frame the cop reaches the caret");
        assert_eq!((t.busted, t.finished), (Some(now), Some(now)));
        // 30 chars at a flat 25wpm is 14.4s; the ramp gets there sooner
        let secs = (now - stall).as_secs_f64();
        assert!((13.5..14.4).contains(&secs), "caught after {secs}s");
        assert!(t.cop_wpm() > 25.0, "it sped up while you stalled");
        t.type_char(t.target[5], now);
        assert_eq!(t.typed.len(), 5, "input ignored after busted");
    }

    #[test]
    fn pursuit_busts_a_key_that_comes_too_late() {
        let t0 = Instant::now();
        for c in ['a', '#'] {
            let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &["ab"; 100]);
            let now = type_steady(&mut t, 5, 100, t0);
            // the cop a tenth of a char behind: a key right away beats it...
            t.cop = 4.9;
            t.type_char(t.target[5], now + Duration::from_millis(10));
            assert_eq!((t.typed.len(), t.finished), (6, None), "in time");
            // ...one after it has driven past the caret (~2 chars a second) doesn't
            t.cop = 5.9;
            let before = (t.keystrokes, t.errors, t.combo, t.score);
            let late = now + Duration::from_millis(500);
            t.type_char(c, late);
            assert_eq!((t.busted, t.finished), (Some(late), Some(late)));
            assert_eq!(t.typed.len(), 6, "{c:?} came too late to land");
            assert_eq!((t.keystrokes, t.errors, t.combo, t.score), before);
        }
    }

    #[test]
    fn pursuit_busts_cleanly_after_a_huge_gap() {
        // a week asleep: the ramp alone would overflow the cop to inf (and a
        // second update on the same instant to NaN)
        let t0 = Instant::now();
        let week = t0 + Duration::from_secs(7 * 24 * 3600);
        for key_first in [true, false] {
            let mut t = Test::new(Mode::Pursuit { start_level: 1 }, &["ab"; 100]);
            type_str(&mut t, "ab", t0);
            if key_first {
                t.type_char(' ', week);
            }
            t.tick(week);
            t.tick(week);
            assert_eq!((t.busted, t.finished), (Some(week), Some(week)));
            assert!(t.cop.is_finite(), "cop at {}", t.cop);
            assert_eq!((t.cop_wpm(), t.lead()), (COP_WPM_MAX, 0.0));
            // lv1 tops out at 25wpm × 1.05^75.6: the level stops there too
            assert_eq!(t.level(), 76);
            assert_eq!(t.typed.len(), 2);
        }
    }

    /// A hot-one test that's already started (the round does that at t0).
    fn dinner(t0: Instant) -> Test {
        let mut t = Test::new(Mode::HotOne, &["ab"; 200]);
        t.started = Some(t0);
        t.rng = Rng::new(7);
        t
    }

    #[test]
    fn hot_one_serves_a_plate_on_three_strikes_and_the_hot_one_ends_it() {
        let t0 = Instant::now();
        let mut t = dinner(t0);
        type_str(&mut t, "ab ", t0);
        for (i, ms) in [100, 200].into_iter().enumerate() {
            t.type_char('x', t0 + Duration::from_millis(ms));
            assert_eq!(
                (t.strikes, t.typed.len()),
                (i as u32 + 1, 3),
                "a strike, stays put"
            );
            assert!(t.plate.is_none());
        }
        type_str(&mut t, "a", t0 + Duration::from_millis(300));
        assert_eq!(t.strikes, 0, "a right key clears the strikes");
        let t1 = t0 + Duration::from_millis(400);
        type_str(&mut t, "xxx", t1);
        let plate = t.plate.expect("three in a row: a plate");
        assert_eq!((plate.peppers, plate.why), (PLATE, Served::Strikes));
        assert!(plate.hot < PLATE);
        assert_eq!(t.errors, 5);
        type_str(&mut t, "b x", t1);
        t.backspace(t1);
        assert_eq!(
            (t.typed.len(), t.keystrokes, t.errors),
            (4, 9, 5),
            "nothing types over a plate"
        );
        assert!(!t.can_pick(PLATE) && t.can_pick(PLATE - 1));
        // a mild one: chewed first (nothing types), then back to typing with
        // one pepper fewer next time
        let mild = (plate.hot + 1) % PLATE;
        let t2 = t1 + Duration::from_secs(2);
        t.pick(mild, t2);
        assert!(t.plate.is_none() && t.finished.is_none());
        assert_eq!((t.plates, t.strikes, t.bites.len()), (0, 0, 1));
        assert!(!t.bites[0].hot && t.chewing == Some(t2));
        assert_eq!(
            t.chewed(t2 + Duration::from_secs_f64(BITE_SECS / 2.0)),
            Some(0.5)
        );
        type_str(&mut t, "b", t2);
        assert_eq!(t.typed.len(), 4, "chewing: nothing types");
        t.tick(t2 + Duration::from_secs_f64(BITE_SECS));
        assert_eq!((t.chewing, t.plates), (None, 1));
        let t3 = t2 + Duration::from_secs(2);
        type_str(&mut t, "xxx", t3);
        let plate = t.plate.expect("a second plate");
        assert_eq!(plate.peppers, PLATE - 1);
        t.pick(plate.hot, t3);
        assert!(
            t.bites[1].hot && t.finished.is_none(),
            "not known until it's down"
        );
        let t4 = t3 + Duration::from_secs_f64(BITE_SECS);
        t.tick(t4);
        assert_eq!((t.bitten, t.finished), (Some(t4), Some(t4)));
        assert_eq!(t.words, 1);
        t.type_char('a', t4);
        assert_eq!(t.typed.len(), 4, "done typing");
    }

    #[test]
    fn hot_one_courses_come_on_the_clock_and_a_plate_picks_itself() {
        let t0 = Instant::now();
        let mut t = dinner(t0);
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        type_str(&mut t, "ab ", t0);
        assert_eq!(t.course_in(t0), Some(COURSE_SECS));
        // typing keeps the stall clock quiet; the course comes anyway
        let mut now = t0;
        while now < at(COURSE_SECS - 1.0) {
            now = type_steady(&mut t, 1, 1000, now);
            t.tick(now);
        }
        assert!(t.plate.is_none() && t.strikes == 0);
        t.tick(at(COURSE_SECS));
        let plate = t.plate.expect("the first course");
        assert_eq!((plate.why, plate.served), (Served::Course, at(COURSE_SECS)));
        assert_eq!(t.course_in(at(COURSE_SECS)), None);
        assert_eq!(t.plate_age(at(COURSE_SECS + 1.0)), Some(1.0));
        // left on the table, it picks for you
        t.tick(at(COURSE_SECS + PICK_SECS - 0.1));
        assert!(t.plate.is_some());
        t.tick(at(COURSE_SECS + PICK_SECS));
        assert!(t.plate.is_none() && t.chewing.is_some());
        assert_eq!(t.bites.len(), 1);
        assert_eq!(t.bites[0].hot, t.bites[0].pepper == plate.hot);
        let down = at(COURSE_SECS + PICK_SECS + BITE_SECS);
        t.tick(down);
        match t.bites[0].hot {
            true => assert_eq!((t.bitten, t.plates), (Some(down), 0)),
            false => {
                assert_eq!((t.finished, t.plates), (None, 1));
                assert_eq!(t.course_in(down), Some(COURSE_SECS - PICK_SECS - BITE_SECS));
            }
        }
    }

    #[test]
    fn hot_one_standing_still_is_a_strike_once_you_have_typed() {
        let t0 = Instant::now();
        let mut t = dinner(t0);
        let at = |s: f64| t0 + Duration::from_secs_f64(s);
        // before the first key, only the courses come
        t.tick(at(STALL_SECS * 4.0));
        assert_eq!((t.strikes, t.plate.is_none()), (0, true));
        type_str(&mut t, "a", at(13.0));
        t.tick(at(13.0 + STALL_SECS - 0.1));
        assert_eq!(t.strikes, 0);
        t.tick(at(13.0 + STALL_SECS));
        assert_eq!(
            (t.strikes, t.errors, t.struck),
            (1, 0, Some(at(13.0 + STALL_SECS)))
        );
        t.tick(at(13.0 + 2.0 * STALL_SECS));
        t.tick(at(13.0 + 3.0 * STALL_SECS));
        assert_eq!(t.strikes, 3);
        assert_eq!(t.plate.map(|p| p.why), Some(Served::Strikes));
        assert_eq!(t.combo, 1, "a stall costs no heat or combo");
    }

    #[test]
    fn hot_one_plates_shrink_to_one_and_heat_adds_a_pepper() {
        let t0 = Instant::now();
        let mut t = dinner(t0);
        t.plates = PLATE as u32 + 3;
        t.serve(t0, Served::Strikes);
        let p = t.plate.unwrap();
        assert_eq!(
            (p.peppers, p.hot),
            (1, 0),
            "one pepper: you know what it is"
        );
        let mut t = dinner(t0);
        t.rank = HOT;
        t.serve(t0, Served::Strikes);
        assert_eq!(t.plate.unwrap().peppers, PLATE + 1);
    }

    #[test]
    fn scoville_follows_rank_anchors() {
        assert_eq!(scoville(0.0), 0);
        assert_eq!(scoville(RANKS[6].at), 1_641_000);
        assert_eq!(scoville(100.0), 2_200_000);
        let s: Vec<u32> = (0..=100).map(|h| scoville(h as f64)).collect();
        assert!(s.windows(2).all(|w| w[0] <= w[1]), "monotonic");
    }

    #[test]
    fn heat_rewards_speed_and_punishes_misses() {
        let t0 = Instant::now();
        let mut slow = Test::new(Mode::Time(600), &["a"; 500]);
        type_steady(&mut slow, 200, 300, t0); // 40wpm
        assert!(
            slow.rank <= 1,
            "40wpm stays mild, got {}",
            RANKS[slow.rank].name
        );

        let mut fast = Test::new(Mode::Time(600), &["a"; 500]);
        let now = type_steady(&mut fast, 200, 100, t0); // 120wpm
        assert_eq!(RANKS[fast.rank].name, "GHOST PEPPER");
        assert_eq!(fast.multiplier(), 4);
        assert!(fast.fever());
        assert_eq!(fast.milestone.map(|(n, _)| n), Some(200));
        // habanero + ghost rank-ups, combo 50/100/150/200
        assert_eq!(fast.jalapenos.len(), 6);
        assert_eq!(slow.jalapenos.len(), 4, "slow gets combo jalapeños only");
        // same char count: speed alone earns 2x+ (a timed test also gives 3x the chars)
        assert!(
            fast.score > slow.score * 2,
            "{} vs {}",
            fast.score,
            slow.score
        );

        fast.type_char('x', now + Duration::from_millis(100));
        assert_eq!(fast.broke.map(|(n, _)| n), Some(200));
        assert!(fast.rank <= 3, "a miss cools you down");
        assert_eq!(fast.multiplier(), 1);
    }
}
