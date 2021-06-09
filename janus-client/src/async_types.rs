use crate::incoming::{Event, VideoRoomPluginData, VideoRoomPluginEvent};
use crate::{
    error,
    types::{incoming, incoming::JanusMessage, HandleId, Success},
    PluginData, SessionId,
};
use futures::{Future, FutureExt};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::oneshot;

pub(crate) struct CreateSessionRequest {
    pub(crate) rx: oneshot::Receiver<JanusMessage>,
}

impl Future for CreateSessionRequest {
    type Output = Option<SessionId>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.rx.poll_unpin(cx) {
            Poll::Ready(result) => {
                log::trace!("CreateSessionRequest got return: {:?}", &result);
                match result {
                    Ok(JanusMessage::Success(Success::Janus(incoming::JanusSuccess {
                        data,
                        ..
                    }))) => Poll::Ready(Some(data.id.into())),
                    // Should not be passed to here
                    Ok(JanusMessage::Ack { .. }) => Poll::Pending,
                    Err(_) => {
                        // We can panic here, as this should not happen and means there is something fundamentally wrong with our runtime
                        // Especially this means we drop the other side of the channel without sending something.
                        // This should only happen when we have an open request and shutdown drop the client.
                        // todo change this once we have something like timeouts.
                        panic!("Encountered error while receiving from oneshot channel")
                    }
                    _ => {
                        log::debug!("Got the wrong JanusResult for the create request");
                        Poll::Ready(None)
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

pub(crate) struct AttachToPluginRequest {
    pub(crate) rx: oneshot::Receiver<JanusMessage>,
}

impl Future for AttachToPluginRequest {
    type Output = Option<HandleId>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.rx.poll_unpin(cx) {
            Poll::Ready(result) => {
                log::trace!("AttachToPluginRequest got return: {:?}", &result);
                match result {
                    Ok(JanusMessage::Success(Success::Janus(incoming::JanusSuccess {
                        data,
                        ..
                    }))) => Poll::Ready(Some(data.id.into())),
                    // Should not be passed to here
                    Ok(JanusMessage::Ack { .. }) => Poll::Pending,
                    Err(_) => {
                        // We can panic here, as this should not happen and means there is something fundamentally wrong with our runtime
                        // Especially this means we drop the other side of the channel without sending something.
                        // This should only happen when we have an open request and shutdown drop the client.
                        // todo change this once we have something like timeouts.
                        panic!("Encountered error while receiving from oneshot channel")
                    }
                    _ => {
                        log::debug!("Got the wrong JanusResult for the create request");
                        Poll::Ready(None)
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Represents a general request type used when talking to a plugin
pub(crate) struct SendToPluginRequest {
    pub(crate) rx: oneshot::Receiver<JanusMessage>,
}

impl Future for SendToPluginRequest {
    type Output = Result<JanusMessage, error::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.rx.poll_unpin(cx) {
            Poll::Ready(result) => {
                log::trace!("SendToPluginRequest got return: {:?}", &result);
                match result {
                    Ok(JanusMessage::Error(incoming::Error { error, .. })) => {
                        Poll::Ready(Err(error.into()))
                    }
                    // Handle Errors from the VideoRoom plugin
                    #[cfg(feature = "videoroom")]
                    Ok(JanusMessage::Event(Event {
                        plugindata:
                            PluginData::VideoRoom(VideoRoomPluginData::Event(
                                VideoRoomPluginEvent::Error(e),
                            )),
                        ..
                    })) => Poll::Ready(Err(e.into())),
                    Ok(msg) => Poll::Ready(Ok(msg)),
                    Err(_) => {
                        // We can panic here, as this should not happen and means there is something fundamentally wrong with our runtime
                        // Especially this means we drop the other side of the channel without sending something.
                        // This should only happen when we have an open request and shutdown drop the client.
                        // todo change this once we have something like timeouts.
                        panic!("Encountered error while receiving from oneshot channel")
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Represents a general request type used when talking to a plugin
pub(crate) struct SendTrickleRequest {
    pub(crate) rx: oneshot::Receiver<JanusMessage>,
}

impl Future for SendTrickleRequest {
    type Output = Result<JanusMessage, error::Error>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.rx.poll_unpin(cx) {
            Poll::Ready(result) => {
                log::trace!("SendTrickleRequest got return: {:?}", &result);
                match result {
                    // Trickle Requests are sent Janus and not a plugin, thus we do not need to check for PluginErrors here
                    Ok(JanusMessage::Error(incoming::Error { error, .. })) => {
                        Poll::Ready(Err(error.into()))
                    }
                    Ok(msg) => Poll::Ready(Ok(msg)),
                    Err(_) => {
                        // We can panic here, as this should not happen and means there is something fundamentally wrong with our runtime
                        // Especially this means we drop the other side of the channel without sending something.
                        // This should only happen when we have an open request and shutdown drop the client.
                        // todo change this once we have something like timeouts.
                        panic!("Encountered error while receiving from oneshot channel")
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
