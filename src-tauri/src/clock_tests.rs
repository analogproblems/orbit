use super::*;
use chrono::TimeZone;

fn utc(at: &str) -> DateTime<Utc> {
    Utc.from_utc_datetime(&NaiveDateTime::parse_from_str(at, "%Y-%m-%d %H:%M").unwrap())
}
fn shown(t: NaiveDateTime) -> String {
    t.format("%Y-%m-%d %H:%M").to_string()
}

#[test]
fn tells_the_time_in_a_chosen_zone_whatever_the_computer_keeps() {
    // New York keeps summer time until 1 November 2026, then is 5 hours
    // behind UTC.
    let cases = [
        ("America/New_York", "2026-10-30 13:00", "2026-10-30 09:00"),
        ("America/New_York", "2026-11-02 13:59", "2026-11-02 08:59"),
        ("America/New_York", "2026-11-02 14:00", "2026-11-02 09:00"),
        // A day ahead of UTC.
        ("Pacific/Auckland", "2026-11-01 21:00", "2026-11-02 10:00"),
        ("Asia/Kolkata", "2026-11-01 03:30", "2026-11-01 09:00"),
        ("UTC", "2026-11-01 09:00", "2026-11-01 09:00"),
    ];
    for (name, at, want) in cases {
        assert_eq!(shown(local(name, utc(at))), want, "{name} at {at}");
    }
}

#[test]
fn keeps_this_computers_clock_without_a_zone() {
    let now = utc("2026-11-01 09:00");
    assert_eq!(
        local("", now),
        now.with_timezone(&chrono::Local).naive_local()
    );
}

#[test]
fn knows_only_real_zones() {
    for name in [
        "",
        "Europe/London",
        "America/Argentina/Buenos_Aires",
        "UTC",
        "Etc/GMT+5",
    ] {
        assert!(valid(name), "{name}");
    }
    for name in [
        "Mars/Olympus",
        "europe/london",
        " Europe/London",
        "London",
        "+05:00",
        "09:00",
    ] {
        assert!(!valid(name), "{name}");
    }
}

#[test]
fn names_the_computers_zone_only_when_sure() {
    let some = |s: &str| Some(s.to_string());
    let cases = [
        // The system's setting, without TZ.
        (None, some("Europe/Berlin"), "Europe/Berlin"),
        (some(""), some("Etc/UTC"), "Etc/UTC"),
        // TZ wins, as it does for the clock.
        (some("Asia/Tokyo"), some("Etc/UTC"), "Asia/Tokyo"),
        (some(":America/Chicago"), some("Etc/UTC"), "America/Chicago"),
        (
            some("/usr/share/zoneinfo/Australia/Perth"),
            some("Etc/UTC"),
            "Australia/Perth",
        ),
        // A rule names no zone, so neither does Orbit.
        (some("<+03>-3"), some("Etc/UTC"), ""),
        (None, some("Not/AZone"), ""),
        (None, None, ""),
    ];
    for (tz, system, want) in cases {
        assert_eq!(
            read_zone(tz.clone(), system.clone()),
            want,
            "{tz:?} {system:?}"
        );
    }
}
