/// Distributed Redis lock to guard concurrent access to the state machine behind the automod.
use displaydoc::Display;
use r3dlock::Mutex;
use uuid::Uuid;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:lock
#[ignore_extra_doc_attributes]
/// Typed key to the automod lock
pub struct RoomAutoModLock {
    room: Uuid,
}

impl_to_redis_args!(RoomAutoModLock);

/// Utility function to create a new [`r3dlock::Mutex`], to have the same parameters everywhere.
pub fn new(room: Uuid) -> Mutex<RoomAutoModLock> {
    Mutex::new(RoomAutoModLock { room }).with_retries(20)
}
