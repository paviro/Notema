//! Coordinate -> timezone resolution for a located entry, ranked: an IANA name
//! the geocoder supplied first, then an offline lookup from the coordinates, then
//! nothing (the caller keeps the system zone). Stamping an entry with the zone of
//! where it was written — rather than the machine's — keeps travel from skewing
//! its local time, its date-folder, and its sunrise/sunset.

use std::sync::LazyLock;

use jiff::Zoned;
use jiff::tz::TimeZone;
use notema_domain::Coordinates;

// Query the embedded data in place without expanding its polygons into memory.
static FINDER: LazyLock<tzf_rs::EmbeddedFinder> = LazyLock::new(tzf_rs::EmbeddedFinder::new);

/// The IANA zone the offline finder places `coordinates` in, if any. tzf-rs takes
/// longitude before latitude, and returns `""` for a point it can't place (open
/// ocean without maritime data); an empty or unresolvable name yields `None`.
fn finder_zone(coordinates: Coordinates) -> Option<TimeZone> {
    let name = FINDER.get_tz_name(coordinates.longitude(), coordinates.latitude());
    TimeZone::get(name).ok()
}

/// Resolve the timezone for a located entry, preferring `osm_zone` (an IANA name
/// from the geocoder) over the offline lookup. `None` means unresolved — the
/// caller should keep the system zone.
pub fn resolve_zone(coordinates: Coordinates, osm_zone: Option<&str>) -> Option<TimeZone> {
    osm_zone
        .and_then(|name| TimeZone::get(name).ok())
        .or_else(|| finder_zone(coordinates))
}

/// Re-express `datetime` in `zone`: the instant is unchanged, but the offset (and
/// so the wall-clock reading and the date) become those of `zone`.
pub fn rezone(datetime: Zoned, zone: TimeZone) -> Zoned {
    datetime.with_time_zone(zone)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coords(latitude: f64, longitude: f64) -> Coordinates {
        Coordinates::try_new(latitude, longitude).unwrap()
    }

    fn tz(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }

    #[test]
    fn finds_the_zone_for_a_land_point() {
        // Central Tokyo, well inside Japan.
        assert_eq!(finder_zone(coords(35.68, 139.767)), Some(tz("Asia/Tokyo")));
    }

    #[test]
    fn resolves_locations_near_timezone_borders() {
        assert_eq!(
            finder_zone(coords(22.5180, 114.0617)),
            Some(tz("Asia/Shanghai"))
        );
        assert_eq!(
            finder_zone(coords(22.3173, 114.1594)),
            Some(tz("Asia/Hong_Kong"))
        );
        assert_eq!(
            finder_zone(coords(41.903_699_636_969_634, 12.452_899_553_691_935)),
            Some(tz("Europe/Vatican"))
        );
    }

    #[test]
    fn resolves_an_ocean_location_without_geocoder_metadata() {
        assert_eq!(
            resolve_zone(coords(38.3530, -73.7729), None),
            Some(tz("Etc/GMT+5"))
        );
    }

    #[test]
    fn passes_longitude_and_latitude_in_the_right_order() {
        // (139.767, 35.68) is Tokyo; the swapped pair (35.68, 139.767) is an
        // invalid longitude, so a mixed-up call could never yield Asia/Tokyo.
        assert_eq!(finder_zone(coords(35.68, 139.767)), Some(tz("Asia/Tokyo")));
    }

    #[test]
    fn prefers_the_osm_zone_over_the_coordinates() {
        // Tokyo coordinates but an OSM-supplied Berlin zone — OSM wins.
        assert_eq!(
            resolve_zone(coords(35.68, 139.767), Some("Europe/Berlin")),
            Some(tz("Europe/Berlin"))
        );
    }

    #[test]
    fn falls_back_to_the_finder_when_osm_is_absent_or_unparseable() {
        let tokyo = coords(35.68, 139.767);
        assert_eq!(resolve_zone(tokyo, None), Some(tz("Asia/Tokyo")));
        assert_eq!(
            resolve_zone(tokyo, Some("Not/AZone")),
            Some(tz("Asia/Tokyo"))
        );
        assert_eq!(resolve_zone(tokyo, Some("")), Some(tz("Asia/Tokyo")));
    }

    #[test]
    fn rezone_keeps_the_instant_but_takes_the_zone_offset() {
        let utc = notema_domain::Timestamp::parse("2026-07-16T00:30:00+00:00")
            .parsed
            .unwrap();
        let tokyo = rezone(utc.clone(), tz("Asia/Tokyo"));
        // Same instant, Tokyo's summer offset (+09:00), so the wall clock and date roll forward.
        assert_eq!(tokyo.timestamp(), utc.timestamp());
        assert_eq!(tokyo.offset().seconds(), 9 * 3600);
        assert_eq!(
            tokyo.strftime("%Y-%m-%d %H:%M").to_string(),
            "2026-07-16 09:30"
        );
    }
}
