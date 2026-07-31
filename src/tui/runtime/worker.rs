//! A lazy-spawned background worker over an mpsc channel pair. Whole-library
//! reloads and the blocking network lookups (geocoding, weather/air quality)
//! each run on a dedicated thread and reply over a channel the event loop drains
//! every frame — the thread is spawned on the first request, so sessions that
//! never reach one pay nothing.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, Sender, TryRecvError, channel},
    },
    thread,
};

/// Handle to a background worker resolving `Req` into `Res`. `in_flight` counts
/// dispatched-but-not-yet-drained requests so the event loop can poll faster
/// while a lookup is outstanding. Shared with every [`Submission`], which
/// uncounts itself if dropped unsent.
pub(crate) struct Worker<Req, Res> {
    channels: Option<Channels<Req, Res>>,
    in_flight: Arc<AtomicUsize>,
    /// The worker died with requests outstanding. Set by the drain that notices,
    /// cleared by the caller that reports it.
    lost: bool,
}

/// A ticket to submit one request from wherever it is carried to. Dropping it
/// unsent uncounts the request, so an abandoned ticket cannot pin the worker
/// busy.
pub(crate) struct Submission<Req> {
    sender: Sender<Req>,
    pending: Arc<AtomicUsize>,
    sent: bool,
}

impl<Req> Submission<Req> {
    pub(crate) fn send(mut self, request: Req) {
        self.sent = true;
        let _ = self.sender.send(request);
    }
}

impl<Req> Drop for Submission<Req> {
    fn drop(&mut self) {
        if !self.sent {
            self.pending.fetch_sub(1, Ordering::Relaxed);
        }
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
            in_flight: Arc::default(),
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
        self.in_flight.fetch_add(1, Ordering::Relaxed);
        Submission {
            sender: channels.requests.clone(),
            pending: Arc::clone(&self.in_flight),
            sent: false,
        }
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
        self.in_flight.fetch_sub(results.len(), Ordering::Relaxed);
        if died {
            self.lost = self.in_flight.load(Ordering::Relaxed) > 0;
            // A fresh counter, not a reset: a ticket for the dead worker may
            // still be in flight elsewhere, and its drop must not uncount a
            // request of the replacement.
            self.in_flight = Arc::default();
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
        self.in_flight.load(Ordering::Relaxed) > 0
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn echo(value: u8) -> u8 {
        value
    }

    fn boom(_: u8) -> u8 {
        panic!("worker loss test")
    }

    #[test]
    fn a_ticket_dropped_unsent_uncounts_its_request() {
        let mut worker = Worker::<u8, u8>::default();
        drop(worker.submission(echo));
        assert!(!worker.has_pending());
    }

    #[test]
    fn a_sent_ticket_counts_until_its_result_is_drained() {
        let mut worker = Worker::<u8, u8>::default();
        worker.submission(echo).send(7);
        assert!(worker.has_pending());
        let deadline = Instant::now() + Duration::from_secs(5);
        let results = loop {
            let results = worker.drain();
            if !results.is_empty() {
                break results;
            }
            assert!(Instant::now() < deadline, "result never arrived");
            thread::sleep(Duration::from_millis(1));
        };
        assert_eq!(results, [7]);
        assert!(!worker.has_pending());
    }

    #[test]
    fn a_dead_worker_is_reported_lost_once() {
        let mut worker = Worker::<u8, u8>::default();
        worker.submission(boom).send(1);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let _ = worker.drain();
            if worker.take_lost() {
                break;
            }
            assert!(Instant::now() < deadline, "loss never observed");
            thread::sleep(Duration::from_millis(1));
        }
        assert!(!worker.has_pending());
        assert!(!worker.take_lost());
    }
}
