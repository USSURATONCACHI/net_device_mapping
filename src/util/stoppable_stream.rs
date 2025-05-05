use tokio::sync::broadcast::{Receiver, error::RecvError};

use super::OneshotRecv;

pub struct StoppableStream<T: Clone>(Option<Receiver<T>>, OneshotRecv<()>);

impl<T: Clone> StoppableStream<T> {
    pub fn new(stream: Receiver<T>) -> (Self, async_oneshot::Sender<()>) {
        let (stop_tx, stop_rx) = async_oneshot::oneshot();

        (Self(Some(stream), OneshotRecv::from(stop_rx)), stop_tx)
    }

    pub fn inner(&self) -> Option<&Receiver<T>> {
        self.0.as_ref()
    }
    pub fn inner_mut(&mut self) -> Option<&mut Receiver<T>> {
        self.0.as_mut()
    }
    pub fn into_inner(self) -> (Option<Receiver<T>>, OneshotRecv<()>) {
        (self.0, self.1)
    }
    pub fn from_inner(stream: Option<Receiver<T>>, stop: OneshotRecv<()>) -> Self {
        Self(stream, stop)
    }

    pub async fn recv(&mut self) -> Result<T, RecvError> {
        if self.1.is_closed() || self.0.is_none() {
            return Err(RecvError::Closed);
        }

        tokio::select! {
            _ = &mut self.1 => {
                self.0 = None;
                Err(RecvError::Closed)
            },
            result = self.0.as_mut().unwrap().recv() => {
                result
            }
        }
    }
}
