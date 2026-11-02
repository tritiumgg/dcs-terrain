# Guide

This guide shows you how to use `dcsterrain`, step by step.
First you point it at your DCS install.
Then you ask it where to put a FARP, a forward arming and refueling point.
You read the answer and turn it into a position for the Mission Editor.
Next you combine several questions to solve a bigger problem.
At the end, you learn what each error means and how to fix it.

Every command after the download steps was run on a real install.
Every output is what that command printed.
The terrain is Caucasus.
The DCS version, which DCS calls the build, is 2.9.31.29434.
Your numbers match if your build matches.
Times and cache sizes always vary.
On another build, other numbers can differ too, but each answer keeps the same layout.

## What you need

- On Windows: a PC with DCS World installed.
  DCS does not have to be running.
- On macOS or Linux: a copy of part of a Windows DCS install.
  [On macOS or Linux](#on-macos-or-linux) says what to copy and what changes.
- PowerShell, which comes with Windows.
  Open the Start menu, type `PowerShell`, and open "Windows PowerShell".
  PowerShell 7 works too.

## Get the tool

The steps below download the latest release.
You can also build the tool yourself, as the [README](../README.md#build-it-yourself)
explains.

Make a folder for your work, such as `C:\dcs-terrain`:

```powershell
mkdir C:\dcs-terrain
```

Go to that folder:

```powershell
cd C:\dcs-terrain
```

Download the Windows release, which works on Windows 10 and 11:

```powershell
Invoke-WebRequest https://github.com/tritiumgg/dcs-terrain/releases/latest/download/dcsterrain-x86_64-pc-windows-msvc.zip -OutFile dcsterrain.zip
```

Unpack it into this folder:

```powershell
Expand-Archive dcsterrain.zip -DestinationPath .
```

The folder now holds `dcsterrain.exe`.
If you built the tool instead, copy your `dcsterrain.exe` into this folder.

Every command below runs from this folder.
Each time you open PowerShell, go to it with `cd C:\dcs-terrain` first.
Each command starts with `.\dcsterrain.exe`.
The `.\` tells PowerShell to run the program in the current folder.

## Words you will meet

- **Terrain**: one DCS world, such as Caucasus or Syria.
- **Terrain id**: the short name the tool uses for a terrain, such as `Caucasus`.
- **JSON**: a plain-text format for data.
  `{ }` holds a group of named values.
  `[ ]` holds a list.
  Names and text go in double quotes.
  Commas separate the items.
- **Request**: your question, written in JSON.
- **Operation**: the kind of question, such as `find_sites` or `visible`.
- **Answer**: what the tool prints back, also in JSON.
- **x and z**: DCS's own coordinates, in meters.
  `x` grows to the north.
  `z` grows to the east.
- **AGL**: above ground level.
  `alt_agl` is a height above the ground, in meters.
- **Cache**: a folder where the tool keeps what it has worked out.
  It makes later questions fast.
  You can delete it at any time.
  The tool builds it again.
- **Airdrome**: the tool's word for an airfield.

All distances are in meters.
All angles are in degrees.

## Step 1: see which terrains you have

**When to use it:** first, on a new PC, or after you install a new terrain.
You need a terrain's id for every other command.

```powershell
.\dcsterrain.exe list-terrains
```

This command finds your DCS install by itself.
Then it lists the terrains in it.

```text
DCS 2.9.31.29434 at C:\Program Files\Eagle Dynamics\DCS World
cache at C:\Users\tritiumgg\AppData\Local\dcs-terrain\cache

id                  folder          cache                 files
Afghanistan         Afghanistan     6 entries, 110.1 MiB  60 of 60 readable
Caucasus            Caucasus        8 entries, 13.5 MiB   50 of 50 readable
GermanyCW           GermanyColdWar  6 entries, 265.1 MiB  462 of 462 readable
MarianaIslands      MarianaIslands  6 entries, 9.8 MiB    24 of 24 readable
MarianaIslandsWWII  MarianasWWII    6 entries, 7.1 MiB    30 of 30 readable
Nevada              Nevada          6 entries, 25.4 MiB   42 of 42 readable
PersianGulf         PersianGulf     6 entries, 34.6 MiB   68 of 68 readable
Syria               Syria           6 entries, 183.5 MiB  458 of 458 readable
```

What it means:

- The first line is your DCS build.
  It also says where DCS is installed.
- The second line says where the cache is.
- `id` is the terrain id.
  You type it after `--terrain`.
- `folder` is the terrain's folder inside DCS.
  It is not always the same as the id.
  Germany's folder is `GermanyColdWar`.
  Its id is `GermanyCW`.
- `cache` says how much the tool has stored for that terrain.
  A terrain you have never asked about shows nothing here.
- `files` says how many of the terrain's files the tool can read.
  "50 of 50" means all of them.

The tool only reads your DCS files.
It never changes them.

## Step 2: prepare the terrain

**When to use it:** once for each terrain, before your first question.
Run it again after each DCS update.

```powershell
.\dcsterrain.exe index --terrain Caucasus
```

This command builds the terrain's cache ahead of time.
Most questions build what they need by themselves.
Two kinds of question do not: road chokepoints and towns.
They need this step first.

The first run on Caucasus took about 5 seconds and printed this:

```text
Caucasus 2.9.31.29434 at C:\Users\tritiumgg\AppData\Local\dcs-terrain\cache\Caucasus
mesh.toc: built
scene.nodes: built
tables.json: built
roads.graph: built
roads.betweenness: built
places: built
```

Each line is one part of the cache.
`places` holds the towns.
`built` means this run made that part.
If you run the command again, each line says `current`.
`current` means that part was already up to date.
If you asked a question before this step, some lines say `current` too.

Caucasus builds in seconds.
Germany takes several minutes.
While Germany builds, the tool needs about 1.5 GB of free memory.

## How you ask a question

You send a question into the tool on one line of text.
The line names the operation in `op`.
It holds the request in `request`.
The `|` sign sends the line into the tool.

Here is a short question.
It asks for the airfield nearest to a point:

```powershell
'{"op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"nearest","result":{"cell_size":50.0,"dcs_build":"2.9.31.29434","elapsed_ms":1,"results":[{"class":null,"distance":60.0606545632205,"id":"25","kind":"airdrome","name":"Kutaisi","x":-284887.375,"z":683858.71875}],"terrain":"Caucasus","terrain_fingerprint":"6293651208-203468160-1140069192"}}
```

- `query` asks a question.
- `--terrain Caucasus` picks the terrain.
- The single quotes `' '` keep the line together.
  Always keep them.
- `line` counts the lines you sent.
- `result` holds the answer.
  Here it found Kutaisi, 60 m from the point.

Long requests are easier to write in a file.
[Step 3](#step-3-ask-where-to-put-a-farp) shows how.

## Step 3: ask where to put a FARP

**When to use `find_sites`:** you need a place for something.
You can describe the ground it needs.
Examples are a FARP, a SAM site, a bunker or a landing zone.

The question: where can a FARP go?
It needs flat, open ground.
It must be 3 to 15 km from Kutaisi.
It must be close to a road.

First, save the request in a file called `farp.json`.
Copy this whole block, paste it into PowerShell, and press Enter:

```powershell
@'
{
  "region": {"AirdromeBand": {"airdrome": "Kutaisi", "min_dist": 3000, "max_dist": 15000}},
  "footprint": {"width": 120, "depth": 80},
  "criteria": [
    {"name": "surface_in", "params": {"classes": ["land"]}},
    {"name": "slope_max", "params": {"deg": 3}},
    {"name": "dist_road_band", "params": {"min": 100, "max": 1000}},
    {"name": "clearing_max", "params": {"radius": 150, "max": 0}}
  ],
  "spacing": 2000,
  "limit": 3
}
'@ | Set-Content farp.json
```

`@'` and `'@` mark the start and end of a block of text.
`Set-Content farp.json` saves the block in the file.
To change the request later, open it in Notepad with `notepad farp.json`.

Here is what each part of the request asks for:

- `region` is where to look.
  `AirdromeBand` is a ring around an airfield.
  Here the ring runs from 3,000 m to 15,000 m from Kutaisi.
  [queries.md](queries.md) shows the other shapes: a box, a circle and a polygon.
- `footprint` is the size of the site.
  It is 120 m wide and 80 m deep.
- `criteria` are the tests a site must pass.
  A site must pass all of them.
  - `surface_in` with `land`: the whole site is on land, not water.
  - `slope_max` with `deg` 3: no part of the site is steeper than 3 degrees.
  - `dist_road_band`: the nearest road is 100 m to 1,000 m away.
    Trucks can reach the site.
  - `clearing_max`: no building or other object stands within 150 m.
    DCS removes objects near a FARP when the mission starts.
    This test keeps houses out of its way.
- `spacing` keeps the answers at least 2,000 m apart.
  Without it, you would get three spots next to each other.
- `limit` is how many answers you want.

[queries.md](queries.md) lists every test you can use.

Now send the request.
This command reads `farp.json`, adds `"op": "find_sites"`, and sends it as one line:

```powershell
@{ op = 'find_sites'; request = (Get-Content -Raw farp.json | ConvertFrom-Json) } | ConvertTo-Json -Depth 10 -Compress | .\dcsterrain.exe query --terrain Caucasus > farp-answer.jsonl
```

- `Get-Content -Raw farp.json | ConvertFrom-Json` reads the request from the file.
- `@{ op = 'find_sites'; request = ... }` puts the operation next to it.
- `ConvertTo-Json -Depth 10 -Compress` turns both into one line of JSON.
- `> farp-answer.jsonl` saves the answer in a new file.

The command prints nothing when it works.
To check that the answer arrived, list the file:

```powershell
Get-Item farp-answer.jsonl
```

## Step 4: read the answer

The answer is one long line of JSON.
It is hard to read in Notepad.
PowerShell can show it in parts.
First, load the answer into a variable.
A variable is a name that holds a value.
This one is called `$a`:

```powershell
$a = (Get-Content farp-answer.jsonl | ConvertFrom-Json).result
```

`.result` picks the answer out of the line.

### How many positions passed

```powershell
$a.stats
```

```text
cells_considered cells_passed tiles_skipped
---------------- ------------ -------------
           67872        15196            15
```

PowerShell may show this as a list instead of a table.
The numbers are the same.

- `cells_considered` is how many positions the tool tested in the ring: 67,872.
  The positions are 100 m apart.
- `cells_passed` is how many passed every test: 15,196.
  That leaves plenty to choose from.
- `tiles_skipped` is how many blocks of ground the tool ruled out as a whole.
  It did not need to test each position in them.

If only a few positions pass, your tests are strict.
If none pass, see [No candidates](#no-candidates).

### The best sites

```powershell
$a.candidates | Format-Table x, z, orientation_deg, score
```

```text
          x            z orientation_deg score
          -            - --------------- -----
-293887.375 671858.71875             0.0   5.0
-293287.375 694158.71875             0.0   5.0
-291987.375 672558.71875             0.0   5.0
```

PowerShell 7 rounds these numbers to two decimals.
In PowerShell 7, use `Format-List x, z` to see every digit.

Each row is one site.
The tool calls each site a candidate.

- `x` and `z` are the site's center.
  [Step 5](#step-5-find-out-where-a-site-is) turns them into a grid
  reference.
- `orientation_deg` is the heading to give the FARP in the Mission Editor.
  At 0, the 80 m side runs north to south.
- `score` puts the candidates in order.
  A higher score is better.
  Here all three scored 5.0, so they tie.
  The ground around Kutaisi is a flat plain.
  Many sites there are equally good.
  The [worked example](#combine-questions-a-hidden-farp-and-its-fuel-convoy)
  breaks the tie by asking what the enemy can see.

### Why a site passed

Each candidate carries the result of every test.
The tool calls these results terms.
This command shows the terms of the first candidate:

```powershell
$a.candidates[0].terms | Format-Table
```

```text
name           pass score value
----           ---- ----- -----
valid          True   1.0   1.0
surface_in     True   1.0   1.0
slope_max      True   1.0   0.0
dist_road_band True   1.0 100.0
clearing_max   True   1.0   0.0
```

- `name` is the test.
- `pass` says whether the site passed it.
- `value` is what the tool measured.
  The steepest slope is 0 degrees.
  The nearest road is 100 m away.
  No objects stand within 150 m.
- `score` is how well the site did on that test.
  It runs from 0 to 1.
  The candidate's score is the sum of these.
- `valid` is added to every request.
  It checks that no part of the site is on the sea.
  It also checks that no part is off the edge of the terrain.

**The terms are the real answer.**
Read them before you trust a site.

### Where the answer came from

```powershell
$a | Format-List terrain, dcs_build, terrain_fingerprint, cell_size, elapsed_ms
```

```text
terrain             : Caucasus
dcs_build           : 2.9.31.29434
terrain_fingerprint : 6293651208-203468160-1140069192
cell_size           : 50.0
elapsed_ms          : 95
```

Every answer ends with these fields.

- `terrain` and `dcs_build` say which terrain and DCS build answered.
  DCS changes its terrains only in updates.
  So the build tells you which version of the terrain the answer is about.
- `terrain_fingerprint` is made from the sizes of the terrain's files.
  Two answers with the same fingerprint came from the same files.
- `cell_size` is the spacing of the grid the tool reads the ground on, in meters.
- `elapsed_ms` is how long the question took, in milliseconds.

## Step 5: find out where a site is

**When to use `point_info`:** you have a position in `x` and `z`.
You want its grid reference, or you want to know what is near it.

The Mission Editor does not show `x` and `z`.
`point_info` gives the grid reference in MGRS, the military grid reference system.
It also gives the latitude and longitude, and the nearest town, road and airfield.

Copy the first candidate's `x` and `z` from the table in Step 4.
Copy every digit.
Put them into this command:

```powershell
'{"op": "point_info", "request": {"points": [{"x": -293887.375, "z": 671858.71875}]}}' | .\dcsterrain.exe query --terrain Caucasus > where.jsonl
```

Load the answer into a variable called `$w`:

```powershell
$w = (Get-Content where.jsonl | ConvertFrom-Json).result
```

Then show its main fields:

```powershell
$w.points[0] | Format-List mgrs, lat, lon, height, slope, cover
```

```text
mgrs   : 38T KM 78924 65394
lat    : 42.109498476393064
lon    : 42.32608283941332
height : 38.003851824586924
slope  : 0.0
cover  : town
```

- `mgrs` is the grid reference.
  Use it to place the FARP in the Mission Editor.
- `lat` and `lon` are latitude and longitude, in degrees.
- `height` is the ground's height above sea level, in meters.
- `slope` is how steep the ground is at that point, in degrees.
- `cover` is the kind of ground there, such as field, forest or town.
  `town` describes the ground, not a building.
  The `clearing_max` test already checked for buildings.

Show the nearest town:

```powershell
$w.points[0].town | Format-List
```

```text
bearing_grid_deg : 307.3389103727331
bearing_true_deg : 313.62865768230984
compass          : NW
description      : 1 km NW of Dapnari
distance         : 964.7827174996041
name             : Dapnari
x                : -294472.5431930097
z                : 672625.7805974906
```

The site lies about 1 km northwest of the village of Dapnari.
You can paste `description` into a mission briefing.
The two bearings point from the site to the town.
`bearing_true_deg` is measured from true north.

Show the nearest road:

```powershell
$w.points[0].road | Format-List distance, class
```

```text
distance : 125.40836900182634
class    : road
```

Here the road is 125 m away.
The terms table in Step 4 said 100 m.
The two differ because `find_sites` measures on a 50 m grid.
The grid makes it fast, but less exact.
`point_info` measures to the road itself.
Trust the `point_info` number.

## Step 6: check what can see the site

**When to use `visible`:** you want to know if one point can see another.
For example, can an enemy observer see your FARP?

`visible` uses the same kind of line-of-sight test as DCS.
Like DCS's test, it ignores buildings and trees.
Only the ground blocks the view.
It does not matter which point you call `a` and which `b`.

Can someone standing 2 m above the FARP see a hill to its southeast?
In the request, `a` is the FARP and `b` is the hill:

```powershell
'{"op": "visible", "request": {"a": {"x": -293887.375, "z": 671858.71875, "alt_agl": 2}, "b": {"x": -305000, "z": 690000, "alt_agl": 2}}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"visible","result":{"blocking":{"alt":102.70377895993946,"x":-294590.15050691913,"z":673005.9947650398},"cell_size":50.0,"clearance_m":-58.62387875845445,"dcs_build":"2.9.31.29434","elapsed_ms":3,"terrain":"Caucasus","terrain_fingerprint":"6293651208-203468160-1140069192","visible":false}}
```

- `visible` is `false`.
  The two points cannot see each other.
- `blocking` is the point where the ground blocks the view.
  It is a rise 103 m above sea level, close to the FARP.
- `clearance_m` is how close the line of sight comes to the ground.
  A negative number means the ground rises above the line.
  Here the ground is 59 m above it.

Now ask a different question.
Can an aircraft 100 m above Kutaisi see the FARP?
Kutaisi's position came from the `nearest` answer in
[How you ask a question](#how-you-ask-a-question): `x` -284887.375, `z` 683858.71875.

```powershell
'{"op": "visible", "request": {"a": {"x": -293887.375, "z": 671858.71875, "alt_agl": 2}, "b": {"x": -284887.375, "z": 683858.71875, "alt_agl": 100}}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"visible","result":{"blocking":null,"cell_size":50.0,"clearance_m":2.0,"dcs_build":"2.9.31.29434","elapsed_ms":2,"terrain":"Caucasus","terrain_fingerprint":"6293651208-203468160-1140069192","visible":true}}
```

Yes, it can.
`null` means there is no value.
Nothing blocks the line, so `blocking` is `null`.
`clearance_m` is 2.0.
The line comes closest to the ground at the FARP, where it starts 2 m up.

## Step 7: see the answer on a map

**When to use GeoJSON:** you want to see an answer drawn on a map.
You can also load it into another program.

GeoJSON is a standard file format for shapes on a map.
QGIS, a free map program, opens it.
So does the website [geojson.io](https://geojson.io).

Make a new request file called `farp-map.json`.
It is `farp.json` with one more line, `"geojson": true`:

```powershell
@'
{
  "region": {"AirdromeBand": {"airdrome": "Kutaisi", "min_dist": 3000, "max_dist": 15000}},
  "footprint": {"width": 120, "depth": 80},
  "criteria": [
    {"name": "surface_in", "params": {"classes": ["land"]}},
    {"name": "slope_max", "params": {"deg": 3}},
    {"name": "dist_road_band", "params": {"min": 100, "max": 1000}},
    {"name": "clearing_max", "params": {"radius": 150, "max": 0}}
  ],
  "spacing": 2000,
  "limit": 3,
  "geojson": true
}
'@ | Set-Content farp-map.json
```

The answer now has a field called `geojson`.
The first command sends the request and loads the answer into `$m`:

```powershell
$m = (@{ op = 'find_sites'; request = (Get-Content -Raw farp-map.json | ConvertFrom-Json) } | ConvertTo-Json -Depth 10 -Compress | .\dcsterrain.exe query --terrain Caucasus | ConvertFrom-Json).result
```

The second command saves the `geojson` field as its own file:

```powershell
$m.geojson | ConvertTo-Json -Depth 20 | Set-Content farp.geojson
```

This command lists the shapes in the file:

```powershell
$m.geojson.features | ForEach-Object { "$($_.geometry.type) $($_.properties | ConvertTo-Json -Compress)" }
```

```text
Point {"orientation_deg":0.0,"rank":1,"score":5.0}
Polygon {"orientation_deg":0.0,"rank":1,"score":5.0}
Point {"orientation_deg":0.0,"rank":2,"score":5.0}
Polygon {"orientation_deg":0.0,"rank":2,"score":5.0}
Point {"orientation_deg":0.0,"rank":3,"score":5.0}
Polygon {"orientation_deg":0.0,"rank":3,"score":5.0}
```

Each candidate is two shapes.
The `Point` is its center.
The `Polygon` is its 120 × 80 m outline.
`rank` is its place in the list.

To see the shapes, open `farp.geojson` in a map program.
On geojson.io, click Import.
Then choose the file.
The shapes appear on a map of the real world.
The file stores positions as longitude and latitude.
So every map program puts them in the right place.

### Which operations draw

These operations return GeoJSON when the request has `"geojson": true`:

| Operation | What it draws |
|---|---|
| `find_sites`, `score_site` | each site's center and outline |
| `visible` | the line of sight, and the point where the ground blocks it |
| `viewshed` | the area one observer can see |
| `radar_floor` | bands that show how low an aircraft can fly unseen by a site |
| `coverage` | the gaps in a site's view at each altitude |
| `approach_spectrum` | the directions a site cannot see, as wedges |
| `unmask_profile` | a flight path, and the parts of it a site cannot see |
| `exposure` | the area that no site in a group can see |
| `route`, `route_alternatives` | the route along the roads or railways |
| `chokepoints` | the roads that matter most |
| `trafficability` | the line from a point to the nearest road |
| `point_info` | each point you asked about |
| `airfield_info` | the runways and parking spots |
| `validate_placements` | each placement, and anything in its way |
| `towns` | each town's center, and its outline if you ask for it |

`sample`, `nearest`, `scenery_in`, `describe` and `geo` do not return GeoJSON yet.
They accept `"geojson": true`, but their answer has no `geojson` field.

## Many questions at once

**When to use a batch:** you have more than a few questions about one terrain.
The tool starts once and reads the terrain once, so a batch is faster.

A batch is a file with one question per line, each in the same form as in
[How you ask a question](#how-you-ask-a-question).
Each line can also have an `id`.
You choose the `id`, so you can tell the answers apart.
Save this two-line batch as `questions.jsonl`:

```powershell
@'
{"id": "kutaisi", "op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}
{"id": "site1", "op": "point_info", "request": {"points": [{"x": -293887.375, "z": 671858.71875}]}}
'@ | Set-Content questions.jsonl
```

Then send the file's lines into the tool:

```powershell
Get-Content questions.jsonl | .\dcsterrain.exe query --terrain Caucasus > answers.jsonl
```

Each line of `answers.jsonl` answers one line of your file.
The answers come in the same order, with the same `id`.
These are the first 100 characters of each answer line:

```text
{"line":1,"id":"kutaisi","op":"nearest","result":{"cell_size":50.0,"dcs_build":"2.9.31.29434","elaps
{"line":2,"id":"site1","op":"point_info","result":{"cell_size":50.0,"dcs_build":"2.9.31.29434","elap
```

If a line fails, it has `issues` instead of `result`.
The other lines still run.

## Combine questions: a hidden FARP and its fuel convoy

Real problems take more than one question.
The answer to one question becomes part of the next request.
This example starts from the three FARP candidates of Step 3.
It answers three more questions:

1. Which candidate can the enemy not see?
   A batch of `visible` questions answers it.
2. Which road does the fuel convoy take from Kutaisi to that site?
   `route` answers it.
3. How much of that road can the enemy see?
   The same `route` answer holds that too.

The setup is made up for this example.
The enemy holds the hills south of Kutaisi.
It has two observation posts there, A and B.
Each post has a lookout on a 10 m mast.
Their positions are:

| Post | x | z | Ground height | Near |
|---|---|---|---|---|
| A | -297000 | 676000 | 308 m | 1 km N of Didi-Opeti |
| B | -296000 | 668000 | 139 m | 4 km NE of Tolebi |

The heights and places came from one `point_info` question with both points.
They are for your information only.
The next questions use the 10 m mast height above the ground instead.

```powershell
'{"op": "point_info", "request": {"points": [{"x": -297000, "z": 676000}, {"x": -296000, "z": 668000}]}}' | .\dcsterrain.exe query --terrain Caucasus > posts.jsonl
```

```powershell
(Get-Content posts.jsonl | ConvertFrom-Json).result.points | Format-Table x, z, height, @{ n = 'town'; e = { $_.town.description } }
```

```text
        x        z             height town
        -        -             ------ ----
-297000.0 676000.0  308.3237757780157 1 km N of Didi-Opeti
-296000.0 668000.0 139.18046713700417 4 km NE of Tolebi
```

### Hand-off 1: from candidates to lines of sight

Each candidate needs a `visible` check from each post.
That is 3 candidates times 2 posts, so 6 questions.
A batch asks them all at once.

You could type the 6 lines by hand.
It is safer to let PowerShell write them from `farp-answer.jsonl`.
Then no digit gets lost in copying.
Paste this whole block into PowerShell and press Enter:

```powershell
$a = (Get-Content farp-answer.jsonl | ConvertFrom-Json).result
$posts = @{ A = @{ x = -297000; z = 676000; alt_agl = 10 }; B = @{ x = -296000; z = 668000; alt_agl = 10 } }
$n = 0
$lines = foreach ($site in $a.candidates) {
  $n++
  foreach ($name in 'A', 'B') {
    @{ id = "site$n-$name"; op = 'visible'; request = @{ a = $posts[$name]; b = @{ x = $site.x; z = $site.z; alt_agl = 2 } } } | ConvertTo-Json -Depth 5 -Compress
  }
}
$lines | Set-Content lookout.jsonl
```

Here is what it does:

- The first line loads the Step 3 answer into `$a` again.
  A new PowerShell window would not have it.
- The second line writes down the two posts, each 10 m above the ground.
- `$n` counts the candidates.
- The two `foreach` lines go through each candidate, and for each one through
  post A and post B.
- The long line writes one `visible` question.
  `a` is the post.
  `b` is the candidate, 2 m above the ground, the height of a person there.
  The `id` names the pair, such as `site1-A`.
- The last line saves all 6 lines in `lookout.jsonl`.

To use it for your own problem, change three things:
the answer file in the first line, the posts in the second line, and the
two names `'A', 'B'` if you have other posts.

The first line of `lookout.jsonl` looks like this.
PowerShell may put the fields in another order, which does not matter:

```text
{"op":"visible","id":"site1-A","request":{"a":{"alt_agl":10,"z":676000,"x":-297000},"b":{"alt_agl":2,"z":671858.71875,"x":-293887.375}}}
```

Now send the batch:

```powershell
Get-Content lookout.jsonl | .\dcsterrain.exe query --terrain Caucasus > lookout-answers.jsonl
```

Show the answers as a table:

```powershell
Get-Content lookout-answers.jsonl | ConvertFrom-Json | Format-Table id, @{ n = 'visible'; e = { $_.result.visible } }, @{ n = 'clearance_m'; e = { [math]::Round($_.result.clearance_m) } }
```

```text
id      visible clearance_m
--      ------- -----------
site1-A   False         -56
site1-B   False          -7
site2-A    True           2
site2-B   False        -141
site3-A    True           2
site3-B    True           0
```

What it means:

- Post A sees candidates 2 and 3.
- Post B sees candidate 3.
- Neither post sees candidate 1.
- All three candidates had the same score.
  Being hidden breaks the tie.
  Candidate 1 is the site to use.
- At post B, the ground rises only 7 m above the line to candidate 1.
  If B had a taller mast, it might see candidate 1.

### Hand-off 2: from the chosen site to a route

**When to use `route`:** you need the way from one point to another along the
roads or railways.

The route needs a start and an end:

- The start is Kutaisi, at `x` -284887.375, `z` 683858.71875.
  That position came from the `nearest` answer in
  [How you ask a question](#how-you-ask-a-question).
- The end is candidate 1, at `x` -293887.375, `z` 671858.71875.
  That position came from the table in Step 4.

The route request can also take a list of observers, called
`exposure_observers`.
Then the answer says how much of each stretch of road those observers can see.
The observers here are posts A and B, copied from the script above.
`"geojson": true` asks for a map file too:

```powershell
'{"op": "route", "request": {"network": "roads", "from": {"x": -284887.375, "z": 683858.71875}, "to": {"x": -293887.375, "z": 671858.71875}, "exposure_observers": [{"x": -297000, "z": 676000, "alt_agl": 10}, {"x": -296000, "z": 668000, "alt_agl": 10}], "geojson": true}}' | .\dcsterrain.exe query --terrain Caucasus > convoy.jsonl
```

Load the answer and show the totals:

```powershell
$r = (Get-Content convoy.jsonl | ConvertFrom-Json).result
```

```powershell
$r | Format-List length, straight_line, ascent, descent
```

```text
length        : 25821.084562457352
straight_line : 15000.0
ascent        : 68.57054947481198
descent       : 75.44872775378546
```

- `length` is the length of the route: about 25.8 km.
- `straight_line` is the straight distance: 15 km.
  The roads add about 11 km.
- `ascent` and `descent` are how far the route climbs and falls in total, in meters.

The answer also has a list called `legs`.
A leg is one stretch of road between two junctions.
This route has 14 legs.
Each leg has a `length` and an `exposure`.
`exposure` is the share of the leg the posts can see, from 0 to 1.
These are the first three legs:

```powershell
$r.legs | Select-Object -First 3 | Format-Table length, exposure
```

```text
            length           exposure
            ------           --------
195.32288587666912                1.0
1478.2657802697236                1.0
3233.4260026464012 0.9694656488549618
```

To find how many meters the posts can see, multiply each leg's length by its
exposure, and add them up:

```powershell
($r.legs | ForEach-Object { $_.length * $_.exposure } | Measure-Object -Sum).Sum
```

```text
24653.4246850342
```

The posts can see about 24.7 km of the 25.8 km route.
24.7 divided by 25.8 is about 95 %.
The site is hidden, but the road to it is not.

Is there a better road?
`route_alternatives` looks for different routes between the same two points.
`k` is how many routes to look for:

```powershell
'{"op": "route_alternatives", "request": {"network": "roads", "from": {"x": -284887.375, "z": 683858.71875}, "to": {"x": -293887.375, "z": 671858.71875}, "k": 3}}' | .\dcsterrain.exe query --terrain Caucasus > alternatives.jsonl
```

This command counts the routes it found:

```powershell
(Get-Content alternatives.jsonl | ConvertFrom-Json).result.routes.Count
```

```text
1
```

It found only one route.
No other road reaches the site without a long detour.
So the convoy cannot hide by its choice of road.
Run it at night, escort it, or deal with the posts first.

### Hand-off 3: from answers to a map

The route answer has a `geojson` field, because the request asked for one.
Save it next to the FARP file from Step 7:

```powershell
$r.geojson | ConvertTo-Json -Depth 20 | Set-Content convoy.geojson
```

Open `farp.geojson` and `convoy.geojson` together in QGIS.
Or open them one after the other on geojson.io.
You see the three candidates, and the road from Kutaisi to candidate 1.

## Other questions you can ask

Each operation fits a situation.
[queries.md](queries.md) describes each one in full.
`.\dcsterrain.exe schema --op <operation>` prints every field a request can take.

| Use | When you |
|---|---|
| `sample` | need the height, slope or ground type at many points, or over an area |
| `point_info` | have one position and want its grid reference and what is near it |
| `geo` | need to convert between x and z, latitude and longitude, and MGRS |
| `describe` | want to confirm which terrain and build the tool is using |
| `find_sites` | need the best places for something |
| `score_site` | have a place in mind and want to test it |
| `validate_placements` | want to check that units do not sit on water, a slope, a building or a runway |
| `visible` | want to know if one point can see another |
| `viewshed` | want the whole area one observer can see |
| `radar_floor` | want to know how low a plane can fly past a site unseen |
| `coverage` | are placing a radar or SAM and want the gaps it leaves at each altitude |
| `approach_spectrum` | want the directions from which a site can be approached unseen |
| `unmask_profile` | have a flight path and want to know where a site first sees it |
| `exposure` | have several radars or SAMs and want the holes between them |
| `route` | need the way by road or rail |
| `route_alternatives` | want several different ways |
| `chokepoints` | want the roads or bridges that matter most, to cut or to guard (needs Step 2) |
| `trafficability` | want to know if a ground group can drive from the road to a point |
| `nearest` | want the nearest road, railway, airfield, town or object |
| `scenery_in` | want the objects standing in an area |
| `towns` | want a real town for an objective or a briefing (needs Step 2) |
| `airfield_info` | need an airfield's runways, radios and parking |

**When you need the airfields' names and positions**, ask `nearest` for many of
them from any point:

```powershell
'{"op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800, "n": 30}}' | .\dcsterrain.exe query --terrain Caucasus > airfields.jsonl
```

This shows the first five as a table:

```powershell
(Get-Content airfields.jsonl | ConvertFrom-Json).result.results | Select-Object -First 5 | Format-Table id, name, x, z
```

```text
id name                           x                 z
-- ----                           -                 -
25 Kutaisi              -284887.375      683858.71875
23 Senaki-Kolkhi      -281782.46875 647279.4687499999
24 Kobuleti          -317962.296875      635632.96875
22 Batumi            -355810.703125       617386.1875
20 Sukhumi-Babushara -220592.328125      564391.96875
```

In a request, an airfield can be named by its `name`, such as `"Senaki-Kolkhi"`.
It can also be named by its `id`, such as `"23"` or `23`.
Use its `x` and `z` as the start or end of a route.

The other commands:

- **`check`**: use it when a terrain will not load.
  It lists the terrain's files and says which ones the tool can read.
- **`cache list`**: use it to see how much space the cache takes.
- **`cache clear`**: use it to free that space.
  The tool builds the cache again when it needs it.
- **`schema`**: use it to look up every field of a request or an answer.
- **`--help`**: use it after any command to list its options.

## When something goes wrong

When a question fails, its answer line has `issues` instead of `result`.
Each issue has a `code`, a `message` and a `path`.
The `message` says what is wrong.
The `path` names the part of your request that is wrong.
It is empty when no single part is wrong.

### A misspelled field

`slope_max` takes `deg`.
This request gave it `degrees`:

```powershell
'{"op": "find_sites", "request": {"region": {"Circle": {"x": -284887, "z": 683859, "radius": 5000}}, "footprint": {"width": 60, "depth": 40}, "criteria": [{"name": "slope_max", "params": {"degrees": 3}}], "spacing": 1000}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"find_sites","issues":[{"code":"invalid_request","message":"missing field `deg`","path":"criteria"}]}
```

The message names the field the tool wanted.
`path` says the problem is in `criteria`.
Fix the spelling and run the command again.

### A line that is not JSON

This line has no double quotes around its names:

```powershell
'{op: visible}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"issues":[{"code":"invalid_request","message":"the line is not JSON: key must be a string at line 1 column 2","path":""}]}
```

Check the quotes, commas and brackets.
Every name and every piece of text needs double quotes.

### The request after the operation, in Windows PowerShell

The tool can also take a request after the operation's name:

```powershell
.\dcsterrain.exe query --terrain Caucasus nearest '{"kind": "airdrome", "x": -284900, "z": 683800}'
```

That works in PowerShell 7.
In Windows PowerShell 5.1, it fails:

```text
ERROR invalid_request: the request is not JSON: key must be a string at line 1 column 2
{"issues":[{"code":"invalid_request","message":"the request is not JSON: key must be a string at line 1 column 2","path":""}]}
```

Windows PowerShell 5.1 removes the double quotes before the tool sees them.
Send the question with `|`, as this guide does.
That works in every PowerShell.

### The wrong terrain id

This command used the folder name `GermanyColdWar` instead of the id:

```powershell
'{"op": "describe"}' | .\dcsterrain.exe query --terrain GermanyColdWar
```

```text
{"line":1,"op":"describe","issues":[{"code":"terrain_refused","message":"no terrain GermanyColdWar in this install; the ids it holds are Afghanistan, Caucasus, GermanyCW, MarianaIslands, MarianaIslandsWWII, Nevada, PersianGulf, Syria","path":""}]}
```

The message lists the ids you can use.
`list-terrains` shows them next to their folders.

### DCS is not where the tool looked

This command pointed the tool at a folder with no DCS in it:

```powershell
'{"op": "describe"}' | .\dcsterrain.exe query --install C:\Games --terrain Caucasus
```

```text
{"line":1,"op":"describe","issues":[{"code":"terrain_refused","message":"C:\\Games has no Mods/terrains to read: The system cannot find the path specified. (os error 3)","path":""}]}
```

The tool found no terrains in that folder.
This happens when `--install` names the wrong folder.
It also happens when you have moved DCS since you installed it.
Give `--install` the DCS folder.
On Windows, that folder holds the `bin` and `Mods` folders.
For example: `--install "D:\DCS World"`.
Put `--install` after `query`, `index` or `list-terrains`, next to `--terrain`.

### An airfield name it does not know

This request misspelled Kutaisi:

```powershell
'{"op": "airfield_info", "request": {"airdrome": "Kutaisy"}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"airfield_info","issues":[{"code":"unknown_airdrome","message":"no airdrome has the name, name_id or id `Kutaisy`","path":"airdrome"}]}
```

To see the right names, list the airfields as
[Other questions you can ask](#other-questions-you-can-ask) shows.

### The region is too big

This request asked for sites in a circle 120 km across:

```powershell
'{"op": "find_sites", "request": {"region": {"Circle": {"x": -284887, "z": 683859, "radius": 60000}}, "footprint": {"width": 60, "depth": 40}, "criteria": [{"name": "slope_max", "params": {"deg": 3}}], "spacing": 1000}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"find_sites","issues":[{"code":"too_large","message":"the region holds more than 131072 lattice points at this step, over the 262144 sites a search may weigh at 2 orientations; widen the step or split the region","path":"region"}]}
```

`find_sites` tests positions on a grid.
This region holds too many positions.
There are three fixes:

- Make the region smaller.
- Split it into pieces, and ask about each piece.
- Add a larger `step` to the request, such as `"step": 300`.
  `step` is the distance between the positions it tests.
  It is 100 m unless you set it.

### No candidates

This request asked for a site on the sea, near Kutaisi, where there is no sea:

```powershell
'{"op": "find_sites", "request": {"region": {"Circle": {"x": -284887, "z": 683859, "radius": 5000}}, "footprint": {"width": 60, "depth": 40}, "criteria": [{"name": "surface_in", "params": {"classes": ["sea"]}}], "spacing": 1000}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"find_sites","result":{"candidates":[],"cell_size":50.0,"dcs_build":"2.9.31.29434","elapsed_ms":3,"stats":{"cells_considered":7845,"cells_passed":0,"tiles_skipped":15},"terrain":"Caucasus","terrain_fingerprint":"6293651208-203468160-1140069192"}}
```

This is not an error.
The request worked.
But `candidates` is empty, and `cells_passed` is 0.
No position passed every test.
Make one test less strict and ask again.
Repeat until some positions pass.

### The terrain is not prepared

`chokepoints` and `towns` answer this until you run [Step 2](#step-2-prepare-the-terrain):

```text
{"line":1,"op":"chokepoints","issues":[{"code":"not_ready","message":"betweenness is not built yet; `dcsterrain index --terrain Caucasus` builds it","path":""}]}
```

Run the command in the message.
Then ask your question again.

### No route

This request allowed no detour at all:

```powershell
'{"op": "route", "request": {"network": "roads", "from": {"x": -284887, "z": 683859}, "to": {"x": -250000, "z": 650000}, "max_detour_factor": 1.0}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"route","issues":[{"code":"no_route","message":"the network joins no route between the ends within the detour allowed, past the regions avoided","path":""}]}
```

`route` found no way along the roads between the start and the end.
There are three reasons this can happen:

- No road joins the start to the end.
- An area you asked it to avoid cuts every way through.
- Every way is too long.
  By default, a route may be at most 3 times as long as the straight line.
  `max_detour_factor` sets that limit.
  This request set it to 1.0.
  No road is that straight.

### Off the terrain

A point outside the terrain has no ground.
Most operations refuse it with the code `off_terrain`:

```powershell
'{"op": "visible", "request": {"a": {"x": 900000, "z": 900000, "alt_agl": 2}, "b": {"x": -284887.375, "z": 683858.71875, "alt_agl": 100}}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"visible","issues":[{"code":"off_terrain","message":"no mesh covers this point, so it has no ground to stand on","path":"a"}]}
```

`path` is `a`, so point `a` is the one off the terrain.
`point_info` answers anyway, with `height`, `slope` and `cover` set to `null`.
Do not trust its other fields for such a point.
Check that you did not swap `x` and `z`.

### Other codes

`.\dcsterrain.exe schema` lists every code under `issue_codes`, with its meaning.

## On macOS or Linux

DCS does not run on macOS or Linux.
So the tool cannot find DCS by itself.
Copy these three things from a Windows DCS install to your computer:

- The file `autoupdate.cfg`.
  It names the DCS build.
- The folder `Scripts`.
  The tool reads radio and object lists from it.
- The folder `Mods\terrains`.
  You can copy only the terrains you want from it.
  Each one is large: Caucasus is 18 GB.

Put them in one folder, such as `dcs` in your home folder.
Keep the folders inside it as they were.
For example, `dcs/Mods/terrains/Caucasus` holds Caucasus.
Then add `--install ~/dcs` to every `query`, `index` and `list-terrains`.
`~` means your home folder.

Download the file for your computer from the latest release:

- `dcsterrain-aarch64-apple-darwin.tar.gz` for a Mac with Apple silicon.
- `dcsterrain-x86_64-apple-darwin.tar.gz` for a Mac with an Intel chip.
- `dcsterrain-x86_64-unknown-linux-gnu.tar.gz` for Linux on a 64-bit PC.

For example, on a Mac with Apple silicon, open Terminal and run:

```bash
curl -LO https://github.com/tritiumgg/dcs-terrain/releases/latest/download/dcsterrain-aarch64-apple-darwin.tar.gz
```

```bash
tar -xzf dcsterrain-aarch64-apple-darwin.tar.gz
```

If you download the file with a browser instead, macOS blocks the program.
This command unblocks it:

```bash
xattr -d com.apple.quarantine dcsterrain
```

The program is `./dcsterrain` instead of `.\dcsterrain.exe`.
Questions go in the same way, with `|`:

```bash
echo '{"op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}' | ./dcsterrain query --install ~/dcs --terrain Caucasus
```

To pick fields out of an answer, use `jq`, a free tool that reads JSON in the terminal.

A DCS update can change a terrain file while a question is reading it.
Then the question may stop with the message "bus error".
Ask the question again.

## Where to go next

- [queries.md](queries.md) describes every operation, test and layer.
- `.\dcsterrain.exe schema --op <operation>` prints what one operation takes and
  returns.
- The [skill](../skill/SKILL.md) teaches an AI assistant to ask these questions
  for you.
