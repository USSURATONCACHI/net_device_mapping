use std::{pin::pin, task::Poll};

pub struct OneshotRecv<T>(pub Option<async_oneshot::Receiver<T>>);

impl<T> OneshotRecv<T> {
    pub fn is_closed(&self) -> bool {
        self.0.is_none()
    }
}

impl<T> From<async_oneshot::Receiver<T>> for OneshotRecv<T> {
    fn from(value: async_oneshot::Receiver<T>) -> Self {
        Self(Some(value))
    }
}

impl<T> Future for &mut OneshotRecv<T> {
    type Output = <async_oneshot::Receiver<T> as Future>::Output;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match &mut self.0 {
            Some(x) => match pin!(x).poll(cx) {
                Poll::Ready(result) => {
                    self.0 = None;
                    Poll::Ready(result)
                }
                Poll::Pending => Poll::Pending,
            },
            None => Poll::Pending,
        }
    }
}
