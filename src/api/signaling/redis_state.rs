use anyhow::{Context, Result};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::StreamExt;

pub enum Command {}

pub async fn start_keyspace_notification_loop(
    shutdown_sig: broadcast::Receiver<()>,
    redis: &redis::Client,
) -> Result<mpsc::Sender<()>> {
    let mut pub_sub = redis
        .get_async_connection()
        .await
        .context("Failed to get connection for keyspace notifications channel")?
        .into_pubsub();

    pub_sub
        .psubscribe("__keyspace@0__:k3k-signaling:*")
        .await
        .context("Failed to subscribe to signaling keyspace events")?;

    let (send, receive) = mpsc::channel(24);

    tokio::spawn(listen_change_events(shutdown_sig, pub_sub, receive));

    Ok(send)
}

async fn listen_change_events(
    mut shutdown_sig: broadcast::Receiver<()>,
    pub_sub: redis::aio::PubSub,
    mut receive: mpsc::Receiver<()>,
) {
    let mut stream = pub_sub.into_on_message();

    loop {
        tokio::select! {
            _ = shutdown_sig.recv() => {
                log::debug!("redis keyspace notification loop got shutdown signal, exiting task");
                return;
            }
            _ = stream.next() => {

            }
            command = receive.recv() => {}
        }
    }
}
