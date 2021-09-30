#!/bin/sh

ROOT=$(git rev-parse --show-toplevel)

cd "${ROOT}" || exit



export POSTGRES_BASE_URL="postgres://postgres:password123@localhost:5432"
export DATABASE_NAME="k3k_migr"

DB_THING="${POSTGRES_BASE_URL}/${DATABASE_NAME}"

cargo test -p k3k-controller-core --lib -- db::migrations::migration_tests::test_migration --exact

diesel --database-url ${DB_THING} print-schema | rustfmt > crates/controller/src/db/schema.rs
