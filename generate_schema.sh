#!/bin/sh

ROOT=$(git rev-parse --show-toplevel)

cd "${ROOT}" || exit



export POSTGRES_BASE_URL="postgres://postgres:password123@localhost:5432"


cd crates/db-storage && cargo run -p k3k-db-storage --bin generate_schema --features build-binary
