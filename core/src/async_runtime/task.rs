use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

#[cfg(feature = "smol")]
use smol::Task as SmolTask;
#[cfg(feature = "tokio")]
use tokio::task::JoinHandle;

use async_task::Task as CustomTask;

use crate::error::Error;

/// A spawned task: a smol or tokio task, or an `async_task` task on a
/// user runtime. Dropping it cancels the task (smol, user runtime);
/// `detach` lets it run on.
pub struct Task<T> {
    inner: Inner<T>,
}

enum Inner<T> {
    #[cfg(feature = "smol")]
    Smol(SmolTask<T>),
    #[cfg(feature = "tokio")]
    Tokio(JoinHandle<T>),
    Custom(CustomTask<T>),
}

impl<T> Task<T> {
    /// A task on a user runtime. Not a `From`: on smol, `smol::Task` is the
    /// same type.
    pub(crate) fn custom(t: CustomTask<T>) -> Task<T> {
        Task {
            inner: Inner::Custom(t),
        }
    }

    /// Cancels the task.
    pub async fn cancel(self) {
        match self.inner {
            #[cfg(feature = "smol")]
            Inner::Smol(t) => {
                t.cancel().await;
            }
            #[cfg(feature = "tokio")]
            Inner::Tokio(t) => t.abort(),
            Inner::Custom(t) => {
                t.cancel().await;
            }
        }
    }

    /// Detaches the task so it continues running in the background without
    /// being cancelled when dropped.
    pub fn detach(self) {
        match self.inner {
            #[cfg(feature = "smol")]
            Inner::Smol(t) => t.detach(),
            #[cfg(feature = "tokio")]
            Inner::Tokio(t) => drop(t),
            Inner::Custom(t) => t.detach(),
        }
    }
}

impl<T> Future for Task<T> {
    type Output = Result<T, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut self.inner {
            #[cfg(feature = "smol")]
            Inner::Smol(t) => Pin::new(t).poll(cx).map(Ok),
            #[cfg(feature = "tokio")]
            Inner::Tokio(t) => Pin::new(t).poll(cx).map_err(Error::from),
            Inner::Custom(t) => Pin::new(t).poll(cx).map(Ok),
        }
    }
}

#[cfg(feature = "smol")]
impl<T> From<SmolTask<T>> for Task<T> {
    fn from(t: SmolTask<T>) -> Task<T> {
        Task {
            inner: Inner::Smol(t),
        }
    }
}

#[cfg(feature = "tokio")]
impl<T> From<JoinHandle<T>> for Task<T> {
    fn from(t: JoinHandle<T>) -> Task<T> {
        Task {
            inner: Inner::Tokio(t),
        }
    }
}
