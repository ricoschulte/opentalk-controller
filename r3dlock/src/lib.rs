//! Implementation of a redlock mutex for a single redis instance

use rand::{thread_rng, Rng};
use redis::aio::ConnectionLike;
use redis::{Script, ToRedisArgs, Value};
use std::time::{Duration, Instant};
use tokio::time::sleep;

mod error;
pub use error::{Error, Result};

const LOCK_TIME: Duration = Duration::from_secs(30);

const UNLOCK_SCRIPT: &str = r"
if redis.call('get',KEYS[1]) == ARGV[1] then
    return redis.call('del',KEYS[1])
else
    return 0
end";

/// Represents a redlock mutex over a resource inside a single redis instance
///
/// The lock can be aquired using [`lock()`](Mutex::lock()).
pub struct Mutex<C, K> {
    redis: C,
    key: K,
}

/// Represents a locked redlock mutex
///
/// As these locks can expire in redis, this carries an Instant. Call [`is_locked()`](MutexGuard::is_locked())
/// During unlock, it is checked whether the canary is still present as the locks key value.
pub struct MutexGuard<'a, C, K> {
    key: &'a K,
    redis: &'a mut C,
    canary: Vec<u8>,
    created: Instant,
    locked: bool,
}

impl<C, K> MutexGuard<'_, C, K> {
    /// Returns true when the [`MutexGuard`] / locked redlock mutex is still valid
    ///
    /// If the [`MutexGuard`]/lock expired in Redis, this returns false.
    pub fn is_locked(&self) -> bool {
        self.is_locked_internal() && !self.is_expired()
    }

    fn is_expired(&self) -> bool {
        self.created.elapsed() > LOCK_TIME
    }

    fn is_locked_internal(&self) -> bool {
        self.locked
    }
}

impl<C, K> MutexGuard<'_, C, K>
where
    C: ConnectionLike,
    K: ToRedisArgs,
{
    /// Unlocks this [`MutexGuard`] / locked redlock mutex
    ///
    /// If Redis fails to unlock this lock, or this lock is already unlocked, this method returns a [`RedisError`]
    pub async fn unlock(self) -> Result<()> {
        if self.is_expired() {
            return Err(Error::AlreadyExpired);
        }

        let script = Script::new(UNLOCK_SCRIPT);
        let result: i32 = script
            .key(ToRedisArgsRef(self.key))
            .arg(&self.canary[..])
            .invoke_async(self.redis)
            .await?;

        if result == 1 {
            Ok(())
        } else {
            Err(Error::FailedToUnlock)
        }
    }
}

impl<C, K> Drop for MutexGuard<'_, C, K> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            debug_assert!(self.is_locked(), "MutexGuard must be unlocked before drop");
        }
    }
}

impl<C, K> Mutex<C, K>
where
    C: ConnectionLike,
    K: ToRedisArgs,
{
    /// Creates a new [`Mutex`]
    ///
    /// Takes any async redis [`ConnectionLike`] implementation to drive the mutex
    /// and a key which represents the resource used as a lock
    pub fn new(redis: C, key: K) -> Self
    where
        K: ToRedisArgs,
    {
        Self { redis, key }
    }

    /// Locks the [`Mutex`] and returns a [`MutexGuard`] / redlock mutex
    ///
    /// When the lock cannot be acquired, this returns a [`RedisError`] with Kind [`TryAgain`](redis::ErrorKind::TryAgain) and `failed to acquire lock`
    pub async fn lock(&mut self) -> Result<MutexGuard<'_, C, K>> {
        let canary = thread_rng()
            .sample_iter(rand::distributions::Alphanumeric)
            .take(20)
            .collect::<Vec<u8>>();

        for _ in 0..10 {
            let created = Instant::now();

            // Send the SET command to create a lock with the following args:
            // Key: The lock key
            // Value: Canary which is checked during unlock, to see if this is poised
            // NX: Only set the key if it not exists on the server
            // PX + Time: Set expire time
            let res: redis::Value = redis::cmd("SET")
                .arg(ToRedisArgsRef(&self.key))
                .arg(&canary[..])
                .arg("NX")
                .arg("PX")
                .arg(LOCK_TIME.as_millis() as u64)
                .query_async(&mut self.redis)
                .await?;

            if let Value::Okay = res {
                let guard = MutexGuard {
                    key: &self.key,
                    redis: &mut self.redis,
                    canary,
                    created,
                    locked: true,
                };
                return Ok(guard);
            } else {
                sleep(Duration::from_millis(thread_rng().gen_range(10..50))).await;
            }
        }

        Err(Error::CouldNotAcquireLock)
    }
}

/// This as a workaround for the missing impl ToRedisArg for &ToRedisArg, to avoid clones and copies
struct ToRedisArgsRef<'k, K>(&'k K);

impl<K> ToRedisArgs for ToRedisArgsRef<'_, K>
where
    K: ToRedisArgs,
{
    fn write_redis_args<W>(&self, out: &mut W)
    where
        W: ?Sized + redis::RedisWrite,
    {
        self.0.write_redis_args(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lock_unlock_and_relock() {
        let redis_url =
            std::env::var("REDIS_ADDR").unwrap_or_else(|_| "redis://localhost:6379/".to_owned());
        let redis = redis::Client::open(redis_url).expect("Invalid redis url");

        let redis_conn = redis
            .get_multiplexed_async_connection()
            .await
            .expect("Failed to get redis connection");

        let mut mutex = Mutex::new(redis_conn, "test-1MY-REDIS-LOCK");

        let guard = mutex.lock().await.unwrap();
        guard.unlock().await.unwrap();
        let guard2 = mutex.lock().await.unwrap();
        guard2.unlock().await.unwrap();
    }

    #[tokio::test]
    async fn test_double_locking() {
        let redis_url =
            std::env::var("REDIS_ADDR").unwrap_or_else(|_| "redis://localhost:6379/".to_owned());
        let redis = redis::Client::open(redis_url).expect("Invalid redis url");

        let redis_conn = redis
            .get_multiplexed_async_connection()
            .await
            .expect("Failed to get redis connection");

        let mut mutex1 = Mutex::new(redis_conn.clone(), "test2-MY-REDIS-LOCK");
        let mut mutex2 = Mutex::new(redis_conn, "test2-MY-REDIS-LOCK");

        let guard1 = mutex1.lock().await.unwrap();
        let guard2 = mutex2.lock().await.err().unwrap();

        // Unlock first to cleanup redis ressources.
        guard1.unlock().await.unwrap();

        assert_eq!(guard2, Error::CouldNotAcquireLock);
    }
}
