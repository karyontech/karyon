use std::pin::Pin;
use std::task::{Context, Poll};

use pin_project_lite::pin_project;
use smol::future::Future;

/// Returns the result of the future that completes first, preferring future1
/// if both are ready.
///
/// # Examples
///
/// ```
/// use std::future;
///
/// use karyons_core::async_utils::{select, Either};
///
///  async {
///     let fut1 = future::pending::<String>();
///     let fut2 = future::ready(0);
///     let res = select(fut1, fut2).await;
///     assert!(matches!(res, Either::Right(0)));
///     // ....
///  };
///
/// ```
///
pub fn select<T1, T2, F1, F2>(future1: F1, future2: F2) -> Select<F1, F2>
where
    F1: Future<Output = T1>,
    F2: Future<Output = T2>,
{
    Select { future1, future2 }
}

pin_project! {
    #[derive(Debug)]
    pub struct Select<F1, F2> {
        #[pin]
        future1: F1,
        #[pin]
        future2: F2,
    }
}

/// The return value from the `select` function, indicating which future
/// completed first.
#[derive(Debug)]
pub enum Either<T1, T2> {
    Left(T1),
    Right(T2),
}

// Implement the Future trait for the Select struct.
impl<T1, T2, F1, F2> Future for Select<F1, F2>
where
    F1: Future<Output = T1>,
    F2: Future<Output = T2>,
{
    type Output = Either<T1, T2>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        if let Poll::Ready(t) = this.future1.poll(cx) {
            return Poll::Ready(Either::Left(t));
        }

        if let Poll::Ready(t) = this.future2.poll(cx) {
            return Poll::Ready(Either::Right(t));
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::{select, Either};
    use smol::Timer;
    use std::future;

    #[test]
    fn test_async_select() {
        smol::block_on(async move {
            let fut = select(Timer::never(), future::ready(0 as u32)).await;
            assert!(matches!(fut, Either::Right(0)));

            let fut1 = future::pending::<String>();
            let fut2 = future::ready(0);
            let res = select(fut1, fut2).await;
            assert!(matches!(res, Either::Right(0)));

            let fut1 = future::ready(0);
            let fut2 = future::pending::<String>();
            let res = select(fut1, fut2).await;
            assert!(matches!(res, Either::Left(_)));
        });
    }
}
