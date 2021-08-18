#!/bin/sh

ROOT=$(git rev-parse --show-toplevel)

cd "${ROOT}" || exit

export DATABASE_URL="postgres://postgres:password123@localhost:5432/k3k"

diesel print-schema | rustfmt > crates/controller/src/db/schema.rs
