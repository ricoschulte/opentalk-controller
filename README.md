# K3K Controller

Authentication and scheduling backend for the K3K conference system.

The controller is split into multiple crates to allow for extensibility.

This root crate uses all crates to run the controller with its full feature set.

## crates

Inside the crates folder following crates can be found

### controller

- core crate which contains all of the controllers core features
- OpenID Connect user authentication
- Database connection and interfacing
- `actix_web` based HTTP servers for external and internal APIs
- Extensible signaling websocket endpoint for video room signaling

### client

- Client side implementation of the controllers APIs used for testing

### r3dlock

- redis redlock distributed lock implementation for a single instance

### janus-client

- janus client library for interfacing with the [Janus WebRTC Server](https://janus.conf.meetecho.com/)

### janus-media

- media signaling module using the `janus-client` crate

### chat

- chat signaling module which implements a simple room- and private chat

### ee-chat

- chat signaling module for the enterprise edition which implements group chats inside a room

### automod

- signaling module implementing automoderation for videoconferences
