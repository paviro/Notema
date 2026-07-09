//! Background geocoding for the location dialog, over the shared [`Worker`]. The
//! worker resolves requests serially, which also keeps us under Nominatim's
//! one-request-per-second ceiling.

use crate::tui::worker::Worker;
use journal_storage::{GeocodeHit, device_location, geocode, reverse_geocode};

/// How many forward-geocode candidates to request (Nominatim `limit`).
const CANDIDATE_LIMIT: usize = 6;

/// The background geocoding worker, spawned on first use.
pub(crate) type GeocodeWorker = Worker<GeocodeRequest, GeocodeResult>;

/// What a request wants resolved: a typed address, coordinates to name, or the
/// device's own current location (which is then named like any coordinates).
pub(crate) enum GeocodeQuery {
    Address(String),
    Coords { lat: f64, lon: f64 },
    Device,
}

/// A lookup handed to the worker, tagged with the request id the dialog assigned.
pub(crate) struct GeocodeRequest {
    pub(crate) id: u64,
    pub(crate) query: GeocodeQuery,
}

/// A finished lookup coming back. `hits` holds the candidates (forward) or the
/// zero/one reverse result; `Err` carries a human-readable failure for the
/// status line. `device_coords` is the fix a `Device` request grabbed, so the
/// dialog can seed its query field before the reverse names are applied.
pub(crate) struct GeocodeResult {
    pub(crate) id: u64,
    pub(crate) reverse: bool,
    pub(crate) hits: Result<Vec<GeocodeHit>, String>,
    pub(crate) device_coords: Option<(f64, f64)>,
}

/// Resolve one geocoding request. Runs on the worker thread.
pub(crate) fn resolve(request: GeocodeRequest) -> GeocodeResult {
    let mut device_coords = None;
    let (reverse, hits) = match request.query {
        GeocodeQuery::Address(query) => (
            false,
            geocode(&query, CANDIDATE_LIMIT).map_err(|error| error.to_string()),
        ),
        GeocodeQuery::Coords { lat, lon } => (true, reverse_hits(lat, lon)),
        // Grab the device's position, then name it through the same reverse path.
        GeocodeQuery::Device => match device_location() {
            Ok(fix) => {
                device_coords = Some((fix.latitude, fix.longitude));
                (true, reverse_hits(fix.latitude, fix.longitude))
            }
            Err(error) => (true, Err(error.to_string())),
        },
    };
    GeocodeResult {
        id: request.id,
        reverse,
        hits,
        device_coords,
    }
}

/// Reverse-geocode coordinates into the zero-or-one hit the dialog expects.
fn reverse_hits(lat: f64, lon: f64) -> Result<Vec<GeocodeHit>, String> {
    reverse_geocode(lat, lon)
        .map(|hit| hit.into_iter().collect())
        .map_err(|error| error.to_string())
}
