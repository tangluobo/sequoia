//! Original client-side result filtering, including exact decimal comparisons.
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operator {
    Equal,
    NotEqual,
    Like,
    NotLike,
    Less,
    Greater,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Condition {
    pub operator: Operator,
    pub expected: Option<String>,
}
fn whitespace(ch: char) -> bool {
    matches!(ch,'\t'..='\r'|'\u{1c}'..=' '|'\u{1680}'|'\u{2000}'..='\u{2006}'|'\u{2008}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{205f}'|'\u{3000}')
}
impl Condition {
    pub fn parse(expression: Option<&str>) -> Self {
        let text = expression.unwrap_or_default().trim_matches(whitespace);
        for (prefix, operator) in [
            ("!=", Operator::NotEqual),
            ("!~", Operator::NotLike),
            ("=", Operator::Equal),
            ("~", Operator::Like),
            ("<", Operator::Less),
            (">", Operator::Greater),
        ] {
            if let Some(value) = text.strip_prefix(prefix) {
                return Self {
                    operator,
                    expected: Some(value.trim_matches(whitespace).into()),
                };
            }
        }
        Self {
            operator: Operator::Like,
            expected: Some(text.into()),
        }
    }
    pub fn matches(&self, actual: Option<&str>) -> bool {
        let expected = self.expected.as_deref();
        match self.operator {
            Operator::Equal => actual == expected,
            Operator::NotEqual => actual != expected,
            Operator::Like => actual
                .zip(expected)
                .is_some_and(|(actual, expected)| actual.contains(expected)),
            Operator::NotLike => actual
                .zip(expected)
                .is_none_or(|(actual, expected)| !actual.contains(expected)),
            Operator::Less => compare(actual, expected).is_lt(),
            Operator::Greater => compare(actual, expected).is_gt(),
        }
    }
}
// Character.digit(char,10) ranges from the original Java 25 oracle. Java's
// BigDecimal char-array parser does not accept supplementary-plane digits.
const ZEROES: [u32; 37] = [
    48, 1632, 1776, 1984, 2406, 2534, 2662, 2790, 2918, 3046, 3174, 3302, 3430, 3558, 3664, 3792,
    3872, 4160, 4240, 6112, 6160, 6470, 6608, 6784, 6800, 6992, 7088, 7232, 7248, 42528, 43216,
    43264, 43472, 43504, 43600, 44016, 65296,
];
fn digit(ch: char) -> Option<u8> {
    let code = u32::from(ch);
    let index = ZEROES
        .partition_point(|zero| *zero <= code)
        .checked_sub(1)?;
    let value = code - ZEROES[index];
    if value < 10 { Some(value as u8) } else { None }
}
fn exponent(text: &str) -> Option<i64> {
    let (negative, text) = if let Some(text) = text.strip_prefix('-') {
        (true, text)
    } else {
        (false, text.strip_prefix('+').unwrap_or(text))
    };
    if text.is_empty() {
        return None;
    }
    let mut value = 0u64;
    let limit = i64::MAX as u64;
    for ch in text.chars() {
        value = value.checked_mul(10)?.checked_add(u64::from(digit(ch)?))?;
        if value > limit {
            return None;
        }
    }
    Some(if negative {
        -(value as i64)
    } else {
        value as i64
    })
}
struct Number<'a> {
    mantissa: &'a str,
    leading: usize,
    precision: usize,
    scale: i32,
    sign: i8,
}
impl<'a> Number<'a> {
    fn parse(text: &'a str) -> Option<Self> {
        let text = text.trim_matches(whitespace);
        let (negative, text) = if let Some(text) = text.strip_prefix('-') {
            (true, text)
        } else {
            (false, text.strip_prefix('+').unwrap_or(text))
        };
        let (mantissa, exponent) = if let Some(index) = text.find(['e', 'E']) {
            (&text[..index], exponent(&text[index + 1..])?)
        } else {
            (text, 0)
        };
        let mut decimal = false;
        let mut fraction = 0usize;
        let mut digits = 0usize;
        let mut leading = 0usize;
        let mut nonzero = false;
        for ch in mantissa.chars() {
            if ch == '.' {
                if decimal {
                    return None;
                }
                decimal = true;
                continue;
            }
            let value = digit(ch)?;
            digits += 1;
            if decimal {
                fraction += 1;
            }
            if !nonzero && value == 0 {
                leading += 1;
            } else {
                nonzero = true;
            }
        }
        if digits == 0 {
            return None;
        }
        let scale = i32::try_from(i64::try_from(fraction).ok()?.checked_sub(exponent)?).ok()?;
        Some(Self {
            mantissa,
            leading,
            precision: digits - leading,
            scale,
            sign: if !nonzero {
                0
            } else if negative {
                -1
            } else {
                1
            },
        })
    }
    fn compare(&self, other: &Self) -> Ordering {
        let signs = self.sign.cmp(&other.sign);
        if !signs.is_eq() {
            return signs;
        }
        if self.sign == 0 {
            return Ordering::Equal;
        }
        let adjusted = (self.precision as i64 - i64::from(self.scale))
            .cmp(&(other.precision as i64 - i64::from(other.scale)));
        let magnitude = if !adjusted.is_eq() {
            adjusted
        } else {
            let mut left = self.mantissa.chars().filter_map(digit).skip(self.leading);
            let mut right = other.mantissa.chars().filter_map(digit).skip(other.leading);
            let mut result = Ordering::Equal;
            loop {
                let (a, b) = (left.next(), right.next());
                if a.is_none() && b.is_none() {
                    break;
                }
                result = a.unwrap_or(0).cmp(&b.unwrap_or(0));
                if !result.is_eq() {
                    break;
                }
            }
            result
        };
        if self.sign < 0 {
            magnitude.reverse()
        } else {
            magnitude
        }
    }
}
fn fold(ch: char) -> char {
    let mut upper = ch.to_uppercase();
    let first = upper.next().unwrap_or(ch);
    let upper = if upper.next().is_none() { first } else { ch };
    upper.to_lowercase().next().unwrap_or(upper)
}
pub fn compare(left: Option<&str>, right: Option<&str>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(left), Some(right)) => match (Number::parse(left), Number::parse(right)) {
            (Some(left), Some(right)) => left.compare(&right),
            _ => left.chars().map(fold).cmp(right.chars().map(fold)),
        },
    }
}
#[cfg(test)]
#[path = "result_filter_tests.rs"]
mod tests;
