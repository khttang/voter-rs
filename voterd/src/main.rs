//! voterd-rs: votes three altimeter channels received over UDP and logs what changes.
//! The Rust twin of the C++ voterd from Y3: same datagrams, same log lines.
//!
//! One datagram per cycle, three space-separated fields in metres: "1000.5 999.7 1000.0".
//! A field of "-" means the channel's built-in test failed (valid = false); "nan" is accepted.
//! Output goes to stdout, which systemd sends to the journal.
//!
//! voterd-rs [port [tolerance_m [persistence]]]    defaults 5005, 5 m, 3; bound to 127.0.0.1 only

use std::io::ErrorKind;
use std::net::UdpSocket;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use signal_hook::consts::{SIGINT, SIGTERM};
use voter_core::{Config, Mode, Reading, VoteError, Voter, checked_vote};

/// "-" is an invalid channel; anything that isn't a whole number is treated the same way.
fn parse_field(s: &str) -> Reading {
    match s {
        "-" => Reading { value: f64::NAN, valid: false },
        _ => match s.parse::<f64>() {
            Ok(value) => Reading { value, valid: true },
            Err(_) => Reading { value: f64::NAN, valid: false },
        },
    }
}

/// Exactly three fields, or None.
fn parse_datagram(text: &str) -> Option<[Reading; 3]> {
    let mut it = text.split_whitespace().map(parse_field);
    let out = [it.next()?, it.next()?, it.next()?];
    if it.next().is_some() { None } else { Some(out) }
}

/// Whole argument must be a finite number > 0; anything else is rejected, not guessed.
fn parse_positive(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

struct Args {
    port: u16,
    cfg: Config,
}

fn parse_args() -> Option<Args> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let port = match a.first() {
        Some(p) => p.parse().ok()?,
        None => 5005,
    };
    let tolerance = match a.get(1) {
        Some(t) => parse_positive(t)?,
        None => 5.0,
    };
    let persistence = match a.get(2) {
        Some(p) => p.parse::<u32>().ok().filter(|n| *n >= 1)?,
        None => 3,
    };
    if a.len() > 3 {
        return None;
    }
    Some(Args { port, cfg: Config { tolerance, persistence } })
}

fn mode_name(m: Mode) -> &'static str {
    match m {
        Mode::Triplex => "Triplex",
        Mode::Duplex => "Duplex",
        Mode::Simplex => "Simplex",
    }
}

fn error_name(e: VoteError) -> &'static str {
    match e {
        VoteError::NoHealthyChannel => "NoHealthyChannel",
        VoteError::DualMiscompare => "DualMiscompare",
        VoteError::OutOfRange => "OutOfRange",
    }
}

fn main() -> ExitCode {
    let Some(args) = parse_args() else {
        eprintln!("usage: voterd-rs [port [tolerance_m > 0 [persistence >= 1, whole]]]");
        return ExitCode::from(2);
    };

    // SIGTERM sets a flag. The socket has a read timeout, so the loop sees the flag
    // within 200 ms even when no datagrams arrive.
    let stop = Arc::new(AtomicBool::new(false));
    for sig in [SIGTERM, SIGINT] {
        if let Err(e) = signal_hook::flag::register(sig, Arc::clone(&stop)) {
            eprintln!("signal: {e}");
            return ExitCode::FAILURE;
        }
    }

    let sock = match UdpSocket::bind(("127.0.0.1", args.port)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bind: {e}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(e) = sock.set_read_timeout(Some(Duration::from_millis(200))) {
        eprintln!("set_read_timeout: {e}");
        return ExitCode::FAILURE;
    }

    let cfg = args.cfg;
    let mut voter = Voter::new(cfg);
    println!(
        "voterd: listening on 127.0.0.1:{}, tolerance {} m, persistence {}",
        args.port, cfg.tolerance, cfg.persistence
    );

    let mut was_latched = [false; 3];
    let mut last_mode: Option<Mode> = None;
    let mut last_good = 0.0;
    let (mut cycle, mut errors) = (0u64, 0u64);
    let mut in_mode = [0u64; 3]; // cycles spent in Triplex, Duplex, Simplex

    while !stop.load(Ordering::Relaxed) {
        let mut buf = [0u8; 256];
        let n = match sock.recv(&mut buf) {
            Ok(n) => n,
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {
                continue;
            }
            Err(e) => {
                eprintln!("recv: {e}");
                return ExitCode::FAILURE;
            }
        };

        let Some(input) = std::str::from_utf8(&buf[..n]).ok().and_then(parse_datagram) else {
            println!("cycle {cycle}: malformed datagram ignored");
            continue;
        };

        match checked_vote(&mut voter, &input) {
            Ok(r) => {
                if last_mode != Some(r.mode) {
                    println!("cycle {cycle}: mode {}, value {:.2} m", mode_name(r.mode), r.value);
                }
                last_mode = Some(r.mode);
                last_good = r.value;
                in_mode[r.mode as usize] += 1;
            }
            Err(e) => {
                errors += 1;
                println!("cycle {cycle}: {}, holding {last_good:.2} m", error_name(e));
            }
        }
        for (ch, was) in was_latched.iter_mut().enumerate() {
            if voter.latched(ch) && !*was {
                println!("cycle {cycle}: channel {} latched failed", (b'A' + ch as u8) as char);
            }
            *was = voter.latched(ch);
        }
        cycle += 1;
    }

    println!(
        "voterd: stopping after {cycle} cycles, {errors} errors, latched A={} B={} C={}",
        voter.latched(0) as u8,
        voter.latched(1) as u8,
        voter.latched(2) as u8
    );
    println!("voterd: cycles in Triplex={} Duplex={} Simplex={}", in_mode[0], in_mode[1], in_mode[2]);
    ExitCode::SUCCESS
}
