//! Bounded queue + retry worker behind [`super::InfluxDbExporter`] (task 81
//! [A-M8, A-M10]).
//!
//! Before this module every sample rode a synchronous write on the caller's
//! task, so a stalled InfluxDB parked one hanging task per request - unbounded
//! in both task count and hang duration (captured at f1efa80: an 8-sample
//! flood left 8/8 tasks pending and `export()` never completed). Now
//! `export()` is a non-blocking `try_send` into a queue of [`CAPACITY`]
//! samples: past the ceiling the sample is dropped, counted, and logged at
//! DEBUG - the overflow policy sacrifices telemetry, never the request path.
//! The worker owns the network: exactly ONE thread per exporter (fixed spawn
//! count) and 3 retries per point. The drain-exit HONESTY IS BOUNDED: when
//! the senders close the worker delivers every queued sample on its OWN
//! thread — FIFO, never discarded at close — but nobody is forced to wait for
//! that thread. Only [`WriterQueue::shutdown`] (via `MetricsExporter::shutdown`
//! through `ExporterManager::shutdown_all`, which the server calls AFTER the
//! graceful connection drain per the F12 contract) joins it, within a budget;
//! a plain `Drop` closes without joining, so there the accepted-but-unwritten
//! samples drain in racing with process exit and no loss is ever reported.
//!
//! All queue plumbing is plain `std::sync::mpsc` and the worker runs on its
//! own OS thread, so neither the senders nor the shutdown join ever depends
//! on a tokio runtime existing, being alive, or making progress.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{build_data_point, ExportError, InfluxDbConfig, RequestMetrics};

/// Samples buffered ahead of the writer. The queue's whole purpose is a
/// ceiling: past it, new samples lose to the request path.
pub const CAPACITY: usize = 1024;

/// Sleep before retry N of the same point: 3 retries after the first attempt.
const RETRY_DELAYS_MS: &[u64] = &[100, 500, 2_000];

/// Bounded worker wake-up poll: `mpsc::Receiver` has no timed recv, so the
/// idle loop probes this often. Only ever delays a drain ack by this much.
const IDLE_POLL: Duration = Duration::from_millis(2);

/// Upper bound on a flush waiter: generously above 3-retry backoff (2.6 s)
/// plus four 10 s request timeouts. Only a worker stuck in a never-returning
/// call can reach it.
const FLUSH_WAIT: Duration = Duration::from_secs(60);

/// Wall-clock bound on ONE write attempt (task 83 [A-M8/L8]). influxdb2 0.5
/// pins reqwest 0.11 internally and its builder exposes no timeout knob our
/// reqwest 0.12 can drive, so the timeout is enforced at the call site with
/// tokio - same contract: a stalled endpoint fails the attempt in 10 s and
/// the retry chain runs, instead of parking the worker forever.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Upper bound on a shutdown join: the worker may carry a network call that
/// cannot finish (pre-task-83 clients had no timeout). Shutdown warns and
/// proceeds instead of wedging the process; the worker still drains and
/// exits on its own once the stuck call returns.
const JOIN_BUDGET: Duration = Duration::from_secs(120);

/// Queue statistics shared with the exporter.
pub type Counter = Arc<AtomicU64>;

/// One flush waiter: the worker holds the ack half, the waiter keeps the
/// receive half and an identity tag, so a waiter that gave up can find and
/// remove its own slot (`mpsc::Sender` is not comparable by identity itself).
struct Ticket {
    ack: Sender<()>,
    id: Arc<()>,
}

impl Ticket {
    fn new() -> (Self, Receiver<()>) {
        let (ack, ack_rx) = mpsc::channel();
        let id = Arc::new(());
        (
            Self {
                ack,
                id: Arc::clone(&id),
            },
            ack_rx,
        )
    }

    fn is_same_waiter(&self, id: &Arc<()>) -> bool {
        Arc::ptr_eq(&self.id, id)
    }
}

/// What [`WriterQueue::enqueue`] decided about one sample.
#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Queued,
    /// Queue at [`CAPACITY`]: refused, request path untouched, counted in
    /// [`WriterQueue::dropped_total`].
    Dropped,
    /// The worker is gone (shutdown ran); nothing will accept again.
    ShutDown,
}

/// A queued sample is available, the queue is empty, or the senders closed.
enum Poll {
    Ready(Box<RequestMetrics>),
    Empty,
    Gone,
}

fn poll_samples(samples: &mpsc::Receiver<RequestMetrics>) -> Poll {
    match samples.try_recv() {
        Ok(sample) => Poll::Ready(Box::new(sample)),
        Err(mpsc::TryRecvError::Empty) => Poll::Empty,
        Err(mpsc::TryRecvError::Disconnected) => Poll::Gone,
    }
}

/// Everything a write needs: the client, its destination, the per-attempt
/// bound, and the two counters the chain owns. Bundled so the writer chain
/// stays narrow and no call site can desync one piece from another.
struct WriteCtx {
    client: influxdb2::Client,
    config: Arc<InfluxDbConfig>,
    request_timeout: Duration,
    retries: Counter,
    dropped: Counter,
}

/// Deliver queued samples until the queue reports empty or closed. A drain
/// ticket registered before the worker picked up its drain ping guards
/// enqueues that happened before the ticket - those sit in this same FIFO,
/// so an empty queue here means every guarded enqueue is delivered or
/// exhaustion-dropped.
async fn drain_queue(ctx: &WriteCtx, samples: &mpsc::Receiver<RequestMetrics>, acked: &mut dyn FnMut()) {
    loop {
        match poll_samples(samples) {
            Poll::Ready(sample) => {
                write_with_retry(ctx, &sample).await;
            }
            Poll::Empty => {
                acked();
                return;
            }
            Poll::Gone => return,
        }
    }
}

fn spawn_writer(
    samples: mpsc::Receiver<RequestMetrics>,
    drains: mpsc::Receiver<()>,
    waiters: Arc<Mutex<Vec<Ticket>>>,
    ctx: WriteCtx,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("influxdb-exporter".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("current-thread runtime for the influxdb exporter");
            let ack_waiters = |waiters: &Arc<Mutex<Vec<Ticket>>>| {
                // Swap-remove dead slots (their receivers are gone) and ack
                // the living ones; a failed send means the waiter gave up.
                let mut slots = waiters.lock().unwrap();
                let mut i = 0;
                while i < slots.len() {
                    if slots[i].ack.send(()).is_err() {
                        slots.swap_remove(i);
                    } else {
                        i += 1;
                    }
                }
            };
            rt.block_on(async move {
                loop {
                    match drains.try_recv() {
                        Ok(()) => {
                            // A ping may answer flush tickets registered after
                            // this iteration's Empty check: the drain below
                            // re-acks an empty queue, so a too-early ping is
                            // harmless.
                            drain_queue(&ctx, &samples, &mut || ack_waiters(&waiters)).await;
                            continue;
                        }
                        Err(mpsc::TryRecvError::Disconnected) => break,
                        Err(mpsc::TryRecvError::Empty) => {}
                    }
                    match poll_samples(&samples) {
                        Poll::Ready(sample) => {
                            write_with_retry(&ctx, &sample).await;
                        }
                        Poll::Gone => break,
                        Poll::Empty => {
                            // Provable ack point: the FIFO says every sample
                            // enqueued before any current ticket's ping has
                            // been delivered or exhaustion-dropped.
                            ack_waiters(&waiters);
                            tokio::time::sleep(IDLE_POLL).await;
                        }
                    }
                }
                // Senders all closed: deliver everything still queued, then
                // leave - never discard what export() reported as Queued.
                drain_queue(&ctx, &samples, &mut || ack_waiters(&waiters)).await;
                ack_waiters(&waiters);
            })
        })
        .expect("spawn influxdb exporter writer thread")
}

/// One point, up to 4 attempts (first + 3 retries). Exhaustion is counted in
/// the ctx's dropped counter - the write path never reports a success it did
/// not get.
async fn write_with_retry(ctx: &WriteCtx, sample: &RequestMetrics) {
    let point = match build_data_point(sample) {
        Ok(Some(point)) => point,
        // Unrepresentable timestamp: skipped, counted, debugged - never a
        // fabricated epoch-0 line on the wire.
        Ok(None) => {
            tracing::debug!(
                model = %sample.model,
                timestamp = %sample.timestamp,
                "influxdb timestamp unrepresentable - point skipped (never epoch 0)"
            );
            ctx.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        Err(e) => {
            tracing::debug!(error = %e, "influxdb point rejected before write");
            ctx.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
    };
    for attempt in 0..=RETRY_DELAYS_MS.len() {
        match write_once(ctx, point.clone()).await {
            Ok(()) => return,
            Err(e) => {
                let Some(delay_ms) = RETRY_DELAYS_MS.get(attempt).copied() else {
                    ctx.dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(
                        error = %e,
                        attempts = attempt + 1,
                        "influxdb write exhausted its retries - sample dropped"
                    );
                    return;
                };
                ctx.retries.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

async fn write_once(ctx: &WriteCtx, point: influxdb2::models::DataPoint) -> Result<(), ExportError> {
    use futures::stream;
    let write = ctx.client.write(&ctx.config.bucket, stream::iter(vec![point]));
    match tokio::time::timeout(ctx.request_timeout, write).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(ExportError::Write(e.to_string())),
        Err(_) => Err(ExportError::Write(format!("write timed out after {:?}", ctx.request_timeout))),
    }
}

/// The queue end held by [`super::InfluxDbExporter`]. `shutdown()` and
/// `Drop` close the senders by TAKING them out of their Options, so both are
/// idempotent and the close is real (dropping local references closes
/// nothing).
pub struct WriterQueue {
    samples_tx: Mutex<Option<SyncSender<RequestMetrics>>>,
    drains_tx: Mutex<Option<Sender<()>>>,
    waiters: Arc<Mutex<Vec<Ticket>>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    retries: Counter,
    dropped: Counter,
}

impl WriterQueue {
    pub fn spawn(client: influxdb2::Client, config: InfluxDbConfig, request_timeout: Duration) -> Self {
        let (samples_tx, samples_rx) = mpsc::sync_channel(CAPACITY);
        let (drains_tx, drains_rx) = mpsc::channel();
        let waiters = Arc::new(Mutex::new(Vec::new()));
        let retries = Arc::new(AtomicU64::new(0));
        let dropped = Arc::new(AtomicU64::new(0));
        let ctx = WriteCtx {
            client,
            config: Arc::new(config),
            request_timeout,
            retries: Arc::clone(&retries),
            dropped: Arc::clone(&dropped),
        };
        let worker = spawn_writer(samples_rx, drains_rx, Arc::clone(&waiters), ctx);
        Self {
            samples_tx: Mutex::new(Some(samples_tx)),
            drains_tx: Mutex::new(Some(drains_tx)),
            waiters,
            worker: Mutex::new(Some(worker)),
            retries,
            dropped,
        }
    }

    /// Never blocks: full means refused, and the refusal is a counted fact,
    /// not a silent loss.
    pub fn enqueue(&self, sample: RequestMetrics) -> SendOutcome {
        let guard = self.samples_tx.lock().unwrap();
        let Some(tx) = guard.as_ref() else {
            return SendOutcome::ShutDown;
        };
        match tx.try_send(sample) {
            Ok(()) => SendOutcome::Queued,
            Err(TrySendError::Full(sample)) => {
                let total = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                tracing::debug!(
                    dropped_total = total,
                    model = %sample.model,
                    "influxdb queue full ({CAPACITY}) - sample dropped instead of delaying the request"
                );
                SendOutcome::Dropped
            }
            Err(TrySendError::Disconnected(_)) => SendOutcome::ShutDown,
        }
    }

    /// Returns once every sample enqueued before this call has been delivered
    /// or exhaustion-dropped: the ticket is pushed, the worker is pinged, and
    /// the ping's drain answers the ticket (or the worker's exit drain does).
    /// An already-closed queue is trivially drained - the exit drain answers
    /// every registered ticket before any join can return.
    pub fn flush(&self) -> Result<(), ExportError> {
        let (ticket, ack_rx) = Ticket::new();
        let my_id = Arc::clone(&ticket.id);
        self.waiters.lock().unwrap().push(ticket);
        let pinged = match self.drains_tx.lock().unwrap().as_ref() {
            Some(tx) => tx.send(()).is_ok(),
            None => false,
        };
        if !pinged {
            // Worker gone: its exit drain answers this ticket right away, so
            // waiting for it is trivially satisfied.
            return Ok(());
        }
        match ack_rx.recv_timeout(FLUSH_WAIT) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!("influxdb flush did not drain within 60 s");
                // Drop our ack half: later worker sends fail and the ack
                // sweep removes the slot. A ticket never outlives its waiter.
                let mut waiters = self.waiters.lock().unwrap();
                if let Some(pos) = waiters.iter().position(|t| t.is_same_waiter(&my_id)) {
                    waiters.swap_remove(pos);
                }
                Ok(())
            }
        }
    }

    /// Close both senders, then join the drain-exit worker within a bounded
    /// budget. Blocking is the contract; the budget only exists because a
    /// pre-task-83 client can park a write forever.
    pub fn shutdown(&self) {
        self.shutdown_after(JOIN_BUDGET);
    }

    /// The same close-and-join with an explicit budget, so a stalled-backend
    /// test can pin "shutdown is never wedged by the worker" in wall time
    /// instead of waiting out [`JOIN_BUDGET`].
    pub fn shutdown_after(&self, budget: Duration) {
        // Only the sample sender closes: drains stays open so the worker
        // exits via "channel empty -> all senders dropped -> Disconnected"
        // after its exit drain, never while a sample is still queued.
        drop(self.samples_tx.lock().unwrap().take());
        let Some(worker) = self.worker.lock().unwrap().take() else {
            return;
        };
        let deadline = Instant::now() + budget;
        while !worker.is_finished() {
            if Instant::now() >= deadline {
                tracing::warn!(
                    "influxdb exporter worker still writing after {budget:?} - shutdown proceeds; \
                     the worker exits on its own once the stuck call returns"
                );
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Err(e) = worker.join() {
            tracing::warn!("influxdb exporter worker thread did not exit cleanly: {e:?}");
        }
    }

    pub fn dropped_total(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn retries_total(&self) -> u64 {
        self.retries.load(Ordering::Relaxed)
    }
}

impl Drop for WriterQueue {
    fn drop(&mut self) {
        // Taking the senders out is the close: the worker drains and exits on
        // its own thread. No join here - Drop must not block whoever drops
        // the exporter last, so a Drop-only teardown races process exit and
        // reports nothing; the bounded JOIN lives in shutdown_after, reached
        // through ExporterManager::shutdown_all at the server's drain point.
        drop(self.samples_tx.lock().unwrap().take());
    }
}
