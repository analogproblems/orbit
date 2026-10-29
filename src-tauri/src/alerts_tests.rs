use super::*;

const COOLDOWN: Duration = Duration::hours(6);

fn at(hours: i64) -> DateTime<Utc> {
    DateTime::<Utc>::UNIX_EPOCH + Duration::hours(hours)
}

#[test]
fn a_problem_alerts_once_then_again_only_after_the_cool_down() {
    assert_eq!(raise(Standing::New, false, at(0), COOLDOWN), Raise::Alert);
    let alerted = Standing::Alerted { last_alert: at(0) };
    assert_eq!(raise(alerted, false, at(5), COOLDOWN), Raise::Quiet);
    assert_eq!(raise(alerted, false, at(6), COOLDOWN), Raise::Alert);
}

#[test]
fn worse_alerts_at_once_even_after_an_acknowledgment() {
    let alerted = Standing::Alerted { last_alert: at(0) };
    assert_eq!(raise(alerted, true, at(1), COOLDOWN), Raise::Alert);
    assert_eq!(
        raise(Standing::Acknowledged, false, at(100), COOLDOWN),
        Raise::Quiet
    );
    assert_eq!(
        raise(Standing::Acknowledged, true, at(100), COOLDOWN),
        Raise::Alert
    );
}

#[test]
fn a_problem_back_within_the_cool_down_waits_for_it_to_end() {
    let back = Standing::Returned { last_alert: at(0) };
    assert_eq!(
        raise(back, false, at(2), COOLDOWN),
        Raise::AlertAfter(at(6))
    );
    assert_eq!(raise(back, true, at(2), COOLDOWN), Raise::Alert);
    assert_eq!(raise(back, false, at(7), COOLDOWN), Raise::Alert);
}
