# Recording

## Overview

The recording module allows for the recording of a room

## Joining the room

When joining a room, the `join_success` message contains the recording status of the room.

The module data has the following structure:

| Field          | Type     | Required                     | Description                          |
| -------------- | -------- | ---------------------------- | ------------------------------------ |
| `status`       | `enum`   | yes                          | Either `initializing` or `recording` |
| `recording_id` | `string` | when `status` is `recording` | The id of the recording              |

The participant list contains the recording-consent status in the `recording`-module namespace under the variable
`recording_consent`.

## Commands

### Overview

* [`start`](#start)
* [`stop`](#stop)
* [`set_consent`](#SetConsent)

### Start

The `Start` message can be sent by a moderator to request a recording for the current room.

#### Response

A [`Started`](#started) message with the recording id is sent to every participant in the room.

#### Fields

| Field    | Type   | Required | Description      |
| -------- | ------ | -------- | ---------------- |
| `action` | `enum` | yes      | Must be "start". |

#### Example

```json
{
    "action": "start"
}
```

### Stop

The `Stop` message can be sent by a moderator to stop a recording in the current room.

#### Response

A [`Stopped`](#stopped) message with the recording id is sent to every participant in the room.

#### Fields

| Field          | Type     | Required | Description         |
| -------------- | -------- | -------- | ------------------- |
| `action`       | `enum`   | yes      | Must be "stop".     |
| `recording_id` | `string` | yes      | Id of the recording |

#### Example

```json
{
    "action": "stop",
    "recording_id": "00000000-0000-0000-0000-000000000000"
}
```

### SetConsent

The `SetConsent` message must be sent by every participant to consent to a recording of their video+audio.
Can always be set regardless if a recording is running or a new one is being started.
By default, the consent is always off (`false`).

#### Fields

| Field     | Type   | Required | Description                             |
| --------- | ------ | -------- | --------------------------------------- |
| `action`  | `enum` | yes      | Must be "set_consent".                  |
| `consent` | `bool` | yes      | Set `true` if consenting to a recording |

#### Example

```json
{
    "action": "set_consent",
    "consent": true
}
```

---

## Events

### Overview

* [`started`](#started)
* [`stopped`](#stopped)

### Started

Is received by every participant when a moderator started a recording.

#### Fields

| Field          | Type     | Required | Description      |
| -------------- | -------- | -------- | ---------------- |
| `message`      | `enum`   | yes      | Is "started".    |
| `recording_id` | `string` | yes      | The recording id |

#### Example

```json
{
    "message": "started",
    "recording_id": "00000000-0000-0000-0000-000000000000"
}
```

### Stopped

Is received by every participant when a moderator stopped a recording.

#### Fields

| Field          | Type     | Required | Description      |
| -------------- | -------- | -------- | ---------------- |
| `message`      | `enum`   | yes      | Is "stopped".    |
| `recording_id` | `string` | yes      | The recording id |

#### Example

```json
{
    "message": "stopped",
    "recording_id": "00000000-0000-0000-0000-000000000000"
}
```

---

### Error

An error has occurred while issuing a command.

#### Fields

| Field     | Type   | Required | Description                                                                         |
| --------- | ------ | -------- | ----------------------------------------------------------------------------------- |
| `message` | `enum` | yes      | Is "error".                                                                         |
| `error`   | `enum` | yes      | Is any of `insufficient_permissions`, `invalid_recording_id` or `already_recording` |

#### Example

```json
{
    "message": "error",
    "error": "insufficient_permissions"
}
```
