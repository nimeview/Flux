//! Timers and the event loop: the host contract for time.
//!
//! `after(ms, Event(…))` and `every(ms, Event(…))` set a timer that sends the
//! event later. After `main` (or, without one, after the modules start) the
//! host runs [`Executor::run_event_loop`]: it waits for the next timer and
//! sends its event, each one its own batch, until no timer is left or the
//! program calls `stop()`. Timers due at the same moment fire in the order
//! they were set. A host may also send events from other threads through an
//! [`EventSender`]; the loop delivers them in the order they arrive, and
//! keeps running while a sender exists.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::Executor;
use crate::error::RuntimeError;
use crate::graph::RuntimeValue;

/// A function the host gives a program for its `extern fn`: it receives
/// the arguments and returns a value of the declared type, or a message.
pub type HostFn = Box<dyn FnMut(&[RuntimeValue]) -> Result<RuntimeValue, String> + Send>;

/// What a host gives a program: its functions for `extern fn`s, by the
/// name the program declares.
#[derive(Default)]
pub struct Host {
    pub(super) functions: std::collections::HashMap<String, HostFn>,
}

impl Host {
    pub fn new() -> Self {
        Self::default()
    }

    /// Provides `extern fn name`.
    pub fn function(
        mut self,
        name: &str,
        f: impl FnMut(&[RuntimeValue]) -> Result<RuntimeValue, String> + Send + 'static,
    ) -> Self {
        self.functions.insert(name.to_string(), Box::new(f));
        self
    }
}

struct Timer {
    id: i64,
    period: Option<Duration>,
    event: String,
    values: Vec<RuntimeValue>,
    /// For a component's own event: the instance it goes to (which the
    /// timer does not keep alive).
    receiver: Option<u64>,
}

#[derive(Default)]
struct Inbox {
    events: Mutex<VecDeque<(String, Vec<RuntimeValue>)>>,
    arrived: Condvar,
}

/// Sends events into a running program from another thread.
#[derive(Clone)]
pub struct EventSender {
    inbox: Arc<Inbox>,
}

impl EventSender {
    /// Queues `event` with its values; the event loop sends it as one batch.
    pub fn send(&self, event: &str, values: Vec<RuntimeValue>) {
        let mut queue = self.inbox.events.lock().unwrap_or_else(|e| e.into_inner());
        queue.push_back((event.to_string(), values));
        self.inbox.arrived.notify_one();
    }
}

#[derive(Default)]
pub(super) struct Timers {
    /// By when each fires, then the order it was set.
    queue: BTreeMap<(Instant, u64), Timer>,
    next_seq: u64,
    stopped: bool,
    inbox: Arc<Inbox>,
}

impl Executor {
    /// Calls the host's function for `extern fn name`.
    pub(super) fn call_host(&mut self, name: &str, args: &[RuntimeValue]) -> RuntimeValue {
        let Some(f) = self.host.functions.get_mut(name) else {
            self.raise(flux_value::ArithError::Internal);
            return RuntimeValue::Unit;
        };
        match f(args) {
            Ok(value) => value,
            Err(message) => {
                self.raise(flux_value::ArithError::HostFailed);
                if let Some(trap) = self.trap.as_mut() {
                    trap.detail
                        .get_or_insert_with(|| format!("`{name}`: {message}"));
                }
                RuntimeValue::Unit
            }
        }
    }

    /// `after`, `every`, `cancel`, and `stop`.
    pub(super) fn call_timer(&mut self, name: &str, args: &[RuntimeValue]) -> RuntimeValue {
        match name {
            "after" | "every" => {
                let ms = args
                    .first()
                    .and_then(RuntimeValue::as_int)
                    .unwrap_or(0)
                    .max(0) as u64;
                let Some(RuntimeValue::String(event)) = args.get(1) else {
                    self.raise(flux_value::ArithError::Internal);
                    return RuntimeValue::Unit;
                };
                let delay = Duration::from_millis(ms);
                // `every(0, …)` would fire forever at one instant.
                let period = (name == "every").then(|| delay.max(Duration::from_millis(1)));
                let receiver = self
                    .live
                    .member_events
                    .contains_key(event)
                    .then_some(self.receiver)
                    .flatten();
                let timers = &mut self.timers;
                timers.next_seq += 1;
                let id = timers.next_seq as i64;
                timers.queue.insert(
                    (Instant::now() + delay, timers.next_seq),
                    Timer {
                        id,
                        period,
                        event: event.clone(),
                        values: args[2..].to_vec(),
                        receiver,
                    },
                );
                RuntimeValue::int_i64(id)
            }
            "cancel" => {
                let id = args.first().and_then(RuntimeValue::as_int).unwrap_or(0);
                self.timers.queue.retain(|_, t| t.id != id);
                RuntimeValue::Unit
            }
            "stop" => {
                self.timers.stopped = true;
                self.timers.queue.clear();
                RuntimeValue::Unit
            }
            _ => RuntimeValue::Unit,
        }
    }

    /// A handle for sending events into this program from another thread.
    pub fn event_sender(&self) -> EventSender {
        EventSender {
            inbox: Arc::clone(&self.timers.inbox),
        }
    }

    /// Whether the program called `stop()`.
    pub fn stopped(&self) -> bool {
        self.timers.stopped
    }

    /// How long until the next timer is due (zero if one is overdue), or
    /// `None` when no timer is set.
    pub fn next_timer_in(&self) -> Option<Duration> {
        if self.timers.stopped {
            return None;
        }
        let (due, _) = self.timers.queue.keys().next()?;
        Some(due.saturating_duration_since(Instant::now()))
    }

    /// Sends the events of the timers that are due, each as one batch, in
    /// the order they are due (and were set). An `every` timer is set again.
    pub fn run_due_timers(&mut self) {
        let now = Instant::now();
        while self.trap.is_none() && !self.timers.stopped {
            let Some((&(due, seq), _)) = self.timers.queue.iter().next() else {
                break;
            };
            if due > now {
                break;
            }
            let timer = self.timers.queue.remove(&(due, seq)).expect("timer");
            if let Some(receiver) = timer.receiver {
                if !self.is_alive(receiver) {
                    continue;
                }
            }
            if let Some(period) = timer.period {
                self.timers.next_seq += 1;
                let again = Timer {
                    id: timer.id,
                    period: timer.period,
                    event: timer.event.clone(),
                    values: timer.values.clone(),
                    receiver: timer.receiver,
                };
                self.timers
                    .queue
                    .insert((due + period, self.timers.next_seq), again);
            }
            let receiver = timer.receiver;
            self.with_receiver(receiver, |this| {
                this.emit_values(&timer.event, timer.values)
            });
        }
    }

    /// Runs the program's events until no timer is set and no sender
    /// exists, or until `stop()`: each due timer and each event sent from
    /// outside is one batch. Returns the first runtime error.
    pub fn run_event_loop(&mut self) -> Result<(), RuntimeError> {
        loop {
            if let Some(err) = &self.trap {
                return Err((**err).clone());
            }
            // Events sent from outside, in the order they came.
            let posted: Vec<_> = {
                let mut queue = self
                    .timers
                    .inbox
                    .events
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                queue.drain(..).collect()
            };
            for (event, values) in posted {
                if self.timers.stopped || self.trap.is_some() {
                    break;
                }
                self.emit_values(&event, values);
            }
            if self.trap.is_some() {
                continue;
            }
            if self.timers.stopped {
                return Ok(());
            }
            let senders = Arc::strong_count(&self.timers.inbox) > 1;
            let wait = match self.next_timer_in() {
                Some(wait) => wait,
                None if senders => Duration::from_secs(3600),
                None => {
                    // The last sender may have posted and gone after the
                    // queue was drained above: its events still run.
                    let pending = !self
                        .timers
                        .inbox
                        .events
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .is_empty();
                    if pending {
                        continue;
                    }
                    return Ok(());
                }
            };
            if !wait.is_zero() {
                let queue = self
                    .timers
                    .inbox
                    .events
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if queue.is_empty() {
                    let _ = self
                        .timers
                        .inbox
                        .arrived
                        .wait_timeout(queue, wait)
                        .unwrap_or_else(|e| e.into_inner());
                }
            }
            self.run_due_timers();
        }
    }
}
