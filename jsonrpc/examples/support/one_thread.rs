//! A user supplied runtime, as a WASI host brings it: one thread runs every
//! karyon task, and the event loop keeps the timers. Used by
//! `examples/custom_runtime.rs`; runs natively and on wasm32-wasip2.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        mpsc::{Receiver, Sender, channel},
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant},
};

use karyon_core::async_runtime::{Executor, Runnable, Runtime, RuntimeRef, SleepFuture};

/// Runs every task on the thread that drains `ready`; timers are sent to
/// that thread too.
struct OneThread {
    ready: Sender<Runnable>,
    timers: Sender<(Instant, Waker)>,
}

impl Runtime for OneThread {
    fn schedule(&self, runnable: Runnable) {
        let _ = self.ready.send(runnable);
    }

    fn sleep(&self, duration: Duration) -> SleepFuture {
        Box::pin(Sleep {
            deadline: Instant::now() + duration,
            timers: self.timers.clone(),
            registered: false,
        })
    }
}

/// Completes at `deadline`; the event loop wakes it then.
struct Sleep {
    deadline: Instant,
    timers: Sender<(Instant, Waker)>,
    registered: bool,
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if Instant::now() >= self.deadline {
            return Poll::Ready(());
        }
        if !self.registered {
            let _ = self.timers.send((self.deadline, cx.waker().clone()));
            self.registered = true;
        }
        Poll::Pending
    }
}

/// Builds the one-thread runtime and runs `main` with an `Executor` on it,
/// and every task spawned there, on this thread until `main` is done.
pub fn run<T, F, Fut>(main: F) -> T
where
    T: Send + 'static,
    F: FnOnce(Executor) -> Fut,
    Fut: Future<Output = T> + Send + 'static,
{
    let (ready_tx, ready) = channel();
    let (timers_tx, timer_rx) = channel();
    let runtime: RuntimeRef = Arc::new(OneThread {
        ready: ready_tx,
        timers: timers_tx,
    });
    let ex = Executor::from(runtime);
    let future = main(ex.clone());

    let (done_tx, done_rx) = channel();
    ex.spawn(async move {
        let _ = done_tx.send(future.await);
    })
    .detach();

    event_loop(&ready, &timer_rx, &done_rx)
}

/// Runs ready tasks, fires due timers, and sleeps until the next timer
/// when there is nothing to do. A WASI host would wait on its pollables.
fn event_loop<T>(
    ready: &Receiver<Runnable>,
    timer_rx: &Receiver<(Instant, Waker)>,
    done: &Receiver<T>,
) -> T {
    let mut timers: Vec<(Instant, Waker)> = Vec::new();
    loop {
        while let Ok(runnable) = ready.try_recv() {
            runnable.run();
        }
        if let Ok(out) = done.try_recv() {
            return out;
        }

        timers.extend(timer_rx.try_iter());
        let now = Instant::now();
        let (due, waiting): (Vec<_>, Vec<_>) = timers.drain(..).partition(|(d, _)| *d <= now);
        timers = waiting;
        if !due.is_empty() {
            due.into_iter().for_each(|(_, waker)| waker.wake());
            continue;
        }

        let next = timers
            .iter()
            .map(|(d, _)| *d)
            .min()
            .expect("no task ready and no timer: stuck");
        thread::sleep(next.saturating_duration_since(now));
    }
}
