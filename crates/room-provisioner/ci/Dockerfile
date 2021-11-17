FROM rust:slim AS Builder

WORKDIR /controller

RUN apt-get update && apt-get install -y libpq-dev

COPY ./crates/automod/Cargo.toml ./crates/automod/Cargo.toml
COPY ./crates/chat/Cargo.toml ./crates/chat/Cargo.toml
COPY ./crates/client/Cargo.toml ./crates/client/Cargo.toml
COPY ./crates/controller/Cargo.toml ./crates/controller/Cargo.toml
COPY ./crates/ee-chat/Cargo.toml ./crates/ee-chat/Cargo.toml
COPY ./crates/janus-client/Cargo.toml ./crates/janus-client/Cargo.toml
COPY ./crates/janus-media/Cargo.toml ./crates/janus-media/Cargo.toml
COPY ./crates/r3dlock/Cargo.toml ./crates/r3dlock/Cargo.toml
COPY ./crates/legal-vote/Cargo.toml ./crates/legal-vote/Cargo.toml
COPY ./crates/room-provisioner/Cargo.toml ./crates/room-provisioner/Cargo.toml
COPY ./crates/test-util/Cargo.toml ./crates/test-util/Cargo.toml
COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock

RUN find crates -maxdepth 1 -type d -exec mkdir {}/src \; -exec touch {}/src/lib.rs \;
RUN mkdir src
# We need a proper main.rs to make the first cargo build run
RUN echo 'fn main() {println!("Hello World!");}' > src/main.rs
RUN echo 'fn main() {println!("Hello World!");}' > crates/room-provisioner/src/main.rs
# As we haven't copied any source files yet, this will only compile the dependencies
RUN cargo build --release --locked

# Now copy the real source over
COPY src src
COPY crates crates
# Cargo detects changes only by mtime
RUN touch src/main.rs && touch crates/room-provisioner/src/main.rs && find crates -iname lib.rs -exec touch {} \;
RUN cargo build --release --locked --package k3k-room-provisioner 

ENTRYPOINT [ "bash -c sleep 3600" ]

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libpq5

WORKDIR /provisioner

COPY --from=Builder /controller/target/release/k3k-room-provisioner .

ENV DB_USER="pguser"
ENV DB_PASSWORD="pgpass"
ENV DB_NAME="OpenTalk"
ENV PROVISIONING_YAML_PATH="./provisioning"

ENTRYPOINT [ "./room-provisioner" ]
