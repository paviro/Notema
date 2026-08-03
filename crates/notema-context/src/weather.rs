//! Weather for an entry's place and date via Open-Meteo — free and keyless.
//!
//! Two endpoints cover the timeline: the archive (ERA5 reanalysis, back to 1940
//! but ~5 days behind real time) for older entries, and the forecast API (which
//! also serves the recent past) for anything within the last few days. We request
//! hourly variables for the entry's local date and keep the sample nearest the
//! hour it was written, matching Day One's point-in-time weather snapshot.

use crate::Result;
use crate::http::get;
use jiff::Zoned;
use jiff::civil::DateTime;
use notema_domain::{Coordinates, Weather};
use serde::Deserialize;

const ENDPOINT_FORECAST: &str = "https://api.open-meteo.com/v1/forecast";
const ENDPOINT_ARCHIVE: &str = "https://archive-api.open-meteo.com/v1/archive";
/// The archive (ERA5) lags real time by about five days; within that window the
/// forecast API is the one that has the data, so switch on the entry's age.
const ARCHIVE_CUTOFF_DAYS: i64 = 5;
/// The hourly variables we map onto [`Weather`]. `timezone=auto` makes the
/// returned `time[]` local to the coordinates, matching the entry's wall clock.
const HOURLY: &str = "temperature_2m,apparent_temperature,relative_humidity_2m,dew_point_2m,surface_pressure,visibility,cloud_cover,precipitation,wind_speed_10m,wind_gusts_10m,wind_direction_10m,weather_code";
/// Attribution stored in the `[weather]` table's `source`.
const SOURCE: &str = "Open-Meteo";

/// Fetch the weather for a point at an instant. `Ok(None)` when Open-Meteo has no
/// usable sample there/then (out-of-range date, or a gap in the data). Errors are
/// transport/HTTP failures — the caller drops them silently so a save never fails.
pub fn fetch_weather(coordinates: Coordinates, datetime: Zoned) -> Result<Option<Weather>> {
    let age_days = (jiff::Timestamp::now().as_second() - datetime.timestamp().as_second()) / 86_400;
    // The archive/forecast handoff sits right on ERA5's ~5-day lag, and that lag
    // drifts, so an entry near the boundary can hit the endpoint that has no data
    // yet. Try the age-appropriate endpoint first, then fall back to the other on
    // an empty sample rather than silently returning no weather.
    let (primary, fallback) = if age_days >= ARCHIVE_CUTOFF_DAYS {
        (ENDPOINT_ARCHIVE, ENDPOINT_FORECAST)
    } else {
        (ENDPOINT_FORECAST, ENDPOINT_ARCHIVE)
    };
    if let Some(weather) = fetch_weather_from(primary, coordinates, &datetime)? {
        return Ok(Some(weather));
    }
    fetch_weather_from(fallback, coordinates, &datetime)
}

fn fetch_weather_from(
    endpoint: &str,
    coordinates: Coordinates,
    datetime: &Zoned,
) -> Result<Option<Weather>> {
    let lat = coordinates.latitude();
    let lon = coordinates.longitude();
    let date = datetime.strftime("%Y-%m-%d");
    let url = format!(
        "{endpoint}?latitude={lat}&longitude={lon}&timezone=auto&wind_speed_unit=kmh\
         &start_date={date}&end_date={date}&hourly={HOURLY}"
    );
    let response: OpenMeteoResponse = serde_json::from_str(&get(&url)?)?;
    Ok(response
        .hourly
        .and_then(|hourly| extract_weather(&hourly, datetime.datetime())))
}

/// The hourly block of an Open-Meteo response. Every series is optional and holds
/// optional values — the archive omits some variables, and any hour can be a gap.
#[derive(Deserialize)]
struct OpenMeteoResponse {
    hourly: Option<Hourly>,
}

#[derive(Deserialize)]
struct Hourly {
    /// Local timestamps, one per hour, e.g. `"2026-07-08T14:00"`.
    time: Vec<String>,
    temperature_2m: Option<Vec<Option<f64>>>,
    apparent_temperature: Option<Vec<Option<f64>>>,
    relative_humidity_2m: Option<Vec<Option<f64>>>,
    dew_point_2m: Option<Vec<Option<f64>>>,
    surface_pressure: Option<Vec<Option<f64>>>,
    visibility: Option<Vec<Option<f64>>>,
    cloud_cover: Option<Vec<Option<f64>>>,
    precipitation: Option<Vec<Option<f64>>>,
    wind_speed_10m: Option<Vec<Option<f64>>>,
    wind_gusts_10m: Option<Vec<Option<f64>>>,
    wind_direction_10m: Option<Vec<Option<f64>>>,
    weather_code: Option<Vec<Option<i64>>>,
}

/// Pull the sample nearest `target` from the hourly series and map it onto
/// [`Weather`]. `None` when there is no parseable hour or the nearest one carries
/// no data at all (so we don't persist a table holding only the attribution).
fn extract_weather(hourly: &Hourly, target: DateTime) -> Option<Weather> {
    let index = nearest_hour_index(&hourly.time, target)?;
    let at = |series: &Option<Vec<Option<f64>>>| {
        series
            .as_ref()
            .and_then(|values| values.get(index).copied().flatten())
    };

    let weather_code = hourly
        .weather_code
        .as_ref()
        .and_then(|values| values.get(index).copied().flatten());
    let weather = Weather {
        condition: map_wmo_code(weather_code),
        temperature_celsius: at(&hourly.temperature_2m),
        feels_like_celsius: at(&hourly.apparent_temperature),
        // Open-Meteo reports relative humidity and cloud cover as percentages; the
        // store keeps 0–1 fractions.
        humidity: at(&hourly.relative_humidity_2m).map(|value| value / 100.0),
        dew_point_celsius: at(&hourly.dew_point_2m),
        pressure_mb: at(&hourly.surface_pressure),
        // Open-Meteo reports visibility in metres; the store keeps kilometres.
        visibility_km: at(&hourly.visibility).map(|value| value / 1000.0),
        cloud_cover: at(&hourly.cloud_cover).map(|value| value / 100.0),
        precipitation_mm: at(&hourly.precipitation),
        wind_speed_kph: at(&hourly.wind_speed_10m),
        wind_gust_kph: at(&hourly.wind_gusts_10m),
        wind_direction: at(&hourly.wind_direction_10m),
        source: None,
    };

    if weather.is_empty() {
        return None;
    }
    Some(Weather {
        source: Some(SOURCE.to_string()),
        ..weather
    })
}

/// `strftime` pattern for Open-Meteo's hourly timestamps (`2024-01-02T15:00`),
/// shared by the weather and air-quality series and their tests.
pub(crate) const OPEN_METEO_HOUR_FORMAT: &str = "%Y-%m-%dT%H:%M";

/// The index of the hourly sample whose timestamp is closest to `target`. Shared
/// with the air-quality fetch, which uses the same hourly-series shape.
pub(crate) fn nearest_hour_index(times: &[String], target: DateTime) -> Option<usize> {
    let target_secs = civil_seconds(target);
    times
        .iter()
        .enumerate()
        .filter_map(|(index, time)| {
            DateTime::strptime(OPEN_METEO_HOUR_FORMAT, time)
                .ok()
                .map(|parsed| (index, parsed))
        })
        .min_by_key(|(_, parsed)| (civil_seconds(*parsed) - target_secs).abs())
        .map(|(index, _)| index)
}

/// A civil datetime as a UTC-anchored second count, for measuring how far apart
/// two same-zone hourly timestamps are. The anchor zone is irrelevant — only the
/// difference between two such counts is used.
fn civil_seconds(datetime: DateTime) -> i64 {
    datetime
        .to_zoned(jiff::tz::TimeZone::UTC)
        .map_or(0, |zoned| zoned.timestamp().as_second())
}

/// Map a WMO weather-interpretation code to a condition slug in the same
/// kebab-case vocabulary Day One imports use. `None` for an unknown code so the
/// field is simply omitted rather than guessed.
fn map_wmo_code(code: Option<i64>) -> Option<String> {
    let slug = match code? {
        0 => "clear",
        1 => "mostly-clear",
        2 => "partly-cloudy",
        3 => "cloudy",
        45 | 48 => "fog",
        51 | 53 | 55 | 56 | 57 => "drizzle",
        61 | 63 | 65 | 66 | 67 | 80 | 81 | 82 => "rain",
        71 | 73 | 75 | 77 | 85 | 86 => "snow",
        95 | 96 | 99 => "thunderstorm",
        _ => return None,
    };
    Some(slug.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(text: &str) -> DateTime {
        DateTime::strptime(OPEN_METEO_HOUR_FORMAT, text).unwrap()
    }

    // A minimal two-hour forecast response covering 13:00 and 14:00 local.
    const SAMPLE: &str = r#"{
        "hourly": {
            "time": ["2026-07-08T13:00", "2026-07-08T14:00"],
            "temperature_2m": [18.0, 21.5],
            "apparent_temperature": [17.0, 20.0],
            "relative_humidity_2m": [80, 62],
            "dew_point_2m": [14.0, 13.5],
            "surface_pressure": [1010.0, 1013.2],
            "visibility": [24000.0, 15000.0],
            "cloud_cover": [90, 40],
            "precipitation": [0.5, 0.0],
            "wind_speed_10m": [9.0, 12.0],
            "wind_gusts_10m": [20.0, 28.0],
            "wind_direction_10m": [180.0, 210.0],
            "weather_code": [3, 2]
        }
    }"#;

    #[test]
    fn maps_nearest_hour_and_converts_units() {
        let response: OpenMeteoResponse = serde_json::from_str(SAMPLE).unwrap();
        // 14:20 is nearest the 14:00 sample.
        let weather =
            extract_weather(&response.hourly.unwrap(), naive("2026-07-08T14:20")).unwrap();

        assert_eq!(weather.condition.as_deref(), Some("partly-cloudy"));
        assert_eq!(weather.temperature_celsius, Some(21.5));
        assert_eq!(weather.feels_like_celsius, Some(20.0));
        assert_eq!(weather.dew_point_celsius, Some(13.5));
        assert_eq!(weather.precipitation_mm, Some(0.0));
        // Percentages folded to 0–1 fractions, metres folded to kilometres.
        assert_eq!(weather.humidity, Some(0.62));
        assert_eq!(weather.cloud_cover, Some(0.4));
        assert_eq!(weather.visibility_km, Some(15.0));
        assert_eq!(weather.pressure_mb, Some(1013.2));
        assert_eq!(weather.source.as_deref(), Some("Open-Meteo"));
        assert_eq!(weather.wind_speed_kph, Some(12.0));
        assert_eq!(weather.wind_gust_kph, Some(28.0));
        assert_eq!(weather.wind_direction, Some(210.0));
    }

    #[test]
    fn picks_the_earlier_sample_when_nearest() {
        let response: OpenMeteoResponse = serde_json::from_str(SAMPLE).unwrap();
        let weather =
            extract_weather(&response.hourly.unwrap(), naive("2026-07-08T13:10")).unwrap();
        assert_eq!(weather.condition.as_deref(), Some("cloudy"));
        assert_eq!(weather.temperature_celsius, Some(18.0));
    }

    #[test]
    fn missing_series_and_gaps_yield_none_fields_and_drop_empty() {
        // Archive-style: some series omitted, others present but null at the hour.
        let body = r#"{"hourly":{"time":["2026-07-08T14:00"],
            "temperature_2m":[null],"weather_code":[null]}}"#;
        let response: OpenMeteoResponse = serde_json::from_str(body).unwrap();
        assert!(extract_weather(&response.hourly.unwrap(), naive("2026-07-08T14:00")).is_none());
    }

    #[test]
    fn no_hours_yields_none() {
        let body = r#"{"hourly":{"time":[]}}"#;
        let response: OpenMeteoResponse = serde_json::from_str(body).unwrap();
        assert!(extract_weather(&response.hourly.unwrap(), naive("2026-07-08T14:00")).is_none());
    }

    #[test]
    fn wmo_codes_map_to_condition_slugs() {
        assert_eq!(map_wmo_code(Some(0)).as_deref(), Some("clear"));
        assert_eq!(map_wmo_code(Some(61)).as_deref(), Some("rain"));
        assert_eq!(map_wmo_code(Some(75)).as_deref(), Some("snow"));
        assert_eq!(map_wmo_code(Some(95)).as_deref(), Some("thunderstorm"));
        assert_eq!(map_wmo_code(Some(7)), None);
        assert_eq!(map_wmo_code(None), None);
    }
}
