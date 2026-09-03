# Spec: MCP server

`dcsterrain serve <file>` exposes the operations of
`query-operations.md` as MCP tools over stdio. It is a thin layer:
no logic lives here that is not in `dcsterrain-core::ops`. Build it with
the `mcp-builder` skill loaded, using the official Rust MCP SDK
(`rmcp`), and follow that skill's guidance on tool naming, descriptions,
error shape and response size.

## Process

- `dcsterrain serve <file> [--file <file>...] [--max-rows N]
  [--log <path>]`. One or more packed files; each is opened read-only at
  start and `describe`d, and the theatre id becomes the value of a
  `theatre` parameter that every tool accepts (required when more than
  one file is loaded, defaulted otherwise).
- Transport: stdio, JSON-RPC per the MCP spec. No network listener.
- Logging to stderr or `--log`; never to stdout.
- Startup fails fast with a readable message if a file does not open or
  `check`-level `meta` keys are missing.

## Tools

One tool per operation, same names, same request and response structs
serialised as the tool's input schema and result. The input schema is
generated from the `serde` types with `schemars`, so a field added to an
operation appears in the tool without a second edit. Tool descriptions
are the one-paragraph summaries from `query-operations.md`, in the
imperative, and each states units (metres, degrees, DCS x north and z
east) and the region-bound forms it accepts.

| Tool | Operation |
|---|---|
| `terrain_describe` | `describe` |
| `terrain_sample` | `sample` |
| `terrain_score_site` | `score_site` |
| `terrain_find_sites` | `find_sites` |
| `terrain_visible` | `visible` |
| `terrain_viewshed` | `viewshed` |
| `terrain_coverage` | `coverage` |
| `terrain_approach_spectrum` | `approach_spectrum` |
| `terrain_unmask_profile` | `unmask_profile` |
| `terrain_route` | `route` |
| `terrain_route_alternatives` | `route_alternatives` |
| `terrain_nearest` | `nearest` |
| `terrain_scenery_in` | `scenery_in` |
| `terrain_chokepoints` | `chokepoints` |
| `terrain_trafficability` | `trafficability` |
| `terrain_geo` | `geo` |

Two conveniences that exist only here, because an assistant needs them
and a program does not:

- `terrain_criteria_help`: returns the criteria table of
  `query-operations.md` as structured data, so a model can
  discover the closed set without reading source.
- `terrain_metrics_help`: returns the metric definitions of
  `using-the-data.md` ("The metrics") as structured data, one entry
  per field with its source operation and its unit. It carries no
  difficulty tables; how a campaign turns metrics into difficulty is
  the campaign's business.

## Response discipline

- Results are the operation's JSON. `viewshed` bitmasks, large
  `sample` sets and `scenery_in` object lists are the outputs that can
  be big; the server caps `sample` rows and `scenery_in` objects at
  `--max-rows` (default 2 000, lower than the CLI's) and
  returns `viewshed` as polygons by default with the bitmask only on
  request.
- Errors come back as MCP tool errors carrying the `Issue` list, never
  as a successful result with an error field.
- Every result includes the `theatre`, `dcs_build`, `terrain_digest`
  and `cell_size` fields the operations already carry, so the model can
  cite what it used.

## Resources

One MCP resource per loaded file, `dcsterrain://<theatre>/describe`,
returning the `describe` output, so a client can read the dataset's
self-description without a tool call.

## Testing

- Unit: the tool list equals the operation list plus the two
  conveniences; every tool's generated schema round-trips a sample
  request; an unknown `theatre` value errors.
- Integration: spawn the server on the synthetic packed file, drive it
  with the SDK's test client over an in-process stdio pair (`rmcp`
  accepts any `(AsyncRead, AsyncWrite)` pair as a transport, so a
  `tokio::io::duplex` pair serves), call every
  tool once with a request from the operation tests, and compare the
  result to calling the operation directly. Run the `mcp-builder`
  skill's evaluation approach on the tool descriptions with a handful
  of natural-language tasks (the worked tasks in the design doc are the
  set) to check a model picks the right tool with the right region
  form.

## Acceptance

1. `dcsterrain serve` on the synthetic file answers every tool from a
   stock MCP client.
2. An assistant given only the tool list and the design doc's FARP
   example produces the same candidate list as the direct operation.
3. No stdout output other than JSON-RPC frames.
