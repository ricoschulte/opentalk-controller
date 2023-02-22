# Protocol

## Terminology

* __Etherpad__ - Open source online editor that allows users to collaboratively edit a document in real-time.

## Overview

The Protocol module allows participants to to collaboratively edit a document in real-time by utilizing `Etherpad`.
The Etherpad is a separate process that runs along the controller. When in a room, participants can click an access link that
opens another browser tab where the Etherpad document (`pad`) can be accessed.

The Etherpad's state is lazily initialized for each room. To initialize and make the etherpad accessible, the moderator has to select
one or more `writers` from the present participants. Once selected, each participant will receive either a write- or read url,
depending on if they were picked as a `writer`.

Each participant has an individual etherpad session that gets set as cookie on the client by the `ep_auth_session` Etherpad plugin.
The plugin provides the additional endpoint `/auth_session` that takes the clients session id as a query parameter, sets the
session cookie on the clients browser, and the forwards the client to the actual `pad`.

A participants write access can be revoked with the `deselect_write` message. The participant will then receive a new read url.
