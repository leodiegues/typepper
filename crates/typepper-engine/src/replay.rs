//! A round as bytes: what the game uploads, the server stores and scores,
//! and the web player plays back (docs/reference/replays.md).
//!
//! ```text
//! "TPR" | format u8 | engine u32 LE | mode key (u8 length + UTF-8) | seed u64 LE
//!       | count varint | count × (Δms varint, op varint)
//! op: 0 Backspace, 1 End, n ≥ 2 the char n − 2
//! ```
//!
//! The mode travels as its text key (`Mode::key`), not as `Mode`, so this
//! module only knows `Input`: the header stays readable whatever the rules
//! become, and the day the server runs two engines it can move to its own
//! crate.

use std::fmt;

use crate::Input;

const MAGIC: &[u8; 3] = b"TPR";
/// This layout. A reader refuses any other.
pub const FORMAT: u8 = 1;
/// The biggest replay anyone takes: about 7 hours of zen at 60 wpm.
pub const MAX_BYTES: usize = 1 << 20;
const MAX_MODE: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// The rules that recorded it (`crate::VERSION`).
    pub engine: u32,
    /// `Mode::key`, e.g. `pursuit:7`.
    pub mode: String,
    pub seed: u64,
    /// Each input at its millisecond of the round, in order.
    pub inputs: Vec<(u32, Input)>,
}

/// Why bytes aren't a replay (the server answers `err bad-replay <why>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bad {
    TooBig,
    NotAReplay,
    Format(u8),
    Truncated,
    Mode,
    Char,
    Leftover,
    Clock,
}

impl fmt::Display for Bad {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Bad::TooBig => write!(f, "over {MAX_BYTES} bytes"),
            Bad::NotAReplay => write!(f, "not a typepper replay"),
            Bad::Format(n) => write!(f, "format {n} is unknown here"),
            Bad::Truncated => write!(f, "truncated"),
            Bad::Mode => write!(f, "bad mode"),
            Bad::Char => write!(f, "bad character"),
            Bad::Leftover => write!(f, "bytes after the last input"),
            Bad::Clock => write!(f, "the clock runs past u32 milliseconds"),
        }
    }
}

impl std::error::Error for Bad {}

impl Replay {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(32 + self.inputs.len() * 3);
        b.extend(MAGIC);
        b.push(FORMAT);
        b.extend(self.engine.to_le_bytes());
        b.push(self.mode.len() as u8);
        b.extend(self.mode.as_bytes());
        b.extend(self.seed.to_le_bytes());
        varint(&mut b, self.inputs.len() as u64);
        let mut last = 0;
        for &(ms, input) in &self.inputs {
            varint(&mut b, (ms - last).into());
            last = ms;
            varint(
                &mut b,
                match input {
                    Input::Backspace => 0,
                    Input::End => 1,
                    Input::Char(c) => c as u64 + 2,
                },
            );
        }
        b
    }

    /// Reads one replay off the front of `bytes`; returns it and what's left
    /// (a submission is several back to back).
    pub fn decode_prefix(bytes: &[u8]) -> Result<(Replay, &[u8]), Bad> {
        let mut r = Reader(bytes);
        if r.take(3)? != MAGIC {
            return Err(Bad::NotAReplay);
        }
        match r.byte()? {
            FORMAT => {}
            n => return Err(Bad::Format(n)),
        }
        let engine = u32::from_le_bytes(r.take(4)?.try_into().unwrap());
        let len = r.byte()? as usize;
        if len > MAX_MODE {
            return Err(Bad::Mode);
        }
        let mode = std::str::from_utf8(r.take(len)?).map_err(|_| Bad::Mode)?;
        let seed = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
        let count = r.varint()?;
        // every input is at least 2 bytes: a count past that is a lie
        if count > r.0.len() as u64 / 2 {
            return Err(Bad::Truncated);
        }
        let mut inputs = Vec::with_capacity(count as usize);
        let mut ms: u32 = 0;
        for _ in 0..count {
            let gap = u32::try_from(r.varint()?).map_err(|_| Bad::Clock)?;
            ms = ms.checked_add(gap).ok_or(Bad::Clock)?;
            let input = match r.varint()? {
                0 => Input::Backspace,
                1 => Input::End,
                n => Input::Char(
                    u32::try_from(n - 2)
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or(Bad::Char)?,
                ),
            };
            inputs.push((ms, input));
        }
        let replay = Replay {
            engine,
            mode: mode.to_string(),
            seed,
            inputs,
        };
        Ok((replay, r.0))
    }

    /// Exactly one replay, nothing more.
    pub fn decode(bytes: &[u8]) -> Result<Replay, Bad> {
        if bytes.len() > MAX_BYTES {
            return Err(Bad::TooBig);
        }
        match Replay::decode_prefix(bytes)? {
            (replay, []) => Ok(replay),
            _ => Err(Bad::Leftover),
        }
    }
}

/// Unsigned LEB128: 7 bits a byte, the top bit says another follows.
fn varint(b: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        b.push(n as u8 | 0x80);
        n >>= 7;
    }
    b.push(n as u8);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Bad> {
        if self.0.len() < n {
            return Err(Bad::Truncated);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }

    fn byte(&mut self) -> Result<u8, Bad> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self) -> Result<u64, Bad> {
        let mut n = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.byte()?;
            n |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(n);
            }
        }
        Err(Bad::Truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blitz() -> Replay {
        Replay {
            engine: 1,
            mode: "time:15".into(),
            seed: 0x0102_0304_0506_0708,
            inputs: vec![
                (0, Input::Char('h')),
                (130, Input::Char('i')),
                (225, Input::Backspace),
            ],
        }
    }

    #[test]
    fn format_1_bytes_are_these() {
        // docs/reference/replays.md's example: if this moves, format 1 changed
        #[rustfmt::skip]
        let want = [
            b'T', b'P', b'R',
            1,
            1, 0, 0, 0,
            7, b't', b'i', b'm', b'e', b':', b'1', b'5',
            8, 7, 6, 5, 4, 3, 2, 1,
            3,
            0x00, 0x6a,
            0x82, 0x01, 0x6b,
            0x5f, 0x00,
        ];
        assert_eq!(blitz().encode(), want);
        assert_eq!(Replay::decode(&want), Ok(blitz()));
    }

    #[test]
    fn round_trips_long_gaps_and_any_char() {
        let r = Replay {
            engine: 7,
            mode: "lava:free:10".into(),
            seed: u64::MAX,
            inputs: vec![
                (5, Input::Char('é')),
                (5, Input::End),
                (300_000, Input::Char('🌶')),
                (u32::MAX, Input::Char(' ')),
            ],
        };
        assert_eq!(Replay::decode(&r.encode()), Ok(r));
    }

    #[test]
    fn junk_is_refused_never_a_panic() {
        let good = blitz().encode();
        assert_eq!(Replay::decode(&[]), Err(Bad::Truncated));
        assert_eq!(Replay::decode(b"GIF89a"), Err(Bad::NotAReplay));
        let mut v = good.clone();
        v[3] = 2;
        assert_eq!(Replay::decode(&v), Err(Bad::Format(2)));
        for cut in 0..good.len() {
            assert!(Replay::decode(&good[..cut]).is_err(), "cut at {cut}");
        }
        let mut v = good.clone();
        v.push(0);
        assert_eq!(Replay::decode(&v), Err(Bad::Leftover));
        let mut v = good[..good.len() - 1].to_vec();
        v.push(0xff); // op past char::MAX (and an unfinished varint)
        assert!(Replay::decode(&v).is_err());
        let mut v = good.clone();
        v.pop(); // the last op, swapped for 0xd800 + 2: a surrogate, not a char
        v.extend([0x82, 0xb0, 0x03]);
        assert_eq!(Replay::decode(&v), Err(Bad::Char));
        assert_eq!(Replay::decode(&vec![0; MAX_BYTES + 1]), Err(Bad::TooBig));
        // a count claiming more inputs than there are bytes for
        let mut v = good[..24].to_vec();
        v.extend([0xff, 0xff, 0x03]);
        assert_eq!(Replay::decode(&v), Err(Bad::Truncated));
    }

    #[test]
    fn replays_come_off_a_stream_one_by_one() {
        let mut stream = blitz().encode();
        stream.extend(blitz().encode());
        let (a, rest) = Replay::decode_prefix(&stream).unwrap();
        let (b, rest) = Replay::decode_prefix(rest).unwrap();
        assert_eq!((a, b, rest.len()), (blitz(), blitz(), 0));
    }
}
