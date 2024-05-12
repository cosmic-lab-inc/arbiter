use chrono::{DateTime, Datelike, LocalResult, NaiveDate, TimeZone, Timelike, Utc, Weekday};
use log::error;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Time {
  pub year: i32,
  pub month: Month,
  pub day: Day,
  pub hour: Option<u32>,
  pub minute: Option<u32>,
  pub second: Option<u32>,
}

impl From<DateTime<Utc>> for Time {
  fn from(dt: DateTime<Utc>) -> Self {
    Self {
      year: dt.year(),
      month: Month::from_num(dt.month()),
      day: Day::from_num(dt.day()),
      hour: Some(dt.hour()),
      minute: Some(dt.minute()),
      second: Some(dt.second()),
    }
  }
}

impl Time {
  pub fn new(
    year: i32,
    month: u32,
    day: u32,
    hour: Option<u32>,
    minute: Option<u32>,
    second: Option<u32>
  ) -> Self {
    Self {
      year,
      month: Month::from_num(month),
      day: Day::from_num(day),
      hour,
      minute,
      second
    }
  }

  pub fn from_datetime(dt: DateTime<Utc>) -> Self {
    Self {
      year: dt.year(),
      month: Month::from_num(dt.month()),
      day: Day::from_num(dt.day()),
      hour: Some(dt.hour()),
      minute: Some(dt.minute()),
      second: Some(dt.second()),
    }
  }

  /// Create vector of Time starting at self and ending at end_date
  pub fn time_period(&self, end_date: &Time) -> Vec<Time> {
    let mut time_period = Vec::new();
    let mut current_date = *self;
    while current_date <= *end_date {
      time_period.push(current_date);
      current_date = current_date.delta_date(1);
    }
    time_period
  }

  pub fn is_weekend(&self) -> bool {
    let weekday = self.to_naive_date().weekday();
    weekday == Weekday::Sat || weekday == Weekday::Sun
  }

  pub fn from_eclipse_date_format(date: &str) -> Self {
    let end_year_index = date.find(' ').unwrap();
    let year = date[..end_year_index].parse::<i32>().unwrap();
    let start_month_index = end_year_index + 1;
    let end_month_index = date[start_month_index..].find(' ').unwrap() + start_month_index;
    let month = Month::from_name(
      date[start_month_index..end_month_index]
        .parse::<String>()
        .unwrap()
        .as_str(),
    );
    let start_day_index = end_month_index + 1;
    let end_day_index = date.len();
    let day = Day::from_num(date[start_day_index..end_day_index].parse::<u32>().unwrap());

    Self {
      year,
      month,
      day,
      hour: None,
      minute: None,
      second: None
    }
  }

  pub fn from_api_format(date: &str) -> Self {
    let year = date[..4].parse::<i32>().unwrap();
    let month = Month::from_num(date[5..7].parse::<u32>().unwrap());
    let day = Day::from_num(date[8..10].parse::<u32>().unwrap());

    Self {
      year,
      month,
      day,
      hour: None,
      minute: None,
      second: None
    }
  }

  #[allow(clippy::inherent_to_string)]
  pub fn to_string(&self) -> String {
    format!(
      "{}-{}-{}.{}h.{}m.{}s",
      self.year,
      self.month.to_string(),
      self.day.to_string(),
      self.hour.unwrap_or(0),
      self.minute.unwrap_or(0),
      self.second.unwrap_or(0)
    )
  }

  pub fn to_string_daily(&self) -> String {
    format!(
      "{}-{}-{}",
      self.year,
      self.month.to_string(),
      self.day.to_string()
    )
  }

  pub fn to_naive_date(&self) -> NaiveDate {
    NaiveDate::from_ymd_opt(self.year, self.month.to_num(), self.day.to_num())
      .expect("failed to convert Time to chrono::NaiveDate")
  }

  pub fn to_datetime(&self) -> anyhow::Result<DateTime<Utc>> {
    let res = Utc.with_ymd_and_hms(
      self.year,
      self.month.to_num(),
      self.day.to_num(),
      self.hour.unwrap_or(0),
      self.minute.unwrap_or(0),
      0,
    );
    match res {
      LocalResult::None => {
        error!("self: {}", self.to_string());
        Err(anyhow::anyhow!("Invalid date: {}", self.to_string()))
      }
      LocalResult::Single(t) => Ok(t),
      LocalResult::Ambiguous(t, ..) => Ok(t),
    }
  }

  /// Convert `chrono::DateTime` to `Time`
  pub fn now() -> Self {
    let date = Utc::now();
    let year = date.naive_utc().year();
    let month = date.naive_utc().month();
    let day = date.naive_utc().day();
    let hour = date.naive_utc().hour();
    let minute = date.naive_utc().minute();
    let second = date.naive_utc().second();
    Time::new(year, month, day, Some(hour), Some(minute), Some(second))
  }

  /// Increment Time by a number of days
  pub fn delta_date(&self, days: i64) -> Self {
    let chrono_date =
      NaiveDate::from_ymd_opt(self.year, self.month.to_num(), self.day.to_num())
        .expect("failed to convert Time to chrono::NaiveDate");
    // convert NaiveDate to NaiveDateTime
    let chrono_date = chrono::NaiveDateTime::new(
      chrono_date,
      chrono::NaiveTime::from_hms_opt(self.hour.unwrap_or(0), self.minute.unwrap_or(0), 0)
        .expect("failed to create NaiveTime"),
    );

    let date = chrono_date + chrono::Duration::days(days);
    Time::new(date.year(), date.month(), date.day(), None, None, None)
  }

  /// Check if Time is within range of dates
  pub fn within_range(&self, start: Self, stop: Self) -> bool {
    self.to_naive_date() >= start.to_naive_date()
      && self.to_naive_date() <= stop.to_naive_date()
  }

  /// Difference in days between two dates
  pub fn diff_days(&self, other: &Self) -> anyhow::Result<i64> {
    let date1 = self.to_datetime()?;
    let date2 = other.to_datetime()?;
    Ok(date2.signed_duration_since(date1).num_days())
  }

  pub fn diff_minutes(&self, other: &Self) -> anyhow::Result<i64> {
    let date1 = self.to_datetime()?;
    let date2 = other.to_datetime()?;
    Ok(date2.signed_duration_since(date1).num_minutes())
  }

  /// Create Time from UNIX timestamp
  pub fn from_unix(unix: i64) -> Self {
    let date = Utc.timestamp_opt(unix, 0).unwrap();
    Time::new(
      date.naive_utc().year(),
      date.naive_utc().month(),
      date.naive_utc().day(),
      Some(date.naive_utc().hour()),
      Some(date.naive_utc().minute()),
      Some(date.naive_utc().second())
    )
  }

  pub fn from_unix_ms(unix: i64) -> Self {
    let date = Utc.timestamp_millis_opt(unix).unwrap();
    Time::new(
      date.naive_utc().year(),
      date.naive_utc().month(),
      date.naive_utc().day(),
      Some(date.naive_utc().hour()),
      Some(date.naive_utc().minute()),
      Some(date.naive_utc().second())
    )
  }

  pub fn to_unix(&self) -> i64 {
    self.to_datetime()
        .expect("Failed to convert Time to DateTime")
        .timestamp()
  }

  pub fn to_unix_ms(&self) -> i64 {
    self.to_datetime()
        .expect("Failed to convert Time to DateTime")
        .timestamp_millis()
  }

  pub fn from_unix_msec(unix: i64) -> Self {
    let date = chrono::Utc.timestamp_millis_opt(unix).unwrap();
    Time::new(
      date.naive_utc().year(),
      date.naive_utc().month(),
      date.naive_utc().day(),
      Some(date.naive_utc().hour()),
      Some(date.naive_utc().minute()),
      Some(date.naive_utc().second())
    )
  }
}

impl PartialEq for Time {
  fn eq(&self, other: &Self) -> bool {
    self.to_naive_date() == other.to_naive_date()
  }
}

impl PartialOrd for Time {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    // self.to_naive_date().partial_cmp(&other.to_naive_date())
    self.to_unix().partial_cmp(&other.to_unix())
  }
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum Month {
  January = 1,
  February = 2,
  March = 3,
  April = 4,
  May = 5,
  June = 6,
  July = 7,
  August = 8,
  September = 9,
  October = 10,
  November = 11,
  December = 12,
}

impl Month {
  pub fn to_string(&self) -> &str {
    match self {
      Month::January => "01",
      Month::February => "02",
      Month::March => "03",
      Month::April => "04",
      Month::May => "05",
      Month::June => "06",
      Month::July => "07",
      Month::August => "08",
      Month::September => "09",
      Month::October => "10",
      Month::November => "11",
      Month::December => "12",
    }
  }
  /// Used to convert 'Horizon API' time response to `Month`
  pub fn from_abbrev(abbrev: &str) -> Self {
    match abbrev {
      "Jan" => Month::January,
      "Feb" => Month::February,
      "Mar" => Month::March,
      "Apr" => Month::April,
      "May" => Month::May,
      "Jun" => Month::June,
      "Jul" => Month::July,
      "Aug" => Month::August,
      "Sep" => Month::September,
      "Oct" => Month::October,
      "Nov" => Month::November,
      "Dec" => Month::December,
      _ => panic!("Invalid month abbreviation: {}", abbrev),
    }
  }

  pub fn from_name(month: &str) -> Self {
    match month {
      "January" => Month::January,
      "February" => Month::February,
      "March" => Month::March,
      "April" => Month::April,
      "May" => Month::May,
      "June" => Month::June,
      "July" => Month::July,
      "August" => Month::August,
      "September" => Month::September,
      "October" => Month::October,
      "November" => Month::November,
      "December" => Month::December,
      _ => panic!("Invalid month: {}", month),
    }
  }

  pub fn from_num(num: u32) -> Self {
    match num {
      1 => Month::January,
      2 => Month::February,
      3 => Month::March,
      4 => Month::April,
      5 => Month::May,
      6 => Month::June,
      7 => Month::July,
      8 => Month::August,
      9 => Month::September,
      10 => Month::October,
      11 => Month::November,
      12 => Month::December,
      _ => panic!("Invalid month number: {}", num),
    }
  }

  pub fn to_num(&self) -> u32 {
    match self {
      Month::January => 1,
      Month::February => 2,
      Month::March => 3,
      Month::April => 4,
      Month::May => 5,
      Month::June => 6,
      Month::July => 7,
      Month::August => 8,
      Month::September => 9,
      Month::October => 10,
      Month::November => 11,
      Month::December => 12,
    }
  }

  pub fn to_mm(&self) -> String {
    match self {
      Month::January => "01",
      Month::February => "02",
      Month::March => "03",
      Month::April => "04",
      Month::May => "05",
      Month::June => "06",
      Month::July => "07",
      Month::August => "08",
      Month::September => "09",
      Month::October => "10",
      Month::November => "11",
      Month::December => "12",
    }.to_string()
  }

  pub fn days_per_month(&self) -> u32 {
    match &self {
      Month::January => 31,
      Month::February => 28,
      Month::March => 31,
      Month::April => 30,
      Month::May => 31,
      Month::June => 30,
      Month::July => 31,
      Month::August => 30,
      Month::September => 30,
      Month::October => 31,
      Month::November => 30,
      Month::December => 31,
    }
  }
}

#[allow(dead_code)]
#[derive(Debug, Copy, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum Day {
  One = 1,
  Two = 2,
  Three = 3,
  Four = 4,
  Five = 5,
  Six = 6,
  Seven = 7,
  Eight = 8,
  Nine = 9,
  Ten = 10,
  Eleven = 11,
  Twelve = 12,
  Thirteen = 13,
  Fourteen = 14,
  Fifteen = 15,
  Sixteen = 16,
  Seventeen = 17,
  Eighteen = 18,
  Nineteen = 19,
  Twenty = 20,
  TwentyOne = 21,
  TwentyTwo = 22,
  TwentyThree = 23,
  TwentyFour = 24,
  TwentyFive = 25,
  TwentySix = 26,
  TwentySeven = 27,
  TwentyEight = 28,
  TwentyNine = 29,
  Thirty = 30,
  ThirtyOne = 31,
}

impl Day {
  pub fn to_string(&self) -> &str {
    match self {
      Day::One => "01",
      Day::Two => "02",
      Day::Three => "03",
      Day::Four => "04",
      Day::Five => "05",
      Day::Six => "06",
      Day::Seven => "07",
      Day::Eight => "08",
      Day::Nine => "09",
      Day::Ten => "10",
      Day::Eleven => "11",
      Day::Twelve => "12",
      Day::Thirteen => "13",
      Day::Fourteen => "14",
      Day::Fifteen => "15",
      Day::Sixteen => "16",
      Day::Seventeen => "17",
      Day::Eighteen => "18",
      Day::Nineteen => "19",
      Day::Twenty => "20",
      Day::TwentyOne => "21",
      Day::TwentyTwo => "22",
      Day::TwentyThree => "23",
      Day::TwentyFour => "24",
      Day::TwentyFive => "25",
      Day::TwentySix => "26",
      Day::TwentySeven => "27",
      Day::TwentyEight => "28",
      Day::TwentyNine => "29",
      Day::Thirty => "30",
      Day::ThirtyOne => "31",
    }
  }

  pub fn from_string(day: &str) -> Self {
    match day {
      "01" => Day::One,
      "02" => Day::Two,
      "03" => Day::Three,
      "04" => Day::Four,
      "05" => Day::Five,
      "06" => Day::Six,
      "07" => Day::Seven,
      "08" => Day::Eight,
      "09" => Day::Nine,
      "10" => Day::Ten,
      "11" => Day::Eleven,
      "12" => Day::Twelve,
      "13" => Day::Thirteen,
      "14" => Day::Fourteen,
      "15" => Day::Fifteen,
      "16" => Day::Sixteen,
      "17" => Day::Seventeen,
      "18" => Day::Eighteen,
      "19" => Day::Nineteen,
      "20" => Day::Twenty,
      "21" => Day::TwentyOne,
      "22" => Day::TwentyTwo,
      "23" => Day::TwentyThree,
      "24" => Day::TwentyFour,
      "25" => Day::TwentyFive,
      "26" => Day::TwentySix,
      "27" => Day::TwentySeven,
      "28" => Day::TwentyEight,
      "29" => Day::TwentyNine,
      "30" => Day::Thirty,
      "31" => Day::ThirtyOne,
      _ => panic!("Invalid day: {}", day),
    }
  }

  pub fn from_num(num: u32) -> Self {
    match num {
      1 => Day::One,
      2 => Day::Two,
      3 => Day::Three,
      4 => Day::Four,
      5 => Day::Five,
      6 => Day::Six,
      7 => Day::Seven,
      8 => Day::Eight,
      9 => Day::Nine,
      10 => Day::Ten,
      11 => Day::Eleven,
      12 => Day::Twelve,
      13 => Day::Thirteen,
      14 => Day::Fourteen,
      15 => Day::Fifteen,
      16 => Day::Sixteen,
      17 => Day::Seventeen,
      18 => Day::Eighteen,
      19 => Day::Nineteen,
      20 => Day::Twenty,
      21 => Day::TwentyOne,
      22 => Day::TwentyTwo,
      23 => Day::TwentyThree,
      24 => Day::TwentyFour,
      25 => Day::TwentyFive,
      26 => Day::TwentySix,
      27 => Day::TwentySeven,
      28 => Day::TwentyEight,
      29 => Day::TwentyNine,
      30 => Day::Thirty,
      31 => Day::ThirtyOne,
      _ => panic!("Invalid day number: {}", num),
    }
  }

  pub fn to_num(&self) -> u32 {
    match self {
      Day::One => 1,
      Day::Two => 2,
      Day::Three => 3,
      Day::Four => 4,
      Day::Five => 5,
      Day::Six => 6,
      Day::Seven => 7,
      Day::Eight => 8,
      Day::Nine => 9,
      Day::Ten => 10,
      Day::Eleven => 11,
      Day::Twelve => 12,
      Day::Thirteen => 13,
      Day::Fourteen => 14,
      Day::Fifteen => 15,
      Day::Sixteen => 16,
      Day::Seventeen => 17,
      Day::Eighteen => 18,
      Day::Nineteen => 19,
      Day::Twenty => 20,
      Day::TwentyOne => 21,
      Day::TwentyTwo => 22,
      Day::TwentyThree => 23,
      Day::TwentyFour => 24,
      Day::TwentyFive => 25,
      Day::TwentySix => 26,
      Day::TwentySeven => 27,
      Day::TwentyEight => 28,
      Day::TwentyNine => 29,
      Day::Thirty => 30,
      Day::ThirtyOne => 31,
    }
  }

  pub fn to_dd(&self) -> String {
    match self {
      Day::One => "01",
      Day::Two => "02",
      Day::Three => "03",
      Day::Four => "04",
      Day::Five => "05",
      Day::Six => "06",
      Day::Seven => "07",
      Day::Eight => "08",
      Day::Nine => "09",
      Day::Ten => "10",
      Day::Eleven => "11",
      Day::Twelve => "12",
      Day::Thirteen => "13",
      Day::Fourteen => "14",
      Day::Fifteen => "15",
      Day::Sixteen => "16",
      Day::Seventeen => "17",
      Day::Eighteen => "18",
      Day::Nineteen => "19",
      Day::Twenty => "20",
      Day::TwentyOne => "21",
      Day::TwentyTwo => "22",
      Day::TwentyThree => "23",
      Day::TwentyFour => "24",
      Day::TwentyFive => "25",
      Day::TwentySix => "26",
      Day::TwentySeven => "27",
      Day::TwentyEight => "28",
      Day::TwentyNine => "29",
      Day::Thirty => "30",
      Day::ThirtyOne => "31",
    }.to_string()
  }
}
