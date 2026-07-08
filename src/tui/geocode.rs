//! Background geocoding for the location dialog. Network calls block, so they
//! run on a dedicated thread and reply over a channel the event loop drains each
//! frame — the same shape as the image worker. The thread is spawned lazily on
//! the first lookup, so sessions that never open the dialog pay nothing.

use std::{
    sync::mpsc::{Receiver, Sender, channel},
    thread,
};

use journal_storage::{GeocodeHit, geocode, reverse_geocode};

/// How many forward-geocode candidates to request (Nominatim `limit`).
const CANDIDATE_LIMIT: usize = 6;

/// What a request wants resolved: a typed address, or coordinates to name.
pub(crate) enum GeocodeQuery {
    Address(String),
    Coords { lat: f64, lon: f64 },
}

/// A lookup handed to the worker, tagged with the request id the dialog assigned.
pub(crate) struct GeocodeRequest {
    pub(crate) id: u64,
    pub(crate) query: GeocodeQuery,
}

/// A finished lookup coming back. `hits` holds the candidates (forward) or the
/// zero/one reverse result; `Err` carries a human-readable failure for the
/// status line.
pub(crate) struct GeocodeResult {
    pub(crate) id: u64,
    pub(crate) reverse: bool,
    pub(crate) hits: Result<Vec<GeocodeHit>, String>,
}

/// Handle to the background geocoding thread, spawned on first use.
#[derive(Default)]
pub(crate) struct GeocodeWorker {
    channels: Option<Channels>,
}

struct Channels {
    requests: Sender<GeocodeRequest>,
    results: Receiver<GeocodeResult>,
}

impl GeocodeWorker {
    /// Dispatch a lookup, spawning the worker thread on the first call.
    pub(crate) fn request(&mut self, request: GeocodeRequest) {
        let channels = self.channels.get_or_insert_with(spawn);
        let _ = channels.requests.send(request);
    }

    /// Drain every finished lookup (empty when the worker was never started).
    pub(crate) fn drain(&self) -> Vec<GeocodeResult> {
        match &self.channels {
            Some(channels) => channels.results.try_iter().collect(),
            None => Vec::new(),
        }
    }
}

fn spawn() -> Channels {
    let (request_tx, request_rx) = channel::<GeocodeRequest>();
    let (result_tx, result_rx) = channel::<GeocodeResult>();
    thread::spawn(move || worker_loop(request_rx, result_tx));
    Channels {
        requests: request_tx,
        results: result_rx,
    }
}

/// Resolve each request in turn — serial by construction, which also keeps us
/// under Nominatim's one-request-per-second ceiling. Exits when the request
/// channel is dropped (the app is shutting down).
fn worker_loop(requests: Receiver<GeocodeRequest>, results: Sender<GeocodeResult>) {
    while let Ok(request) = requests.recv() {
        let (reverse, hits) = match request.query {
            GeocodeQuery::Address(query) => (
                false,
                geocode(&query, CANDIDATE_LIMIT).map_err(|error| error.to_string()),
            ),
            GeocodeQuery::Coords { lat, lon } => (
                true,
                reverse_geocode(lat, lon)
                    .map(|hit| hit.into_iter().collect())
                    .map_err(|error| error.to_string()),
            ),
        };
        let _ = results.send(GeocodeResult {
            id: request.id,
            reverse,
            hits,
        });
    }
}
