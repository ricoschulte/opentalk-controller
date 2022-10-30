<!--
SPDX-FileCopyrightText: OpenTalk GmbH <mail@opentalk.eu>

SPDX-License-Identifier: EUPL-1.2
-->

# kustos

Kustos (from latin `custos`, means guard)

An authorization provider for use in k3k/opentalk.
Currently casbin is used as the main library to provide access checks.
One benefit over using casbin directly is more typesafety with the added cost of removing some flexibility.
This also aims to add an abstraction to be able to switch out the underlying authz framework when needed.


This abstraction allows for request-level and resource-level permissions by enforcing a single resource URL (without scheme and authority).
For this currently GET, POST, PUT, and DELETE are supported as request-level actions and are also used to enforce resource-level permissions.
While the underlying authz framework can use a variance of types Kustos provides strong types for most applications.

Subjects:
* Users (represented by a UUID)
* Groups, user-facing groups (represented by String)
  E.g. Groups from a OIDC Provider such as Keycloak
* Roles, internal-roles (represented by String)
  Like `administrator`, used to provide default super permissions.
  The default model allows access to all endpoints for administrators.

## Getting Started:

1. Make sure to execute the migrations.
2. Make sure default rules are set. E.g. If `/resource` is a authenticated REST endpoint to add elements to that collection via `POST`,
   add the needed rules to allow normal authenticated users to POST to that endpoint to create `resource`s.


## Database

This crate provides barrel migration files to create the correct database schema for use with the current version.
For this call all migrations in the migrations folder in the respective order. You can use a crate like refinery to version your current deployed schema.

## Examples:

```rust
let db = Arc::new(Db::connect_url("postgres://postgres:postgres@localhost/kustos"));
let (mut shutdown, _) = tokio::sync::broadcast::channel(1);
let authz = Authz::new_with_autoload(db, shutdown.subscribe());
authz.grant_role_access("admin".into(), [(&"/users/*", AccessMethod::POST)]);
authz.enforce()
```
