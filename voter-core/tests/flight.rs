//! The lesson C5 Part D flight: 60 cycles at 10 Hz, the same numbers as the C++ test.
//!
//!   A: truth + 0.5.  Built-in test fails (valid = false) at cycles 40 and 41.
//!   B: truth - 0.3.  One spike of +100 m at cycle 5. From cycle 10 it drifts up 1 m per cycle.
//!   C: truth.        One glitch to 0 m at cycle 25. NaN from cycle 50 on.

use voter_core::{Config, Mode, Reading, Voter};

fn truth(k: usize) -> f64 {
    1000.0 + 2.0 * k as f64
}

fn sensors(k: usize) -> [Reading; 3] {
    let t = truth(k);
    let a = Reading { value: t + 0.5, valid: !(k == 40 || k == 41) };
    let mut b = Reading { value: t - 0.3, valid: true };
    if k == 5 {
        b.value = t + 100.0;
    }
    if k >= 10 {
        b.value = t - 0.3 + (k - 10) as f64;
    }
    let mut c = Reading { value: t, valid: true };
    if k == 25 {
        c.value = 0.0;
    }
    if k >= 50 {
        c.value = f64::NAN;
    }
    [a, b, c]
}

#[test]
fn six_second_flight() {
    let mut v = Voter::new(Config { tolerance: 5.0, persistence: 3 });
    let (mut triplex, mut duplex, mut simplex, mut errors) = (0, 0, 0, 0);
    let (mut worst, mut last_good, mut held_at_25) = (0.0f64, truth(0), 0.0);
    for k in 0..60 {
        match v.vote(&sensors(k)) {
            Ok(r) => {
                match r.mode {
                    Mode::Triplex => triplex += 1,
                    Mode::Duplex => duplex += 1,
                    Mode::Simplex => simplex += 1,
                }
                worst = worst.max((r.value - truth(k)).abs());
                last_good = r.value;
            }
            Err(_) => {
                errors += 1;
                if k == 25 {
                    held_at_25 = last_good;
                }
            }
        }
        match k {
            6 => assert_eq!(v.bad_count(1), 0, "spike on B at cycle 5 forgotten by cycle 6"),
            17 => assert!(!v.latched(1), "B drifting: not latched at cycle 17"),
            18 => assert!(v.latched(1), "B drifting: latched at cycle 18"),
            41 => assert_eq!(v.bad_count(0), 2, "A invalid twice: bad_count 2 at cycle 41"),
            42 => assert_eq!(v.bad_count(0), 0, "A good again at cycle 42"),
            _ => {}
        }
    }
    assert_eq!(errors, 1, "C glitch at 25 (Duplex): DualMiscompare, one error only");
    assert!((held_at_25 - (truth(24) + 0.25)).abs() < 1e-12);
    assert!(!v.latched(0) && v.latched(1) && v.latched(2));
    assert_eq!((triplex, duplex, simplex), (19, 28, 12));
    assert!((worst - 0.5).abs() < 1e-9, "the drift never got through");
}
