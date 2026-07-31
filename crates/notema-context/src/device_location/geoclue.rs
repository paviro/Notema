//! Linux provider: GeoClue2 over the system D-Bus. Note that GeoClue's default
//! Wi-Fi backend (Mozilla Location Service) was retired in 2024, so on a machine
//! without a GPS device and without a reconfigured backend (e.g. BeaconDB) this
//! returns "no location" — which the dialog surfaces plainly.

use super::{DeviceFix, DeviceLocationSource};
use crate::{ContextError, Result};
use futures_lite::StreamExt;
use notema_domain::Coordinates;
use std::time::Duration;
use zbus::{Connection, proxy, zvariant::OwnedObjectPath};

/// How long to wait for GeoClue to produce a fix before giving up.
const TIMEOUT: Duration = Duration::from_secs(30);
/// GeoClue's stable identifier for us; also the accuracy level we ask for.
const DESKTOP_ID: &str = "de.paviro.notema";
/// `GCLUE_ACCURACY_LEVEL_EXACT` — ask for the most precise fix available.
const ACCURACY_EXACT: u32 = 8;

#[proxy(
    interface = "org.freedesktop.GeoClue2.Manager",
    default_service = "org.freedesktop.GeoClue2",
    default_path = "/org/freedesktop/GeoClue2/Manager"
)]
trait Manager {
    fn get_client(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.freedesktop.GeoClue2.Client",
    default_service = "org.freedesktop.GeoClue2"
)]
trait Client {
    fn start(&self) -> zbus::Result<()>;
    fn stop(&self) -> zbus::Result<()>;

    #[zbus(property)]
    fn set_desktop_id(&self, id: &str) -> zbus::Result<()>;
    #[zbus(property)]
    fn set_requested_accuracy_level(&self, level: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn location_updated(&self, old: OwnedObjectPath, new: OwnedObjectPath) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.GeoClue2.Location",
    default_service = "org.freedesktop.GeoClue2"
)]
trait Location {
    #[zbus(property)]
    fn latitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn longitude(&self) -> zbus::Result<f64>;
    #[zbus(property)]
    fn accuracy(&self) -> zbus::Result<f64>;
}

pub(super) fn locate() -> Result<DeviceFix> {
    // Race the whole exchange against the deadline: no backend may ever answer.
    // On timeout the query future is dropped, which closes the D-Bus connection;
    // GeoClue destroys the client (stopping the location hardware) when its
    // peer vanishes from the bus.
    zbus::block_on(futures_lite::future::or(query_geoclue(), async {
        async_io::Timer::after(TIMEOUT).await;
        Err(ContextError::message(
            "timed out waiting for a location fix from GeoClue",
        ))
    }))
}

async fn query_geoclue() -> Result<DeviceFix> {
    let connection = Connection::system().await.map_err(|error| {
        ContextError::message(format!("cannot reach the system D-Bus: {error}"))
    })?;

    let manager = ManagerProxy::new(&connection)
        .await
        .map_err(|_| ContextError::message("GeoClue2 is not available on this system"))?;
    let client_path = manager
        .get_client()
        .await
        .map_err(|error| ContextError::message(format!("GeoClue refused a client: {error}")))?;

    let client = ClientProxy::builder(&connection)
        .path(client_path)?
        .build()
        .await?;
    client.set_desktop_id(DESKTOP_ID).await?;
    client.set_requested_accuracy_level(ACCURACY_EXACT).await?;

    // Subscribe before Start so we can't miss the first update.
    let mut updates = client.receive_location_updated().await?;
    client.start().await.map_err(|error| {
        ContextError::message(format!("GeoClue could not start locating: {error}"))
    })?;

    let fix = match updates.next().await {
        Some(signal) => {
            let args = signal.args()?;
            read_location(&connection, args.new).await
        }
        None => Err(ContextError::message("GeoClue returned no location")),
    };
    let _ = client.stop().await;
    fix
}

async fn read_location(connection: &Connection, path: OwnedObjectPath) -> Result<DeviceFix> {
    let location = LocationProxy::builder(connection)
        .path(path)?
        .build()
        .await?;
    let coordinates = Coordinates::try_new(location.latitude().await?, location.longitude().await?)
        .map_err(|error| ContextError::message(error.to_string()))?;
    Ok(DeviceFix {
        coordinates,
        accuracy_m: location.accuracy().await.ok(),
        source: DeviceLocationSource::GeoClue,
    })
}
