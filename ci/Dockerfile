FROM rust:alpine AS Builder

WORKDIR /controller

RUN apk add gcc musl-dev postgresql-dev

RUN rustup target add x86_64-unknown-linux-musl
RUN rustup update

COPY ./crates/automod/Cargo.toml ./crates/automod/Cargo.toml
COPY ./crates/chat/Cargo.toml ./crates/chat/Cargo.toml
COPY ./crates/client/Cargo.toml ./crates/client/Cargo.toml
COPY ./crates/controller/Cargo.toml ./crates/controller/Cargo.toml
COPY ./crates/ee-chat/Cargo.toml ./crates/ee-chat/Cargo.toml
COPY ./crates/janus-client/Cargo.toml ./crates/janus-client/Cargo.toml
COPY ./crates/janus-media/Cargo.toml ./crates/janus-media/Cargo.toml
COPY ./crates/r3dlock/Cargo.toml ./crates/r3dlock/Cargo.toml
COPY ./crates/legal-vote/Cargo.toml ./crates/legal-vote/Cargo.toml
COPY ./Cargo.toml ./Cargo.toml
COPY ./Cargo.lock ./Cargo.lock

RUN find crates -maxdepth 1 -type d -exec mkdir {}/src \; -exec touch {}/src/lib.rs \;
RUN mkdir src
RUN touch src/main.rs

RUN mkdir .cargo
RUN cargo vendor > .cargo/config

COPY src src
COPY crates crates

RUN cargo build --release --target=x86_64-unknown-linux-musl

FROM alpine

RUN apk add musl-dev postgresql-dev

WORKDIR /controller

COPY --from=Builder /controller/target/x86_64-unknown-linux-musl/release/k3k-controller .
COPY extra/example.toml config.toml

EXPOSE 11311
ENTRYPOINT [ "./k3k-controller" ]
