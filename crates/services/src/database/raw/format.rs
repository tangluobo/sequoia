//! Connector/J's getString uses BigDecimal/Double formatting for numeric fields.
pub(super) fn decimal(text: String) -> String {
    let unsigned = text.strip_prefix('-').unwrap_or(&text);
    let Some((whole, fraction)) = unsigned.split_once('.') else {
        return text;
    };
    let digits = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let exponent = digits.len() as i64 - fraction.len() as i64 - 1;
    if exponent >= -6 {
        return text;
    }
    let sign = if text.starts_with('-') && digits != "0" {
        "-"
    } else {
        ""
    };
    let coefficient = if digits.len() == 1 {
        digits.to_owned()
    } else {
        format!("{}.{}", &digits[..1], &digits[1..])
    };
    format!("{sign}{coefficient}E{exponent}")
}

pub(super) fn double(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into();
    }
    let scientific = format!("{value:e}");
    let (coefficient, exponent) = scientific.split_once('e').unwrap();
    let exponent: i32 = exponent.parse().unwrap();
    if (-3..7).contains(&exponent) {
        let mut text = value.to_string();
        if !text.contains('.') {
            text.push_str(".0");
        }
        return text;
    }
    // Java selects at least two significant digits for scientific notation.
    if !coefficient.contains('.') {
        let rounded = format!("{value:.1e}");
        let (coefficient, exponent) = rounded.split_once('e').unwrap();
        return format!("{coefficient}E{}", exponent.parse::<i32>().unwrap());
    }
    format!("{coefficient}E{exponent}")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn connector_numeric_strings_match_actual_java() {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/sql-types/numbers.json"
        ))
        .unwrap();
        for case in golden["decimal"].as_array().unwrap() {
            assert_eq!(
                decimal(case["input"].as_str().unwrap().into()),
                case["output"].as_str().unwrap()
            );
        }
        for case in golden["double"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            assert_eq!(
                double(input.parse().unwrap()),
                case["output"].as_str().unwrap(),
                "{input}"
            );
        }
    }
}
