use crate::error;
use lapin::{
    options::{
        BasicConsumeOptions, BasicPublishOptions, ExchangeDeclareOptions, QueueBindOptions,
        QueueDeclareOptions,
    },
    types::FieldTable,
    BasicProperties, Channel, Consumer, Queue,
};

/// Configuration for the Rabbit MQ connection the janus-client uses.
#[derive(Debug, Clone)]
pub struct RabbitMqConfig {
    channel: Channel,
    to_janus_routing_key: String,
    to_janus_queue: String,
    janus_exchange: String,
    from_janus_routing_key: String,
    tag: String,
}

/// A Rabbit MQ 'Connection', created by setup method of [RabbitMqConfig].
///
/// This includes the required keys and consumer to receive from and send to a single Janus instance.
#[derive(Debug)]
pub struct RabbitMqConnection {
    to_janus_routing_key: String,
    to_janus_queue: String,
    janus_exchange: String,
    from_janus_routing_key: String,
    incoming_queue: Queue,
    tag: String,
    pub(crate) consumer: Consumer,
    channel: Channel,
}

impl RabbitMqConfig {
    /// Creates a new RabbitMqConfig using the passed channel.
    ///
    /// **Make sure that the Connection of this channel outlives the RabbitMqConfig and dependents.**
    pub fn new_from_channel(
        channel: Channel,
        to_janus_queue: String,
        to_janus_routing_key: String,
        janus_exchange: String,
        from_janus_routing_key: String,
        tag: String,
    ) -> Self {
        Self {
            channel,
            to_janus_routing_key,
            to_janus_queue,
            janus_exchange,
            from_janus_routing_key,
            tag,
        }
    }

    /// Returns a [RabbitMqConnection] with already declared queues and setup [Consumer]
    pub(crate) async fn setup(self) -> Result<RabbitMqConnection, error::Error> {
        log::debug!(
            "Setup RabbitMQ Outgoing({},{}), Incoming({},{}",
            self.to_janus_routing_key,
            self.to_janus_queue,
            self.from_janus_routing_key,
            self.janus_exchange
        );
        let exclusive_queue_options = QueueDeclareOptions {
            exclusive: true,
            ..Default::default()
        };
        let from_janus = self
            .channel
            .queue_declare("", exclusive_queue_options, FieldTable::default())
            .await?;
        self.channel
            .queue_declare(
                &self.to_janus_queue,
                QueueDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        self.channel
            .exchange_declare(
                &self.janus_exchange,
                lapin::ExchangeKind::Fanout,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await?;

        self.channel
            .queue_bind(
                from_janus.name().as_str(),
                &self.janus_exchange,
                &self.from_janus_routing_key,
                QueueBindOptions::default(),
                FieldTable::default(),
            )
            .await?;

        let consumer = self
            .channel
            .basic_consume(
                from_janus.name().as_str(),
                "",
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await?;

        Ok(RabbitMqConnection {
            to_janus_routing_key: self.to_janus_routing_key.clone(),
            to_janus_queue: self.to_janus_queue.clone(),
            janus_exchange: self.janus_exchange.clone(),
            from_janus_routing_key: self.from_janus_routing_key.clone(),
            incoming_queue: from_janus,
            tag: self.tag.clone(),
            consumer,
            channel: self.channel,
        })
    }
}

impl RabbitMqConnection {
    /// Send a message to the queue setup in this 'connection'
    ///
    /// Returns the correlationID, which can be in principle be ignored. We already use a transaction_id.
    // todo, can we use the same correlationId and TXId
    pub(crate) async fn send<T: Into<Vec<u8>>>(&self, msg: T) -> Result<String, error::Error> {
        let correlation_id = rand::random::<u64>().to_string();
        // routing_key is the queue we are targeting
        self.channel
            .basic_publish(
                "",
                &self.to_janus_queue,
                BasicPublishOptions::default(),
                msg.into(),
                BasicProperties::default().with_correlation_id(correlation_id.clone().into()),
            )
            .await?;
        Ok(correlation_id)
    }
}
