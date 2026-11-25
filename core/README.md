# karyon core

Runtime-agnostic async building blocks for Rust, shared by all karyon crates.

One async API over tokio or smol (executor, spawning, locks, IO, net), plus task groups, queues,
condition variables, `select`, timeouts and backoff. Tasks and timers can run on your own runtime.
Optional Ed25519 crypto and test helpers. Builds for WASI (`wasm32-wasip1`, `wasm32-wasip2`).

Use cases:

- Async code that runs on tokio or smol, chosen by a Cargo feature.
- Running karyon crates on a custom executor or a single-threaded WASI host.
- Spawning, tracking and cancelling groups of background tasks.
- Ed25519 node identities: generate, sign, verify, persist.

## Install

```bash
$ cargo add karyon_core
```

## Example

```rust
use std::time::Duration;

use karyon_core::async_util::{sleep, timeout, TaskGroup, TaskResult};

async fn fetch_value() -> u32 {
    42
}

async {
    let group = TaskGroup::new();

    // Spawn a task. It removes itself from the group when it finishes
    group.spawn(fetch_value());

    // Spawn with a completion callback. The callback reports whether
    // the task completed or was cancelled
    group.spawn_then(fetch_value(), |res| async move {
        if let TaskResult::Completed(v) = res {
            assert_eq!(v, 42);
        }
    });

    // Bound any future with a timeout
    let res = timeout(Duration::from_millis(100), sleep(Duration::MAX)).await;
    assert!(res.is_err());

    // Cancel all remaining tasks in the group
    group.cancel().await;
};
```

## Feature Flags

| Feature | Description |
|---------|-------------|
| `tokio` | Use tokio async runtime (default) |
| `smol` | Use smol async runtime |
| `crypto` | Ed25519 key pairs: generate, sign, verify |
| `testing` | Async test runners with executors and timeouts |

Exactly one runtime feature (`smol` or `tokio`) must be enabled.

With `tokio`, karyon_core enables only the tokio features it uses (`sync`, `io-util`, `rt`,
`rt-multi-thread`, `time`, `net`). Enable others, e.g. `macros`, on your own tokio dependency.

```toml
# Default (tokio)
karyon_core = "2.0"

# With crypto
karyon_core = { version = "2.0", features = ["crypto"] }

# Smol runtime
karyon_core = { version = "2.0", default-features = false, features = ["smol"] }
```

## Modules

### async_runtime

One API over smol or tokio: `Executor` and `global_executor()`, `spawn()` and `Task`, async locks
(`lock::Mutex`, `RwLock`, `OnceCell`, `Semaphore`), IO traits (`io`), and net types (`net`, with
`lookup_host`).

The caller owns the runtime. `global_executor()` is the convenience path: a multi-threaded smol or
tokio executor, started on the first call.

To share the tokio runtime your app already runs, e.g. under `#[tokio::main]`, pass
`Executor::from(Handle::current())`. `Executor::handle()` returns the tokio `Handle`, or `None`
on a user runtime.

**Custom runtime**

To bring your own runtime, implement the `Runtime` trait and pass it as `Executor::from(runtime)`.
The trait has two methods. `schedule` runs a woken task (call `Runnable::run`), and `sleep`
returns a timer future.

`Executor::spawn`, `Executor::sleep`, `Executor::timeout`, `Backoff::with_executor` and
`TaskGroup::with_executor` then run on it. The free `spawn`, `sleep` and `timeout` functions
stay on smol or tokio. Locks, IO and net types still come from the `smol` or `tokio` feature.

```rust
use std::{sync::Arc, time::Duration};

use karyon_core::async_runtime::{Executor, Runnable, Runtime, RuntimeRef, SleepFuture};
# use std::thread;
# // Stand-ins for your executor and timer.
# fn my_spawn(f: impl FnOnce() + Send + 'static) {
#     thread::spawn(f);
# }
# async fn my_sleep(_duration: Duration) {}

struct MyRuntime;

impl Runtime for MyRuntime {
    // karyon hands every task here; run it on your executor.
    fn schedule(&self, runnable: Runnable) {
        my_spawn(move || {
            runnable.run();
        });
    }

    // karyon's timers (sleep, timeout, backoff) wait on this.
    fn sleep(&self, duration: Duration) -> SleepFuture {
        Box::pin(my_sleep(duration))
    }
}

let runtime: RuntimeRef = Arc::new(MyRuntime);
let ex = Executor::from(runtime);

// Runs on MyRuntime.
ex.spawn(async { println!("on my runtime") }).detach();
```

### async_util

`TaskGroup` (spawn, track and cancel tasks), `AsyncQueue` (bounded queue), `CondVar` and
`CondWait`, `select()` with `Either`, `timeout()`, `sleep()`, and `Backoff` (exponential).

### crypto

Requires the `crypto` feature. `KeyPair` and `PublicKey` generate, sign and verify. Only Ed25519
is supported for now.

`KeyPair::secret()` and `KeyPair::from_secret` export and load the secret key, so a node can keep
its identity across restarts.

### testing

Requires the `testing` feature. `run_test` and `run_test_with_executor` run an async test on a
multi-threaded executor and panic if it does not finish within the timeout.

### util

`random_16()`, `random_32()` and `random_64()` return random integers.

## WASI

karyon_core builds for `wasm32-wasip1` and `wasm32-wasip2` with only the `tokio` feature. smol
does not build for wasm.

The `net` module is not available there. Tasks and timers come from your `Runtime`, as above.
`global_executor()` is a single-threaded tokio runtime that nothing drives, so tasks spawned on it
never run. Use `Executor::from(runtime)` instead.

```toml
karyon_core = { version = "2.0", default-features = false, features = ["tokio"] }
```
