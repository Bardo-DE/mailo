# mailo-logic

The decisions of a mail client that need no I/O: how an instant is written for the person
reading it, how a typed time and a typed search line are read, expanded, ranked and marked, what
an undo reverses, what a filter means, what a discovery answer comes to and which provider an
address is on, what the junk filter's words say, when a rule may send on its own, and which
profile a store belongs to.
Values in, values out. Nothing here opens a file or a socket, reads a clock or the environment,
asks the store, or draws a window; a clock is a value, and a store or an index is a trait the caller implements (`search::Source`).

What the decisions are made over is `mailo-domain`; what acts on them, the sync, the store and
the network, is `mail-core` and the crates under it, which depend on this one.

Part of [mailo](https://github.com/Bardo-DE/mailo). Licensed under MIT or Apache-2.0, at your
option.
