# Live gate: reading terrain files beside a running DCS

Can the tool read a terrain's files while DCS has that terrain loaded?
Measured 2026-10-27 on DCS build 2.9.30.28738 (`autoupdate.cfg` timestamp
`20261006-075422`), Windows 11 Pro 10.0.26200.

## Open test

DCS was in the Mission Editor on Caucasus; the hook state's
`terrain.GetTerrainConfig("id")` answered `Caucasus`.
Every `.surface5`, `.scn5` and `.rn4` under `Mods/terrains/Caucasus` was
opened for read and its first 16 bytes read, twice, with two share modes:

| share mode | what a failure would mean | opened |
|---|---|---|
| read, write, delete (what Rust's `File::open` asks for) | DCS denies readers | 24 of 24 |
| read only | some open handle has write access | 24 of 24 |

So DCS does not deny readers, and nothing holds these files open for write.
The tool can read them beside a live DCS on the same terrain.

```powershell
$root = (Get-ItemProperty 'HKCU:\Software\Eagle Dynamics\DCS World').Path
Get-ChildItem (Join-Path $root 'Mods\terrains\Caucasus') -Recurse -Include *.surface5,*.rn4,*.scn5 |
  ForEach-Object {
    $fs = [System.IO.FileStream]::new($_.FullName, 'Open', 'Read', [System.IO.FileShare]'Read')
    $b = New-Object byte[] 16; [void]$fs.Read($b, 0, 16); $fs.Dispose()
  }
```

## How DCS opens them

Process Monitor, filtered to `DCS.exe` and the terrain's `.surface5`, `.scn5`
and `.rn4`, across a Caucasus load in the Mission Editor (about 3 s):

| file | `CreateFile` | mapped | closed during the capture |
|---|---|---|---|
| `Caucasus.surface5` | Generic Read, ShareMode Read, Synchronous IO Non-Alert | `PAGE_READONLY` | no |
| `Caucasus.scn5` | the same | `PAGE_READONLY` | no |
| `Caucasus.rn4` and the 21 airfield `.rn4` | the same | no | yes, within about 1 s |

- No open asks for `No Buffering`, and no read is non-cached: DCS reads
  through the OS file cache, so its pages are the tool's pages.
- ShareMode Read admits a second reader and refuses a writer. The tool opens
  for read only, so it fits.
- DCS reads about 44 MB of the 6.29 GB `.surface5` with `ReadFile` at load,
  mostly 16 KiB in sequence from offset 32; the rest it touches through the
  mapping.
- It keeps `.surface5` and `.scn5` open and mapped after the load. When it
  lets go was not captured: no other terrain was loaded during the capture.

## What is resident

RAMMap's File Summary with Caucasus loaded, after the capture:

| file | size | resident (all standby) | share |
|---|---|---|---|
| `Caucasus.surface5` | 6,146,144 K | 92,412 K | 1.5 % |
| `Caucasus.scn5` | 1,113,349 K | 16,860 K | 1.5 % |
| `Caucasus.rn4` | 198,699 K | 198,700 K | 100 % |
| `Caucasus.routes` | 957,545 K | 956,416 K | 99.9 % |
| `Caucasus.ng5` | 840,627 K | 272,640 K | 32 % |

None of it is active: DCS's mappings hold no page in its working set, and
every cached page sits in the standby list, where the tool reuses it until
Windows reclaims the memory. So the tool's first reads of the mesh and the
scene mostly come from disk, and its reads of the road network do not. This
decides a benefit, not whether the tool runs.
