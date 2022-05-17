use controller::prelude::redis::aio::ConnectionManager;
use controller::prelude::*;

pub async fn setup() -> RedisConnection {
    let redis_url =
        std::env::var("REDIS_ADDR").unwrap_or_else(|_| "redis://0.0.0.0:6379/".to_owned());
    let redis = redis::Client::open(redis_url).expect("Invalid redis url");

    let mut redis_conn = ConnectionManager::new(redis).await.unwrap();

    redis::cmd("FLUSHALL")
        .query_async::<_, ()>(&mut redis_conn)
        .await
        .unwrap();

    RedisConnection::new(redis_conn)
}
