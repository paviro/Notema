//! Background weather/air-quality/celestial fetching over the shared [`Worker`].
//! A located entry's environment is fetched off the event loop — spawned when a
//! location is picked (attached to the entry on save) or paced out at startup to
//! backfill entries that never had it — so no save ever blocks on the network.

use std::path::PathBuf;

use jiff::Zoned;
use notema_context::{EnvironmentReport, EnvironmentWants, fetch_environment};
use notema_domain::{Coordinates, MetadataField};

use crate::tui::runtime::worker::Worker;

/// The background environment worker, spawned on first use.
pub(crate) type EnvironmentWorker = Worker<EnvironmentRequest, EnvironmentResult>;

/// Where a finished lookup's data belongs, so the drain step can route it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EnvironmentTarget {
    /// Attach to the open editor's draft, matched by the request id.
    Editor,
    /// Write back to this entry file (direct location-set or parse-time backfill).
    Entry(PathBuf),
}

/// A environment lookup handed to the worker, tagged with an id and its destination.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EnvironmentRequest {
    pub(crate) id: u64,
    pub(crate) coordinates: Coordinates,
    pub(crate) datetime: Zoned,
    pub(crate) target: EnvironmentTarget,
}

/// A finished environment lookup coming back to the event loop.
pub(crate) struct EnvironmentResult {
    pub(crate) id: u64,
    pub(crate) target: EnvironmentTarget,
    pub(crate) environment: EnvironmentReport,
}

/// Resolve one environment request. Runs on the worker thread. Celestial is
/// offline and always present; weather/air quality are dropped to `None` on
/// no-data or transport failure (the caller can't do anything with the error
/// mid-save).
pub(crate) fn resolve(request: EnvironmentRequest) -> EnvironmentResult {
    let environment = fetch_environment(
        request.coordinates,
        request.datetime,
        EnvironmentWants::all(),
    );
    EnvironmentResult {
        id: request.id,
        target: request.target,
        environment,
    }
}

/// The metadata fields to persist for a fetched environment — only the parts that
/// came back present, so an absent weather/air reading isn't written as cleared.
pub(crate) fn environment_fields(environment: &EnvironmentReport) -> Vec<MetadataField> {
    let mut fields = Vec::new();
    fields.push(MetadataField::Celestial(Some(Box::new(
        environment.celestial.clone(),
    ))));
    if let Some(weather) = &environment.weather {
        fields.push(MetadataField::Weather(Some(Box::new(weather.clone()))));
    }
    if let Some(air_quality) = &environment.air_quality {
        fields.push(MetadataField::AirQuality(Some(Box::new(
            air_quality.clone(),
        ))));
    }
    fields
}
