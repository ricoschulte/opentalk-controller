# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- controller: `time_independent` filter to events GET request
- mail-worker-protocol: add types to support event-update emails ([#221](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/221))
- controller: send email notification to invitees on event update ([#221](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/221))

### Changed

- strictly follow keep-a-changelog format in `CHANGELOG.md` ([#254](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/254))
- controller: rename `spacedeck` module to `whiteboard` ([#240](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/240))
- controller: return any entry for `GET /events` overlapping `time_min..time_max` range, not just those fully enclosed by it.

## [1.0.0-rc.7] - 2022-10-27

### Added

- controller: added metrics for the number of participants with audio or video unmuted
- controller: add `is_adhoc` flag to events
- chat: allow moderators to clear the global chat history
- janus-media: add the `presenter` role to restrict screenshare access
- janus-media: add reconnect capabilities for mcu clients

### Changed

- controller: runner's websocket error messages straightened (`text` field renamed to `error`, values changed to slug style)

## [1.0.0-rc.6] - 2022-10-12

### Added

- controller: add `waiting_room` flag to event responses

### Fixed

- janus-media: update focus detection on mute

### Changed

- trace: replace the setting `enable_opentelemetry` with `jaeger_agent_endpoint`
- chat/ee-chat: increase maximum chat message size to 4096 bytes

## [1.0.0-rc.5] - 2022-09-30

### Added

- protocol: added the `deselect_writer` action to revoke write access ([#145](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/145))
- controller: added the spacedeck module that allows participants to collaboratively edit a whiteboard ([#209](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/209))
- controller: added a query parameter to the `GET /events` endpoint to allow filtering by `invite_status` ([#213](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/213))
- breakout: added `joined_at` & `left_at` attributes to participants
- controller: toggle raise hands status (actions `enable_raise_hands`, `disable_raise_hands` and according messages) ([#228](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/228))
- controller: added moderator feature to forcefully lower raised hands of all participants ([#227](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/227))
- chat: added feature to toggle chat status (actions `enable_chat`, `disable_chat` and according messages) ([#229](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/229))
- ee-chat: added check for chat status (enabled/disabled) ([#229](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/229))
- controller: added waiting room flag to stored events ([#224](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/224))
- controller: events now include unregistered invitees in invitees lists, distinguishable by `kind` profile property ([#196](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/196))

### Fixed

- controller: fixed a bug where a wrong `ends_at` value for reoccurring events was sent to the mail worker ([#218](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/218))
- controller: fix pagination serialization ([#217](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/217))
- janus-media: added target and type information to some error responses ([#219](https://git.heinlein-video.de/heinlein-video/k3k-controller/-/issues/219))

## [1.0.0-rc.4] - 2022-08-29

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

## [1.0.0-rc.3] - 2022-07-20

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

## [1.0.0-rc.2] - 2022-06-22

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

## [1.0.0-rc.1] - 2022-06-14

### Added

- initial release candidate

[Unreleased]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/compare/v1.0.0-rc.7...main
[1.0.0-rc.7]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/compare/v1.0.0-rc.6...v1.0.0-rc.7
[1.0.0-rc.6]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/compare/v1.0.0-rc.5...v1.0.0-rc.6
[1.0.0-rc.5]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/compare/v1.0.0-rc.4...v1.0.0-rc.5
[1.0.0-rc.4]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/compare/v1.0.0-rc.3...v1.0.0-rc.4
[1.0.0-rc.3]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/compare/v1.0.0-rc.2...v1.0.0-rc.3
[1.0.0-rc.2]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/compare/v1.0.0-rc.1...v1.0.0-rc.2
[1.0.0-rc.1]: https://git.heinlein-video.de/heinlein-video/k3k-controller/-/commits/v1.0.0-rc.1
