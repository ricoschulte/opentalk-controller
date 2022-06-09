use anyhow::Result;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tokio_executor_trait::Tokio as TokioExecutor;
use tokio_reactor_trait::Tokio as TokioReactor;

/// [`lapin::Channel`] wrapper which maintains a ref counter to the channels underlying connection
pub struct RabbitMqChannel {
    channel: lapin::Channel,
    ref_counter: Arc<AtomicU32>,
}

impl Drop for RabbitMqChannel {
    fn drop(&mut self) {
        self.ref_counter.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Deref for RabbitMqChannel {
    type Target = lapin::Channel;

    fn deref(&self) -> &Self::Target {
        &self.channel
    }
}

impl DerefMut for RabbitMqChannel {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.channel
    }
}

/// RabbitMQ connection pool which manages connection based on the amount of channels used per connection
///
/// Keeps a configured minimum of connections alive but creates them lazily when enough channels are requested
pub struct RabbitMqPool {
    url: String,
    min_connections: u32,
    max_channels_per_connection: u32,
    connections: Mutex<Vec<ConnectionEntry>>,
}

struct ConnectionEntry {
    connection: lapin::Connection,
    channels: Arc<AtomicU32>,
}

impl RabbitMqPool {
    /// Creates a new [`RabbitMqPool`] from given parameters
    ///
    /// Spawns a connection reaper task on the tokio runtime
    pub fn from_config(
        url: &str,
        min_connections: u32,
        max_channels_per_connection: u32,
    ) -> Arc<Self> {
        let this = Arc::new(Self {
            url: url.into(),
            min_connections,
            max_channels_per_connection,
            connections: Mutex::new(vec![]),
        });

        tokio::spawn(reap_unused_connections(this.clone()));

        this
    }

    /// Create a connection with the pools given params
    ///
    /// This just creates a connection and does not add it to its pool. Connections will automatically be created when
    /// creating channels.
    pub async fn make_connection(&self) -> Result<lapin::Connection> {
        let connection = lapin::Connection::connect(
            &self.url,
            lapin::ConnectionProperties::default()
                .with_executor(TokioExecutor::current())
                .with_reactor(TokioReactor),
        )
        .await?;

        Ok(connection)
    }

    /// Create a rabbitmq channel using one of the connections of the pool
    ///
    /// If there are no connections available or all connections are at the channel cap
    /// a new connection will be created
    pub async fn create_channel(&self) -> Result<RabbitMqChannel> {
        let mut connections = self.connections.lock().await;

        let entry = connections.iter().find(|entry| {
            entry.channels.load(Ordering::Relaxed) < self.max_channels_per_connection
        });

        let entry = if let Some(entry) = entry {
            entry
        } else {
            let connection = self.make_connection().await?;

            let channels = Arc::new(AtomicU32::new(0));

            connections.push(ConnectionEntry {
                connection,
                channels,
            });

            connections.last().unwrap()
        };

        let channel = entry.connection.create_channel().await?;

        entry.channels.fetch_add(1, Ordering::Relaxed);

        Ok(RabbitMqChannel {
            channel,
            ref_counter: entry.channels.clone(),
        })
    }

    /// Close all connections managed by the pool with the given code and message
    pub async fn close(&self, reply_code: u16, reply_message: &str) -> Result<()> {
        let mut connections = self.connections.lock().await;

        for entry in connections.drain(..) {
            entry.connection.close(reply_code, reply_message).await?;
        }

        Ok(())
    }
}

async fn reap_unused_connections(pool: Arc<RabbitMqPool>) {
    let mut interval = interval(Duration::from_secs(10));

    loop {
        interval.tick().await;

        let mut connections = pool.connections.lock().await;

        if connections.len() <= pool.min_connections as usize {
            continue;
        }

        let removed_entries = remove_where(&mut connections, |entry| {
            entry.channels.load(Ordering::Relaxed) == 0
        });

        drop(connections);

        for entry in removed_entries {
            if let Err(e) = entry.connection.close(0, "closing").await {
                log::error!("Failed to close connection in gc {}", e);
            }
        }
    }
}

fn remove_where<T>(vec: &mut Vec<T>, mut f: impl FnMut(&T) -> bool) -> Vec<T> {
    let mut ret = vec![];

    let mut skip = 0;

    while let Some((i, _)) = vec
        .iter()
        .enumerate()
        .skip(skip)
        .find(|(_, entry)| f(entry))
    {
        ret.push(vec.remove(i));
        skip = i;
    }

    ret
}

#[cfg(test)]
mod test {
    #[test]
    fn test_remove_where() {
        let mut items = vec![0, 1, 2, 3, 4, 5];

        let mut iterations = 0;

        let removed = super::remove_where(&mut items, |i| {
            iterations += 1;
            *i > 2
        });

        assert_eq!(iterations, 6);
        assert_eq!(items, vec![0, 1, 2]);
        assert_eq!(removed, vec![3, 4, 5]);
    }
}
