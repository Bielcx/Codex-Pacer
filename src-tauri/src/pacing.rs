//! Pace calculation: compares actual usage against a straight-line target
//! trajectory from the start of the current rate-limit window to its reset,
//! ending at a configurable safety buffer instead of zero.
//!
//! Example: a 24h window, 3% safety buffer, halfway through the window.
//! The target trajectory says remaining % should have dropped from 100 to
//! about 51.5 by now (linear path from 100 -> 3 over the full window). If
//! the actual remaining % is well below that, usage is running hot
//! (`SlowDown`); well above, there's unused budget (`RoomToUseMore`);
//! close to it, `OnTrack`.

use crate::history::UsageSample;
use chrono::{DateTime, Utc};

pub const DEFAULT_SAFETY_BUFFER_PERCENT: f32 = 3.0;
pub const DEFAULT_TOLERANCE_PERCENT: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    SlowDown,
    OnTrack,
    RoomToUseMore,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct PacingReport {
    pub verdict: Verdict,
    /// Where the linear target trajectory says remaining % should be right now.
    pub target_remaining_percent: f32,
    /// Max percent/hour still burnable between now and reset without dipping
    /// below the safety buffer, assuming remaining_percent as the starting point.
    pub safe_burn_rate_per_hour: f32,
    /// Percent/hour actually burned recently, computed from history samples
    /// within the current window. `None` when there's not enough history yet
    /// (e.g. right after a window starts, or on first run).
    pub recent_burn_rate_per_hour: Option<f32>,
}

/// Core pacing calculation. Pure function of its inputs (no I/O) so it's
/// easy to unit test; callers are responsible for fetching `history` and
/// figuring out `window_start` (typically `reset_at - window_duration`).
#[allow(clippy::too_many_arguments)]
pub fn evaluate(
    remaining_percent: f32,
    window_start: DateTime<Utc>,
    reset_at: DateTime<Utc>,
    now: DateTime<Utc>,
    safety_buffer_percent: f32,
    tolerance_percent: f32,
    history: &[UsageSample],
) -> PacingReport {
    let target_remaining_percent =
        target_remaining_at(window_start, reset_at, now, safety_buffer_percent);

    let verdict = if remaining_percent < target_remaining_percent - tolerance_percent {
        Verdict::SlowDown
    } else if remaining_percent > target_remaining_percent + tolerance_percent {
        Verdict::RoomToUseMore
    } else {
        Verdict::OnTrack
    };

    let hours_left = ((reset_at - now).num_seconds().max(0) as f32) / 3600.0;
    let safe_burn_rate_per_hour = if hours_left > 0.0 {
        (remaining_percent - safety_buffer_percent).max(0.0) / hours_left
    } else {
        0.0
    };

    let recent_burn_rate_per_hour = recent_burn_rate(history, window_start);

    PacingReport {
        verdict,
        target_remaining_percent,
        safe_burn_rate_per_hour,
        recent_burn_rate_per_hour,
    }
}

/// Straight-line target: 100% at `window_start`, `safety_buffer_percent` at
/// `reset_at`. Clamped so a `now` outside the window doesn't extrapolate
/// past either end.
fn target_remaining_at(
    window_start: DateTime<Utc>,
    reset_at: DateTime<Utc>,
    now: DateTime<Utc>,
    safety_buffer_percent: f32,
) -> f32 {
    let total_seconds = (reset_at - window_start).num_seconds().max(1) as f32;
    let elapsed_seconds = (now - window_start)
        .num_seconds()
        .clamp(0, total_seconds as i64) as f32;
    let progress = elapsed_seconds / total_seconds;
    100.0 - (100.0 - safety_buffer_percent) * progress
}

/// Percent/hour burned between the earliest and latest sample observed
/// within the current window. `None` when there are fewer than two such
/// samples (not enough data to derive a rate yet).
fn recent_burn_rate(history: &[UsageSample], window_start: DateTime<Utc>) -> Option<f32> {
    let mut in_window: Vec<(DateTime<Utc>, f32)> = history
        .iter()
        .filter_map(|sample| {
            let observed_at = DateTime::parse_from_rfc3339(&sample.observed_at)
                .ok()?
                .with_timezone(&Utc);
            (observed_at >= window_start).then_some((observed_at, sample.remaining_percent))
        })
        .collect();

    if in_window.len() < 2 {
        return None;
    }
    in_window.sort_by_key(|(observed_at, _)| *observed_at);

    let (first_time, first_remaining) = in_window.first().copied()?;
    let (last_time, last_remaining) = in_window.last().copied()?;

    let hours = (last_time - first_time).num_seconds() as f32 / 3600.0;
    if hours <= 0.0 {
        return None;
    }

    Some((first_remaining - last_remaining) / hours)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::UsageSample;
    use chrono::{Duration, TimeZone};

    /// `hour` counts hours since 2026-07-27T00:00:00Z rather than a literal
    /// hour-of-day, so `at(24)` etc. stay valid instead of overflowing into
    /// a nonexistent "24:00" on the same day.
    fn at(hour: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 27, 0, 0, 0).unwrap() + Duration::hours(hour)
    }

    // 24h window, buffer 3%, tolerance 5%, evaluated at the halfway point
    // (hour 12), where the target trajectory works out to 51.5% remaining.
    fn evaluate_halfway(remaining_percent: f32) -> PacingReport {
        evaluate(
            remaining_percent,
            at(0),
            at(24),
            at(12),
            DEFAULT_SAFETY_BUFFER_PERCENT,
            DEFAULT_TOLERANCE_PERCENT,
            &[],
        )
    }

    #[test]
    fn verdict_slow_down_when_well_below_target() {
        let report = evaluate_halfway(30.0);
        assert_eq!(report.verdict, Verdict::SlowDown);
    }

    #[test]
    fn verdict_on_track_when_close_to_target() {
        let report = evaluate_halfway(50.0);
        assert_eq!(report.verdict, Verdict::OnTrack);
    }

    #[test]
    fn verdict_room_to_use_more_when_well_above_target() {
        let report = evaluate_halfway(70.0);
        assert_eq!(report.verdict, Verdict::RoomToUseMore);
    }

    #[test]
    fn target_trajectory_matches_expected_midpoint() {
        let target = target_remaining_at(at(0), at(24), at(12), 3.0);
        assert!((target - 51.5).abs() < 0.01);
    }

    #[test]
    fn safe_burn_rate_is_zero_at_or_past_reset() {
        let report = evaluate(
            50.0,
            at(0),
            at(24),
            at(24),
            DEFAULT_SAFETY_BUFFER_PERCENT,
            DEFAULT_TOLERANCE_PERCENT,
            &[],
        );
        assert_eq!(report.safe_burn_rate_per_hour, 0.0);
    }

    #[test]
    fn recent_burn_rate_none_with_insufficient_samples() {
        let one_sample = [UsageSample {
            observed_at: at(1).to_rfc3339(),
            remaining_percent: 90.0,
            reset_at: at(24).to_rfc3339(),
        }];
        let report = evaluate(
            50.0,
            at(0),
            at(24),
            at(12),
            DEFAULT_SAFETY_BUFFER_PERCENT,
            DEFAULT_TOLERANCE_PERCENT,
            &one_sample,
        );
        assert!(report.recent_burn_rate_per_hour.is_none());
    }

    #[test]
    fn recent_burn_rate_computed_from_window_samples() {
        let samples = [
            UsageSample {
                observed_at: at(2).to_rfc3339(),
                remaining_percent: 90.0,
                reset_at: at(24).to_rfc3339(),
            },
            UsageSample {
                observed_at: at(6).to_rfc3339(),
                remaining_percent: 70.0,
                reset_at: at(24).to_rfc3339(),
            },
        ];
        let report = evaluate(
            65.0,
            at(0),
            at(24),
            at(12),
            DEFAULT_SAFETY_BUFFER_PERCENT,
            DEFAULT_TOLERANCE_PERCENT,
            &samples,
        );
        // 20 points burned over 4 hours = 5 points/hour.
        assert!((report.recent_burn_rate_per_hour.unwrap() - 5.0).abs() < 0.01);
    }
}
