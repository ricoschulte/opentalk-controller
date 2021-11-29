FROM debian:bullseye-slim

RUN apt-get update && apt-get install -y libpq5

WORKDIR /provisioner

COPY target/release/k3k-room-provisioner .

ENV DB_USER="pguser"
ENV DB_PASSWORD="pgpass"
ENV DB_NAME="OpenTalk"
ENV PROVISIONING_YAML_PATH="./provisioning"

ENTRYPOINT [ "./room-provisioner" ]
