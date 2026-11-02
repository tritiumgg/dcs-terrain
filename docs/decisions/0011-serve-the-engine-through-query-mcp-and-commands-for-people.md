# ADR 0011: Serve the engine through `query`, an MCP server and commands for people

## Status

Accepted

## Context

**Affects:** `design.md` §1 (one interface), §7 (the command line and the
skill's paragraph), §10 risk 13; `plan.md`, the M and H tasks and MS4.

The design had one interface, `dcsterrain query`, JSON in and JSON out, and an
assistant learned it from a skill by writing JSON and running a shell command.
It said there was no MCP server, because the warm state a server would hold
lives in the cache, and left an MCP wrapper for a user who needed one.

The user has now asked for both: an MCP server, the standard way assistants
call tools, and a command line people can use. Reviewing the guide, the user
judged the JSON interface much less friendly than expected, though suited to
assistants and software. Every operation's request and response is already
described by `dcsterrain schema`, which is generated from the types the
handlers take.

## Decision

The engine has three ways in, and all three run the same operations and give
the same answers. `dcsterrain query` stays the interface for software: JSON in
and JSON out, one request or JSON-lines batches. `dcsterrain mcp` is an MCP
server over stdio, with one tool per operation whose input is that operation's
request schema. Commands for people take one subcommand per operation, with
flags generated from the same schema, places given as DCS x and z, MGRS,
latitude and longitude or a name, and readable output. A readable answer is
never the source of truth: `--json` prints the answer `query` gives, byte for
byte. Neither new way adds terrain logic: each turns its input into the
request `query` takes.

### Alternatives considered

- **A thin MCP wrapper in another language or package**, calling the binary
  per request. It adds a runtime to install, loses the open terrain between
  calls, and goes out of step with the schema.
- **A separate binary for people.** Two release archives per platform and two
  argument parsers to keep in step, for no gain.
- **Hand-written flags per operation.** Readable, but every operation's fields
  drift from the schema; generating them keeps every operation covered.
- **Leaving the skill as the only way for assistants.** Assistants without a
  shell cannot use the tool. Speed is not the reason: a fresh `query` process
  answers in 8–23 ms on every installed terrain (`data/targets.md`), since the
  cache holds the warm state.

## Consequences

An assistant that speaks MCP calls the operations as typed tools, with no shell. A person can answer a question without writing JSON.
The skill stays for assistants with only a shell.

The surface to keep correct triples. Generated tool schemas and flags follow
the request types, but the readable output for each operation is written by
hand and can fall behind; operations without it show a plain list. A
long-lived server must hold the 64 MiB limit across many calls, not one. The
MCP library is a new dependency. The README and guide change again.

A user who finds the commands for people unused, or an MCP client that needs
what stdio cannot give, would reopen this.
