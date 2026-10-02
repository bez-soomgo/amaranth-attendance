use crate::attendance::{
    model::{Summary, TodayPlan},
    time::fmt_clock,
};

pub fn plan_today(summary: &Summary, arrival_min: i32, bonus: bool) -> TodayPlan {
    let needed_min = (summary.work_days as i32 + 1) * 480 - summary.total_min;
    let work_needed_min = needed_min - if bonus { 240 } else { 0 };
    let recognized_min = arrival_min.max(480);
    let already_met = work_needed_min <= 0;
    let mut leave_min = recognized_min + work_needed_min;
    let break_applied = !already_met && leave_min > 840;
    if break_applied {
        leave_min += 60;
    }
    TodayPlan {
        needed_min,
        work_needed_min,
        recognized_in: fmt_clock(recognized_min),
        leave_at: (!already_met).then(|| fmt_clock(leave_min)),
        break_applied,
        already_met,
        half_day_notice: bonus,
    }
}

#[cfg(test)]
mod tests {
    use super::plan_today;
    use crate::attendance::model::Summary;

    fn summary(total_min: i32) -> Summary {
        Summary {
            work_days: 20,
            actual_days: 18,
            leave_days: 2,
            target_min: 9_600,
            total_min,
            diff_min: total_min - 9_600,
        }
    }

    #[test]
    fn plans_departure_with_break_and_half_day_credit() {
        let normal = plan_today(&summary(9_678), 510, false);
        assert_eq!(normal.leave_at.as_deref(), Some("16:12"));
        assert!(normal.break_applied);

        let half_day = plan_today(&summary(9_678), 510, true);
        assert_eq!(half_day.leave_at.as_deref(), Some("11:12"));
        assert!(!half_day.break_applied);
        assert!(half_day.half_day_notice);
    }

    #[test]
    fn recognizes_early_arrival_and_skips_target_when_already_met() {
        let early = plan_today(&summary(9_678), 460, false);
        assert_eq!(early.recognized_in, "08:00");
        assert_eq!(early.leave_at.as_deref(), Some("15:42"));

        let met = plan_today(&summary(10_200), 510, false);
        assert!(met.already_met);
        assert_eq!(met.leave_at, None);
    }
}
