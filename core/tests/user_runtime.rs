//! karyon's tasks and timers on a runtime the test supplies, passed as an
//! `Executor`: one thread,
//! a task queue and timers of its own, the shape of a WASI host. Runs
//! natively and on wasm32-wasip2 (under wasmtime in CI).

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, Sender, channel},
    },
    task::{Context, Poll, Waker},
    thread,
    time::{Duration, Instant},
};

use karyon_core::{
    async_runtime::{Executor, Runnable, Runtime, RuntimeRef, SleepFuture},
    async_util::{Backoff, TaskGroup},
};

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

/// The event loop: runs `future` and every task it spawns on this thread
/// until `future` is done.
fn block_on<T: Send + 'static>(
    ex: &Executor,
    ready: &Receiver<Runnable>,
    timer_rx: &Receiver<(Instant, Waker)>,
    future: impl Future<Output = T> + Send + 'static,
) -> T {
    let (done_tx, done_rx) = channel();
    ex.spawn(async move {
        let _ = done_tx.send(future.await);
    })
    .detach();

    let mut timers: Vec<(Instant, Waker)> = Vec::new();
    loop {
        while let Ok(runnable) = ready.try_recv() {
            runnable.run();
        }
        if let Ok(out) = done_rx.try_recv() {
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

        // Nothing to run: sleep until the next timer.
        let next = timers
            .iter()
            .map(|(d, _)| *d)
            .min()
            .expect("no task ready and no timer: the test is stuck");
        thread::sleep(next.saturating_duration_since(now));
    }
}

#[test]
fn tasks_and_timers_on_user_runtime() {
    let (ready_tx, ready_rx) = channel();
    let (timer_tx, timer_rx) = channel();
    let runtime: RuntimeRef = Arc::new(OneThread {
        ready: ready_tx,
        timers: timer_tx,
    });
    // Passed explicitly, like hyper's executor and timer.
    let ex = Executor::from(runtime);

    let task_ex = ex.clone();
    let out = block_on(&ex, &ready_rx, &timer_rx, async move {
        let ex = task_ex;
        // A spawned task returns its value.
        let value = ex.spawn(async { 21 * 2 }).await.unwrap();

        // `sleep` waits on the user timers.
        let start = Instant::now();
        ex.sleep(Duration::from_millis(20)).await;
        assert!(start.elapsed() >= Duration::from_millis(20));

        // `timeout` fires on them too.
        let slow = ex.sleep(Duration::from_secs(60));
        assert!(ex.timeout(Duration::from_millis(10), slow).await.is_err());

        // So does a Backoff.
        Backoff::with_executor(1, 10, ex.clone()).sleep().await;

        // A TaskGroup runs on the user runtime and cancels its tasks.
        let group = TaskGroup::with_executor(ex.clone());
        let sleeper = ex.clone();
        group.spawn(async move { sleeper.sleep(Duration::from_secs(60)).await });
        group.cancel().await;

        // Dropping a task cancels it.
        let done = Arc::new(AtomicBool::new(false));
        let (flag, sleeper) = (done.clone(), ex.clone());
        drop(ex.spawn(async move {
            sleeper.sleep(Duration::from_millis(10)).await;
            flag.store(true, Ordering::SeqCst);
        }));
        ex.sleep(Duration::from_millis(30)).await;
        assert!(!done.load(Ordering::SeqCst));

        value
    });
    assert_eq!(out, 42);
}
