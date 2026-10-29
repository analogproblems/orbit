//! Alerting once. A problem tells you once. Raised again while it is still
//! open, it stays quiet until the cool-down since its last alert has passed,
//! and only then tells you again, unless it got worse, which tells you at
//! once. Once you acknowledge it, only worse tells you again. A problem that
//! cleared and came back within the cool-down waits for the cool-down to end,
//! so one that flaps does not alert each time.
//!
//! `crate::problems` keeps each problem's record and works out where it
//! stands; this module only decides.
use chrono::{DateTime, Duration, Utc};

/// What you have been told about a problem, as raising it again finds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// Nothing yet: it is new.
    New,
    /// Alerted at `last_alert` and still open.
    Alerted { last_alert: DateTime<Utc> },
    /// You acknowledged it, and it has not cleared.
    Acknowledged,
    /// It cleared, and came back; its last alert was at `last_alert`.
    Returned { last_alert: DateTime<Utc> },
}

/// What raising a problem does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Raise {
    /// Tell you.
    Alert,
    /// Tell you, but not before this time.
    AlertAfter(DateTime<Utc>),
    /// Keep it up to date without telling you again.
    Quiet,
}

/// What raising a problem standing as `standing` does at `now`. `worse` says
/// it got worse since you were last told.
pub fn raise(standing: Standing, worse: bool, now: DateTime<Utc>, cooldown: Duration) -> Raise {
    if worse {
        return Raise::Alert;
    }
    match standing {
        Standing::New => Raise::Alert,
        Standing::Alerted { last_alert } if now - last_alert >= cooldown => Raise::Alert,
        Standing::Alerted { .. } | Standing::Acknowledged => Raise::Quiet,
        Standing::Returned { last_alert } if now - last_alert < cooldown => {
            Raise::AlertAfter(last_alert + cooldown)
        }
        Standing::Returned { .. } => Raise::Alert,
    }
}

#[cfg(test)]
#[path = "alerts_tests.rs"]
mod tests;
