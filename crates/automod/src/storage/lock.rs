//! Distributed Redis lock to guard concurrent access to the state machine behind the automod.
use controller::prelude::*;
use displaydoc::Display;
use r3dlock::Mutex;

#[derive(Display)]
/// k3k-signaling:room={room}:automod:lock
#[ignore_extra_doc_attributes]
/// Typed key to the automod lock
pub struct RoomAutoModLock {
    room: SignalingRoomId,
}

impl_to_redis_args!(RoomAutoModLock);

/// Utility function to create a new [`r3dlock::Mutex`], to have the same parameters everywhere.
pub fn new(room: SignalingRoomId) -> Mutex<RoomAutoModLock> {
    Mutex::new(RoomAutoModLock { room }).with_retries(20)
}
