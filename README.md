# SpaTch

SpaTch is a rudimentary message dispatching server, using HTTP and WebSocket to handle user requests and
notifications. For now, it's only using Redis as a database backend.

## Routes

### GET `/`

Returns `Hello, world!`, allows to test if the server is up.

### GET `/last_message`

Returns the last message that has been recorded in the database.

### GET `/message/<id>`

Returns the message corresponding to the requested id.

### POST `/new_message/<message>`

Handles a new message by:
- Saving it to the database
- Notifying all connected websocket channels that a new message has been published
- Returning the id of the new message.

### WS `/notify?messages`

Instantiate a new websocket channel in order fot the client to be notified when a new message is being handled
by the server. In this case, the server sends a `N<id>` message where `<id>` represents the record id of the handled 
message.