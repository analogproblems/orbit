//! What time it is where you are. Bit's morning brief comes by it
//! (`crate::brief`): in the time zone Bit's settings give
//! (`Buddy::time_zone`), or by the clock of the computer Orbit runs on when
//! you choose none, so an Orbit on a server set to another zone still
//! briefs you at your time. Everything else Orbit keeps is in UTC.
use chrono::{DateTime, NaiveDateTime, Utc};
use chrono_tz::Tz;
use std::sync::OnceLock;

/// The time zone named `name`, such as `Europe/London`, if Orbit knows it.
pub fn zone(name: &str) -> Option<Tz> {
    name.parse().ok()
}
/// Whether Bit's settings can keep `name`: a zone Orbit knows, or empty for
/// the computer's own.
pub fn valid(name: &str) -> bool {
    name.is_empty() || zone(name).is_some()
}
/// The time at `now` in the zone named `name`, or by this computer's clock
/// when `name` is empty.
pub fn local(name: &str, now: DateTime<Utc>) -> NaiveDateTime {
    match zone(name) {
        Some(tz) => now.with_timezone(&tz).naive_local(),
        None => now.with_timezone(&chrono::Local).naive_local(),
    }
}
/// The name of this computer's time zone, such as `Etc/UTC` on many
/// servers, for the app to say whose clock the brief keeps without a
/// choice. Empty when it can't be told. Read once, as Orbit starts.
pub fn computer_zone() -> String {
    static ZONE: OnceLock<String> = OnceLock::new();
    ZONE.get_or_init(|| {
        read_zone(
            std::env::var("TZ").ok(),
            iana_time_zone::get_timezone().ok(),
        )
    })
    .clone()
}
/// The computer's zone, from `TZ` when that is set, as the clock itself
/// reads it, else from the system's setting. A `TZ` that names no zone
/// Orbit knows, such as a rule like `EST5`, says nothing: naming the
/// system's zone then could be wrong.
pub fn read_zone(tz: Option<String>, system: Option<String>) -> String {
    let named = |name: &str| zone(name).map(|_| name.to_string());
    match tz.as_deref().map(|t| t.trim_start_matches(':')) {
        Some(t) if !t.is_empty() => named(t)
            .or_else(|| t.split_once("zoneinfo/").and_then(|(_, n)| named(n)))
            .unwrap_or_default(),
        _ => system.as_deref().and_then(named).unwrap_or_default(),
    }
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;
