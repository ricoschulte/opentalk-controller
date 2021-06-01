use actix_web::web;
use bytes::Bytes;
use futures::channel::mpsc as fut_mpsc;
use futures::{AsyncRead, AsyncWrite, Stream, TryStreamExt};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct ActixTungsteniteAdapter {
    send: fut_mpsc::UnboundedSender<io::Result<Bytes>>,
    read: Box<dyn AsyncRead + Unpin>,
}

impl ActixTungsteniteAdapter {
    pub fn from_actix_payload(
        payload: web::Payload,
    ) -> (Self, impl Stream<Item = io::Result<Bytes>>) {
        let async_read = TryStreamExt::into_async_read(
            payload.map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e)),
        );

        let (send, recv) = fut_mpsc::unbounded::<io::Result<Bytes>>();

        (
            Self {
                send,
                read: Box::new(async_read),
            },
            recv,
        )
    }
}

impl AsyncWrite for ActixTungsteniteAdapter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.send
            .unbounded_send(Ok(Bytes::copy_from_slice(buf)))
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "ActixTungsteniteAdapter channel receiver has been dropped",
                )
            })?;

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for ActixTungsteniteAdapter {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        AsyncRead::poll_read(Pin::new(&mut self.read), cx, buf)
    }
}
