# Room Provisioner

A small cli tool to preprovision rooms and users.

The rooms and users need to be defined in a `provisioning.yaml`-file.
An example can be found in `./example/provisioning.yaml`.

It is important that the user, to which a room belongs exists.
That means the user already exists (as a entree in the user table of the postgresql database) and there uuid is valid.

The uuid in the `owner: <uuid>`-field of the rooms has to belong to a already created user.

## Environment Variables

A couple of environment variables have to be set.

#### **DB_USER** (default: "pguser")

User to interact with the Prostgresql user.

#### **DB_PASSWORD** (default: "pgpass")

Password for the Prostgresql user.

#### **DB_HOST** (default: "localhost")

Url of the Postgresql instance.

#### **DB_PORT** (default: 5432)

Port to establish the connection over. Default value is the default port used by Postgresql.

#### **DB_NAME** (default: "OpenTalk")

Name of the Postgresql database to apply the changes to.

#### **PROVISIONING_YAML_PATH** (default: "./provisioning")

Path to the `provisioning.yaml`. The default value assumes it is located in `/provisioner` directly besides the provisioner executable.

## How it works

The provisioning tool reads users and rooms from the `provisioning.yaml`-file.

The users are created first because the users `<user-id>` is needed for creation of the room.

Afterwards the provisioning tool replaces the `owner: <user-uuid>` of each room with the `<user-id>` (The `<user-id>` is the primary key of the postgresql user table) and creates the necessary entree in the rooms table.

## Building the container

The room-provisioner is tought to run as some kind of pre-/init-container before the controller itself starts.

The container can be build with the `Dockerfile` in `./ci/`.
Just run from root of that repository:

```bash
docker build -f ./crates/room-provisioner/ci/Dockerfile . --tag room-provisioner:latest
```
