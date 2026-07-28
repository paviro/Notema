//! A lazy-spawned background worker over an mpsc channel pair. Whole-library
//! reloads and the blocking network lookups (geocoding, weather/air quality)
//! each run on a dedicated thread and reply over a channel the event loop drains
//! every frame — the thread is spawned on the first request, so sessions that
//! never reach one pay nothing.

use std::{
    sync::mpsc::{Receiver, Sender, TryRecvError, channel},
    thread,
};

/// Handle to a background worker resolving `Req` into `Res`. `in_flight` counts
/// dispatched-but-not-yet-drained requests so the event loop can poll faster
/// while a lookup is outstanding.
pub(crate) struct Worker<Req, Res> {
    channels: Option<Channels<Req, Res>>,
    in_flight: usize,
    /// The worker died with requests outstanding. Set by the drain that notices,
    /// cleared by the caller that reports it.
    lost: bool,
}

/// A ticket to submit one request from wherever it is carried to. Dropping it
/// without sending leaves the worker counted busy until a drain notices, which
/// is why it is consumed by the send.
pub(crate) struct Submission<Req>(Sender<Req>);

impl<Req> Submission<Req> {
    pub(crate) fn send(self, request: Req) {
        let _ = self.0.send(request);
    }
}

struct Channels<Req, Res> {
    requests: Sender<Req>,
    results: Receiver<Res>,
}

impl<Req, Res> Default for Worker<Req, Res> {
    fn default() -> Self {
        Self {
            channels: None,
            in_flight: 0,
            lost: false,
        }
    }
}

impl<Req: Send + 'static, Res: Send + 'static> Worker<Req, Res> {
    /// Dispatch a request, spawning the worker thread on the first call. `handler`
    /// resolves each request on that thread; it's a plain `fn` (no captured
    /// state) shared by every call.
    pub(crate) fn request(&mut self, request: Req, handler: fn(Req) -> Res) {
        self.submission(handler).send(request);
    }

    /// A ticket for one request to be sent later, possibly from another thread.
    ///
    /// The work counts as outstanding from the moment the ticket is taken, not
    /// from the send: a caller that hands one to a background thread means the
    /// request to happen, and a poll in between must not read the worker as idle.
    pub(crate) fn submission(&mut self, handler: fn(Req) -> Res) -> Submission<Req> {
        let channels = self.channels.get_or_insert_with(|| spawn(handler));
        self.in_flight += 1;
        Submission(channels.requests.clone())
    }

    /// Drain every finished result (empty when the worker was never started).
    ///
    /// A worker whose thread died — a panic inside the handler — is forgotten
    /// here rather than left counted as busy forever, which would pin
    /// [`Self::has_pending`] true and starve every later request. The next
    /// request spawns a fresh thread; [`Self::take_lost`] reports the loss.
    pub(crate) fn drain(&mut self) -> Vec<Res> {
        let Some(channels) = &self.channels else {
            return Vec::new();
        };
        let mut results = Vec::new();
        let died = loop {
            match channels.results.try_recv() {
                Ok(result) => results.push(result),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        self.in_flight = self.in_flight.saturating_sub(results.len());
        if died {
            self.lost = self.in_flight > 0;
            self.in_flight = 0;
            self.channels = None;
        }
        results
    }

    /// Whether the worker died mid-request since this was last asked.
    pub(crate) fn take_lost(&mut self) -> bool {
        std::mem::take(&mut self.lost)
    }

    /// Whether a request is still outstanding.
    pub(crate) fn has_pending(&self) -> bool {
        self.in_flight > 0
    }
}

fn spawn<Req: Send + 'static, Res: Send + 'static>(handler: fn(Req) -> Res) -> Channels<Req, Res> {
    let (request_tx, request_rx) = channel::<Req>();
    let (result_tx, result_rx) = channel::<Res>();
    thread::spawn(move || {
        // Resolve each request in turn — serial by construction. Exits when the
        // request channel is dropped (the app is shutting down).
        while let Ok(request) = request_rx.recv() {
            if result_tx.send(handler(request)).is_err() {
                break;
            }
        }
    });
    Channels {
        requests: request_tx,
        results: result_rx,
    }
}
