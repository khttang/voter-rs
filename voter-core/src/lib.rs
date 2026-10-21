//! Triple-redundant voter from lesson C5: mid-value select, persistence, fault latch.
//!
//! `no_std`: no heap, no OS, no I/O. The same crate builds for the Linux daemon (`voterd`)
//! and for a bare microcontroller (`thumbv7em-none-eabihf`). Host unit tests link `std`.
#![cfg_attr(not(test), no_std)]

/// Middle value of three. Compares only: no sort, no array, no heap.
/// The voter only calls this with finite values.
pub const fn mid_value(a: f64, b: f64, c: f64) -> f64 {
    let (a, b) = if a > b { (b, a) } else { (a, b) };
    let (_a, c) = if a > c { (c, a) } else { (a, c) };
    let (b, _c) = if b > c { (c, b) } else { (b, c) };
    b
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub value: f64,
    /// The sensor's own built-in test says it is good.
    pub valid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Triplex,
    Duplex,
    Simplex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoteError {
    /// Nothing usable this cycle.
    NoHealthyChannel,
    /// Two usable channels disagree: the voter can't tell which is wrong.
    DualMiscompare,
    /// The voted value is physically impossible.
    OutOfRange,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voted {
    pub value: f64,
    pub mode: Mode,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Max |channel - voted| that still counts as agreement.
    pub tolerance: f64,
    /// Consecutive BAD cycles before a channel is latched. 1 or more.
    pub persistence: u32,
}

/// One channel's state. The C++ version kept `latched` and `bad_count` in two arrays, so
/// "latched, with bad_count 1" could exist. Here a latched channel has no count at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    Active { bad_count: u32 },
    Latched,
}

/// Rules, applied once per cycle (the same as C5):
///
///  1. A channel is USABLE this cycle if it is not latched, its valid flag is set, and its
///     value is finite. Latched channels are ignored completely: no voting, no counting.
///  2. By the number of usable channels:
///       3: value = mid_value of the three, Triplex.
///          A channel with |x - value| > tolerance is BAD this cycle, the others GOOD.
///       2: if |x1 - x2| > tolerance, DualMiscompare, and both counts stay unchanged.
///          Otherwise value = their average, Duplex, both GOOD.
///       1: value = that channel, Simplex, it is GOOD.
///       0: NoHealthyChannel.
///  3. A channel that is not latched but not usable is BAD this cycle.
///  4. BAD: bad_count += 1; when it reaches persistence, the channel is latched.
///     GOOD: bad_count = 0. A latch is permanent until reset() (maintenance only).
pub struct Voter {
    cfg: Config,
    ch: [Channel; 3],
}

impl Voter {
    pub const fn new(cfg: Config) -> Self {
        Self { cfg, ch: [Channel::Active { bad_count: 0 }; 3] }
    }

    pub fn vote(&mut self, input: &[Reading; 3]) -> Result<Voted, VoteError> {
        let mut usable_cnt = 0;
        let mut usable: [bool; 3] = [false; 3];

        for i in 0..3 {
            if self.ch[i] != Channel::Latched {
                if input[i].valid && input[i].value.is_finite() {
                    usable_cnt += 1;
                    usable[i] = true;
                } else {
                    self.record(i, true);
                    usable[i] = false;
                }
            }
        }

        match usable_cnt {
            1 => {
                let mut ch = 0;
                for i in 0..3 {
                    if usable[i] {
                        ch = i;
                        break;
                    }
                }
                self.record(ch, false);
                Ok(Voted{ value: input[ch].value, mode: Mode::Simplex })
            },
            2 => {
                let mut first = 5;
                let mut second: usize = 5;
                for i in 0..3 {
                    if usable[i] {
                        if first != 5 {
                            if (input[i].value - input[first].value).abs() > self.cfg.tolerance {
                                return Err(VoteError::DualMiscompare);
                            } else {
                                second = i;
                                break;
                            }
                        } else {
                            first = i;
                        }
                    }
                }
                self.record(first, false);
                self.record(second, false);
                Ok(Voted {value: 0.5 * {input[first].value + input[second].value}, mode: Mode::Duplex})
            },
            3 => {
                let mid = mid_value(input[0].value, input[1].value, input[2].value);
                for ch in 0..3 {
                    self.record(ch, (mid - input[ch].value).abs() > self.cfg.tolerance);
                }
                Ok(Voted {value: mid, mode: Mode::Triplex})
            },
            _ => {
                return Err(VoteError::NoHealthyChannel);
            }
        }
    }

    pub fn latched(&self, ch: usize) -> bool {
        self.ch[ch] == Channel::Latched
    }

    /// Consecutive BAD cycles; a latched channel reports `persistence`.
    pub fn bad_count(&self, ch: usize) -> u32 {
        match self.ch[ch] {
            Channel::Active { bad_count } => bad_count,
            Channel::Latched => self.cfg.persistence,
        }
    }

    /// Maintenance only: never called by the voter itself.
    pub fn reset(&mut self, ch: usize) {
        self.ch[ch] = Channel::Active { bad_count: 0 };
    }

    // Rule 4.
    fn record(&mut self, ch: usize, bad: bool) {
        // TODO A4: rule 4. Only an Active channel changes. A `match` on (bad, will_reach_persistence) reads well.
        if bad {
            match &mut self.ch[ch] {
                Channel::Active { bad_count } => {
                    *bad_count += 1;
                    if *bad_count >= self.cfg.persistence {
                        self.ch[ch] = Channel::Latched;
                    }
                },
                Channel::Latched => {
                    //let count = *bad_count + 1;
                }
            }
        } else {
            self.ch[ch] = Channel::Active { bad_count: 0 }
        }
    }
}

pub const MIN_ALTITUDE: f64 = -500.0; // m, Dead Sea is -430
pub const MAX_ALTITUDE: f64 = 15000.0; // m, above this aircraft's ceiling

/// Vote, then reject a physically impossible value. Same shape as C5's `.and_then()`.
pub fn checked_vote(v: &mut Voter, input: &[Reading; 3]) -> Result<Voted, VoteError> {
    // A5: one expression, `v.vote(input).and_then(|x| ...)`, as C5's .and_then()
    v.vote(input).and_then(|x| if x.value < MIN_ALTITUDE || x.value > MAX_ALTITUDE { Err(VoteError::OutOfRange) } else { Ok(x) })
}

/// The altitude to use this cycle: the checked vote's value, or `last_good` on any error.
pub fn altitude_command(v: &mut Voter, input: &[Reading; 3], last_good: f64) -> f64 {
    // A5: one expression with .map() and .unwrap_or(), as C5's .transform().value_or()
    checked_vote(v, input).map(|x| x.value).unwrap_or(last_good)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CFG: Config = Config { tolerance: 5.0, persistence: 3 };
    const NAN: f64 = f64::NAN;

    fn r(value: f64, valid: bool) -> Reading {
        Reading { value, valid }
    }
    fn all(a: f64, b: f64, c: f64) -> [Reading; 3] {
        [r(a, true), r(b, true), r(c, true)]
    }

    #[test]
    fn mid_value_all_orders_and_ties() {
        for [a, b, c] in [[1., 2., 3.], [1., 3., 2.], [2., 1., 3.], [2., 3., 1.], [3., 1., 2.], [3., 2., 1.]] {
            assert_eq!(mid_value(a, b, c), 2.0);
        }
        assert_eq!(mid_value(5., 5., 1.), 5.);
        assert_eq!(mid_value(1., 5., 5.), 5.);
        assert_eq!(mid_value(5., 1., 5.), 5.);
        assert_eq!(mid_value(1000., 1e9, 1001.), 1001.);
        assert_eq!(mid_value(-3., -1., -2.), -2.);
        const AT_COMPILE_TIME: f64 = mid_value(3., 1., 2.);
        assert_eq!(AT_COMPILE_TIME, 2.);
    }

    #[test]
    fn triplex_and_outliers() {
        let mut v = Voter::new(CFG);
        assert_eq!(v.vote(&all(100., 101., 102.)), Ok(Voted { value: 101., mode: Mode::Triplex }));

        let mut v = Voter::new(CFG);
        assert_eq!(v.vote(&all(100., 500., 101.)).unwrap().value, 101.);
        assert_eq!((v.bad_count(0), v.bad_count(1), v.bad_count(2)), (0, 1, 0));
        let _ = v.vote(&all(100., 101., -400.));
        assert_eq!(v.bad_count(2), 1);
    }

    #[test]
    fn persistence_latch_and_reset() {
        let mut v = Voter::new(CFG);
        for _ in 0..2 {
            let _ = v.vote(&all(100., 500., 101.));
        }
        assert!(!v.latched(1));
        let _ = v.vote(&all(100., 100., 101.));
        assert_eq!(v.bad_count(1), 0);
        for _ in 0..3 {
            let _ = v.vote(&all(100., 500., 101.));
        }
        assert!(v.latched(1));
        let res = v.vote(&all(100., 100.5, 101.));
        assert!(v.latched(1), "a latch is permanent");
        assert_eq!(res, Ok(Voted { value: 100.5, mode: Mode::Duplex }));
        v.reset(1);
        assert!(!v.latched(1) && v.bad_count(1) == 0);
    }

    #[test]
    fn invalid_nan_inf_are_bad() {
        let mut v = Voter::new(CFG);
        assert_eq!(v.vote(&[r(100., true), r(100., false), r(101., true)]).unwrap().mode, Mode::Duplex);
        assert_eq!(v.bad_count(1), 1);

        let mut v = Voter::new(CFG);
        let res = v.vote(&all(100., NAN, 101.)).unwrap();
        assert!(res.mode == Mode::Duplex && res.value.is_finite());
        assert_eq!(v.vote(&all(100., f64::INFINITY, 101.)).unwrap().mode, Mode::Duplex);
        assert_eq!(v.bad_count(1), 2);
    }

    #[test]
    fn duplex_miscompare_simplex_none() {
        let mut v = Voter::new(CFG);
        assert_eq!(v.vote(&[r(100., true), r(200., true), r(0., false)]), Err(VoteError::DualMiscompare));
        assert_eq!((v.bad_count(0), v.bad_count(1), v.bad_count(2)), (0, 0, 1));

        let mut v = Voter::new(CFG);
        assert_eq!(v.vote(&[r(0., false), r(42., true), r(0., false)]), Ok(Voted { value: 42., mode: Mode::Simplex }));
        assert_eq!(v.vote(&[r(0., false), r(0., false), r(NAN, true)]), Err(VoteError::NoHealthyChannel));
    }

    #[test]
    fn chaining() {
        let mut v = Voter::new(CFG);
        assert!(checked_vote(&mut v, &all(100., 101., 102.)).is_ok());
        assert_eq!(checked_vote(&mut v, &all(20000., 20000., 20000.)), Err(VoteError::OutOfRange));
        assert_eq!(checked_vote(&mut v, &[r(0., false); 3]), Err(VoteError::NoHealthyChannel));
        assert_eq!(altitude_command(&mut v, &all(300., 301., 302.), -1.), 301.);
        assert_eq!(altitude_command(&mut v, &all(-900., -900., -900.), 301.), 301.);
    }
}
