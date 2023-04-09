This is a 'local-copy-fork' of the opentalk controller from https://gitlab.opencode.de/opentalk/controller


<!--
SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>

SPDX-License-Identifier: EUPL-1.2
-->

# K3K Controller

Authentication and scheduling backend for the K3K conference system.

The controller is split into multiple crates to allow for extensibility.

This root crate uses all crates to run the controller with its full feature set.

## Manual

```text
k3k-controller 0.1.0-rc.1

USAGE:
    k3k-controller [FLAGS] [OPTIONS] [SUBCOMMAND]

FLAGS:
    -h, --help       Prints help information
        --reload     Triggers a reload of some controller settings
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

The `Dockerfile` is located at `container/Dockerfile`.

To build the image, execute in the root of the repository:

```bash
docker build -f container/Dockerfile . --tag <your tag>
```

## Configuration

k3k-controller looks for a config file `./config.toml` in the project root.

There is an example config file in `extra/example.toml` that can be copied:

```sh
cp ./extra/example.toml ./config.toml
```

You can specify a different config file using the `-c` argument:

```sh
k3k-controller -c other_config.toml
```

Settings specified in the config file can be overwritten by environment variables.
To do so, set an environment variable with the prefix `K3K_CTRL_` followed by the field names you want to set.
Nested fields are separated by two underscores `__`.

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

## Upgrading

After installing/deploying the new version you can run the `fix-acl` subcommand to update the authz rules/ACLs to match the newest version. This is most certainly needed when new endpoints are added for already present resources.

## Configure ACLs

A couple of ACLs can be set via the binary. For this use the `acl` subcommand.
Currently only room access is a supported option for this subcommand.
This subcommand is expected to change frequently when more features for access rules (e.g. invites) getting implemented.

## Sub-crates

Inside the crates folder following crates can be found:

- [controller](crates/controller)
    - core crate which contains all of the controllers core features
    - OpenID Connect user authentication
    - Database connection and interfacing
    - `actix_web` based HTTP servers for external and internal APIs
    - Extensible signaling websocket endpoint for video room signaling
- [controller-shared-types](crates/controller-shared-types)
    - Shared types among the controller and modules
- [db-storage](crates/db-storage)
    - Database types used for the controller and modules
- [janus-media](crates/janus-media)
    - media signaling module using the `janus-client` crate
- [community-modules](crates/community-modules)
    - functionality for registering all modules in the community edition
    - depends on all modules in the community edition
- [chat](crates/chat)
    - chat signaling module which implements a simple room, group and private chat
- [automod](crates/automod)
    - signaling module implementing automoderation for videoconferences
- [legal-vote](crates/legal-vote)
    - signaling module implementing legal vote for videoconferences
- [polls](crates/polls)
    - signaling module implementing polls for videoconferences
- [client](crates/client) EXPERIMENTAL
    - Client side implementation of the controllers APIs used for testing
- [r3dlock](crates/r3dlock)
    - redis redlock distributed lock implementation for a single instance
- [janus-client](crates/janus-client)
    - janus client library for interfacing with the [Janus WebRTC Server](https://janus.conf.meetecho.com/)
- [kustos](crates/kustos)
    - authz abstraction based on casbin-rs
- [test-util](crates/test-util)
- [types](crates/types)
    - types that are shared across different crates, such as Web API and signaling messages
