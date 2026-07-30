#![forbid(unsafe_code)]

//! Context providers: the external data sources sampled when an entry is written
//! — place (Nominatim geocoding + platform GPS), weather and air quality
//! (Open-Meteo), and celestial state (sun/moon). Each takes coordinates and a
//! time and returns a `notema-domain` value; none of this is local storage, so it
//! lives outside `notema-storage`.

use chrono::{DateTime, FixedOffset};
use notema_domain::{AirQuality, Celestial, Coordinates, Weather};

mod air;
mod celestial;
mod device_location;
mod error;
mod geocode;
mod http;
mod timezone;
mod weather;

pub use air::fetch_air_quality;
pub use celestial::compute_celestial;
pub use device_location::{DeviceFix, DeviceLocationSource, device_location};
pub use error::{ContextError, Result};
pub use geocode::{GeocodeHit, geocode, reverse_geocode};
pub use timezone::{resolve_zone, rezone};
pub use weather::fetch_weather;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentProvider {
    Weather,
    AirQuality,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentWarning {
    pub provider: EnvironmentProvider,
    pub message: String,
}

/// Which network providers [`fetch_environment`] should consult. A caller that
/// already holds one reading asks for only the other, so a refetch never costs a
/// request whose result would be discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentWants {
    pub weather: bool,
    pub air_quality: bool,
}

impl EnvironmentWants {
    pub fn all() -> Self {
        Self {
            weather: true,
            air_quality: true,
        }
    }
}

/// The environment captured for one place and instant. Celestial data is local
/// and always present; independent network providers may return no observation
/// or record a warning without discarding the other results.
#[derive(Debug, Clone, Default)]
pub struct EnvironmentReport {
    pub celestial: Celestial,
    pub weather: Option<Weather>,
    pub air_quality: Option<AirQuality>,
    pub warnings: Vec<EnvironmentWarning>,
    /// How many HTTP requests this actually issued, so a caller pacing itself
    /// against the providers can tell a real fetch from a skipped one.
    pub requests: usize,
}

pub fn fetch_environment(
    coordinates: Coordinates,
    datetime: DateTime<FixedOffset>,
    wants: EnvironmentWants,
) -> EnvironmentReport {
    let celestial = compute_celestial(coordinates, datetime);
    let mut warnings = Vec::new();
    let mut requests = 0;

    let weather = wants.weather.then(|| {
        requests += 1;
        fetch_weather(coordinates, datetime).unwrap_or_else(|error| {
            warnings.push(EnvironmentWarning {
                provider: EnvironmentProvider::Weather,
                message: error.to_string(),
            });
            None
        })
    });
    // Skip air outside its coverage rather than spend a request on a certain 400.
    let air_quality = (wants.air_quality && air::covers(datetime)).then(|| {
        requests += 1;
        fetch_air_quality(coordinates, datetime).unwrap_or_else(|error| {
            warnings.push(EnvironmentWarning {
                provider: EnvironmentProvider::AirQuality,
                message: error.to_string(),
            });
            None
        })
    });

    EnvironmentReport {
        celestial,
        weather: weather.flatten(),
        air_quality: air_quality.flatten(),
        warnings,
        requests,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn berlin() -> Coordinates {
        Coordinates::try_new(52.52, 13.4).unwrap()
    }

    fn at(text: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(text).unwrap()
    }

    #[test]
    fn wanting_nothing_still_reports_celestial_offline() {
        let report = fetch_environment(
            berlin(),
            at("2026-07-08T14:20:00+02:00"),
            EnvironmentWants {
                weather: false,
                air_quality: false,
            },
        );

        assert_eq!(report.requests, 0);
        assert!(report.warnings.is_empty());
        assert!(report.celestial.sunrise.is_some());
    }

    #[test]
    fn air_before_coverage_costs_no_request_and_no_warning() {
        let report = fetch_environment(
            berlin(),
            at("2012-12-21T05:54:08+01:00"),
            EnvironmentWants {
                weather: false,
                air_quality: true,
            },
        );

        assert_eq!(report.requests, 0);
        assert!(report.warnings.is_empty());
        assert!(report.air_quality.is_none());
    }
}
