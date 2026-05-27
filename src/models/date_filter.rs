use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone, Utc};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DateFilter {
    #[default]
    AnyTime,
    Today,
    Last7Days,
    Last30Days,
    ThisYear,
    Custom {
        from: NaiveDate,
        to: NaiveDate,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DateCounts {
    pub any_time: usize,
    pub today: usize,
    pub last_7_days: usize,
    pub last_30_days: usize,
    pub this_year: usize,
}

impl DateFilter {
    pub fn resolve(&self, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
        let local_now = now.with_timezone(&Local);
        let today = local_now.date_naive();

        let (start_date, end_date) = match self {
            Self::AnyTime => return None,
            Self::Today => (today, today.checked_add_days(Days::new(1))?),
            Self::Last7Days => (
                today.checked_sub_days(Days::new(6))?,
                today.checked_add_days(Days::new(1))?,
            ),
            Self::Last30Days => (
                today.checked_sub_days(Days::new(29))?,
                today.checked_add_days(Days::new(1))?,
            ),
            Self::ThisYear => (
                NaiveDate::from_ymd_opt(today.year(), 1, 1)?,
                NaiveDate::from_ymd_opt(today.year() + 1, 1, 1)?,
            ),
            Self::Custom { from, to } => {
                if from > to {
                    return None;
                }

                (*from, to.checked_add_days(Days::new(1))?)
            }
        };

        let start = local_midnight(start_date)?;
        let end = local_midnight(end_date)?;

        Some((start.with_timezone(&Utc), end.with_timezone(&Utc)))
    }

    pub fn pill_label(&self) -> String {
        match self {
            Self::AnyTime => String::new(),
            Self::Today => "Today".to_string(),
            Self::Last7Days => "Last 7 days".to_string(),
            Self::Last30Days => "Last 30 days".to_string(),
            Self::ThisYear => "This year".to_string(),
            Self::Custom { from, to } if from == to => format_date(*from),
            Self::Custom { from, to } => format!("{} - {}", format_date(*from), format_date(*to)),
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, Self::AnyTime)
    }
}

fn local_midnight(date: NaiveDate) -> Option<DateTime<Local>> {
    let naive = date.and_hms_opt(0, 0, 0)?;
    Local
        .from_local_datetime(&naive)
        .single()
        .or_else(|| Local.from_local_datetime(&naive).earliest())
}

fn format_date(date: NaiveDate) -> String {
    date.format("%b %-d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};

    fn utc(y: i32, m: u32, d: u32, h: u32, min: u32, s: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, min, s).single().unwrap()
    }

    #[test]
    fn any_time_resolves_to_no_window_and_empty_label() {
        let filter = DateFilter::AnyTime;

        assert_eq!(filter.resolve(utc(2026, 5, 27, 12, 0, 0)), None);
        assert_eq!(filter.pill_label(), "");
        assert!(!filter.is_active());
    }

    #[test]
    fn today_resolves_to_local_day_window() {
        let now = utc(2026, 5, 27, 12, 34, 56);
        let (start, end) = DateFilter::Today.resolve(now).unwrap();
        let local_today = now.with_timezone(&chrono::Local).date_naive();

        assert!(start <= now);
        assert!(end > now);
        assert_eq!(
            start.with_timezone(&chrono::Local).date_naive(),
            local_today
        );
        assert_eq!(
            end.with_timezone(&chrono::Local).date_naive(),
            local_today.checked_add_days(Days::new(1)).unwrap()
        );
    }

    #[test]
    fn last_7_days_includes_today_and_six_prior_days() {
        let now = utc(2026, 5, 27, 12, 0, 0);
        let (start, end) = DateFilter::Last7Days.resolve(now).unwrap();
        let local_today = now.with_timezone(&chrono::Local).date_naive();

        assert!(start <= now);
        assert!(end > now);
        assert_eq!(
            start.with_timezone(&chrono::Local).date_naive(),
            local_today.checked_sub_days(Days::new(6)).unwrap()
        );
        assert_eq!(
            end.with_timezone(&chrono::Local).date_naive(),
            local_today.checked_add_days(Days::new(1)).unwrap()
        );
        assert_eq!(DateFilter::Last7Days.pill_label(), "Last 7 days");
    }

    #[test]
    fn last_30_days_includes_today_and_twenty_nine_prior_days() {
        let now = utc(2026, 5, 27, 12, 0, 0);
        let (start, end) = DateFilter::Last30Days.resolve(now).unwrap();
        let local_today = now.with_timezone(&chrono::Local).date_naive();

        assert!(start <= now);
        assert!(end > now);
        assert_eq!(
            start.with_timezone(&chrono::Local).date_naive(),
            local_today.checked_sub_days(Days::new(29)).unwrap()
        );
        assert_eq!(
            end.with_timezone(&chrono::Local).date_naive(),
            local_today.checked_add_days(Days::new(1)).unwrap()
        );
        assert_eq!(DateFilter::Last30Days.pill_label(), "Last 30 days");
    }

    #[test]
    fn this_year_starts_on_january_first_and_ends_next_year() {
        let now = utc(2026, 12, 31, 23, 30, 0);
        let (start, end) = DateFilter::ThisYear.resolve(now).unwrap();
        let local_year = now.with_timezone(&chrono::Local).date_naive().year();

        assert_eq!(start.with_timezone(&chrono::Local).date_naive().month(), 1);
        assert_eq!(start.with_timezone(&chrono::Local).date_naive().day(), 1);
        assert_eq!(
            end.with_timezone(&chrono::Local).date_naive().year(),
            local_year + 1
        );
        assert_eq!(DateFilter::ThisYear.pill_label(), "This year");
    }

    #[test]
    fn custom_range_is_inclusive_of_both_dates() {
        let filter = DateFilter::Custom {
            from: NaiveDate::from_ymd_opt(2024, 2, 28).unwrap(),
            to: NaiveDate::from_ymd_opt(2024, 2, 29).unwrap(),
        };
        let (start, end) = filter.resolve(utc(2024, 2, 29, 12, 0, 0)).unwrap();

        assert_eq!(
            start.with_timezone(&chrono::Local).date_naive(),
            NaiveDate::from_ymd_opt(2024, 2, 28).unwrap()
        );
        assert_eq!(
            end.with_timezone(&chrono::Local).date_naive(),
            NaiveDate::from_ymd_opt(2024, 3, 1).unwrap()
        );
    }

    #[test]
    fn custom_same_day_label_uses_single_date() {
        let date = NaiveDate::from_ymd_opt(2026, 4, 5).unwrap();
        let filter = DateFilter::Custom {
            from: date,
            to: date,
        };

        assert_eq!(filter.pill_label(), format_date(date));
        assert!(filter.is_active());
    }

    #[test]
    fn custom_range_label_uses_short_month_day_range() {
        let from = NaiveDate::from_ymd_opt(2026, 4, 5).unwrap();
        let to = NaiveDate::from_ymd_opt(2026, 4, 17).unwrap();
        let filter = DateFilter::Custom { from, to };

        assert_eq!(
            filter.pill_label(),
            format!("{} - {}", format_date(from), format_date(to))
        );
    }

    #[test]
    fn custom_range_with_from_after_to_resolves_to_none() {
        let filter = DateFilter::Custom {
            from: NaiveDate::from_ymd_opt(2026, 4, 17).unwrap(),
            to: NaiveDate::from_ymd_opt(2026, 4, 5).unwrap(),
        };

        assert_eq!(filter.resolve(utc(2026, 4, 17, 12, 0, 0)), None);
    }
}
