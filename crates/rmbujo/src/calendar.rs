//! Calendar data: year → months → days, with weekday labels and week grouping.

use chrono::{Datelike, NaiveDate, Weekday};

pub const MONTH_NAMES: [&str; 13] = [
    "",
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub day: u32,
    pub weekday: &'static str,
    pub week_start: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Month {
    pub year: i32,
    pub month: u32,
    pub name: &'static str,
    pub days: Vec<Day>,
}

fn weekday_abbr(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "Mon",
        Weekday::Tue => "Tue",
        Weekday::Wed => "Wed",
        Weekday::Thu => "Thu",
        Weekday::Fri => "Fri",
        Weekday::Sat => "Sat",
        Weekday::Sun => "Sun",
    }
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    NaiveDate::from_ymd_opt(ny, nm, 1)
        .unwrap()
        .pred_opt()
        .unwrap()
        .day()
}

fn week_start_weekday(week_start: &str) -> anyhow::Result<Weekday> {
    match week_start {
        "sun" => Ok(Weekday::Sun),
        "mon" => Ok(Weekday::Mon),
        other => anyhow::bail!("week_start must be 'sun' or 'mon', got {other:?}"),
    }
}

pub fn build_month(year: i32, month: u32, week_start: &str) -> anyhow::Result<Month> {
    let ws = week_start_weekday(week_start)?;
    let n = days_in_month(year, month);
    let mut days = Vec::with_capacity(n as usize);
    for d in 1..=n {
        let date = NaiveDate::from_ymd_opt(year, month, d).unwrap();
        let wd = date.weekday();
        days.push(Day {
            day: d,
            weekday: weekday_abbr(wd),
            week_start: d != 1 && wd == ws,
        });
    }
    Ok(Month {
        year,
        month,
        name: MONTH_NAMES[month as usize],
        days,
    })
}

pub fn build_year(year: i32, week_start: &str) -> anyhow::Result<Vec<Month>> {
    (1..=12).map(|m| build_month(year, m, week_start)).collect()
}
