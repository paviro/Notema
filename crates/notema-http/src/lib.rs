#![forbid(unsafe_code)]

//! The blocking HTTP GET shared by the two network callers: context provider
//! lookups (geocoding, weather, air quality) and remote asset download. One
//! process-wide `ureq` agent, a global timeout, a short connect cap, and a
//! per-call bound on the response body.

use std::{path::Path, sync::OnceLock, sync::mpsc, thread, time::Duration};

/// Budget for a whole request once the connection is up.
pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Cap on establishing the TCP connection. Shorter than the global timeout so a
/// dead or black-holed network (offline, dropped SYNs, captive portals) gives up
/// fast, while a slow-but-alive server still gets the full [`TIMEOUT`] to respond.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Identifies the application. Nominatim's policy requires a descriptive
/// `User-Agent` (a stock HTTP library one is rejected); nobody else objects, so
/// one value serves every caller.
const USER_AGENT: &str = concat!("notema-tui/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error(transparent)]
    Transport(#[from] ureq::Error),
    #[error("request timed out after {} seconds", .0.as_secs())]
    TimedOut(Duration),
    #[error("request worker stopped unexpectedly")]
    WorkerLost,
}

/// Fetch `url` as a UTF-8 string, reading at most `max_bytes` of body.
pub fn get_string(url: &str, max_bytes: u64) -> Result<String, HttpError> {
    run(url, move |url| {
        Ok(agent()
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()?
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_string()?)
    })
}

/// Fetch `url` as bytes, reading at most `max_bytes` of body.
pub fn get_bytes(url: &str, max_bytes: u64) -> Result<Vec<u8>, HttpError> {
    run(url, move |url| {
        Ok(agent()
            .get(url)
            .header("User-Agent", USER_AGENT)
            .call()?
            .body_mut()
            .with_config()
            .limit(max_bytes)
            .read_to_vec()?)
    })
}

/// Run `fetch`, enforcing [`TIMEOUT`] in user space on iSH, where the agent has
/// no timeout of its own (see [`agent_config_for`]).
fn run<T: Send + 'static>(
    url: &str,
    fetch: impl FnOnce(&str) -> Result<T, HttpError> + Send + 'static,
) -> Result<T, HttpError> {
    if !is_ish() {
        return fetch(url);
    }
    let url = url.to_string();
    let (tx, rx) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = tx.send(fetch(&url));
    });
    match rx.recv_timeout(TIMEOUT) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(HttpError::TimedOut(TIMEOUT)),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(HttpError::WorkerLost),
    }
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| agent_config_for(is_ish()).into())
}

/// The agent configuration, split by platform because iSH cannot carry the
/// normal one: its Linux emulation rejects the socket options `no_delay` and the
/// timeouts are built from (`TCP_NODELAY`, `SO_RCVTIMEO`/`SO_SNDTIMEO`), so a
/// request configured that way fails outright instead of timing out. There the
/// agent is left bare and [`run`] enforces the deadline from a helper thread.
fn agent_config_for(ish: bool) -> ureq::config::Config {
    let builder = ureq::Agent::config_builder();
    let builder = if ish {
        builder.no_delay(false)
    } else {
        builder
            .timeout_global(Some(TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
    };
    #[cfg(feature = "tls-native")]
    let builder = builder.tls_config(
        ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .build(),
    );
    builder.build()
}

/// Whether we're running under iSH, which emulates a 32-bit Linux and marks
/// itself with `/proc/ish/version`. The storage folder side of iSH support lives
/// in the binary's `platform::ish`.
pub fn is_ish() -> bool {
    cfg!(target_os = "linux") && Path::new("/proc/ish/version").exists()
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "tls-native")]
    #[test]
    fn native_tls_feature_selects_native_tls_provider() {
        assert_eq!(
            super::agent_config_for(false).tls_config().provider(),
            ureq::tls::TlsProvider::NativeTls
        );
    }

    #[test]
    fn ish_avoids_kernel_socket_options() {
        let config = super::agent_config_for(true);
        assert!(!config.no_delay());
        assert_eq!(config.timeouts().global, None);
    }

    #[test]
    fn other_platforms_keep_the_global_timeout() {
        let config = super::agent_config_for(false);
        assert!(config.no_delay());
        assert_eq!(config.timeouts().global, Some(super::TIMEOUT));
        // A short connect cap fails fast on a dead network without shortening the
        // read budget for a slow-but-alive server.
        assert_eq!(config.timeouts().connect, Some(super::CONNECT_TIMEOUT));
    }

    #[cfg(all(feature = "tls-ring", not(feature = "tls-native")))]
    #[test]
    fn ring_feature_selects_rustls_provider() {
        assert_eq!(
            super::agent_config_for(false).tls_config().provider(),
            ureq::tls::TlsProvider::Rustls
        );
    }
}
