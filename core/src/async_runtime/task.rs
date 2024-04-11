use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::error::Error;

pub struct Task<T> {
    #[cfg(feature = "smol")]
    inner_task: smol::Task<T>,
    #[cfg(feature = "tokio")]
    inner_task: tokio::task::JoinHandle<T>,
}

impl<T> Task<T> {
    pub async fn cancel(self) {
        #[cfg(feature = "smol")]
        self.inner_task.cancel().await;
        #[cfg(feature = "tokio")]
        self.inner_task.abort();
    }
}

impl<T> Future for Task<T> {
    type Output = Result<T, Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        #[cfg(feature = "smol")]
        let result = smol::Task::poll(Pin::new(&mut self.inner_task), cx);
        #[cfg(feature = "tokio")]
        let result = tokio::task::JoinHandle::poll(Pin::new(&mut self.inner_task), cx);

        #[cfg(feature = "smol")]
        return result.map(Ok);

        #[cfg(feature = "tokio")]
        return result.map_err(|e| e.into());
    }
}

#[cfg(feature = "smol")]
impl<T> From<smol::Task<T>> for Task<T> {
    fn from(t: smol::Task<T>) -> Task<T> {
        Task { inner_task: t }
    }
}

#[cfg(feature = "tokio")]
impl<T> From<tokio::task::JoinHandle<T>> for Task<T> {
    fn from(t: tokio::task::JoinHandle<T>) -> Task<T> {
        Task { inner_task: t }
    }
}
