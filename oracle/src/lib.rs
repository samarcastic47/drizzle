//! Pure, host-testable logic for the Drizzle oracle.
//!
//! Responsibilities: parse the round input, turn the unix hour into an
//! Open-Meteo `YYYY-MM-DDTHH:00` timestamp, format integer-e4 coordinates the
//! same way JavaScript's `(x / 1e4).toString()` does, build the single batched
//! forecast URL, and turn the response into `mm_tenths`.
//!
//! No networking lives here so `cargo test` runs on the host.

use serde::{Deserialize, Serialize};

/// Round input read from stdin.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RoundInput {
    pub hour: u64,
    /// `[lat_e4, lon_e4]` pairs, in output order.
    pub cities: Vec<[i32; 2]>,
}

/// Successful stdout payload.
#[derive(Debug, Clone, Serialize)]
pub struct RoundOutput {
    pub hour: u64,
    pub mm_tenths: Vec<u16>,
    pub source: &'static str,
    /// OutLayer's `NEAR_REQUEST_ID` as a decimal string, or null when absent.
    pub outlayer_request_id: Option<String>,
}

/// Parse and validate the round input.
pub fn parse_input(input: &str) -> Result<RoundInput, String> {
    let parsed: RoundInput =
        serde_json::from_str(input).map_err(|e| format!("invalid input JSON: {e}"))?;
    if parsed.cities.is_empty() {
        return Err("input must contain at least 1 city".to_string());
    }
    if parsed.cities.len() > 50 {
        return Err(format!(
            "input must contain at most 50 cities, got {}",
            parsed.cities.len()
        ));
    }
    Ok(parsed)
}

/// Convert unix hours to `YYYY-MM-DDTHH:00` (UTC).
///
/// Uses Howard Hinnant's `civil_from_days` so no date crate is needed.
pub fn format_hour(hour: u64) -> Result<String, String> {
    let days = (hour / 24) as i64;
    let hour_of_day = hour % 24;
    let (year, month, day) = civil_from_days(days);
    if !(0..=9999).contains(&year) {
        return Err(format!("hour {hour} is outside the supported year range"));
    }
    Ok(format!(
        "{:04}-{:02}-{:02}T{:02}:00",
        year, month, day, hour_of_day
    ))
}

/// Days since 1970-01-01 -> (year, month, day).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (y + if m <= 2 { 1 } else { 0 }, m as u32, d as u32)
}

/// Format an integer-e4 coordinate as a decimal string, matching
/// JavaScript's `(x / 1e4).toString()` for all values this oracle sees:
/// trailing fractional zeros (and a bare trailing dot) are stripped, and
/// integers are printed without a decimal point.
pub fn format_coord(e4: i32) -> String {
    let value = e4 as i64;
    let sign = if value < 0 { "-" } else { "" };
    let abs = value.unsigned_abs();
    let int_part = abs / 10_000;
    let frac = abs % 10_000;
    if frac == 0 {
        return format!("{sign}{int_part}");
    }
    let mut frac_str = format!("{frac:04}");
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{sign}{int_part}.{frac_str}")
}

/// Build the exact Open-Meteo URL (parameter order is part of the interface).
pub fn build_url(hour: &str, cities: &[[i32; 2]]) -> String {
    let lat = cities
        .iter()
        .map(|c| format_coord(c[0]))
        .collect::<Vec<_>>()
        .join(",");
    let lon = cities
        .iter()
        .map(|c| format_coord(c[1]))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &hourly=precipitation&start_hour={hour}&end_hour={hour}&timezone=UTC"
    )
}

/// Parse an Open-Meteo forecast response into tenths of a millimetre.
///
/// `expected_cities` is the number of input cities and `hour` is the exact
/// `YYYY-MM-DDTHH:00` string requested. A single-location response is an
/// object; a multi-location response is an array in request order.
pub fn parse_response(
    body: &str,
    expected_cities: usize,
    hour: &str,
) -> Result<Vec<u16>, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("invalid open-meteo JSON: {e}"))?;

    let elements: Vec<&serde_json::Value> = match &value {
        serde_json::Value::Array(items) => items.iter().collect(),
        serde_json::Value::Object(_) => vec![&value],
        _ => return Err("open-meteo response is neither an object nor an array".to_string()),
    };

    if elements.len() != expected_cities {
        return Err(format!(
            "open-meteo returned {} locations, expected {}",
            elements.len(),
            expected_cities
        ));
    }

    let mut readings = Vec::with_capacity(elements.len());
    for (city_index, element) in elements.iter().enumerate() {
        let times = element
            .get("hourly")
            .and_then(|h| h.get("time"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| {
                format!("open-meteo response for city {city_index} has no hourly.time array")
            })?;
        let index = times
            .iter()
            .position(|t| t.as_str() == Some(hour))
            .ok_or_else(|| {
                format!("open-meteo response for city {city_index} is missing hour {hour}")
            })?;

        let precip = element
            .get("hourly")
            .and_then(|h| h.get("precipitation"))
            .and_then(|p| p.as_array())
            .ok_or_else(|| {
                format!(
                    "open-meteo response for city {city_index} has no hourly.precipitation array"
                )
            })?;

        let value = precip.get(index).ok_or_else(|| {
            format!(
                "open-meteo response for city {city_index} has no precipitation at index {index}"
            )
        })?;

        let mm = match value {
            serde_json::Value::Null => 0.0,
            serde_json::Value::Number(n) => n.as_f64().ok_or_else(|| {
                format!("open-meteo precipitation for city {city_index} is not a finite number")
            })?,
            _ => {
                return Err(format!(
                    "open-meteo precipitation for city {city_index} is not a number"
                ))
            }
        };

        if !mm.is_finite() || mm < 0.0 {
            return Err(format!(
                "open-meteo precipitation for city {city_index} is invalid: {mm}"
            ));
        }

        let tenths = (mm * 10.0).round();
        readings.push(tenths.min(u16::MAX as f64) as u16);
    }

    Ok(readings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hour_497294() -> &'static str {
        "2026-09-24T14:00"
    }

    #[test]
    fn coordinate_formatter_matches_javascript() {
        let cases: &[(i32, &str)] = &[
            (603_900, "60.39"),
            (-1_300, "-0.13"),
            (13_500, "1.35"),
            (-62_000, "-6.2"),
            (515_100, "51.51"),
            (0, "0"),
            (-5, "-0.0005"),
            (1_000_000, "100"),
        ];
        for &(e4, expected) in cases {
            assert_eq!(format_coord(e4), expected, "e4={e4}");
        }
    }

    #[test]
    fn hour_formatting() {
        assert_eq!(format_hour(497_294).unwrap(), hour_497294());
        assert_eq!(format_hour(0).unwrap(), "1970-01-01T00:00");
        // 2026-09-24T15:00 is one hour later.
        assert_eq!(format_hour(497_295).unwrap(), "2026-09-24T15:00");
        // Leap day.
        assert_eq!(format_hour(439_704).unwrap(), "2020-02-29T00:00");
    }

    #[test]
    fn url_is_exact() {
        let cities = [[603_900, 53_200], [13_500, 1_038_200]];
        assert_eq!(
            build_url(hour_497294(), &cities),
            "https://api.open-meteo.com/v1/forecast?latitude=60.39,1.35&longitude=5.32,103.82\
             &hourly=precipitation&start_hour=2026-09-24T14:00&end_hour=2026-09-24T14:00\
             &timezone=UTC"
        );
    }

    #[test]
    fn input_fixture_parses() {
        let input = parse_input(include_str!("../fixtures/input_2026-09-24T14.json")).unwrap();
        assert_eq!(input.hour, 497_294);
        assert_eq!(input.cities.len(), 12);
        assert_eq!(input.cities[0], [603_900, 53_200]);
        assert_eq!(input.cities[11], [252_000, 552_700]);
    }

    #[test]
    fn input_validation() {
        assert!(parse_input(r#"{"hour":1,"cities":[]}"#).is_err());
        let too_many = format!(
            r#"{{"hour":1,"cities":[{}]}}"#,
            vec!["[0,0]"; 51].join(",")
        );
        assert!(parse_input(&too_many).is_err());
        assert!(parse_input(r#"{"hour":1}"#).is_err());
    }

    #[test]
    fn recorded_fixture_parses() {
        let body = include_str!("../fixtures/openmeteo_2026-09-24T14.json");
        let got = parse_response(body, 12, hour_497294()).unwrap();
        // Seattle (index 9) is null upstream and must become 0.
        assert_eq!(got, vec![0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn wrong_array_length_is_rejected() {
        let body = r#"[{"hourly":{"time":["2026-09-24T14:00"],"precipitation":[1.0]}}]"#;
        let err = parse_response(body, 2, hour_497294()).unwrap_err();
        assert!(err.contains("expected 2"), "{err}");
    }

    #[test]
    fn missing_hour_is_rejected() {
        let body = r#"{"hourly":{"time":["2026-09-24T13:00"],"precipitation":[1.0]}}"#;
        let err = parse_response(body, 1, hour_497294()).unwrap_err();
        assert!(err.contains(hour_497294()), "{err}");
    }

    #[test]
    fn null_value_becomes_zero() {
        let body = r#"{"hourly":{"time":["2026-09-24T14:00"],"precipitation":[null]}}"#;
        assert_eq!(parse_response(body, 1, hour_497294()).unwrap(), vec![0]);
    }

    #[test]
    fn object_is_one_city_but_not_twelve() {
        let body = r#"{"hourly":{"time":["2026-09-24T14:00"],"precipitation":[2.1]}}"#;
        assert_eq!(parse_response(body, 1, hour_497294()).unwrap(), vec![21]);
        let err = parse_response(body, 12, hour_497294()).unwrap_err();
        assert!(err.contains("expected 12"), "{err}");
    }

    #[test]
    fn negative_and_non_numeric_are_rejected() {
        let negative = r#"{"hourly":{"time":["2026-09-24T14:00"],"precipitation":[-0.1]}}"#;
        assert!(parse_response(negative, 1, hour_497294()).is_err());

        let text = r#"{"hourly":{"time":["2026-09-24T14:00"],"precipitation":["0.1"]}}"#;
        assert!(parse_response(text, 1, hour_497294()).is_err());
    }

    #[test]
    fn output_serializes_request_id() {
        let with_id = RoundOutput {
            hour: 497_294,
            mm_tenths: vec![0, 3],
            source: "open-meteo",
            outlayer_request_id: Some("123".to_string()),
        };
        assert_eq!(
            serde_json::to_string(&with_id).unwrap(),
            r#"{"hour":497294,"mm_tenths":[0,3],"source":"open-meteo","outlayer_request_id":"123"}"#
        );

        let without_id = RoundOutput {
            hour: 497_294,
            mm_tenths: vec![0, 3],
            source: "open-meteo",
            outlayer_request_id: None,
        };
        assert_eq!(
            serde_json::to_string(&without_id).unwrap(),
            r#"{"hour":497294,"mm_tenths":[0,3],"source":"open-meteo","outlayer_request_id":null}"#
        );
    }

    #[test]
    fn tenths_round_and_clamp() {
        let body = r#"{"hourly":{"time":["2026-09-24T14:00"],"precipitation":[2.1]}}"#;
        assert_eq!(parse_response(body, 1, hour_497294()).unwrap(), vec![21]);
        let body = r#"{"hourly":{"time":["2026-09-24T14:00"],"precipitation":[6553.5]}}"#;
        assert_eq!(
            parse_response(body, 1, hour_497294()).unwrap(),
            vec![u16::MAX]
        );
    }
}
