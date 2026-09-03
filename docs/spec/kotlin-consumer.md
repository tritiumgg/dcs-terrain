# Spec: Kotlin consumer

How the Kotlin campaign uses `dcsterrain`. The campaign contains no
terrain logic. This spec is deliberately small; if it grows, the logic
belongs in `dcsterrain-core` instead.

## Shape

A single Kotlin module, `dcsterrain-client`, with one interface:

```kotlin
interface Terrain {
  suspend fun describe(): Describe
  suspend fun sample(req: SampleRequest): SampleResponse
  suspend fun scoreSite(req: ScoreSiteRequest): ScoreSiteResponse
  suspend fun findSites(req: FindSitesRequest): FindSitesResponse
  suspend fun visible(req: VisibleRequest): VisibleResponse
  suspend fun viewshed(req: ViewshedRequest): ViewshedResponse
  suspend fun coverage(req: CoverageRequest): CoverageResponse
  suspend fun approachSpectrum(req: ApproachSpectrumRequest): ApproachSpectrumResponse
  suspend fun unmaskProfile(req: UnmaskProfileRequest): UnmaskProfileResponse
  suspend fun route(req: RouteRequest): RouteResponse
  suspend fun routeAlternatives(req: RouteAlternativesRequest): RouteAlternativesResponse
  suspend fun nearest(req: NearestRequest): NearestResponse
  suspend fun sceneryIn(req: SceneryInRequest): SceneryInResponse
  suspend fun chokepoints(req: ChokepointsRequest): ChokepointsResponse
  suspend fun trafficability(req: TrafficabilityRequest): TrafficabilityResponse
  suspend fun geo(req: GeoRequest): GeoResponse
}
```

Every method is `suspend`: `ProcessTerrain` awaits the Kotlin MCP SDK
client, and `CliTerrain` awaits the child process, so no call blocks a
thread.

Request and response classes are `kotlinx.serialization` data classes
generated from the JSON schemas `dcsterrain schema` emits (a subcommand
that prints the `schemars` output for every operation). Generation is a
Gradle task running `quicktype --lang kotlin --framework kotlinx` over
the schema (`json-kotlin-schema-codegen` on Maven Central is the
JVM-native alternative); the classes are not hand-written, so they
cannot drift from the Rust structs.

## Transports

Two implementations of `Terrain`, chosen by configuration:

- `ProcessTerrain`: spawns `dcsterrain serve <file>` once and speaks MCP
  over its stdio using the Kotlin MCP SDK client. One process per
  theatre file, kept alive for the campaign's life, restarted on exit
  with backoff. This is the default: one JSON round trip per operation,
  the same tools the assistant uses, and the campaign never links native
  code.
- `CliTerrain`: runs `dcsterrain query <file> <op> <json>` per call.
  Slower (process start per call) but dependency-free; for scripts and
  tests.

The binary's path comes from configuration; the module ships no binary.
A `TerrainFactory.open(file, binary, installDir?)` checks the binary
runs and the file's `describe` succeeds, and surfaces `dcs_build` and
`terrain_digest`. When `installDir` is given it runs
`dcsterrain check --install <dir>` (the `matches_install` check) and
the factory acts per `onMismatch: REFUSE | WARN` (default `REFUSE`)
when the running DCS's
core build or terrain files differ from the ones the file was extracted
from. A campaign keeps one file per `(theatre, dcs_build,
terrain_digest)` and picks by the install it drives.

Direct SQLite reads through JDBC are allowed for the vector tables
(`airdrome`, `beacon`, `poi`, `runway`, `stand`) when the campaign only
needs a lookup, since the file is stock SQLite. Grid tables are never
read from Kotlin.

## Campaign-side responsibilities

The things the design doc leaves to the consumer, and where they live:

- **Overlay** of placed and destroyed objects: a campaign table keyed by
  `(x, z)` with a class and footprint, merged into `scenery_in` results
  and passed as extra `scenery_max` inputs where relevant. The file
  stays read-only.
- **Selection** ("Choosing a site that is not the best" in the design
  doc): a `SitePolicy` that takes a `find_sites` or `coverage` result
  and a caller-supplied predicate over the metric vector, and picks a
  candidate at random among those that pass. Pure Kotlin over the
  metric vectors; no terrain math, and no difficulty tables: the
  module ships none, and `using-the-data.md` explains how a campaign
  may build its own.
- **Waypoint emission**: converting `route` output to DCS mission
  waypoints with `action = "On Road"`.
- **Route rotation**: a `RoutePlan` that holds the `route_alternatives`
  result for a recurring movement (depot to base) and picks the next
  route per departure: weighted random by a caller-supplied weight, never
  the same route twice in a row, and re-planned when the campaign
  overlay adds a threat or a destroyed bridge.

## Testing

Contract tests against `dcsterrain serve` on the synthetic packed file
produced by the Rust test suite (the Kotlin build fetches or builds the
binary; the file is generated, never committed): every interface method
round-trips one request. `SitePolicy` unit tests on hand-written metric
vectors. No DCS, no real data.

## Acceptance

1. Generated classes compile from `dcsterrain schema` output with no
   manual edits.
2. `ProcessTerrain` survives a killed server process and answers the
   next call.
3. The campaign's FARP placement path calls `findSites` and places the
   result with the chosen orientation, end to end on the synthetic file.
