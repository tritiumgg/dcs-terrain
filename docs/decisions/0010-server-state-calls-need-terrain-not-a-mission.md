# ADR 0010: `server`-state `land` and `world` calls need loaded terrain, not a running mission

## Status

Accepted

## Context

**Affects:** `design-and-facts.md` "Sources you have", the phase-`sim` rule;
`extractor-hook.md` "Lifecycle" step 4 and "Mission-pass sweeps";
`probe-log-2.9.29.27278.md` "Crash record"; `plan.md` tasks X4, X8a, X8b, X10.
It also changes the DCS boundary section of `CLAUDE.md`, which is living and
edited directly.

The frozen rule is stated in `design-and-facts.md`:

> **Mission-state `land` and `world` calls only while `dcs_bridge_status`
> reports phase `sim`.** A `land.getHeight` reaching the `server` state at the
> main menu forces `Terrain.dll createGlobalLand` with no terrain and crashes
> DCS with an access violation (crash log 2026-09-02 16:49:11 on this build).

`extractor-hook.md` builds the hook's lifecycle on it. Step 4 says the mission
pass

> Enters only when `DCS.getModelTime()` advances (a mission is running) and
> `passes.mission` is true; otherwise the hook writes
> `passes.mission.complete = false` and finishes.

and "Mission-pass sweeps" says "Never send a chunk before
`DCS.getModelTime()` has advanced."

The rule generalises one crash. The probe log's own record of it gives the
proximate cause as missing terrain rather than a missing mission: a stale
`server`-state request (`world.searchObjects` + `land.isVisible`) left in the
transport directory was executed by a relaunched DCS at its first tick, "with
no terrain loaded", and died in `Terrain.dll createGlobalLand` under
`Scripting::regLuaGroup`. Every other `server`-state figure in the probe log
was taken with a mission running, so the case between the two — terrain
loaded, no mission — was never measured.

Measured 2026-09-04 on build **2.9.29.27468** (`autoupdate.cfg` timestamp
20260902-093323), Mission Editor open on Caucasus, no mission running, bridge
phase `menu`, through `dcs_eval_in` with `state: "server"`:

- The state is populated: `land`, `world`, `Object`, `env` and `timer` are all
  tables; `land.getHeight`, `land.getSurfaceType` and `world.searchObjects` are
  all functions; `Object.Category.SCENERY` is 5 and `world.VolumeType.SPHERE`
  is 2.
- `land.getHeight({x = -284887, y = 683859})` returns 45.010047912598. That is
  the Kutaisi reference point `extractor-hook.md` "Acceptance" gives as
  45.010 m, so the terrain layer answers correctly and the `y`-means-DCS-z
  convention holds.
- `land.getSurfaceType` returns 5 at the same point and 1 at
  (−284887, 700000). 5 is `RUNWAY`, one of the two classes the hook-side
  string API cannot express and the whole reason the `surface` sweep needs
  this state at all.
- `world.searchObjects` over a 500 m sphere at Kutaisi returns 49 scenery
  objects, the first `KAMAZ-FIRE` with `getName()` 277373117. This is the call
  that appears in the crash record.

DCS did not crash, and the hook-side check that distinguishes the two
situations was confirmed on the same build in the same session:
`terrain.GetTerrainConfig("id")` returns nil at the main menu and `Caucasus`
with the editor open on that map.

## Decision

The precondition for a `server`-state `land` or `world` call is **loaded
terrain**, not a running mission. The extractor gates those calls on
`terrain.GetTerrainConfig("id")` returning non-nil, which is a hook-state call
that is safe at the main menu, and re-checks it on every frame of the pass
that makes them; a run whose terrain goes away stops making them. Nothing is
gated on `DCS.getModelTime()`, on `onSimulationStart`, or on the bridge's
`phase` field, and the extractor does not wait for a mission before running
the sweeps that use the `server` state.

The manifest keeps `passes.mission` as its key and the code keeps the name
"mission pass". The key is frozen by ADR 0007 and renaming it would be a
format change for a word.

Probes driven by hand through the bridge follow the same precondition: check
`terrain.GetTerrainConfig("id")` rather than `dcs_bridge_status` phase, which
reports `menu` with the editor open on a map and so cannot distinguish loaded
terrain from none.

### Alternatives considered

**Keep the phase-`sim` rule and require a mission.** Rejected on the
measurement: it forbids calls that demonstrably work, and it costs more than
inconvenience. A mission can alter the data being extracted — a
heliport-category static removes every scenery object within about 150 m at
spawn, permanently, and `design-and-facts.md` notes the terrain module and the
mission share one scene, so that hole lands in the extract. Two Mission Editor
trigger actions clear scenery as well, and anything shooting during a sweep of
tens of minutes destroys buildings under it. Requiring a mission makes the
user build a clean one before extracting anything and keeps a risk that the
editor path does not have.

**Gate on the bridge's `phase` field.** Rejected: it reports `menu` with the
editor open on Caucasus, so it does not answer the question the rule needs
answered. It is also a property of the probe bridge, which the shipped hook
does not have.

**Check terrain once when the pass starts.** Rejected: terrain unloads when
DCS returns to the main menu, and a pass that ran for tens of minutes on a
check made at its start would keep calling into an unloaded terrain layer. The
check is cheap — a `package.loaded` lookup and one C call — so it is made
every frame.

## Consequences

The whole extract can be taken from a bare map in the Mission Editor. No
mission needs to be built, and the FARP-clearing and combat-damage risks stop
being routine concerns of a normal run; `allow_helipads` remains for a user
who chooses to extract from inside a mission anyway.

`extractor-hook.md` "Lifecycle" step 4 and the "Never send a chunk before
`DCS.getModelTime()` has advanced" sentence in "Mission-pass sweeps" now read
false, as does the phase-`sim` paragraph in `design-and-facts.md` and the rule
line at the end of the probe log's crash record. The corresponding paragraph
in `CLAUDE.md` is rewritten in the same change.

X4's done test is unaffected, but its state machine changes: the gate on
`DCS.getModelTime()` advancing, the `run.simulation` flag that
`onSimulationStart` set, and the wait for the clock to move all go, replaced
by the terrain check. X8a loses "Never send a chunk before
`DCS.getModelTime()` has advanced" from what it implements. X8b's scenery
sweep and X10's live cropped run can both be done in the editor, so X10 no
longer needs a mission built for it, and the acceptance step that says both
passes complete is now reachable without one.

What this does not establish: that the calls are safe with no terrain loaded.
They are not — the crash record stands, and the main-menu case is exactly the
one the new gate excludes. Nor is it measured on any build but 2.9.29.27468,
on any theatre but Caucasus, or for `server`-state calls beyond the three
above. A build that changes when the terrain layer initialises would reopen
this; the gate would still be the right shape, because it tests the condition
directly rather than standing in for it.
