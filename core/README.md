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
| `smol` | Use smol async runtime (default) |
| `tokio` | Use tokio async runtime |
| `crypto` | Ed25519 key pairs: generate, sign, verify |
| `testing` | Async test runners with executors and timeouts |

Exactly one runtime feature (`smol` or `tokio`) must be enabled.

```toml
# Default (smol)
karyon_core = "1.0"

# With crypto
karyon_core = { version = "1.0", features = ["crypto"] }

# Tokio runtime
karyon_core = { version = "1.0", default-features = false, features = ["tokio"] }
```

## Modules

### async_runtime

One API over smol or tokio: `Executor` / `global_executor()`,
`spawn()` / `Task`, async locks (`Mutex`, `RwLock`, `OnceCell`), io
traits, and net types. karyon does not drive your executor; the
caller owns the runtime.

### async_util

`TaskGroup` (spawn, track, cancel tasks), `AsyncQueue` (bounded
queue), `CondVar` / `CondWait`, `select()`, `timeout()`, `sleep()`,
and `Backoff`.

### crypto (feature `crypto`)

`KeyPair` and `PublicKey`: generate, sign, verify. Ed25519 only for now.

### util

`random_16() / random_32() / random_64()`: random integers.
