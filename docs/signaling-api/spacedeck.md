# Spacedeck

## Terminology

* __Space__ The actual whiteboard. Terminology inherited from spacedeck.

## Overview

The spacedeck module allows participants to collaboratively edit a whiteboard (space).

A moderator has to initialize the space for a room with the [`initialize`](#initialize) action.
Once initialized, every participant in a room will get an access URL to the created space via the [`space_url`](#spaceurl) message.

A moderator can snapshot the current state of the space as a PDF document with the [`generate_pdf`](#generatepdf) action.
The access URL to the PDF is sent to every participant in the room.

## Joining the room

When joining a room, the `join_success` message contains the current status of the spacedeck module.

The module data has the following structure:

| Field    | Type     | Required                         | Description                                                   |
| -------- | -------- | -------------------------------- | ------------------------------------------------------------- |
| `status` | `enum`   | yes                              | Either `"not_initialized"`, `"initializing"`, `"initialized"` |
| `url`    | `string` | when `status` is `"initialized"` | The access url to the space of the room                       |

## Commands

### Overview

* [`initialize`](#initialize)
* [`generate_pdf`](#generatepdf)

### Initialize

The `Initialize` message can be sent by a moderator to initialize a new space for the room.

Once initialized, joining participants will receive the access URL via the `join_success` message.

#### Response

A [`SpaceUrl`](#spaceurl) message with the access URL to the space is sent to every participant in the room.

#### Fields

| Field    | Type   | Required | Description             |
| -------- | ------ | -------- | ----------------------- |
| `action` | `enum` | yes      | Must be `"initialize"`. |

#### Example

```json
{
    "action": "initialize",
}
```

### GeneratePdf

Allows a moderator to generate a PDF from the current state of the space.

Access to the PDF is given to all participants in the room via the `PdfUrl` event.

#### Response

A [`PdfUrl`](#pdfurl)message with the access URL to the PDF document is sent to every participant in the room.

#### Fields

| Field    | Type   | Required | Description               |
| -------- | ------ | -------- | ------------------------- |
| `action` | `enum` | yes      | Must be `"generate_pdf"`. |

#### Example

```json
{
    "action": "generate_pdf",
}
```

---

## Events

### Overview

* [`space_url`](#spaceurl)
* [`pdf_url`](#pdfurl)
* [`error`](#error)

### SpaceUrl

Contains the URL to the space of the room.

Is received by every participant when a moderator initialized the space.

#### Fields

| Field     | Type     | Required | Description                      |
| --------- | -------- | -------- | -------------------------------- |
| `message` | `enum`   | yes      | Is `"space_url"`.                |
| `url`     | `string` | yes      | The URL to the space of the room |

#### Example

```json
{
    "message": "space_url",
    "url": "https://spacedeck.opentalk.eu/s/0c5a6c7-00000000-0000-0000-0000-000000000000"
}
```

---

### PdfUrl

Contains the URL to the PDF document of the space.

Is received by every participant when a moderator generates a PDF document for the space.

#### Fields

| Field     | Type     | Required | Description                              |
| --------- | -------- | -------- | ---------------------------------------- |
| `message` | `enum`   | yes      | Is `"pdf_url"`.                          |
| `url`     | `string` | yes      | The URL to the PDF document of the space |

#### Example

```json
{
    "message": "generate_pdf",
    "url": "https://spacedeck.opentalk.eu/<path-to-pdf>"
}
```

---

### Error

An error has occurred while issuing a command.

#### Fields

| Field                     | Type       | Required | Description                                                                             |
| ------------------------- | ---------- | -------- | --------------------------------------------------------------------------------------- |
| `message`                 | `enum`     | yes      | Is `"error"`.                                                                           |
| `InsufficientPermissions` | `[string]` | yes      | The requesting user has insufficient permissions for the operation                      |
| `CurrentlyInitializing`   | `[string]` | yes      | The spacedeck initialization was already issued and spacedeck is currently initializing |
| `InitializationFailed`    | `[string]` | yes      | The spacedeck initialization failed                                                     |
| `AlreadyInitialized`      | `[string]` | yes      | Spacedeck is already initialized and accessible                                         |

#### Example

```json
{
    "message": "error",
    "error": "insufficient_permissions"
}
```
