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
COPY ./crates/test-util/Cargo.toml ./crates/test-util/Cargo.toml
COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock

RUN find crates -maxdepth 1 -type d -exec mkdir {}/src \; -exec touch {}/src/lib.rs \;
RUN mkdir src
RUN touch src/main.rs

RUN mkdir .cargo
RUN cargo vendor > .cargo/config

COPY src src
COPY crates crates

RUN cargo build --release

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libpq5

WORKDIR /controller

COPY --from=Builder /controller/target/release/k3k-controller .
COPY extra/example.toml config.toml

EXPOSE 11311
ENTRYPOINT [ "./k3k-controller" ]
