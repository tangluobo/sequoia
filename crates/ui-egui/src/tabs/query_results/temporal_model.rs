//! Borrowed parsing for the original query cell calendar and time controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Date,
    Time,
    DateTime,
}
impl Kind {
    pub fn jdbc(code: i32) -> Option<Self> {
        match code {
            91 => Some(Self::Date),
            92 | 2013 => Some(Self::Time),
            93 | 2014 => Some(Self::DateTime),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Date => "日期",
            Self::Time => "时间",
            Self::DateTime => "日期时间",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}
impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.year < 0 {
            write!(f, "-{:04}", -self.year)?;
        } else if self.year > 9999 {
            write!(f, "+{}", self.year)?;
        } else {
            write!(f, "{:04}", self.year)?;
        }
        write!(f, "-{:02}-{:02}", self.month, self.day)
    }
}
impl Date {
    pub fn days(self) -> u32 {
        match self.month {
            2 => {
                if self.year % 4 == 0 && (self.year % 100 != 0 || self.year % 400 == 0) {
                    29
                } else {
                    28
                }
            }
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        }
    }
    pub fn first(self) -> Self {
        Self { day: 1, ..self }
    }
    pub fn shift_month(self, offset: i32) -> Option<Self> {
        let month = i64::from(self.year) * 12 + i64::from(self.month) - 1 + i64::from(offset);
        let year = month.div_euclid(12);
        if !(-999_999_999..=999_999_999).contains(&year) {
            return None;
        }
        Some(Self {
            year: year as i32,
            month: month.rem_euclid(12) as u32 + 1,
            day: 1,
        })
    }
    fn next(self) -> Option<Self> {
        if self.day < self.days() {
            Some(Self {
                day: self.day + 1,
                ..self
            })
        } else {
            self.shift_month(1)
        }
    }
    pub fn weekday(self) -> u32 {
        let before = |year: i32| {
            let y = i64::from(year);
            365 * y + (y + 3).div_euclid(4) - (y + 99).div_euclid(100) + (y + 399).div_euclid(400)
        };
        let preceding = (1..self.month)
            .map(|month| Self { month, ..self }.days())
            .sum::<u32>();
        (before(self.year) - before(1970) + i64::from(preceding + self.day - 1) + 4).rem_euclid(7)
            as u32
    }
    fn parse(text: &str, smart: bool) -> Option<Self> {
        let (ym, day) = text.rsplit_once('-')?;
        let (year, month) = ym.rsplit_once('-')?;
        if month.len() != 2
            || day.len() != 2
            || !month.bytes().chain(day.bytes()).all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let digits = year.strip_prefix(['+', '-']).unwrap_or(year);
        if !digits.bytes().all(|b| b.is_ascii_digit())
            || digits.len() < 4
            || digits.len() > 9
            || (!year.starts_with(['+', '-']) && digits.len() != 4)
            || (year.starts_with('+') && digits.len() == 4)
        {
            return None;
        }
        let year: i32 = year.parse().ok()?;
        if !(-999_999_999..=999_999_999).contains(&year)
            || (text.starts_with('-') && year == 0)
            || (smart && year <= 0)
        {
            return None;
        }
        let month: u32 = month.parse().ok()?;
        let day: u32 = day.parse().ok()?;
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let mut date = Self { year, month, day };
        if day > date.days() {
            if !smart {
                return None;
            }
            date.day = date.days();
        }
        Some(date)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub date: Date,
    pub time: [u32; 3],
    pub nano: u32,
}
impl DateTime {
    pub fn parse(text: &str) -> Option<Self> {
        fn time(text: &str, smart: bool) -> Option<([u32; 3], u32)> {
            let (whole, fraction) = text
                .split_once('.')
                .map_or((text, None), |(a, b)| (a, Some(b)));
            let mut parts = whole.split(':');
            let mut values = [0; 3];
            let mut count = 0;
            for value in &mut values {
                let Some(part) = parts.next() else {
                    break;
                };
                if part.len() != 2 || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                *value = part.parse().ok()?;
                count += 1;
            }
            if count < 2 || (smart && count != 3) || parts.next().is_some() {
                return None;
            }
            let mut nano = 0;
            if let Some(fraction) = fraction {
                if count != 3
                    || fraction.len() > 9
                    || (smart && fraction.len() != 1)
                    || !fraction.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                if !fraction.is_empty() {
                    nano = fraction.parse::<u32>().ok()? * 10u32.pow(9 - fraction.len() as u32);
                }
            }
            if values[0] > if smart { 24 } else { 23 }
                || values[1] > 59
                || values[2] > 59
                || (values[0] == 24 && (values[1] != 0 || values[2] != 0 || nano != 0))
            {
                return None;
            }
            Some((values, nano))
        }
        if let Some((date, clock)) = text.split_once(' ')
            && let Some(date) = Date::parse(date, true)
            && let Some((mut values, nano)) = time(clock, true)
        {
            let date = if values[0] == 24 {
                values[0] = 0;
                date.next()?
            } else {
                date
            };
            return Some(Self {
                date,
                time: values,
                nano,
            });
        }
        let (date, clock) = text.split_once(['T', 't', ' '])?;
        let date = Date::parse(date, false)?;
        let (time, nano) = time(clock, false)?;
        Some(Self { date, time, nano })
    }
    #[cfg(test)]
    fn iso(self) -> String {
        let mut value = format!("{}T{:02}:{:02}", self.date, self.time[0], self.time[1]);
        if self.time[2] != 0 || self.nano != 0 {
            value.push_str(&format!(":{:02}", self.time[2]));
        }
        if self.nano != 0 {
            let mut fraction = format!("{:09}", self.nano);
            while fraction.len() > 3 && fraction.ends_with("000") {
                fraction.truncate(fraction.len() - 3);
            }
            value.push('.');
            value.push_str(&fraction);
        }
        value
    }
}
pub fn safe_int(text: &str) -> i32 {
    let text = text.trim_matches(|ch| ch <= ' ');
    let special = match text {
        "NaN" | "+NaN" | "-NaN" => Some(f64::NAN),
        "Infinity" | "+Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => None,
    };
    let value = special.or_else(|| {
        let text = text.strip_suffix(['f', 'F', 'd', 'D']).unwrap_or(text);
        let (sign, unsigned) = text.strip_prefix('-').map_or_else(
            || (1., text.strip_prefix('+').unwrap_or(text)),
            |text| (-1., text),
        );
        if let Some(hex) = unsigned
            .strip_prefix("0x")
            .or_else(|| unsigned.strip_prefix("0X"))
        {
            let (mantissa, exponent) = hex.split_once(['p', 'P'])?;
            let exponent: i32 = exponent.parse().ok()?;
            let mut value = 0.;
            let mut fractional = 0;
            let mut dot = false;
            let mut digits = 0;
            for ch in mantissa.chars() {
                if ch == '.' {
                    if dot {
                        return None;
                    }
                    dot = true;
                    continue;
                }
                let digit = ch.to_digit(16)?;
                value = value * 16. + f64::from(digit);
                digits += 1;
                if dot {
                    fractional += 4;
                }
            }
            if digits == 0 {
                return None;
            }
            Some(sign * value * 2f64.powi(exponent.saturating_sub(fractional)))
        } else if unsigned.bytes().any(|b| b.is_ascii_digit())
            && unsigned
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            text.parse().ok()
        } else {
            None
        }
    });
    value.unwrap_or(0.) as i32
}
pub struct Value {
    pub kind: Kind,
    pub selected: Date,
    pub cursor: Date,
    pub time: [u32; 3],
}
impl Value {
    pub fn new(kind: Kind, text: &str, today: Date) -> Self {
        let text = text.trim_matches(|ch| ch <= ' ');
        let mut selected = today;
        let mut time = [0; 3];
        match kind {
            Kind::Date => {
                if let Some(date) = Date::parse(text, false) {
                    selected = date;
                }
            }
            Kind::Time => {
                for (index, part) in text.split(':').take(3).enumerate() {
                    time[index] = safe_int(part).clamp(0, if index == 0 { 23 } else { 59 }) as u32;
                }
            }
            Kind::DateTime => {
                if let Some(value) = DateTime::parse(text) {
                    selected = value.date;
                    time = value.time;
                }
            }
        }
        Self {
            kind,
            selected,
            cursor: selected.first(),
            time,
        }
    }
    pub fn formatted(&self) -> String {
        match self.kind {
            Kind::Date => self.selected.to_string(),
            Kind::Time => format!(
                "{:02}:{:02}:{:02}",
                self.time[0], self.time[1], self.time[2]
            ),
            Kind::DateTime => format!(
                "{} {:02}:{:02}:{:02}",
                self.selected, self.time[0], self.time[1], self.time[2]
            ),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn result_temporal_model_matches_actual_java_parser_numeric_spinner_and_formatter()
    -> anyhow::Result<()> {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../core/tests/fixtures/sql-result/original-temporal.json"
        ))?;
        for case in golden["datetimes"].as_array().unwrap() {
            assert_eq!(
                DateTime::parse(case["input"].as_str().unwrap())
                    .map(DateTime::iso)
                    .as_deref(),
                case["output"].as_str(),
                "{}",
                case["input"]
            );
        }
        for case in golden["integers"].as_array().unwrap() {
            assert_eq!(
                i64::from(safe_int(case["input"].as_str().unwrap())),
                case["output"].as_i64().unwrap(),
                "{}",
                case["input"]
            );
        }
        for case in golden["formats"].as_array().unwrap() {
            let value = Value {
                kind: Kind::jdbc(case["type"].as_i64().unwrap() as i32).unwrap(),
                selected: Date {
                    year: 2024,
                    month: 2,
                    day: 29,
                },
                cursor: Date {
                    year: 2024,
                    month: 2,
                    day: 1,
                },
                time: [13, 4, 5],
            };
            assert_eq!(value.formatted(), case["output"].as_str().unwrap());
        }
        assert_eq!(
            Date {
                year: 2024,
                month: 2,
                day: 1
            }
            .weekday(),
            4
        );
        Ok(())
    }
}
