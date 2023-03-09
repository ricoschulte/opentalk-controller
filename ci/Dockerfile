# SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>
#
# SPDX-License-Identifier: EUPL-1.2

FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libpq5 ca-certificates protobuf-compiler

WORKDIR /controller

COPY target/release/k3k-controller .
COPY extra/example.toml config.toml

EXPOSE 11311
ENTRYPOINT [ "./k3k-controller" ]
