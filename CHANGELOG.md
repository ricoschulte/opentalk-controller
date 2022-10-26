# Unreleased

### Added

- controller: added metrics for number of participants with audio or video
- controller: add `is_adhoc` flag to events

# 1.0.0-rc.6 (12 October, 2022)

### Added

- controller: add `waiting_room` flag to event responses

### Fixed

- janus-media: update focus detection on mute

### Changed

- trace: replace the setting `enable_opentelemetry` with `jaeger_agent_endpoint`
- chat/ee-chat: increase maximum chat message size to 4096 bytes

# 1.0.0-rc.5 (30 September, 2022)

### Added

- protocol: added the `deselect_writer` action to revoke write access (#145)
- controller: added the spacedeck module that allows participants to collaboratively edit a whiteboard (#209)
- controller: added a query parameter to the `GET /events` endpoint to allow filtering by `invite_status` (#213)
- breakout: added `joined_at` & `left_at` attributes to participants
- controller: toggle raise hands status (actions `enable_raise_hands`, `disable_raise_hands` and according messages) (#228)
- controller: added moderator feature to forcefully lower raised hands of all participants (#227)
- chat: added feature to toggle chat status (actions `enable_chat`, `disable_chat` and according messages) (#229)
- ee-chat: added check for chat status (enabled/disabled)
- controller: added waiting room flag to stored events (#224)
- controller: events now include unregistered invitees in invitees lists, distinguishable by `kind` profile property (#196)

### Fixed

- controller: fixed a bug where a wrong `ends_at` value for reoccurring events was sent to the mail worker (#218)
- controller: fix pagination serialization (#217)
- janus-media: added target and type information to some error responses (#219)

# 1.0.0-rc.4 (29 August, 2022)

### Added

- controller: added metrics for number of created rooms and number of issued email tasks
- mail-worker-protocol: added `as_kind_str` method to `MailTask`
- controller: added the `timer` module that allows moderators to start a timer for a room
- janus-media: added support for full trickle mode

### Changed

- events-api: added can_edit fields to event related resources
- controller: removed service_name from metrics
- controller: added error context to the keycloak-admin-client
- controller: added the optional claim `nickname` to the login endpoint that will be used as the users `display_name` when set
- janus-media: stopped forwarding RTP media packets while a client is muted

# 1.0.0-rc.3 (20 July, 2022)

### Added

- controller: added metrics for number of participants and number of destroyed rooms

### Fixed

- removed static role assignment of participant in breakout and moderation module which led to inconsistent behavior if the participant's role was changed by a moderator
- controller: fixed wrong returned created_by for GET /rooms/{roomId}
- janus-media: added a missing rename for the outgoing error websocket message
- controller: remove special characters from phone numbers before parsing them

### Changed

- updated dependency version of pin-project
- changed the login endpoint to return a bad-request with `invalid_claims` when invalid user claims were provided

# 1.0.0-rc.2 (22 June, 2022)

### Added

- email-invites: add `ExternalEventInvite` to invite users via an external email address

### Fixed

- config: add `metrics`, `call_in` and `avatar` to settings reload
- controller: set `role` attribute on join
- config: fix room_server.connections example to have better defaults
- controller: respond with 403 instead of 500 when encountering unknown subject in access token
- mail-worker-protocol: fix the `CallIn` and `Room` types to fit their data representation
- janus-client: fixed a race condition where requests were sent before the transaction was registered

### Changed

- update dependency versions of various controller crates

# 1.0.0-rc.1 (14 June, 2022)

- initial release candidate
