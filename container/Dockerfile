FROM rust:slim AS Builder

WORKDIR /controller

RUN apt-get update && apt-get install -y libpq-dev

COPY ./crates/automod/Cargo.toml ./crates/automod/Cargo.toml
COPY ./crates/chat/Cargo.toml ./crates/chat/Cargo.toml
COPY ./crates/client/Cargo.toml ./crates/client/Cargo.toml
COPY ./crates/controller/Cargo.toml ./crates/controller/Cargo.toml
COPY ./crates/controller-shared-types/Cargo.toml ./crates/controller-shared-types/Cargo.toml
COPY ./crates/database/Cargo.toml ./crates/database/Cargo.toml
COPY ./crates/db-storage/Cargo.toml ./crates/db-storage/Cargo.toml
COPY ./crates/ee-chat/Cargo.toml ./crates/ee-chat/Cargo.toml
COPY ./crates/etherpad-client/Cargo.toml ./crates/etherpad-client/Cargo.toml
COPY ./crates/janus-client/Cargo.toml ./crates/janus-client/Cargo.toml
COPY ./crates/janus-media/Cargo.toml ./crates/janus-media/Cargo.toml
COPY ./crates/kustos/Cargo.toml ./crates/kustos/Cargo.toml
COPY ./crates/legal-vote/Cargo.toml ./crates/legal-vote/Cargo.toml
COPY ./crates/polls/Cargo.toml ./crates/polls/Cargo.toml
COPY ./crates/r3dlock/Cargo.toml ./crates/r3dlock/Cargo.toml
COPY ./crates/protocol/Cargo.toml ./crates/protocol/Cargo.toml
COPY ./crates/test-util/Cargo.toml ./crates/test-util/Cargo.toml
COPY ./xtask/Cargo.toml ./xtask/Cargo.toml
COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock

RUN find crates -mindepth 1 -maxdepth 1 -type d -exec mkdir {}/src \; -exec touch {}/src/lib.rs \;
RUN mkdir src xtask/src
# We need a proper main.rs to make the first cargo build run
RUN echo 'fn main() {println!("Hello World!");}' > src/main.rs
RUN echo 'fn main() {println!("Hello World!");}' > xtask/src/main.rs
# As we haven't copied any source files yet, this will only compile the dependencies
RUN cargo fetch --locked

# Now copy the real source over
COPY src src
COPY crates crates
# Cargo detects changes only by mtime
RUN touch src/main.rs xtask/src/main.rs && find crates -iname lib.rs -exec touch {} \;
RUN cargo build --release --locked

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libpq5

WORKDIR /controller

COPY --from=Builder /controller/target/release/k3k-controller .
COPY extra/example.toml config.toml

EXPOSE 11311
ENTRYPOINT [ "./k3k-controller" ]
