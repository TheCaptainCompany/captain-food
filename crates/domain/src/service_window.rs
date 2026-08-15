//! The service-hours evaluation (RSO-1, DECISIONS §43) — ONE pure function, [`serving_at`],
//! shared by the storefront badge and the checkout guard so the two can never disagree.
//!
//! The contract is `specs/network/api.yaml#/types/ServiceWindow` +
//! `specs/common/scalars.yaml#/ServiceWindowVerdict`, verbatim:
//!
//! - `verdict` is the ONLY discriminator; the evaluation is TOTAL (no panic, no clock read —
//!   `now` is a parameter, the `sms_guard.rs` precedent: a function reading the system clock
//!   internally cannot have both edges of a time boundary asserted).
//! - `[]` opening hours IS `HOURS_UNDECLARED` by definition — never "closed" — because storage
//!   collapses "never provided", "cleared" and "unparseable" into one `[]`.
//! - a NULL timezone, or one that does not parse while hours ARE declared, is `HOURS_UNDECLARED`
//!   too (the once-documented account-timezone fallback has no materialized source).
//! - evaluation converts the UTC instant into the restaurant's LOCAL wall clock (total,
//!   DST-safe via chrono-tz), never slot times to UTC: on the fold-back night the wall clock
//!   repeats an hour, and comparing wall time against wall slots is exactly what a customer at
//!   the door experiences.
//! - overnight slots (`to` < `from`, e.g. Friday 19:00–01:00) are legal, cross midnight, and
//!   require consulting the PREVIOUS day's slots for early-morning instants.
//! - `lastOrderAt` = min(current slot's door-close `to`, the declared cutoff), degrading
//!   EXPLICITLY to door-close while no cutoff source is declared (HubRise `cutoff_time` maps
//!   nowhere today) — a documented degradation, never a silent fallback.
//! - `validUntil` = min(next window transition, `evaluatedAt` + horizon); under
//!   `HOURS_UNDECLARED` (no transition to wait for) it is `evaluatedAt` + horizon. The horizon
//!   is a PARAMETER (`SERVICE_WINDOW_VALIDITY_HORIZON_SECONDS` is the caller's configuration):
//!   baking it in would make a test of this function assert configuration, not behaviour.

use chrono::{DateTime, Datelike, Duration, TimeZone as _, Timelike, Utc};
use chrono_tz::Tz;

use crate::generated::entities::OpeningHoursSlot;
use crate::generated::scalars::{ServiceWindowVerdict, TimeOfDay, TimeZone, Weekday};

/// The full service-hours evaluation AT AN INSTANT — the domain-side twin of the API's
/// `ServiceWindow` conversation shape. `verdict` is the only discriminator; the nullability of
/// `opens_at`/`last_order_at` follows it and must never be read as semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceWindow {
    /// OPEN | OUTSIDE_HOURS | HOURS_UNDECLARED. At checkout, OUTSIDE_HOURS is the ONLY refusing
    /// verdict (rules.yaml#/CheckoutRefusesOnlyOutsideServiceHours).
    pub verdict: ServiceWindowVerdict,
    /// Next opening instant. Set under OUTSIDE_HOURS; None under OPEN (already open) and
    /// HOURS_UNDECLARED (nothing to promise).
    pub opens_at: Option<DateTime<Utc>>,
    /// Start instant of the window the verdict was judged against — Some under OPEN only. Not
    /// part of the API conversation shape; it exists for the acceptance EVIDENCE the checkout
    /// freezes onto `CheckoutSnapshot.windowFrom` (a disputed acceptance is proved from the
    /// window's own instants, not just an enum).
    pub window_from: Option<DateTime<Utc>>,
    /// The latest instant an order can still be placed — min(door-close, cutoff), INCLUSIVE
    /// (at exactly this instant the order is still placeable; see the boundary test). None under
    /// OUTSIDE_HOURS and HOURS_UNDECLARED.
    pub last_order_at: Option<DateTime<Utc>>,
    /// The instant the verdict was evaluated at — the caller's request clock, echoed back.
    pub evaluated_at: DateTime<Utc>,
    /// The instant after which the caller must RE-EVALUATE: min(next transition,
    /// evaluated_at + horizon). NON-NULL on purpose — a nullable expiry reads as "cache forever".
    pub valid_until: DateTime<Utc>,
}

/// One parsed weekly slot: weekday index (Monday = 0) + minutes-since-midnight edges.
struct Slot {
    weekday: u8,
    from_min: u32,
    to_min: u32,
}

/// "HH:mm" → minutes since local midnight. `TimeOfDay`'s spec pattern guarantees the shape, but
/// this function is TOTAL over whatever the store actually holds — a malformed value returns
/// `None` and the caller degrades to HOURS_UNDECLARED rather than panicking (the `.expect` one
/// line above the emitter call site is the anti-pattern this module refuses to copy — PAN-1).
fn minutes_of(t: &TimeOfDay) -> Option<u32> {
    let (h, m) = t.0.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    if h > 23 || m > 59 {
        return None;
    }
    Some(h * 60 + m)
}

/// Weekday → Monday-based index, aligned with `chrono::Weekday::num_days_from_monday`.
fn weekday_index(w: Weekday) -> u8 {
    match w {
        Weekday::MONDAY => 0,
        Weekday::TUESDAY => 1,
        Weekday::WEDNESDAY => 2,
        Weekday::THURSDAY => 3,
        Weekday::FRIDAY => 4,
        Weekday::SATURDAY => 5,
        Weekday::SUNDAY => 6,
    }
}

/// A local wall-clock time on a local date → the UTC instant at which the wall clock first (or,
/// for `prefer_latest`, last) READS that time. Total over DST:
///
/// - `Single` — the ordinary 8,758 hours of the year;
/// - `Ambiguous` (fold-back — the wall repeats an hour): `prefer_latest` picks which occurrence.
///   A door-CLOSE takes the LATEST (under wall-clock semantics the window is still covering
///   during the repeated hour), an OPENING takes the earliest (the first moment the wall reads
///   the opening time);
/// - `None` (spring-forward gap — the wall skips an hour): step forward minute by minute until a
///   representable wall time resolves. The first representable minute at/after the skipped time
///   is exactly the instant of the jump, so the resolution is precise, and the loop is bounded
///   by the gap width (≤ 121 iterations for any real IANA zone).
fn resolve_local(naive: chrono::NaiveDateTime, tz: Tz, prefer_latest: bool) -> Option<DateTime<Utc>> {
    let mut candidate = naive;
    for _ in 0..=121 {
        match tz.from_local_datetime(&candidate) {
            chrono::LocalResult::Single(dt) => return Some(dt.with_timezone(&Utc)),
            chrono::LocalResult::Ambiguous(a, b) => {
                return Some(if prefer_latest { b } else { a }.with_timezone(&Utc))
            }
            chrono::LocalResult::None => candidate += Duration::minutes(1),
        }
    }
    None
}

/// The HOURS_UNDECLARED shape: nothing to promise, nothing to enforce, re-evaluate after the
/// horizon (no transition to wait for).
fn undeclared(now: DateTime<Utc>, horizon: Duration) -> ServiceWindow {
    ServiceWindow {
        verdict: ServiceWindowVerdict::HOURS_UNDECLARED,
        opens_at: None,
        window_from: None,
        last_order_at: None,
        evaluated_at: now,
        valid_until: now + horizon,
    }
}

/// Evaluate the restaurant's service window AT `now`.
///
/// Inputs are the declared weekly `hours`, the restaurant's IANA `timezone`, the declared order
/// `cutoff` (a local time-of-day; `None` — the only value any caller can supply today — degrades
/// explicitly to door-close), the request instant `now`, and the validity `horizon`
/// (`SERVICE_WINDOW_VALIDITY_HORIZON_SECONDS`, passed by the caller). Pure and total: no clock
/// read, no tracing, no panic.
pub fn serving_at(
    hours: &[OpeningHoursSlot],
    timezone: Option<&TimeZone>,
    cutoff: Option<&TimeOfDay>,
    now: DateTime<Utc>,
    horizon: Duration,
) -> ServiceWindow {
    // `[]` IS HOURS_UNDECLARED by definition (never "closed") — storage collapses "never
    // provided", "cleared" and "unparseable" into this one value.
    if hours.is_empty() {
        return undeclared(now, horizon);
    }
    // NULL timezone, or one that does not parse while hours are declared: HOURS_UNDECLARED —
    // there is no materialized fallback clock to judge the declared hours against.
    let Some(tz) = timezone.and_then(|t| t.0.parse::<Tz>().ok()) else {
        return undeclared(now, horizon);
    };
    // A slot the grammar cannot read makes the whole declaration unusable — same bucket as
    // unparseable storage, so the verdict degrades to HOURS_UNDECLARED rather than judging
    // against half a schedule (and never panics).
    let mut slots: Vec<Slot> = Vec::with_capacity(hours.len());
    for s in hours {
        let (Some(from_min), Some(to_min)) = (minutes_of(&s.from), minutes_of(&s.to)) else {
            return undeclared(now, horizon);
        };
        slots.push(Slot { weekday: weekday_index(s.weekday), from_min, to_min });
    }
    // The cutoff is a subsidiary refinement of an OPEN window, not a second schedule: an
    // unparseable cutoff degrades to door-close (the same explicit degradation as an undeclared
    // one) rather than collapsing the whole verdict.
    let cutoff_min = cutoff.and_then(minutes_of);

    // UTC-now → the restaurant's LOCAL wall clock. This is the ONE direction the evaluation
    // converts (total, DST-safe); slot times stay wall times.
    let local = now.with_timezone(&tz);
    let wall_secs = local.num_seconds_from_midnight();
    let today = local.date_naive();
    let wd = local.weekday().num_days_from_monday() as u8;
    let yesterday_wd = (wd + 6) % 7;

    // Effective close of a slot in minutes: min(door-close, cutoff) — the cutoff refines only
    // when it falls INSIDE the window it refines (a cutoff before the door even opens, or on the
    // evening leg of an overnight window it does not reach, is not a narrower close; it is
    // noise, and noise degrades to door-close, explicitly).
    let effective_close = |slot: &Slot, overnight_morning_leg: bool| -> u32 {
        match cutoff_min {
            Some(c) if !overnight_morning_leg && slot.from_min < slot.to_min => {
                // Same-day window: the cutoff refines iff from ≤ cutoff ≤ to.
                if slot.from_min <= c && c <= slot.to_min { c.min(slot.to_min) } else { slot.to_min }
            }
            Some(c) if overnight_morning_leg => {
                // Morning leg of an overnight window (00:00..=to): cutoff refines iff ≤ to.
                if c <= slot.to_min { c } else { slot.to_min }
            }
            _ => slot.to_min,
        }
    };

    // Is the local wall instant covered by any slot? Collect every covering window as
    // (close date+minutes, start date+minutes) and keep the LATEST close (overlapping slots
    // merge implicitly; the evidence window is the one whose close governs). Boundary: INCLUSIVE
    // at the close — at exactly `lastOrderAt` the order is still placeable ("the latest instant
    // an order can still be placed").
    type Wall = (chrono::NaiveDate, u32);
    let mut best: Option<(Wall, Wall)> = None;
    let mut consider = |close: Wall, start: Wall| {
        if best.map(|(b, _)| close > b).unwrap_or(true) {
            best = Some((close, start));
        }
    };
    for slot in &slots {
        if slot.from_min < slot.to_min {
            // Same-day window on `slot.weekday`.
            if slot.weekday == wd {
                let close = effective_close(slot, false);
                if slot.from_min * 60 <= wall_secs && wall_secs <= close * 60 {
                    consider((today, close), (today, slot.from_min));
                }
            }
        } else if slot.from_min > slot.to_min {
            // Overnight window: evening leg on `slot.weekday`, morning leg on the NEXT weekday.
            if slot.weekday == wd && wall_secs >= slot.from_min * 60 {
                // Open since this evening; closes TOMORROW at `to` (or the morning cutoff).
                consider(
                    (today + Duration::days(1), effective_close(slot, true)),
                    (today, slot.from_min),
                );
            }
            if slot.weekday == yesterday_wd {
                // The previous day's overnight slot reaching into this morning.
                let close = effective_close(slot, true);
                if wall_secs <= close * 60 {
                    consider((today, close), (today - Duration::days(1), slot.from_min));
                }
            }
        }
        // from == to: a zero-length window covers nothing (and is not a 24h day — a full day is
        // two slots or an explicit 00:00–23:59).
    }

    if let Some(((close_date, close_min), (start_date, start_min))) = best {
        // OPEN. The door-close wall time → the UTC instant the guard will enforce. Fold-back
        // (the wall repeats the closing hour): the LATEST occurrence — under wall-clock
        // semantics the window still covers during the repeat.
        let last_order_at = close_date
            .and_hms_opt(close_min / 60, close_min % 60, 0)
            .and_then(|n| resolve_local(n, tz, true));
        // The evidence window's start: the first occurrence of its opening wall time.
        let window_from = start_date
            .and_hms_opt(start_min / 60, start_min % 60, 0)
            .and_then(|n| resolve_local(n, tz, false));
        // Next transition = the close; validUntil never promises past the horizon.
        let valid_until = last_order_at.map(|c| c.min(now + horizon)).unwrap_or(now + horizon);
        return ServiceWindow {
            verdict: ServiceWindowVerdict::OPEN,
            opens_at: None,
            window_from,
            last_order_at,
            evaluated_at: now,
            valid_until,
        };
    }

    // OUTSIDE_HOURS: find the next opening instant within the coming week (a non-empty parseable
    // schedule always has one — 8 day-offsets cover the wrap).
    let mut opens_at: Option<DateTime<Utc>> = None;
    for offset in 0..=7i64 {
        let date = today + Duration::days(offset);
        let date_wd = ((wd as i64 + offset) % 7) as u8;
        for slot in &slots {
            if slot.weekday != date_wd || slot.from_min == slot.to_min {
                continue;
            }
            let candidate = date
                .and_hms_opt(slot.from_min / 60, slot.from_min % 60, 0)
                // An OPENING resolves to the EARLIEST occurrence of its wall time.
                .and_then(|n| resolve_local(n, tz, false));
            if let Some(c) = candidate {
                if c > now && opens_at.map(|best| c < best).unwrap_or(true) {
                    opens_at = Some(c);
                }
            }
        }
    }
    // Next transition = the next opening; validUntil never promises past the horizon.
    let valid_until = opens_at.map(|o| o.min(now + horizon)).unwrap_or(now + horizon);
    ServiceWindow {
        verdict: ServiceWindowVerdict::OUTSIDE_HOURS,
        opens_at,
        window_from: None,
        last_order_at: None,
        evaluated_at: now,
        valid_until,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(weekday: Weekday, from: &str, to: &str) -> OpeningHoursSlot {
        OpeningHoursSlot {
            weekday,
            from: TimeOfDay(from.into()),
            to: TimeOfDay(to.into()),
        }
    }

    fn paris() -> TimeZone {
        TimeZone("Europe/Paris".into())
    }

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().expect("test instant parses")
    }

    const HORIZON: Duration = Duration::seconds(900);

    /// (1) Inside a declared slot the verdict is OPEN — and the shape follows it: no opensAt
    /// (already open), lastOrderAt = the door-close instant.
    #[test]
    fn open_inside_a_declared_slot() {
        // Friday 2026-08-14, 19:00–23:00 local; 18:00Z = 20:00 Europe/Paris (CEST, UTC+2).
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        let w = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T18:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::OPEN);
        assert_eq!(w.opens_at, None, "already open: nothing to promise");
        // Evidence window: 19:00–23:00 local = 17:00Z–21:00Z.
        assert_eq!(w.window_from, Some(utc("2026-08-14T17:00:00Z")));
        assert_eq!(w.last_order_at, Some(utc("2026-08-14T21:00:00Z")));
        assert_eq!(w.evaluated_at, utc("2026-08-14T18:00:00Z"));
    }

    /// (2) Outside every slot the verdict is OUTSIDE_HOURS, with the next opening promised.
    #[test]
    fn outside_hours_outside_every_slot() {
        // 02:00Z = 04:00 Europe/Paris on Friday — the issue's own 04:00 order.
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        let w = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T02:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::OUTSIDE_HOURS);
        // Next opening: Friday 19:00 local = 17:00Z.
        assert_eq!(w.opens_at, Some(utc("2026-08-14T17:00:00Z")));
        assert_eq!(w.last_order_at, None, "nothing enforceable while closed");
    }

    /// (3) `[]` IS HOURS_UNDECLARED by definition — never "closed".
    #[test]
    fn empty_hours_are_hours_undeclared() {
        let w = serving_at(&[], Some(&paris()), None, utc("2026-08-14T18:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::HOURS_UNDECLARED);
        assert_eq!(w.opens_at, None);
        assert_eq!(w.last_order_at, None);
        assert_eq!(w.valid_until, utc("2026-08-14T18:15:00Z"), "no transition: horizon only");
    }

    /// (4) A NULL timezone with DECLARED hours is HOURS_UNDECLARED too — a different input than
    /// (3), same verdict: there is no materialized fallback clock to judge the hours against.
    #[test]
    fn null_timezone_with_declared_hours_is_hours_undeclared() {
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        let w = serving_at(&hours, None, None, utc("2026-08-14T18:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::HOURS_UNDECLARED);
    }

    /// (5) An UNPARSEABLE timezone with declared hours: HOURS_UNDECLARED, and above all NO PANIC
    /// — the `.expect` pattern one line above the emitter call site (PAN-1) is what this test
    /// forbids here.
    #[test]
    fn unparseable_timezone_with_declared_hours_is_hours_undeclared_not_a_panic() {
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        let bad = TimeZone("Europe/NotACity".into());
        let w = serving_at(&hours, Some(&bad), None, utc("2026-08-14T18:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::HOURS_UNDECLARED);
    }

    /// (6) DST fold-back (MANDATORY): Europe/Paris leaves summer time on Sun 2026-10-25 —
    /// 03:00 CEST folds back to 02:00 CET at 01:00 UTC. A slot ending 02:30 local, evaluated at
    /// 01:15 UTC: chrono-tz reads the wall as 02:15 CET → OPEN. The `FixedOffset::east_opt(7200)`
    /// mutation reads 03:15 → OUTSIDE_HOURS, so this test is the one that catches a fixed-offset
    /// "timezone" — if it stays green under that mutation the fixture is lying.
    #[test]
    fn dst_fold_back_night_evaluates_on_the_wall_clock_not_a_fixed_offset() {
        // Saturday evening slot crossing midnight into Sunday morning, closing 02:30.
        let hours = [slot(Weekday::SATURDAY, "19:00", "02:30")];
        let w = serving_at(&hours, Some(&paris()), None, utc("2026-10-25T01:15:00Z"), HORIZON);
        assert_eq!(
            w.verdict,
            ServiceWindowVerdict::OPEN,
            "01:15 UTC on the fold-back night is 02:15 LOCAL — inside the window; a fixed +2h \
             offset would read 03:15 and wrongly refuse"
        );
        // The close is the LATEST occurrence of the 02:30 wall time (CET, UTC+1): 01:30 UTC.
        assert_eq!(w.last_order_at, Some(utc("2026-10-25T01:30:00Z")));
    }

    /// (7) A midnight-spanning slot (18:00–02:00) covers an early-morning instant of the NEXT
    /// day — the previous day's slots must be consulted.
    #[test]
    fn midnight_spanning_slot_covers_the_early_morning_of_the_next_day() {
        // Friday 18:00 → Saturday 02:00. Saturday 01:00 local (summer: 23:00Z Friday).
        let hours = [slot(Weekday::FRIDAY, "18:00", "02:00")];
        let w = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T23:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::OPEN);
        // Close: Saturday 02:00 local = Saturday 00:00Z.
        assert_eq!(w.last_order_at, Some(utc("2026-08-15T00:00:00Z")));
    }

    /// (8a) lastOrderAt = min(door-close, cutoff), CUTOFF winning: a 22:30 cutoff inside a
    /// 19:00–23:00 window is the enforced deadline.
    #[test]
    fn last_order_at_is_the_cutoff_when_the_cutoff_is_earlier_than_door_close() {
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        let cutoff = TimeOfDay("22:30".into());
        let w = serving_at(&hours, Some(&paris()), Some(&cutoff), utc("2026-08-14T18:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::OPEN);
        // 22:30 local = 20:30Z — the cutoff, not the 21:00Z door-close.
        assert_eq!(w.last_order_at, Some(utc("2026-08-14T20:30:00Z")));
    }

    /// (8b) …and DOOR-CLOSE winning: a cutoff after the door closes cannot extend the window —
    /// without this fixture, `min` mutated to "always cutoff" would still pass (8a) vacuously.
    #[test]
    fn last_order_at_is_the_door_close_when_the_cutoff_is_later() {
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        let cutoff = TimeOfDay("23:45".into());
        let w = serving_at(&hours, Some(&paris()), Some(&cutoff), utc("2026-08-14T18:00:00Z"), HORIZON);
        assert_eq!(w.verdict, ServiceWindowVerdict::OPEN);
        // Door-close 23:00 local = 21:00Z — the cutoff does not reach past it.
        assert_eq!(w.last_order_at, Some(utc("2026-08-14T21:00:00Z")));
    }

    /// (9) The boundary, decided and named: AT exactly lastOrderAt the order is STILL placeable
    /// (inclusive) — "the latest instant an order can still be placed" means what it says. One
    /// second later is OUTSIDE_HOURS.
    #[test]
    fn at_last_order_at_exactly_the_order_is_still_placeable_inclusive_boundary() {
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        // Exactly 23:00 local = 21:00:00Z.
        let at_close = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T21:00:00Z"), HORIZON);
        assert_eq!(at_close.verdict, ServiceWindowVerdict::OPEN, "inclusive at the boundary");
        assert_eq!(at_close.last_order_at, Some(utc("2026-08-14T21:00:00Z")));
        // One second past the boundary: refused.
        let past = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T21:00:01Z"), HORIZON);
        assert_eq!(past.verdict, ServiceWindowVerdict::OUTSIDE_HOURS);
    }

    /// (10) validUntil = min(next transition, evaluatedAt + horizon): a window edge INSIDE the
    /// horizon caps it — the "always evaluatedAt + horizon" mutation must fail here.
    #[test]
    fn valid_until_is_capped_by_a_window_edge_inside_the_horizon() {
        let hours = [slot(Weekday::FRIDAY, "19:00", "23:00")];
        // OUTSIDE_HOURS at 16:55Z, opening 17:00Z — five minutes away, inside the 15-min horizon.
        let closed = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T16:55:00Z"), HORIZON);
        assert_eq!(closed.verdict, ServiceWindowVerdict::OUTSIDE_HOURS);
        assert_eq!(closed.valid_until, utc("2026-08-14T17:00:00Z"), "the transition, not the horizon");
        // OPEN at 20:50:30Z, closing 21:00Z — the close caps it symmetrically.
        let open = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T20:50:30Z"), HORIZON);
        assert_eq!(open.verdict, ServiceWindowVerdict::OPEN);
        assert_eq!(open.valid_until, utc("2026-08-14T21:00:00Z"));
        // And far from any edge, the horizon holds: 18:00Z + 900s.
        let mid = serving_at(&hours, Some(&paris()), None, utc("2026-08-14T18:00:00Z"), HORIZON);
        assert_eq!(mid.valid_until, utc("2026-08-14T18:15:00Z"));
    }
}
