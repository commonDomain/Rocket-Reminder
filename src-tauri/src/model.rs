use chrono::{DateTime, Days, Duration, Local, LocalResult, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeatType {
    None,
    Daily,
    Weekly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReminderStatus {
    Pending,
    Running,
    Completed,
    Missed,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("事项内容不能为空")]
    EmptyContent,
    #[error("时间格式无效，请使用 HH:mm")]
    InvalidTime,
    #[error("提醒时间已经过去")]
    TimeAlreadyPassed,
    #[error("该时间在当前时区不存在，请选择其他时间")]
    NonexistentLocalTime,
    #[error("循环间隔必须大于 0 分钟")]
    InvalidLoopInterval,
    #[error("循环结束时间必须晚于开始时间")]
    InvalidLoopWindow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopConfig {
    pub interval_minutes: i64,
    pub start_time: String,
    pub end_time: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reminder {
    pub id: i64,
    pub content: String,
    pub scheduled_at: DateTime<Local>,
    pub repeat_type: RepeatType,
    pub status: ReminderStatus,
    pub created_at: DateTime<Local>,
    pub completed_at: Option<DateTime<Local>>,
    pub source_reminder_id: Option<i64>,
    pub loop_enabled: bool,
    pub loop_interval_minutes: Option<i64>,
    pub loop_start_time: Option<String>,
    pub loop_end_time: Option<String>,
}

pub fn validate_content(content: &str) -> Result<(), ValidationError> {
    if content.trim().is_empty() {
        Err(ValidationError::EmptyContent)
    } else {
        Ok(())
    }
}

pub fn schedule_today<Tz: TimeZone>(
    input: &str,
    now: DateTime<Tz>,
) -> Result<DateTime<Tz>, ValidationError> {
    if !is_strict_hh_mm(input) {
        return Err(ValidationError::InvalidTime);
    }

    let time =
        NaiveTime::parse_from_str(input, "%H:%M").map_err(|_| ValidationError::InvalidTime)?;
    match now
        .timezone()
        .from_local_datetime(&now.date_naive().and_time(time))
    {
        LocalResult::None => Err(ValidationError::NonexistentLocalTime),
        LocalResult::Single(scheduled) => future_or_passed(scheduled, &now),
        LocalResult::Ambiguous(first, second) => {
            let (earlier, later) = ordered_occurrences(first, second);
            if earlier > now {
                Ok(earlier)
            } else {
                future_or_passed(later, &now)
            }
        }
    }
}

pub fn schedule_with_repeat<Tz: TimeZone>(
    input: &str,
    repeat: RepeatType,
    now: DateTime<Tz>,
) -> Result<DateTime<Tz>, ValidationError> {
    match schedule_today(input, now.clone()) {
        Ok(scheduled) => Ok(scheduled),
        Err(ValidationError::TimeAlreadyPassed) if repeat != RepeatType::None => {
            let time = parse_hh_mm(input)?;
            let interval_days = match repeat {
                RepeatType::Daily => 1,
                RepeatType::Weekly => 7,
                RepeatType::None => unreachable!(),
            };
            let timezone = now.timezone();
            let mut date = now.date_naive();
            loop {
                date = date
                    .checked_add_days(Days::new(interval_days))
                    .ok_or(ValidationError::TimeAlreadyPassed)?;
                match timezone.from_local_datetime(&date.and_time(time)) {
                    LocalResult::None => {}
                    LocalResult::Single(next) => return Ok(next),
                    LocalResult::Ambiguous(first, second) => {
                        return Ok(ordered_occurrences(first, second).0);
                    }
                }
            }
        }
        Err(error) => Err(error),
    }
}

pub fn next_occurrence<Tz: TimeZone>(
    current: DateTime<Tz>,
    repeat: RepeatType,
) -> Option<DateTime<Tz>> {
    let interval_days = match repeat {
        RepeatType::None => return None,
        RepeatType::Daily => 1,
        RepeatType::Weekly => 7,
    };
    let timezone = current.timezone();
    let time = current.time();
    let mut date = current.date_naive();

    loop {
        date = date.checked_add_days(Days::new(interval_days))?;
        match timezone.from_local_datetime(&date.and_time(time)) {
            LocalResult::None => {}
            LocalResult::Single(next) => return Some(next),
            LocalResult::Ambiguous(first, second) => {
                return Some(ordered_occurrences(first, second).0);
            }
        }
    }
}

pub fn trigger_at(reminder: &Reminder) -> DateTime<Local> {
    if reminder.loop_enabled {
        return reminder.scheduled_at;
    }
    reminder
        .scheduled_at
        .checked_sub_signed(Duration::minutes(5))
        .unwrap_or(reminder.scheduled_at)
}

pub fn validate_loop_config(config: &LoopConfig) -> Result<(), ValidationError> {
    if config.interval_minutes <= 0 {
        return Err(ValidationError::InvalidLoopInterval);
    }
    let start = parse_hh_mm(&config.start_time)?;
    let end = parse_hh_mm(&config.end_time)?;
    if end <= start {
        return Err(ValidationError::InvalidLoopWindow);
    }
    Ok(())
}

pub fn initial_loop_occurrence(
    config: &LoopConfig,
    repeat: RepeatType,
    now: DateTime<Local>,
) -> Result<DateTime<Local>, ValidationError> {
    validate_loop_config(config)?;
    let start = parse_hh_mm(&config.start_time)?;
    let end = parse_hh_mm(&config.end_time)?;
    let mut date = now.date_naive();

    loop {
        if let Some(window_start) = resolve_local(date, start) {
            let mut scheduled = window_start;
            while scheduled <= now {
                scheduled = scheduled
                    .checked_add_signed(Duration::minutes(config.interval_minutes))
                    .ok_or(ValidationError::TimeAlreadyPassed)?;
            }
            if scheduled.date_naive() == date && scheduled.time() <= end {
                return Ok(scheduled);
            }
        }
        date = next_repeat_date(date, repeat).ok_or(ValidationError::TimeAlreadyPassed)?;
    }
}

pub fn next_loop_occurrence(
    reminder: &Reminder,
    after: DateTime<Local>,
) -> Option<DateTime<Local>> {
    let config = LoopConfig {
        interval_minutes: reminder.loop_interval_minutes?,
        start_time: reminder.loop_start_time.clone()?,
        end_time: reminder.loop_end_time.clone()?,
    };
    validate_loop_config(&config).ok()?;
    let start = parse_hh_mm(&config.start_time).ok()?;
    let end = parse_hh_mm(&config.end_time).ok()?;
    let mut scheduled = reminder.scheduled_at;
    let mut date = scheduled.date_naive();

    loop {
        scheduled = scheduled.checked_add_signed(Duration::minutes(config.interval_minutes))?;
        while scheduled <= after && scheduled.date_naive() == date {
            scheduled = scheduled.checked_add_signed(Duration::minutes(config.interval_minutes))?;
        }
        if scheduled.date_naive() == date && scheduled.time() <= end {
            return Some(scheduled);
        }
        date = next_repeat_date(date, reminder.repeat_type)?;
        let Some(window_start) = resolve_local(date, start) else {
            continue;
        };
        if window_start > after {
            return Some(window_start);
        }
        scheduled = window_start;
    }
}

fn parse_hh_mm(input: &str) -> Result<NaiveTime, ValidationError> {
    if !is_strict_hh_mm(input) {
        return Err(ValidationError::InvalidTime);
    }
    NaiveTime::parse_from_str(input, "%H:%M").map_err(|_| ValidationError::InvalidTime)
}

fn resolve_local(date: chrono::NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    match Local.from_local_datetime(&date.and_time(time)) {
        LocalResult::None => None,
        LocalResult::Single(value) => Some(value),
        LocalResult::Ambiguous(first, second) => Some(ordered_occurrences(first, second).0),
    }
}

fn next_repeat_date(date: chrono::NaiveDate, repeat: RepeatType) -> Option<chrono::NaiveDate> {
    let days = match repeat {
        RepeatType::None => return None,
        RepeatType::Daily => 1,
        RepeatType::Weekly => 7,
    };
    date.checked_add_days(Days::new(days))
}

fn future_or_passed<Tz: TimeZone>(
    candidate: DateTime<Tz>,
    now: &DateTime<Tz>,
) -> Result<DateTime<Tz>, ValidationError> {
    if candidate > *now {
        Ok(candidate)
    } else {
        Err(ValidationError::TimeAlreadyPassed)
    }
}

fn ordered_occurrences<Tz: TimeZone>(
    first: DateTime<Tz>,
    second: DateTime<Tz>,
) -> (DateTime<Tz>, DateTime<Tz>) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn is_strict_hh_mm(input: &str) -> bool {
    let bytes = input.as_bytes();
    bytes.len() == 5
        && bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2] == b':'
        && bytes[3].is_ascii_digit()
        && bytes[4].is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Local, NaiveDateTime, TimeZone};
    use chrono_tz::America::New_York;

    fn local_dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, minute, 0)
            .single()
            .unwrap()
    }

    fn reminder(scheduled_at: DateTime<Local>) -> Reminder {
        Reminder {
            id: 7,
            content: "Prepare launch".into(),
            scheduled_at,
            repeat_type: RepeatType::Daily,
            status: ReminderStatus::Pending,
            created_at: local_dt(2026, 6, 1, 8, 0),
            completed_at: None,
            source_reminder_id: None,
            loop_enabled: false,
            loop_interval_minutes: None,
            loop_start_time: None,
            loop_end_time: None,
        }
    }

    fn fixed_local_dt(input: &str) -> DateTime<Local> {
        let parsed = DateTime::parse_from_rfc3339(input).unwrap();
        DateTime::<Local>::from_naive_utc_and_offset(parsed.naive_utc(), *parsed.offset())
    }

    #[test]
    fn rejects_a_time_that_already_passed_today() {
        let now = local_dt(2026, 6, 1, 14, 0);
        assert_eq!(
            schedule_today("09:30", now).unwrap_err(),
            ValidationError::TimeAlreadyPassed
        );
    }

    #[test]
    fn keeps_a_future_time_even_inside_the_five_minute_window() {
        let now = local_dt(2026, 6, 1, 9, 28);
        let scheduled = schedule_today("09:30", now).unwrap();
        assert_eq!(scheduled, local_dt(2026, 6, 1, 9, 30));
    }

    #[test]
    fn rejects_non_strict_time_formats() {
        for input in ["9:30", "09:30:00", "09:3", " 09:30", "24:00"] {
            assert_eq!(
                schedule_today(input, local_dt(2026, 6, 1, 8, 0)),
                Err(ValidationError::InvalidTime),
                "{input:?} should be invalid"
            );
        }
    }

    #[test]
    fn rejects_blank_content() {
        assert_eq!(
            validate_content(" \t\r\n"),
            Err(ValidationError::EmptyContent)
        );
    }

    #[test]
    fn daily_occurrence_uses_the_next_date_and_same_time() {
        let scheduled = local_dt(2026, 6, 1, 9, 30);
        assert_eq!(
            next_occurrence(scheduled, RepeatType::Daily),
            Some(local_dt(2026, 6, 2, 9, 30))
        );
    }

    #[test]
    fn weekly_occurrence_uses_the_same_weekday_and_time() {
        let scheduled = local_dt(2026, 6, 1, 9, 30);
        assert_eq!(
            next_occurrence(scheduled, RepeatType::Weekly),
            Some(local_dt(2026, 6, 8, 9, 30))
        );
    }

    #[test]
    fn daily_occurrence_skips_a_nonexistent_spring_forward_time() {
        let scheduled = New_York
            .with_ymd_and_hms(2026, 3, 7, 2, 30, 0)
            .single()
            .unwrap();
        assert_eq!(
            next_occurrence(scheduled, RepeatType::Daily),
            New_York.with_ymd_and_hms(2026, 3, 9, 2, 30, 0).single()
        );
    }

    #[test]
    fn weekly_occurrence_keeps_stepping_after_a_nonexistent_time() {
        let scheduled = New_York
            .with_ymd_and_hms(2026, 3, 1, 2, 30, 0)
            .single()
            .unwrap();
        assert_eq!(
            next_occurrence(scheduled, RepeatType::Weekly),
            New_York.with_ymd_and_hms(2026, 3, 15, 2, 30, 0).single()
        );
    }

    #[test]
    fn recurring_ambiguous_time_uses_the_earlier_occurrence() {
        let scheduled = New_York
            .with_ymd_and_hms(2026, 10, 31, 1, 30, 0)
            .single()
            .unwrap();
        assert_eq!(
            next_occurrence(scheduled, RepeatType::Daily),
            New_York.with_ymd_and_hms(2026, 11, 1, 1, 30, 0).earliest()
        );
    }

    #[test]
    fn non_repeating_reminder_has_no_next_occurrence() {
        assert_eq!(
            next_occurrence(local_dt(2026, 6, 1, 9, 30), RepeatType::None),
            None
        );
    }

    #[test]
    fn trigger_is_five_minutes_before_the_scheduled_time() {
        let reminder = reminder(local_dt(2026, 6, 1, 9, 30));
        assert_eq!(trigger_at(&reminder), local_dt(2026, 6, 1, 9, 25));
    }

    #[test]
    fn trigger_falls_back_to_the_scheduled_time_on_underflow() {
        let minimum = DateTime::<Local>::from_naive_utc_and_offset(
            NaiveDateTime::MIN,
            FixedOffset::east_opt(0).unwrap(),
        );
        let reminder = reminder(minimum);
        assert_eq!(trigger_at(&reminder), reminder.scheduled_at);
    }

    #[test]
    fn schedule_today_rejects_a_nonexistent_local_time() {
        let now = New_York
            .with_ymd_and_hms(2026, 3, 8, 0, 30, 0)
            .single()
            .unwrap();
        assert_eq!(
            schedule_today("02:30", now),
            Err(ValidationError::NonexistentLocalTime)
        );
        assert_eq!(
            ValidationError::NonexistentLocalTime.to_string(),
            "该时间在当前时区不存在，请选择其他时间"
        );
    }

    #[test]
    fn schedule_today_ambiguous_time_uses_the_earlier_future_occurrence() {
        let now = New_York
            .with_ymd_and_hms(2026, 11, 1, 0, 30, 0)
            .single()
            .unwrap();
        assert_eq!(
            schedule_today("01:30", now),
            Ok(New_York
                .with_ymd_and_hms(2026, 11, 1, 1, 30, 0)
                .earliest()
                .unwrap())
        );
    }

    #[test]
    fn schedule_today_ambiguous_time_uses_the_later_occurrence_if_needed() {
        let now = New_York
            .with_ymd_and_hms(2026, 11, 1, 1, 45, 0)
            .earliest()
            .unwrap();
        assert_eq!(
            schedule_today("01:30", now),
            Ok(New_York
                .with_ymd_and_hms(2026, 11, 1, 1, 30, 0)
                .latest()
                .unwrap())
        );
    }

    #[test]
    fn reminder_serializes_to_the_expected_wire_shape() {
        let reminder = Reminder {
            id: 7,
            content: "Prepare launch".into(),
            scheduled_at: fixed_local_dt("2026-06-01T09:30:00+08:00"),
            repeat_type: RepeatType::Daily,
            status: ReminderStatus::Pending,
            created_at: fixed_local_dt("2026-06-01T08:00:00+08:00"),
            completed_at: None,
            source_reminder_id: None,
            loop_enabled: false,
            loop_interval_minutes: None,
            loop_start_time: None,
            loop_end_time: None,
        };
        assert_eq!(
            serde_json::to_value(reminder).unwrap(),
            serde_json::json!({
                "id": 7,
                "content": "Prepare launch",
                "scheduled_at": "2026-06-01T09:30:00+08:00",
                "repeat_type": "daily",
                "status": "pending",
                "created_at": "2026-06-01T08:00:00+08:00",
                "completed_at": null,
                "source_reminder_id": null,
                "loop_enabled": false,
                "loop_interval_minutes": null,
                "loop_start_time": null,
                "loop_end_time": null
            })
        );
    }
}
