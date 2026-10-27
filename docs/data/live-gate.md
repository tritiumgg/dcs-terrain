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

## Not yet measured

The open test cannot see whether DCS holds these files open, or how.

- DCS's own `CreateFile` access, share mode and options on `.surface5`, and
  whether it maps the file: Process Monitor, filtered to `DCS.exe` and the
  terrain's files, across a terrain load. DCS appears to keep a terrain open
  until the next one loads, so the capture loads another terrain first, then
  Caucasus, and watches for a `CloseFile` when the other loads.
- Whether DCS's pages of `.surface5` are resident in the OS cache before the
  tool reads it: RAMMap's File Summary with the terrain loaded.

These decide whether the tool's first reads are warm, not whether it runs.
