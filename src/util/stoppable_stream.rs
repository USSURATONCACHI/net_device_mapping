use tokio::sync::broadcast::{Receiver, error::RecvError};

use super::OneshotRecv;

pub struct StoppableStream<T: Clone>(Receiver<T>, OneshotRecv<()>);

impl<T: Clone> StoppableStream<T> {
    pub fn new(stream: Receiver<T>) -> (Self, async_oneshot::Sender<()>) {
        let (stop_tx, stop_rx) = async_oneshot::oneshot();

        (Self(stream, OneshotRecv::from(stop_rx)), stop_tx)
    }

    pub fn inner(&self) -> &Receiver<T> {
        &self.0
    }
    pub fn inner_mut(&mut self) -> &mut Receiver<T> {
        &mut self.0
    }
    pub fn into_inner(self) -> (Receiver<T>, OneshotRecv<()>) {
        (self.0, self.1)
    }
    pub fn from_inner(stream: Receiver<T>, stop: OneshotRecv<()>) -> Self {
        Self(stream, stop)
    }

    pub async fn recv(&mut self) -> Result<T, RecvError> {
        if self.1.is_closed() {
            return Err(RecvError::Closed);
        }

        tokio::select! {
            _ = &mut self.1 => Err(RecvError::Closed),
            result = self.0.recv() => result
        }
    }
}
