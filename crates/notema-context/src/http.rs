//! The keyless network lookups (geocoding via Nominatim, weather and air quality
//! via Open-Meteo) on top of the shared agent in `notema-http`.

use crate::Result;

/// Upper bound on a response body (bytes) — the JSON these APIs return is tiny.
const MAX_BODY_BYTES: u64 = 2 * 1024 * 1024;

/// Fetch `url` as a UTF-8 string, or an error on transport/HTTP/decoding failure.
pub(crate) fn get(url: &str) -> Result<String> {
    Ok(notema_http::get_string(url, MAX_BODY_BYTES)?)
}
