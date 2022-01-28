# K3K Controller

Authentication and scheduling backend for the K3K conference system.

The controller is split into multiple crates to allow for extensibility.

This root crate uses all crates to run the controller with its full feature set.

## Manual
```
k3k-controller 0.1.0

USAGE:
    k3k-controller [FLAGS] [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
        --reload     Triggers a reload of the Janus Server configuration
    -V, --version    Prints version information

OPTIONS:
    -c, --config <config>    Specify path to configuration file [default: config.toml]

SUBCOMMANDS:
    acl           Modify the ACLs
    fix-acl       Rebuild ACLs based on current data
    help          Prints this message or the help of the given subcommand(s)
    migrate-db    Migrate the db. This is done automatically during start of the controller, but can be done without
                  starting the controller using this command
```


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

## Building the container image(s)

Building the container image(s) is split in two steps.

### 1. Build the controller binaries itself

Run the following command in the project dir.

```bash
cargo build --release --locked --workspace --target x86_64-unknown-linux-gnu
```

### 2. Build the container image

The Dockerfiles expect the binaries in `./target/release`.
After the build, execute the following commands in the project dir.

```bash
# Build the controller image
docker build -f ci/Dockerfile . --tag <your-image-name>:<your-image-tag>
```

```bash
# Build the room provisioner image
docker build -f crates/room-provisioner/ci/Dockerfile . --tag <your-image-name>:<your-image-tag>
```


## Upgrading

After installing/deploying the new version you can run the `fix-acl` subcommand to update the authz rules/ACLs to match the newest version. This is most certainly needed when new endpoints are added for already present resources.

## Configure ACLs

A couple of ACLs can be set via the binary. For this use the `acl` subcommand.
Currently only room access is a supported option for this subcommand.
This subcommand is expected to change frequently when more features for access rules (e.g. invites) getting implemented.


## Sub-crates

Inside the crates folder following crates can be found:

* [controller](crates/controller)
    - core crate which contains all of the controllers core features
    - OpenID Connect user authentication
    - Database connection and interfacing
    - `actix_web` based HTTP servers for external and internal APIs
    - Extensible signaling websocket endpoint for video room signaling
* [controller-shared-types](crates/controller-shared-types)
    - Shared types among the controller and modules
* [db-storage](crates/db-storage)
    - Database types used for the controller and modules
* [janus-media](crates/janus-media)
    - media signaling module using the `janus-client` crate
* [chat](crates/chat)
    - chat signaling module which implements a simple room- and private chat
* [ee-chat](crates/ee-chat)
    - chat signaling module for the enterprise edition which implements group chats inside a room
* [automod](crates/automod)
    - signaling module implementing automoderation for videoconferences
* [legal-vote](crates/legal-vote)
    - signaling module implementing legal vote for videoconferences
* [polls](crates/polls)
    - signaling module implementing polls for videoconferences
* [client](crates/client) EXPERIMENTAL
    - Client side implementation of the controllers APIs used for testing
* [r3dlock](crates/r3dlock)
    - redis redlock distributed lock implementation for a single instance
* [janus-client](crates/janus-client)
    - janus client library for interfacing with the [Janus WebRTC Server](https://janus.conf.meetecho.com/)
* [kustos](crates/kustos)
    - authz abstraction based on casbin-rs
* [room-provisioner](crates/room-provisioner)
* [test-util](crates/test-util)
