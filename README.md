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

## Build the container image

The `Dockerfile` is located in `ci/Dockerfile`.

To build the image, execute in the root of the repository:

```bash
 docker build -f ci/Dockerfile . --tag <your tag>
```


## Configuration

k3k-controller looks for a config file at ./config.toml.
There is an example config file in extra/example.toml
You can specify a different config file using the `-c` argument.

```sh
k3k-controller -c other_config.toml
```

Settings specified in the config file can be overwritten by environment variables.
To do so, set an environment variable with the prefix `K3K_CTRL_` followed by the field names you want to set.
nested fields are separated by two underscores `__`.
```sh
K3K_CTRL_<field>__<field-of-field>...
```
### Example

set the `database.url` field:
```sh
K3K_CTRL_DATABASE__URL=postgres://k3k:s3cur3_p4ssw0rd@localhost:5432/k3k
```

So the field 'database.max_connections' would resolve to:
```sh
K3K_CTRL_DATABASE__MAX_CONNECTIONS=5
```
### Note
Fields set via environment variables do not affect the underlying config file.
