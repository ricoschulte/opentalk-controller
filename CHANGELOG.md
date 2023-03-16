# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- janus-media: add `resubscribe` message to allow clients to restart the webrtc session of a subscription.
- controller/db-storage: add initial tariff support. Requires JWT claims to include a `tariff_id`.
- controller: invite verify response contains a `password_required` flag ([#329](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/329))
- controller: add `participant_limit` quota to restrict the maximum amount of participants in a room ([#332](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/332))
- controller: add `enabled_modules` ([#334](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/334)), `tariff` as part of `JoinSuccess` message, API endpoints for `users/me/tariff` and `rooms/{room_id}/tariff` ([#331](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/331))
- controller: add `time_limit` quota to restrict the duration of a meeting ([#333](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/333))

### Changed

- controller: authenticated users can join meetings without a password ([#335](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/335))

## [0.2.0] - 2023-03-13

### Added

- controller: enable accepted participants to skip waiting room when joining or returning from a breakout room ([#303](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/303))
- controller: announce available modules in `join_success` message ([#308](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/308))

### Changed

- timer: add the `kind` field to distinguish between a `stopwatch` and `countdown` more clearly ([#316](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/316))
- timer: add the `style` field to the `start` & `started` messages and let clients tag a timer with a custom style ([#316](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/316))
- controller: add support for multi tenancy [#286](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/286)
- timer: distribute timer handling over all participant runners, allowing timers to finish if the moderator has left ([#210](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/210))

## [0.1.0] - 2023-03-01

### Added

- add license information
- controller: allow overriding some build-time environment variables ([#137](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/137))
- chat: add `last_seen_timestamp` fields [#242](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/242)
- legal-vote: add option to automatically create a PDF asset when a vote has ended ([#259](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/259))
- legal-vote: add new `live_roll_call` vote kind which sends out live updates while the vote is running ([#285](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/285))
- controller: add config to grant all participants the presenter role by default ([#318](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/318))

### Fixed

- protocol: fixed the `createAuthorIfNotExistsFor` API call that always returned the same author id due to a typo in the query
- janus-media: fixed a permission check for screen-share media session updates
- protocol: fixed a bug where joining participants got write access by default ([#306](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/306))
- protocol: fixed a bug where the etherpad pad was deleted when any user left the room ([#319](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/319))
- signaling: fixed a bug which caused rooms to never be destroyed if a participant was joining from the waiting-room ([#321](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/321))

### Changed

- controller: use derive and attribute macros for conversion to/from redis values ([#283](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/283))
- protocol: read/write access level information is now sent to every participant [#299](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/299)
- chat/ee-chat: merged ee-chat into chat ([#265](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/265))
- legal-vote: votes are now token-based, allowing for `pseudonymous` votings where only the tokens, not the participants are published ([#271](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/271))
- updated dependencies

### Removed

- legal-vote: live updates for `roll_call` and `pseudonymous` votes, results are instead published with the `stopped` message ([#272](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/272))
- automod: will not be part of the community edition ([#257](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/257))
- legal-vote: will not be part of the community edition ([#257](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/257))

## [0.0.0-internal-release.10] - 2022-12-09

### Added

- controller: added `waiting_room_state` enum to participants in waiting room ([#245](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/245))

### Fixed

- recording: properly read participants consent when they join or update their state
- recording: only delete the current recording state when the actual recorder leaves

## [0.0.0-internal-release.9] - 2022-12-02

### Added

- controller: add an S3 storage interface for saving assets in a long-term storage ([#214](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/214))
- whiteboard: save generated PDF files in S3 storage ([#225](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/225))
- legal-vote: add `hidden` parameter to exclude vote choices from outgoing messages ([#260](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/260))
- protocol: save generated PDF files in S3 storage ([#258](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/258))
- controller: add the `recorder` module allowing moderators to record a meeting

### Removed

- controller: `status` field from event resource ([#221](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/221))

### Changed

- controller: introduce `v1/services/..` path for service related endpoints.
- controller: move call-in's start endpoint from `v1/rooms/sip/start` to `v1/services/call_in/start` to make use of the new service authentication.
- controller: trim unnecessary whitespaces in the display name of users and guests ([#96](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/96))

### Fixed

- Respect custom `--version` implementation ([#255](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/255))
- controller: properly handle `is_adhoc` field in the `PATCH events/<event_id>` ([#264](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/264))
- controller: added the missing permission suffix `/assets` when giving access to a room
- controller: fixed a bug where environment variables did not overwrite config values ([#263](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/263))

## [0.0.0-internal-release.8] - 2022-11-10

### Added

- controller: add `time_independent` filter to events GET request ([#155](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/155))
- mail-worker-protocol: add types to support event-update emails ([#211](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/211))
- controller: send email notification to invitees on event update ([#211](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/211))
- controller: add `suppress_email_notification` flag to event and invite endpoints ([#267](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/267))

### Changed

- strictly follow keep-a-changelog format in `CHANGELOG.md` ([#254](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/254))
- controller: rename `spacedeck` module to `whiteboard` ([#240](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/240))
- controller: return any entry for `GET /events` overlapping `time_min..time_max` range, not just those fully enclosed by it. ([#154](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/154))
- controller: disallow `users/find` queries under 3 characters

### Fixed

- controller/signaling: add missing state checks for control-messages

### Removed

- chat/ee-chat: redundant timestamp removed from outgoing chat messages

## [0.0.0-internal-release.7] - 2022-10-27

### Added

- controller: added metrics for the number of participants with audio or video unmuted
- controller: add `is_adhoc` flag to events
- chat: allow moderators to clear the global chat history
- janus-media: add the `presenter` role to restrict screenshare access
- janus-media: add reconnect capabilities for mcu clients

### Changed

- controller: runner's websocket error messages straightened (`text` field renamed to `error`, values changed to slug style)

## [0.0.0-internal-release.6] - 2022-10-12

### Added

- controller: add `waiting_room` flag to event responses

### Fixed

- janus-media: update focus detection on mute

### Changed

- trace: replace the setting `enable_opentelemetry` with `jaeger_agent_endpoint`
- chat/ee-chat: increase maximum chat message size to 4096 bytes

## [0.0.0-internal-release.5] - 2022-09-30

### Added

- protocol: added the `deselect_writer` action to revoke write access ([#145](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/145))
- controller: added the spacedeck module that allows participants to collaboratively edit a whiteboard ([#209](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/209))
- controller: added a query parameter to the `GET /events` endpoint to allow filtering by `invite_status` ([#213](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/213))
- breakout: added `joined_at` & `left_at` attributes to participants
- controller: toggle raise hands status (actions `enable_raise_hands`, `disable_raise_hands` and according messages) ([#228](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/228))
- controller: added moderator feature to forcefully lower raised hands of all participants ([#227](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/227))
- chat: added feature to toggle chat status (actions `enable_chat`, `disable_chat` and according messages) ([#229](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/229))
- ee-chat: added check for chat status (enabled/disabled) ([#229](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/229))
- controller: added waiting room flag to stored events ([#224](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/224))
- controller: events now include unregistered invitees in invitees lists, distinguishable by `kind` profile property ([#196](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/196))

### Fixed

- controller: fixed a bug where a wrong `ends_at` value for reoccurring events was sent to the mail worker ([#218](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/218))
- controller: fix pagination serialization ([#217](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/217))
- janus-media: added target and type information to some error responses ([#219](https://git.opentalk.dev/opentalk/k3k-controller/-/issues/219))

## [0.0.0-internal-release.4] - 2022-08-29

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

## [0.0.0-internal-release.3] - 2022-07-20

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

## [0.0.0-internal-release.2] - 2022-06-22

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

## [0.0.0-internal-release.1] - 2022-06-14

### Added

- initial release candidate

[Unreleased]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/v0.2.0...main

[0.2.0]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/97c85ca10d136652bc1656792dcf1a539ea4e7a5...v0.2.0

[0.1.0]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/8b6e62c700376aa82fab9eab07346207becf7c78...v0.1.0


[0.0.0-internal-release.10]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/8302382ac420eccc069ca891e0bd067ef6140754...8b6e62c700376aa82fab9eab07346207becf7c78
[0.0.0-internal-release.9]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/446647a13f2e163f1be02cefbdaf04e201598444...8302382ac420eccc069ca891e0bd067ef6140754
[0.0.0-internal-release.8]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/8cbc5ed8d23adc95fa3f8e128bbbe84b50977088...446647a13f2e163f1be02cefbdaf04e201598444
[0.0.0-internal-release.7]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/daf5e7e8279bbe48af4240acf74ecbaf8119eb7a...8cbc5ed8d23adc95fa3f8e128bbbe84b50977088
[0.0.0-internal-release.6]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/312226b387dea53679a85c48c095bce769be843b...daf5e7e8279bbe48af4240acf74ecbaf8119eb7a
[0.0.0-internal-release.5]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/c3d4da97a1fb32c44956281cc70de6568b3e8045...312226b387dea53679a85c48c095bce769be843b
[0.0.0-internal-release.4]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/248350563f6de3bd7dab82c2f183a9764fbe68ee...c3d4da97a1fb32c44956281cc70de6568b3e8045
[0.0.0-internal-release.3]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/f001cf6e5a3f7d8e0da29cc9d1c6d1ad744a717f...248350563f6de3bd7dab82c2f183a9764fbe68ee
[0.0.0-internal-release.2]: https://git.opentalk.dev/opentalk/k3k-controller/-/compare/b64afd058f6cfa16c67cbbad2f98cd0f2be3181d...f001cf6e5a3f7d8e0da29cc9d1c6d1ad744a717f
[0.0.0-internal-release.1]: https://git.opentalk.dev/opentalk/k3k-controller/-/commits/b64afd058f6cfa16c67cbbad2f98cd0f2be3181d
