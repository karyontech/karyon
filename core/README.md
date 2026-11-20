# karyon core

Core building blocks shared by all karyon crates: a runtime-agnostic
async layer (smol or tokio behind one API), async utilities (task
groups, queues, condition variables, timeouts, backoff), and optional
crypto and testing helpers.

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

    // Spawn a task; it removes itself from the group when it finishes
    group.spawn(fetch_value());

    // Spawn with a completion callback; the callback reports whether
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

One API over smol or tokio: `Executor` / `global_executor()`,
`spawn()` / `Task`, async locks (`Mutex`, `RwLock`, `OnceCell`,
`Semaphore`, from smol or tokio), io traits, and net types. karyon does
not drive your executor; the caller owns the runtime.

To bring your own runtime, implement the `Runtime` trait (`schedule` a
task, `sleep`) and pass it as `Executor::from(runtime)`. Tasks and timers
then go through that executor (`spawn`, `sleep`, `timeout`,
`Backoff::with_executor`, `TaskGroup::with_executor`). Locks, IO and net
types still come from the `smol` or `tokio` feature.

To share the tokio runtime your app already runs, e.g. under
`#[tokio::main]`, pass `Executor::from(Handle::current())`.

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

### WASI

karyon_core builds for `wasm32-wasip1` and `wasm32-wasip2` with the `tokio`
feature only (smol does not build for wasm). There it has no `net` module,
and tasks and timers come from your `Runtime` as above.

```toml
karyon_core = { version = "2.0", default-features = false, features = ["tokio"] }
```

### async_util

`TaskGroup` (spawn, track, cancel tasks), `AsyncQueue` (bounded
queue), `CondVar` / `CondWait`, `select()`, `timeout()`, `sleep()`,
and `Backoff`.

### crypto (feature `crypto`)

`KeyPair` and `PublicKey`: generate, sign, verify. Ed25519 only for now.
`KeyPair::secret()` exports the secret key bytes and `KeyPair::from_secret`
loads them back, so a node can keep its identity across restarts.

### util

`random_16() / random_32() / random_64()`: random integers.
