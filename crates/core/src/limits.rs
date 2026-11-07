//! mod.io's rate limits. Every request passes through here, so the app can't exceed them
//! however many things it's doing at once.
//!
//! A token gets 120 requests a minute, 60 of them writes, on a rolling window. Failed requests
//! count too. A 429 means stop until `retry-after` passes, and carrying on can get a token
//! revoked, so once one arrives nothing more is sent until that time is up.
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::progress::{emit, Event};

/// What mod.io allows, as shown to people.
pub const MOD_IO_REQUESTS: usize = 120;
pub const MOD_IO_WRITES: usize = 60;
/// What this app allows itself, so a rolling window that counts slightly differently can't
/// tip it over.
const MAX_REQUESTS: usize = 100;
const MAX_WRITES: usize = 50;
const WINDOW: Duration = Duration::from_secs(60);
/// mod.io sends 0 for a rolling limit and advises waiting a minute.
const ROLLING_WAIT: u64 = 60;

/// Requests sent lately, and anything mod.io asked the app to wait for.
#[derive(Default)]
struct Gate {
    sent: VecDeque<(Instant, bool)>,
    blocked: Option<Instant>,
    endpoints: HashMap<String, Instant>,
    announced: Option<Instant>,
}

impl Gate {
    fn forget_old(&mut self, now: Instant) {
        while self
            .sent
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= WINDOW)
        {
            self.sent.pop_front();
        }
        self.blocked = self.blocked.filter(|until| *until > now);
        self.endpoints.retain(|_, until| *until > now);
    }

    /// How long mod.io asked the app to stay away from this endpoint.
    fn refusal(&self, now: Instant, endpoint: &str) -> Duration {
        let until = [self.blocked, self.endpoints.get(endpoint).copied()]
            .into_iter()
            .flatten()
            .max();
        until.map_or(Duration::ZERO, |until| until.saturating_duration_since(now))
    }

    /// How long until sending one more stays within our own budget.
    fn pacing(&self, now: Instant, write: bool) -> Duration {
        let free_at = |times: &[Instant], limit: usize| {
            let over = (times.len() + 1).checked_sub(limit)?;
            (over > 0).then(|| times[over - 1] + WINDOW)
        };
        let all: Vec<Instant> = self.sent.iter().map(|(at, _)| *at).collect();
        let writes: Vec<Instant> = self
            .sent
            .iter()
            .filter(|(_, write)| *write)
            .map(|(at, _)| *at)
            .collect();
        let mut until = free_at(&all, MAX_REQUESTS);
        if write {
            until = until.max(free_at(&writes, MAX_WRITES));
        }
        until.map_or(Duration::ZERO, |until| until.saturating_duration_since(now))
    }

    /// Note a request as sent if it may go now. Otherwise say how long to wait, and whether
    /// that's mod.io's order rather than our own pacing.
    fn ask(&mut self, now: Instant, write: bool, endpoint: &str) -> Pass {
        self.forget_old(now);
        let refused = self.refusal(now, endpoint);
        if !refused.is_zero() {
            return Pass::Refused(refused);
        }
        let pacing = self.pacing(now, write);
        if !pacing.is_zero() {
            return Pass::Paced(pacing);
        }
        self.sent.push_back((now, write));
        Pass::Go
    }

    /// Stop sending until mod.io's wait is over: everywhere, or at one endpoint.
    fn refused(&mut self, now: Instant, seconds: u64, endpoint: Option<&str>) -> Duration {
        let wait = Duration::from_secs(if seconds == 0 { ROLLING_WAIT } else { seconds });
        match endpoint {
            Some(endpoint) => {
                self.endpoints.insert(endpoint.to_string(), now + wait);
            }
            None => self.blocked = Some(now + wait),
        }
        wait
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Pass {
    Go,
    /// Our own budget is used up for now.
    Paced(Duration),
    /// mod.io asked the app to wait.
    Refused(Duration),
}

static GATE: OnceLock<Mutex<Gate>> = OnceLock::new();
static STORE: OnceLock<PathBuf> = OnceLock::new();

fn gate() -> MutexGuard<'static, Gate> {
    GATE.get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Keep a wait across restarts, because closing and reopening is what people try first.
pub fn remember_in(path: PathBuf) {
    let saved = fs::read_to_string(&path).ok();
    if let Some(until) = saved.and_then(|text| text.trim().parse::<u64>().ok()) {
        let left = until.saturating_sub(unix_now());
        if left > 0 {
            gate().blocked = Some(Instant::now() + Duration::from_secs(left));
        }
    }
    let _ = STORE.set(path);
}

/// The name a limit is kept under: the same endpoint for any mod.
pub fn endpoint(method: &str, path: &str) -> String {
    let parts: Vec<&str> = path
        .split('/')
        .map(|part| {
            if !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) {
                "#"
            } else {
                part
            }
        })
        .collect();
    format!("{method} {}", parts.join("/"))
}

pub fn ask(write: bool, endpoint: &str) -> Pass {
    gate().ask(Instant::now(), write, endpoint)
}

/// mod.io refused a request, so stop. `endpoint` is set when only that endpoint is limited.
pub fn refused(seconds: u64, endpoint: Option<&str>) -> Duration {
    let wait = gate().refused(Instant::now(), seconds, endpoint);
    if endpoint.is_none() {
        if let Some(path) = STORE.get() {
            let _ = fs::write(path, (unix_now() + wait.as_secs()).to_string());
        }
    }
    wait
}

/// Tell the app about a wait, once rather than every time something bumps into it.
pub fn announce(wait: Duration) {
    let until = Instant::now() + wait;
    {
        let mut gate = gate();
        if gate
            .announced
            .is_some_and(|told| told + Duration::from_secs(1) >= until)
        {
            return;
        }
        gate.announced = Some(until);
    }
    emit(Event::Waiting {
        seconds: wait.as_secs().max(1),
    });
}

/// What to tell someone waiting at the screen.
pub fn wait_message(wait: Duration) -> String {
    let seconds = wait.as_secs().max(1);
    let unit = if seconds == 1 { "second" } else { "seconds" };
    format!("mod.io asked the app to wait. It can try again in {seconds} {unit}.")
}

#[derive(Serialize)]
pub struct Usage {
    pub requests: usize,
    pub writes: usize,
    pub request_limit: usize,
    pub write_limit: usize,
    pub wait_seconds: u64,
}

/// What this app has used in the last minute.
pub fn usage() -> Usage {
    let now = Instant::now();
    let mut gate = gate();
    gate.forget_old(now);
    Usage {
        requests: gate.sent.len(),
        writes: gate.sent.iter().filter(|(_, write)| *write).count(),
        request_limit: MOD_IO_REQUESTS,
        write_limit: MOD_IO_WRITES,
        wait_seconds: gate
            .blocked
            .map_or(0, |until| until.saturating_duration_since(now).as_secs()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const READ: &str = "GET /games/#/mods";

    fn filled(now: Instant, count: usize, write: bool) -> Gate {
        let mut gate = Gate::default();
        for _ in 0..count {
            assert_eq!(gate.ask(now, write, READ), Pass::Go);
        }
        gate
    }

    #[test]
    fn stays_well_under_what_mod_io_allows() {
        const { assert!(MAX_REQUESTS < MOD_IO_REQUESTS && MAX_WRITES < MOD_IO_WRITES) };
    }

    #[test]
    fn reads_wait_once_the_budget_is_used() {
        let now = Instant::now();
        let mut gate = filled(now, MAX_REQUESTS, false);
        assert_eq!(gate.ask(now, false, READ), Pass::Paced(WINDOW));
    }

    #[test]
    fn a_slot_frees_up_as_the_window_rolls() {
        let now = Instant::now();
        let mut gate = Gate::default();
        for second in 0..MAX_REQUESTS as u64 {
            let at = now + Duration::from_millis(second * 100);
            assert_eq!(gate.ask(at, false, READ), Pass::Go);
        }
        let later = now + Duration::from_secs(30);
        assert_eq!(
            gate.ask(later, false, READ),
            Pass::Paced(Duration::from_secs(30))
        );
        assert_eq!(gate.ask(now + WINDOW, false, READ), Pass::Go);
    }

    #[test]
    fn writes_have_their_own_smaller_budget() {
        let now = Instant::now();
        let mut gate = filled(now, MAX_WRITES, true);
        assert_eq!(gate.ask(now, true, READ), Pass::Paced(WINDOW));
        assert_eq!(gate.ask(now, false, READ), Pass::Go);
    }

    #[test]
    fn a_refusal_stops_everything_until_it_passes() {
        let now = Instant::now();
        let mut gate = Gate::default();
        gate.refused(now, 42, None);
        let wait = Duration::from_secs(42);
        assert_eq!(gate.ask(now, false, READ), Pass::Refused(wait));
        assert_eq!(gate.ask(now, true, "POST /x"), Pass::Refused(wait));
        assert_eq!(gate.ask(now + wait, false, READ), Pass::Go);
    }

    #[test]
    fn zero_seconds_means_wait_a_minute() {
        let now = Instant::now();
        let mut gate = Gate::default();
        assert_eq!(gate.refused(now, 0, None), Duration::from_secs(60));
    }

    #[test]
    fn a_limit_on_one_endpoint_leaves_the_others_alone() {
        let now = Instant::now();
        let mut gate = Gate::default();
        gate.refused(now, 30, Some("POST /comments"));
        assert_eq!(
            gate.ask(now, true, "POST /comments"),
            Pass::Refused(Duration::from_secs(30))
        );
        assert_eq!(gate.ask(now, false, READ), Pass::Go);
    }

    #[test]
    fn nothing_refused_is_counted_as_sent() {
        let now = Instant::now();
        let mut gate = Gate::default();
        gate.refused(now, 5, None);
        gate.ask(now, false, READ);
        assert!(gate.sent.is_empty());
    }

    #[test]
    fn the_same_endpoint_shares_a_limit_across_mods() {
        assert_eq!(
            endpoint("POST", "/v1/games/3959/mods/12/comments"),
            endpoint("POST", "/v1/games/3959/mods/99/comments")
        );
        assert_ne!(
            endpoint("POST", "/v1/games/3959/mods/12/comments"),
            endpoint("GET", "/v1/games/3959/mods/12/comments")
        );
    }

    #[test]
    fn a_wait_is_said_plainly() {
        assert!(wait_message(Duration::from_secs(1)).ends_with("in 1 second."));
        assert!(wait_message(Duration::from_secs(42)).ends_with("in 42 seconds."));
    }
}
