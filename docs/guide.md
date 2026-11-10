# Guide

This guide shows you how to use `dcsterrain`, step by step.
First you let it find your DCS install.
Then you ask it where to put a FARP, a forward arming and refueling point.
You read the answer and turn it into a position for the Mission Editor.
Next you combine several questions to solve a bigger problem.
At the end, you learn what each error means and how to fix it.

You ask each question with one command.
The command's name says what you ask, such as `find-sites` or `visible`.
Its options say where and how.
The answer comes back as a table you can read.

Every command after the download steps was run on a real install.
Every output is what that command printed.
The terrain is Caucasus.
The DCS version, which DCS calls the build, is 2.9.31.29434.
Your numbers match if your build matches.
Times and cache sizes always vary.
On another build, other numbers can differ too.
Each answer keeps the same layout.

## What you need

- On Windows: a PC with DCS World installed.
  DCS does not have to be running.
- On macOS or Linux: a copy of part of a Windows DCS install.
  [On macOS or Linux](#on-macos-or-linux) says what to copy and what changes.
- PowerShell, which comes with Windows.
  Open the Start menu, type `PowerShell`, and open "Windows PowerShell".
  PowerShell 7 works too.
  To run a command, type or paste the whole line in PowerShell, then press Enter.

## Get the tool

The steps below download release 0.3.0.
You can also build the tool yourself, as the [README](../README.md#build-it-yourself)
explains.

Make a folder for your work, such as `C:\dcs-terrain`:

```powershell
mkdir C:\dcs-terrain
```

Go to that folder.
`cd` means change folder:

```powershell
cd C:\dcs-terrain
```

Download the Windows release, which works on Windows 10 and 11:

```powershell
Invoke-WebRequest https://github.com/tritiumgg/dcs-terrain/releases/download/v0.3.0/dcsterrain-x86_64-pc-windows-msvc.zip -OutFile dcsterrain.zip
```

Unpack it into this folder:

```powershell
Expand-Archive -Force dcsterrain.zip -DestinationPath .
```

The folder now holds `dcsterrain.exe`.
`-Force` replaces an older copy, if you had one.
If you built the tool instead, copy your `dcsterrain.exe` into this folder.

Check that you have release 0.3.0:

```powershell
.\dcsterrain.exe --version
```

```text
dcsterrain 0.3.0
```

Release 0.1.0 does not have the commands this guide uses.

Every command below runs from this folder.
Each time you open PowerShell, go to it with `cd C:\dcs-terrain` first.
Each command starts with `.\dcsterrain.exe`.
The `.\` tells PowerShell to run the program in the current folder.

## Words you will meet

- **Terrain**: one DCS world, such as Caucasus or Syria.
- **Terrain id**: the short name the tool uses for a terrain, such as `Caucasus`.
- **Command**: the word after `.\dcsterrain.exe`, such as `find-sites`.
  It names the kind of question.
- **Option**: a word that starts with `--`, such as `--terrain`.
  The values after it belong to it.
- **x and z**: DCS's own coordinates, in meters.
  `x` grows to the north.
  `z` grows to the east.
- **MGRS**: the military grid reference system, such as `38T KM 78924 65394`.
  The Mission Editor shows it.
  To turn one into `x` and `z`, see [Convert a position](#convert-a-position).
- **Criterion**: a test a site must pass, such as "no steeper than 3 degrees".
  The plural is criteria.
- **Cache**: a folder where the tool keeps what it has worked out.
  It makes later questions fast.
  You can delete it at any time.
  The tool builds it again.
- **Airdrome**: the tool's word for an airfield.
  You write `--kind airdrome` to look for airfields.
  You write `airport:` before an airfield's name.
  Both mean an airfield.

All distances are in meters, unless a column says km.
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
Caucasus            Caucasus        8 entries, 13.4 MiB   50 of 50 readable
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
  A terrain you have never asked about shows `none` here.
- `files` says how many of the terrain's files the tool can read.
  "50 of 50" means all of them.

The tool only reads your DCS files.
It never changes them.

## Step 2: prepare the terrain

**When to use it:** once for each terrain, before your first question.
Run it again after each DCS update.
It is required for towns and road chokepoints.

```powershell
.\dcsterrain.exe index --terrain Caucasus
```

This command builds the terrain's cache ahead of time.
Most questions build what they need by themselves.
Questions about towns and road chokepoints do not.
Without this step, `chokepoints` and `towns` refuse to answer.
`point-info` answers, but leaves out the town or village a point stands in.

The first run on Caucasus takes about 5 seconds and prints this:

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

Caucasus builds in seconds.
Germany takes a minute or two.
While Germany builds, the tool needs about 1.5 GB of free memory.

## How you ask a question

Each kind of question has its own command.
This one asks for the airfield nearest to the town of Zugdidi:

```powershell
.\dcsterrain.exe nearest --terrain Caucasus --kind airdrome --at town:Zugdidi
```

```text
The nearest airdrome to -252637, 630096 (37T GH 36504 11129).

#  Distance (m)  Name           Class  Id        x       z  MGRS
1         33834  Senaki-Kolkhi  —      23  -281782  647279  38T KM 56437 80737

Every field, unrounded: add --json.
```

- `nearest` is the command: find the nearest thing.
- `--terrain Caucasus` picks the terrain.
  Every question needs it.
- `--kind airdrome` says what to find.
- `--at town:Zugdidi` says where to look from.
- The answer is a table.
  Senaki-Kolkhi is 33,834 m from Zugdidi.
  The table gives its position in `x` and `z`, and in MGRS.
- `—` means there is no value.

To see every option a command takes, add `--help`:

```powershell
.\dcsterrain.exe nearest --help
```

It lists the options, says which are required, and ends with an example.
`.\dcsterrain.exe --help` lists every command.

### Places

Many options take a place, such as `--at`.
You can write a place in five ways:

| Form | Example |
|---|---|
| DCS `x,z` in meters, with no space | `-284887,683859` |
| MGRS, in quotes | `"38T KM 91980 72567"` |
| Latitude and longitude, in quotes | `"42.177616N 42.481292E"` |
| An airfield by name | `airport:Kutaisi` |
| A town by name | `town:Zugdidi` |

Put quotes around a place that has a space in it.
Without them, PowerShell splits it into several words.
The tool then refuses each word as a place.

A plain name, such as `Kutaisi`, is refused.
Write `airport:` or `town:` before it, so the tool knows which you mean.

Some options also need a height, such as where an observer stands.
Write it after the place, after `@`, in meters above the ground:
`airport:Kutaisi@100` is 100 m above Kutaisi.
`town:Zugdidi@1200msl` is 1,200 m above sea level: `msl` means mean sea level.
With MGRS, the height goes inside the quotes: `"38T KM 82562 61755@10"`.

Copy an MGRS whole, with its zone, such as `38T`.
Caucasus spans two zones, `37T` in the west and `38T` in the east.

### Convert a position

**When to use `geo`:** you have a position in one form and need another.
For example, you have an MGRS and need DCS `x` and `z`.

```powershell
.\dcsterrain.exe geo --terrain Caucasus --to-xz "38T KM 59748 94822"
```

```text
1 point, in the order given.

#        x       z  MGRS                Latitude (°)  Longitude (°)
1  -267283  648580  38T KM 59748 94822    42.368559N     42.082249E

Each row is one point in every form: copy any of them into an option that takes a place. Latitude
and longitude are WGS84.

Every field, unrounded: add --json.
```

- Each row is one position, in every form.
  `x` is -267283 and `z` is 648580.
- `--to-xz` takes an MGRS, or a latitude and longitude.
- `--to-latlon` takes any place: `x,z`, MGRS, latitude and longitude, `airport:` or `town:`.
- Both print the same table.
  Give several positions after either one, each in quotes if it has a space.

A latitude and longitude goes to `--to-xz` the same way:

```powershell
.\dcsterrain.exe geo --terrain Caucasus --to-xz "42.368559N 42.082249E"
```

```text
1 point, in the order given.

#        x       z  MGRS                Latitude (°)  Longitude (°)
1  -267283  648580  38T KM 59748 94822    42.368559N     42.082249E

Each row is one point in every form: copy any of them into an option that takes a place. Latitude
and longitude are WGS84.

Every field, unrounded: add --json.
```

It takes degrees, minutes and seconds too.
Write them apart by spaces, with the letter first:

```powershell
.\dcsterrain.exe geo --terrain Caucasus --to-xz "N42 22 06.8 E42 04 56.1"
```

```text
1 point, in the order given.

#        x       z  MGRS                Latitude (°)  Longitude (°)
1  -267283  648580  38T KM 59748 94822    42.368556N     42.082250E

Each row is one point in every form: copy any of them into an option that takes a place. Latitude
and longitude are WGS84.

Every field, unrounded: add --json.
```

- The latitude reads 42.368556 here, not 42.368559 as above.
  That is because 06.8 seconds is rounded.
  It is still the same 1 m square.
- Degrees and minutes work the same way: `"N42 22.1135 E42 04.9349"`.
- The letter can also go after each number: `"42 22 06.8N 42 04 56.1E"`.
- The marks `°`, `'` and `"` work too.
  In PowerShell, a `"` inside quotes needs a backtick before it:
  ``"42°22'06.8`"N 42°04'56.1`"E"``.
  A position copied from a word processor may hold curly quotes, `’` and `”`.
  PowerShell reads those as quotes too, and the command fails.
  Spaces are easier to type.

This command gives the position of Kutaisi in every form:

```powershell
.\dcsterrain.exe geo --terrain Caucasus --to-latlon airport:Kutaisi
```

```text
1 point, in the order given.

#        x       z  MGRS                Latitude (°)  Longitude (°)
1  -284887  683859  38T KM 91980 72567    42.177616N     42.481292E

Each row is one point in every form: copy any of them into an option that takes a place. Latitude
and longitude are WGS84.

Every field, unrounded: add --json.
```

An MGRS names a square, not a point.
The tool takes the center of the square.
With five digits in each half, as above, the square is 1 m wide.

### More of the answer

The table shows the main fields, rounded.
To see every field, unrounded, add `--json` to the command.
It prints the whole answer in JSON, a plain-text format for data.
[For software: `query`](#for-software-query) says more about JSON.

## Step 3: ask where to put a FARP

**When to use `find-sites`:** you need a place for something.
You can describe the ground it needs.
Examples are a FARP, a SAM site, a bunker or a landing zone.

The question: where can a FARP go?
It needs flat, open ground.
It must be 3 to 15 km from Kutaisi.
It must be close to a road.

```powershell
.\dcsterrain.exe find-sites --terrain Caucasus --region airdrome-band Kutaisi 3000 15000 --footprint 120 80 --criteria surface-in land --criteria slope-max 3 --criteria dist-road-band 100 1000 --criteria clearing-max 0 --spacing 2000 --limit 3
```

This is one long command on one line.
Paste it as one line, with no line breaks.
Here is what each option asks for:

- `--region` is where to look.
  `airdrome-band Kutaisi 3000 15000` is a ring around Kutaisi.
  It runs from 3,000 m to 15,000 m from the airfield.
  A region can also be a `box`, a `circle` or a `polygon`.
  `find-sites --help` shows how to write each one.
- `--footprint 120 80` is the size of the site: 120 m wide and 80 m deep.
- `--criteria` is a test a site must pass.
  Give `--criteria` once for each test.
  A site must pass all of them.
  - `surface-in land`: the whole site is on land, not water.
  - `slope-max 3`: no part of the site is steeper than 3 degrees.
  - `dist-road-band 100 1000`: the nearest road is 100 m to 1,000 m away.
    Trucks can reach the site.
  - `clearing-max 0`: no building or other object stands within 150 m of the site.
    DCS removes objects near a FARP when the mission starts.
    This test keeps houses out of its way.
- `--spacing 2000` keeps the answers at least 2,000 m apart.
  Without it, you would get three spots next to each other.
- `--limit 3` is how many answers you want.

`find-sites --help` lists every test you can use, with what each one takes.

## Step 4: read the answer

```text
3 sites, best first, of 67872 lattice points in the region; 15196 passed every test, and 15 tiles
skipped whole.
--limit asks for more sites, up to 200.

#  Score        x       z  MGRS                Hdg (°)  surface_in (%)  slope_max (°)
1  5.000  -293887  671859  38T KM 78924 65394        0             100            0.0
2  5.000  -293287  694159  38T LM 00933 62872        0             100            0.0
3  5.000  -291987  672559  38T KM 79878 67165        0             100            0.0
2 columns left out to fit 100 characters: dist_road_band (m), clearing_max (count). --json has them.

Each criterion's score, 0 to 1, is in --json; the site's score is their weighted sum.

Every field, unrounded: add --json.
```

### How many positions passed

The first line says how the search went.

- The tool tested 67,872 positions in the ring.
  It calls them lattice points.
  They are 100 m apart.
- 15,196 positions passed every test.
  That leaves plenty to choose from.
- 15 tiles, blocks of ground, were ruled out as a whole.
  The tool did not need to test each position in them.

If only a few positions pass, your tests are strict.
If none pass, see [No site passes](#no-site-passes).

### The best sites

Each row is one site.

- `#` is its place in the list, best first.
- `Score` puts the sites in order.
  A higher score is better.
  Each test adds at most 1.
  The tool adds one test of its own, `valid`, so 5 tests here give at most 5.
  All three scored 5.000, so they tie.
  The ground around Kutaisi is a flat plain.
  Many sites there are equally good.
  The [worked example](#combine-questions-a-hidden-farp-and-its-fuel-convoy)
  breaks the tie by asking what the enemy can see.
- `x` and `z` are the site's center, in DCS coordinates.
- `MGRS` is the same center as a grid reference.
  Use it to place the FARP in the Mission Editor.
- `Hdg` is the heading to give the FARP in the Mission Editor, in degrees from north.
  At 0, the 80 m side runs north to south.
- The last columns show what the tool measured for some of the tests.
  The table fits in 100 characters, so it leaves some out.
  The line under the table names them.

### Why a site passed

`score-site` tests one site you name against the same tests.
It shows the result of every test.
Give it the first site's position with `--at`:

```powershell
.\dcsterrain.exe score-site --terrain Caucasus --at -293887,671859 --footprint 120 80 --criteria surface-in land --criteria slope-max 3 --criteria dist-road-band 100 1000 --criteria clearing-max 0
```

```text
terrain: Caucasus
dcs_build: 2.9.31.29434
terrain_fingerprint: 6293651208-203468160-1140069192
cell_size: 50.0
elapsed_ms: 12
orientation_deg: 0.0
score: 5.0
terms: 5 items
  {"name":"valid","value":1.0,"pass":true,"score":1.0}
  {"name":"surface_in","value":1.0,"pass":true,"score":1.0}
  {"name":"slope_max","value":0.0,"pass":true,"score":1.0}
  {"name":"dist_road_band","value":100.0,"pass":true,"score":1.0}
  {"name":"clearing_max","value":0.0,"pass":true,"score":1.0}
hard_fail: false
```

This answer is a plain list, not a table.
Each line under `terms` is one test.

- `name` is the test.
- `pass` says whether the site passed it.
- `value` is what the tool measured.
  The steepest slope is 0 degrees.
  The nearest road is 100 m away.
  No objects stand within 150 m.
- `score` is how well the site did on that test.
  It runs from 0 to 1.
  The site's score is the sum of these.
  Here every test has the same weight, 1.
  `--weights` in `find-sites --help` changes them.
- `valid` is added to every question.
  It checks that no part of the site is on the sea.
  It also checks that no part is off the edge of the terrain.

**The tests are the real answer.**
Read them before you trust a site.

The other lines say where the answer came from:

- `terrain` and `dcs_build` say which terrain and DCS build answered.
  DCS changes its terrains only in updates.
  So the build tells you which version of the terrain the answer is about.
- `terrain_fingerprint` is made from the sizes of the terrain's files.
  Two answers with the same fingerprint came from the same files.
- `cell_size` is the spacing of the grid the tool reads the ground on, in meters.
- `elapsed_ms` is how long the question took, in milliseconds.

## Step 5: find out what is at a site

**When to use `point-info`:** you have a position.
You want to know the ground there and what is near it.

```powershell
.\dcsterrain.exe point-info --terrain Caucasus --points "38T KM 78924 65394"
```

```text
1 point, in the order asked.

Ground
#        x       z  MGRS                Height (m)  Slope (°)  Aspect (°)  Water  Depth (m)  Cover
1  -293887  671859  38T KM 78924 65394        38.0        0.0           —  land           —  town

Nearby
#  Town                Place  Road (m)  Road  Airfield  Airfield (km)
1  1 km NW of Dapnari  —           125  road  Kutaisi           15.00

Aspect is the downslope direction from grid north. Latitude and longitude, the bearings from each
town, and where each road and airfield lie: --json.

Every field, unrounded: add --json.
```

The first table is the ground:

- `Height` is the ground's height above sea level.
- `Slope` is how steep the ground is at that point.
- `Aspect` is the direction the slope faces.
  Flat ground has none.
- `Water` and `Depth` say whether the point is on water, and how deep it is.
- `Cover` is the kind of ground there, such as field, forest or town.
  `town` describes the ground, not a building.
  The `clearing-max` test already checked for buildings.

The second table is what is near:

- `Town` names the nearest town and where the site lies from it.
  The site is about 1 km northwest of Dapnari.
  You can paste this into a mission briefing.
- `Place` is the town or village whose outline holds the point.
  `—` means the point is outside every one.
- `Road (m)` is the distance to the nearest road: 125 m.
- The next `Road` column is that road's kind.
- `Airfield` is the nearest airfield, and `Airfield (km)` its distance.

The road here is 125 m away.
`score-site` in Step 4 said 100 m.
The two differ because the tests measure on a 50 m grid.
The grid makes them fast, but less exact.
`point-info` measures to the road itself.
Trust the `point-info` number.

`--points` takes several places.
Give them one after another, separated by spaces.

## Step 6: check what can see the site

**When to use `visible`:** you want to know if one point can see another.
For example, can an enemy observer see your FARP?

`visible` uses the same kind of line-of-sight test as DCS.
Like DCS's test, it ignores buildings and trees.
Only the ground blocks the view.
The yes-or-no answer is the same whichever point you call `--a` and which `--b`.

Can a lookout on a 10 m mast on the hill southeast of the FARP see someone
standing at the FARP?
`--a` is the lookout, 10 m above the ground.
`--b` is the FARP, 2 m above the ground, the height of a person's eyes:

```powershell
.\dcsterrain.exe visible --terrain Caucasus --a -297000,676000@10 --b -293887,671859@2
```

```text
No: the ground blocks the line, rising 55.7 m above it at its highest.
The ends are 5.18 km apart.

Point          x       z  MGRS                Height
A        -297000  676000  38T KM 82562 61755  10.0 m above the ground
B        -293887  671859  38T KM 78924 65394  2.0 m above the ground
Blocked  -296099  674802  38T KM 81510 62808  237.8 m above sea level

Blocked is where the ground first rises above the line from A.

Every field, unrounded: add --json.
```

- The first line answers: no, they cannot see each other.
- The ground rises 55.7 m above the line of sight.
- `Blocked` is where the ground first gets in the way.

Now ask a different question.
Can an aircraft 100 m above Kutaisi see the FARP?

```powershell
.\dcsterrain.exe visible --terrain Caucasus --a airport:Kutaisi@100 --b -293887,671859@2
```

```text
Yes: A and B see each other. The line clears the ground by 2.0 m at its lowest.
The ends are 15.00 km apart.

Point        x       z  MGRS                Height
A      -284887  683859  38T KM 91980 72567  100.0 m above the ground
B      -293887  671859  38T KM 78924 65394  2.0 m above the ground

Every field, unrounded: add --json.
```

Yes, it can.
The line comes closest to the ground at the FARP, where it ends 2 m up.

## Step 7: see the answer on a map

**When to use `--geojson-out`:** you want to see an answer drawn on a map.
You can also load it into another program.

GeoJSON is a standard file format for shapes on a map.
QGIS, a free map program, opens it.
So does the website [geojson.io](https://geojson.io).

Add `--geojson-out` and a file name to the command from Step 3:

```powershell
.\dcsterrain.exe find-sites --terrain Caucasus --region airdrome-band Kutaisi 3000 15000 --footprint 120 80 --criteria surface-in land --criteria slope-max 3 --criteria dist-road-band 100 1000 --criteria clearing-max 0 --spacing 2000 --limit 3 --geojson-out farp.geojson
```

The table is the same as before.
Above the last line, the answer now says:

```text
The GeoJSON is written to farp.geojson.
```

The file holds two shapes for each site.
A point marks its center.
A polygon draws its 120 × 80 m outline.

To see the shapes, open `farp.geojson` in a map program.
On geojson.io, click Import.
Then choose the file.
The shapes appear on a map of the real world.
The file stores positions as longitude and latitude.
So every map program puts them in the right place.

### Which commands draw

These commands take `--geojson-out`:

| Command | What it draws |
|---|---|
| `find-sites`, `score-site` | each site's center and outline |
| `visible` | the line of sight, and the point where the ground blocks it |
| `viewshed` | the area one observer can see |
| `radar-floor` | bands that show how low an aircraft can fly unseen by a site |
| `coverage` | the gaps in a site's view at each altitude |
| `approach-spectrum` | the directions a site cannot see, as wedges |
| `unmask-profile` | a flight path, and the parts of it a site cannot see |
| `exposure` | the area that no site in a group can see |
| `route`, `route-alternatives` | the route along the roads or railways |
| `chokepoints` | the roads that matter most |
| `sample` | each point sampled, with its layer values |
| `nearest` | each thing found, and the road or railway it lies on |
| `scenery-in` | each object's outline, and the area you asked about |
| `trafficability` | the way from a point to the road, and the straight line to the nearest road |
| `point-info` | each point you asked about |
| `airfield-info` | the runways and parking spots |
| `validate-placements` | each placement, and anything in its way |
| `towns` | each town's center, and its outline if you ask for it |

`describe` and `geo` draw nothing.

## Combine questions: a hidden FARP and its fuel convoy

Real problems take more than one question.
The answer to one question becomes part of the next.
This example starts from the FARP question of Step 3.
It answers three more questions:

1. Which site can the enemy not see?
2. Which road does the fuel convoy take from Kutaisi to that site, and how
   much of it can the enemy see?
3. How do you see the site and the road together on a map?

The setup is made up for this example.
The enemy holds the hills south of Kutaisi.
It has two observation posts there, A and B.
Each post has a lookout on a 10 m mast.

First, look at the two posts with one `point-info` question:

```powershell
.\dcsterrain.exe point-info --terrain Caucasus --points -297000,676000 -296000,668000
```

```text
2 points, in the order asked.

Ground
#        x       z  MGRS                Height (m)  Slope (°)  Aspect (°)  Water  Depth (m)  Cover
1  -297000  676000  38T KM 82562 61755       308.3       11.3           3  land           —  forest
2  -296000  668000  38T KM 74834 63855       139.2       10.7         323  land           —  town

Nearby
#  Town                  Place              Road (m)  Road  Airfield  Airfield (km)
1  1 km N of Didi-Opeti  —                      2693  road  Kutaisi           14.44
2  4 km NE of Tolebi     unnamed (village)       190  road  Kutaisi           19.36

Aspect is the downslope direction from grid north. Latitude and longitude, the bearings from each
town, and where each road and airfield lie: --json.

Every field, unrounded: add --json.
```

Post A stands 308 m above sea level, 1 km north of Didi-Opeti.
Post B stands 139 m above sea level, 4 km northeast of Tolebi.

### Question 1: a site the posts cannot see

`find-sites` has a test for this: `not-visible-from`.
It takes the observers, then the share of them that may see the site.
Add it to the command from Step 3:

```powershell
.\dcsterrain.exe find-sites --terrain Caucasus --region airdrome-band Kutaisi 3000 15000 --footprint 120 80 --criteria surface-in land --criteria slope-max 3 --criteria dist-road-band 100 1000 --criteria clearing-max 0 --criteria not-visible-from -297000,676000@10 -296000,668000@10 0 --spacing 2000 --limit 3
```

- `-297000,676000@10` is post A, 10 m above the ground.
- `-296000,668000@10` is post B, 10 m above the ground.
- `0` is the share of the posts that may see the site.
  0 means none of them.
  0.5 would allow one of the two.
- The test looks from each post to a point 2 m above the site, the height
  of a person's eyes.

```text
3 sites, best first, of 67872 lattice points in the region; 630 passed every test, and 15 tiles
skipped whole.
--limit asks for more sites, up to 200.

#  Score        x       z  MGRS                Hdg (°)  surface_in (%)  slope_max (°)
1  6.000  -293887  671859  38T KM 78924 65394        0             100            0.0
2  5.891  -270687  683959  38T KM 94066 86514        0             100            0.3
3  5.797  -294087  690859  38T KM 97577 62546        0             100            0.6
3 columns left out to fit 100 characters: dist_road_band (m), clearing_max (count), not_visible_from
(%). --json has them.

Each criterion's score, 0 to 1, is in --json; the site's score is their weighted sum.

Every field, unrounded: add --json.
```

What it means:

- Only 630 positions pass now, against 15,196 before.
  The posts see most of the plain.
- Every site in the list is hidden from both posts.
- Site 1 has the best score, so it is the site to use.
  It is the same site as site 1 in the Step 4 table.
  The other two sites of that table are gone: a post sees each of them.

Check site 1 against post B with `visible`.
Step 6 already checked post A.

```powershell
.\dcsterrain.exe visible --terrain Caucasus --a -296000,668000@10 --b -293887,671859@2
```

```text
No: the ground blocks the line, rising 6.5 m above it at its highest.
The ends are 4.40 km apart.

Point          x       z  MGRS                Height
A        -296000  668000  38T KM 74834 63855  10.0 m above the ground
B        -293887  671859  38T KM 78924 65394  2.0 m above the ground
Blocked  -295913  668160  38T KM 75003 63918  144.7 m above sea level

Blocked is where the ground first rises above the line from A.

Every field, unrounded: add --json.
```

Post B cannot see site 1.
But the ground rises only 6.5 m above the line.
If B had a taller mast, it might see the site.

### Question 2: the convoy's road

**When to use `route`:** you need the way from one point to another along the
roads or railways.

The convoy starts at Kutaisi and ends at site 1.
`--exposure-observers` names the posts.
Then the answer says how much of each stretch of road those posts can see.

```powershell
.\dcsterrain.exe route --terrain Caucasus --network roads --from airport:Kutaisi --to -293887,671859 --exposure-observers -297000,676000@10 -296000,668000@10
```

```text
The route by roads: 25.82 km, 1.72 × the 15.00 km straight line.
It climbs 69 m and descends 75 m; its steepest 100 m is 9.3 %.
The from end is 504 m from the road, a stretch this route does not plan: --offroad plans it across
country.

Ends, each where it joins the network
End         x       z  MGRS                Moved (m)
From  -285357  683675  38T KM 91734 72132        504
To    -293900  671983  38T KM 79045 65364        125

Legs
 #  Class  Length (km)  Up (m)  Down (m)  Grade (%)  Seen (%)        x       z  MGRS
 1  road          0.20       0         0        0.0       100  -285544  683730  38T KM 91762 71940
 2  road          1.48       0         0        0.0       100  -285112  685134  38T KM 93203 72169
 3  road          3.23      15         0        2.4        97  -282537  685933  38T KM 94348 74589
 4  road          1.55       0        15        1.7       100  -283226  684565  38T KM 92907 74102
 5  road          0.31       0         0        0.2       100  -283395  684306  38T KM 92629 73972
 6  road          1.20       1         0        0.2       100  -283975  683252  38T KM 91512 73549
 7  road          0.09       0         0        0.2       100  -284014  683171  38T KM 91427 73522
 8  road          0.47       0         0        0.1       100  -284199  682742  38T KM 90979 73401
 9  road          2.20       0         8        0.7       100  -285183  680772  38T KM 88904 72708
10  road          1.76      15        14        4.8       100  -285418  679238  38T KM 87363 72691
11  road          3.34       0         4        0.9       100  -287011  676469  38T KM 84418 71513
12  road          2.18       0         2        0.3       100  -287253  674383  38T KM 82333 71565
13  road          2.71       0         7        0.4       100  -288895  672245  38T KM 80001 70250
14  road          5.10      38        24        9.3        79  -293900  671983  38T KM 79045 65364
1 column left out to fit 100 characters: Edge. --json has it.

Each leg's x, z and MGRS are where it ends. 59 waypoints are left out: --json lists them,
--geojson-out draws the way.

Every field, unrounded: add --json.
```

What it means:

- The route is 25.82 km long.
  The straight line is 15 km, so the roads add almost 11 km.
- `Ends` says where the route joins the road.
  Kutaisi's center is 504 m from the nearest road.
  The site is 125 m from it.
- Those 504 m are not in the 25.82 km.
  The route starts on the road, so it does not plan the way from the airfield to it.
- `Legs` lists the stretches of road between junctions, in order.
- `Seen` is the share of each leg the posts can see.
  The posts see all of 12 legs, 97 % of leg 3 and 79 % of leg 14.
- The site is hidden, but the road to it is not.

**Plan the way off the road too.**
`--offroad` plans each end's way across country to a road, round water and steep slopes.
It is not tied to the nearest road: it takes whichever road gives the least costly route.
A meter across country costs as much as 2 m of road, so the route keeps to roads where it can.

```powershell
.\dcsterrain.exe route --terrain Caucasus --network roads --from airport:Kutaisi --to -293887,671859 --exposure-observers -297000,676000@10 -296000,668000@10 --offroad
```

```text
The route by roads: 18.91 km, 1.26 × the 15.00 km straight line.
It climbs 54 m and descends 61 m; its steepest 100 m is 9.3 %.
Across country: 1.04 km of it, 5 %: 765 m from the start to the road and 270 m from the road to the
end.

Ends, each where it joins the network
End         x       z  MGRS                Moved (m)
From  -284319  683346  38T KM 91556 73198        765
To    -293675  672025  38T KM 79118 65579        269

Across country, every turn
Way           #        x       z  MGRS
From to road  1  -284887  683859  38T KM 91980 72567
From to road  2  -284319  683346  38T KM 91556 73198
Road to to    1  -293675  672025  38T KM 79118 65579
Road to to    2  -293725  671975  38T KM 79061 65537
Road to to    3  -293887  671859  38T KM 78924 65394

Legs
#  Class  Length (km)  Up (m)  Down (m)  Grade (%)  Seen (%)        x       z  MGRS
1  road          0.20       0         0        0.1       100  -284146  683250  38T KM 91485 73381
2  road          0.15       0         0        0.3       100  -284025  683167  38T KM 91421 73512
3  road          0.46       0         0        0.1       100  -284199  682742  38T KM 90979 73401
4  road          2.20       0         8        0.7       100  -285183  680772  38T KM 88904 72708
5  road          1.76      15        14        4.8       100  -285418  679238  38T KM 87363 72691
6  road          3.34       0         4        0.9       100  -287011  676469  38T KM 84418 71513
7  road          2.18       0         2        0.3       100  -287253  674383  38T KM 82333 71565
8  road          2.71       0         7        0.4       100  -288895  672245  38T KM 80001 70250
9  road          4.87      36        24        9.3        83  -293675  672025  38T KM 79118 65579
1 column left out to fit 100 characters: Edge. --json has it.

Each leg's x, z and MGRS are where it ends. 39 waypoints are left out: --json lists them,
--geojson-out draws the way.

Every field, unrounded: add --json.
```

What it means:

- The route is now 18.91 km, almost 7 km shorter.
  765 m across country reaches a road that runs more directly to the site.
- `Across country` lists every turn of the two ways off the road.
  In the Mission Editor, make each turn a waypoint with the action `Off Road`, and the rest `On Road`.
  A ground group drives straight between waypoints off the road, so give it every turn.
- The way to the site bends to stay out of the posts' sight, so it is 270 m rather than 125 m.
- Each end's way is searched within 5 km of it, the most `--max-offroad` allows.
  `--max-slope-deg` sets the steepest ground, 30° by default.
- The ways are this tool's own plan.
  DCS was not asked, and its ground units may pick their own way between waypoints.

Is there a better road?
`route-alternatives` looks for different routes between the same two points.
`--k` is how many routes to look for:

```powershell
.\dcsterrain.exe route-alternatives --terrain Caucasus --network roads --from airport:Kutaisi --to -293887,671859 --k 3
```

The answer starts like this:

```text
1 route by roads, the least costly first.
3 were asked for. A route is kept only when it is new, at most 1.50 × the first's length, and shares
at most 50 % of its length with each route kept; the search, at most 12 tries, found no more.
```

It found only one route.
Any other route is over 1.5 times as long, or shares more than half its road with this one.
So the convoy cannot hide by its choice of road.
Run it at night, escort it, or deal with the posts first.

### Question 3: both on one map

Add `--geojson-out` to the `route` command, to draw the route:

```powershell
.\dcsterrain.exe route --terrain Caucasus --network roads --from airport:Kutaisi --to -293887,671859 --exposure-observers -297000,676000@10 -296000,668000@10 --geojson-out convoy.geojson
```

Open `farp.geojson` from Step 7 and `convoy.geojson` together in QGIS.
Or open them one after the other on geojson.io.
`farp.geojson` shows the three sites of Step 4.
Site 1 is the one the convoy goes to.
`convoy.geojson` shows the road from Kutaisi to it.

## Other questions you can ask

Each command fits a situation.
`--help` after a command lists what it takes.
[queries.md](queries.md) describes each one in full.

| Command | When you |
|---|---|
| `sample` | need the height, slope or ground type at many points, or over an area |
| `point-info` | have one position and want what is there and what is near it |
| `geo` | have a position in one form, such as MGRS, and need another, such as x and z ([Convert a position](#convert-a-position)) |
| `describe` | want to confirm which terrain and build the tool is using |
| `find-sites` | need the best places for something |
| `score-site` | have a place in mind and want to test it |
| `validate-placements` | want to check that units do not sit on water, a slope, in forest, on a building or a runway |
| `visible` | want to know if one point can see another |
| `viewshed` | want the whole area one observer can see |
| `radar-floor` | want to know how low a plane can fly past a site unseen |
| `coverage` | are placing a radar or SAM and want the gaps it leaves at each altitude |
| `approach-spectrum` | want the directions from which a site can be approached unseen |
| `unmask-profile` | have a flight path and want to know where a site first sees it |
| `exposure` | have several radars or SAMs and want the holes between them |
| `route` | need the way by road or rail |
| `route-alternatives` | want several different ways |
| `chokepoints` | want the roads or bridges that matter most, to cut or to guard (needs Step 2) |
| `trafficability` | want to know if a ground group can drive between the road and a point, and which way |
| `nearest` | want the nearest road, railway, airfield, town or object |
| `scenery-in` | want the objects, power-line poles or single trees in an area, listed or counted |
| `towns` | want a real town for an objective or a briefing (needs Step 2) |
| `airfield-info` | need an airfield's runways, radios and parking |

**When you need the airfields' names and positions**, ask `nearest` for several:

```powershell
.\dcsterrain.exe nearest --terrain Caucasus --kind airdrome --at airport:Kutaisi --n 5
```

```text
The 5 nearest airdromes to -284887, 683859 (38T KM 91980 72567), nearest first.

#  Distance (m)  Name               Class  Id        x       z  MGRS
1             0  Kutaisi            —      25  -284887  683859  38T KM 91980 72567
2         36711  Senaki-Kolkhi      —      23  -281782  647279  38T KM 56437 80737
3         58478  Kobuleti           —      24  -317962  635633  37T GG 37400 45960
4         97205  Batumi             —      22  -355811  617386  37T GG 16665 09697
5        135669  Sukhumi-Babushara  —      20  -220592  564392  37T FH 73597 47579

Every field, unrounded: add --json.
```

`--n` takes up to 50.
Use a name from this list after `airport:`, such as `airport:Senaki-Kolkhi`.

The other commands:

- **`check`**: use it when a terrain will not load.
  It lists the terrain's files and says which ones the tool can read.
- **`cache list`**: use it to see how much space the cache takes.
- **`cache clear`**: use it to free that space.
  The tool builds the cache again when it needs it.

## When something goes wrong

When a question fails, the tool prints a line that starts with `error:`.
The line says what is wrong, and often what to type instead.
Where a message shows a command starting with `dcsterrain`, type `.\dcsterrain.exe` instead.
Here are the errors you are most likely to meet.

### A name without its kind

```powershell
.\dcsterrain.exe nearest --terrain Caucasus --kind airdrome --at Kutaisi
```

```text
error: --at: "Kutaisi" needs a kind. Type airport:Kutaisi or town:KUTAISI.
```

A place name needs `airport:` or `town:` before it.
Kutaisi is both an airfield and a town, so the message offers both.
Type the one you mean.
Capital letters do not matter.

### A name it does not know

```powershell
.\dcsterrain.exe nearest --terrain Caucasus --kind airdrome --at airport:Kutaisy
```

```text
error: --at: no airport on Caucasus is named "Kutaisy".
The closest names: airport:Kutaisi, airport:Gudauta, airport:Krymsk, airport:Anapa-Vityazevo, airport:Batumi.
```

The message lists the closest names.
Check the spelling, and use one of them.

### A town name two towns share

```powershell
.\dcsterrain.exe nearest --terrain Caucasus --kind airdrome --at town:Mayskiy
```

```text
error: --at: "Mayskiy" names 2 towns on Caucasus. Type the position of the one you mean:
  38T MP 24680 31287   x -107364 z 793444
  38T KQ 93791 08909   x -49159 z 651511
```

Copy the position of the town you mean, in MGRS or as `x,z`.
For the first one, type `--at "38T MP 24680 31287"` or `--at -107364,793444`.

### An MGRS without quotes

```powershell
.\dcsterrain.exe point-info --terrain Caucasus --points 38T KM 78924 65394
```

The answer starts like this:

```text
error: --points: "38T" is not a place: a name needs its kind, and no airport or town on Caucasus has it.
```

This is the first of four errors, one for each part.
Each one is followed by a list of the forms a place takes.
PowerShell split the MGRS at its spaces.
Put it in quotes: `"38T KM 78924 65394"`.

### A misspelled test

```powershell
.\dcsterrain.exe find-sites --terrain Caucasus --region circle airport:Kutaisi 5000 --footprint 60 40 --criteria slop-max 3 --spacing 1000
```

```text
error: --criteria #1 is one of slope-max, span-max, surface-in, surface-enum-in, cover-in, cover-fraction-min, dist-cover-band, dist-road-band, dist-rail-band, dist-water-min, dist-airdrome-band, scenery-max, scenery-in-footprint-max, clearing-max, enclosure-min, masked-from, not-visible-from, visible-to, tpi-class-in, valid, not "slop-max". Did you mean `slope-max`?

For the flags find-sites takes, run: dcsterrain find-sites --help
```

`#1` means the first `--criteria`.
The message lists every test and suggests the one you probably meant.

### A missing option

```powershell
.\dcsterrain.exe find-sites --terrain Caucasus
```

```text
error: --region is required: Where to look.

For the flags find-sites takes, run: dcsterrain find-sites --help
```

Add the option the message names.
`--help` shows every required option.

### The wrong terrain id

This command used the folder name `GermanyColdWar` instead of the id:

```powershell
.\dcsterrain.exe nearest --terrain GermanyColdWar --kind airdrome --at 0,0
```

```text
error: no terrain GermanyColdWar in this install; the ids it holds are Afghanistan, Caucasus, GermanyCW, MarianaIslands, MarianaIslandsWWII, Nevada, PersianGulf, Syria
```

The message lists the ids you can use.
`list-terrains` shows them next to their folders.

### DCS is not where the tool looked

This command pointed the tool at a folder with no DCS in it:

```powershell
.\dcsterrain.exe nearest --install C:\Games --terrain Caucasus --kind airdrome --at 0,0
```

```text
error: C:\Games has no Mods/terrains to read: The system cannot find the path specified. (os error 3)
```

The tool found no terrains in that folder.
This happens when `--install` names the wrong folder.
It also happens when you have moved DCS since you installed it.
Give `--install` the DCS folder.
On Windows, that folder holds the `bin` and `Mods` folders.
For example: `--install "D:\DCS World"`.

### Off the terrain

```powershell
.\dcsterrain.exe visible --terrain Caucasus --a 900000,900000@2 --b airport:Kutaisi@100
```

```text
error: --a: 900000,900000 lies off Caucasus, which covers x -490050 to 270000 and z -570000 to 960050.
```

The point is outside the terrain.
Check that you did not swap `x` and `z`.

### The region is too big

```powershell
.\dcsterrain.exe find-sites --terrain Caucasus --region circle airport:Kutaisi 60000 --footprint 60 40 --criteria slope-max 3 --spacing 1000
```

```text
error: --region: the region holds more than 131072 lattice points at this step, over the 262144 sites a search may weigh at 2 orientations; widen the step or split the region
```

This circle is 120 km across.
`find-sites` tests positions on a grid, and this region holds too many.
There are three fixes:

- Make the region smaller.
- Split it into pieces, and ask about each piece.
- Add `--step 300`.
  `--step` is the distance between the positions it tests.
  It is 100 m unless you set it.

### No site passes

This command asked for a site on the sea, near Kutaisi, where there is no sea:

```powershell
.\dcsterrain.exe find-sites --terrain Caucasus --region circle airport:Kutaisi 5000 --footprint 60 40 --criteria surface-in sea --spacing 1000
```

```text
No site passes every test: of 7845 lattice points in the region, 0 passed, and 15 tiles skipped
whole. Loosen a criterion, or widen the region.

Every field, unrounded: add --json.
```

This is not an error.
The question worked, but no position passed every test.
Make one test less strict and ask again.
Repeat until some positions pass.

### No route

This command allowed no detour at all:

```powershell
.\dcsterrain.exe route --terrain Caucasus --network roads --from airport:Kutaisi --to -250000,650000 --max-detour-factor 1.0
```

```text
error: the network joins no route between the ends within the detour allowed, past the regions avoided
```

`route` found no way along the roads between the start and the end.
There are four reasons this can happen:

- No road joins the start to the end.
- The start or the end joins a short road that meets no other road.
  `route` begins on the road nearest to the start, and ends on the road nearest to the end.
  Kobuleti's nearest road is one such piece, so every route from `airport:Kobuleti` fails.
  The error then says so:

  ```text
  error: the network joins no route between the ends within the detour allowed, past the regions avoided; the from end snaps to a piece of road that joins none the other end's road reaches: set offroad to plan its way across country to a road that leads on
  ```

  Add `--offroad`, and the route crosses country to a road that leads on:

  ```powershell
  .\dcsterrain.exe route --terrain Caucasus --network roads --from airport:Kobuleti --to airport:Batumi --offroad
  ```

  The answer starts like this:

  ```text
  The route by roads: 50.34 km, 1.20 × the 42.02 km straight line.
  It climbs 455 m and descends 463 m; its steepest 100 m is 18.4 %.
  Across country: 1.89 km of it, 4 %: 1.39 km from the start to the road and 507 m from the road to
  the end.
  ```
- An area you asked it to avoid cuts every way through.
- Every way is too long.
  By default, a route may be at most 3 times as long as the straight line.
  `--max-detour-factor` sets that limit.
  This command set it to 1.0.
  No road is that straight.

### The terrain is not prepared

`chokepoints` and `towns` refuse until you run [Step 2](#step-2-prepare-the-terrain):

```text
error: betweenness is not built yet; `dcsterrain index --terrain Caucasus` builds it
```

Betweenness is how the tool ranks chokepoints.
Run the command in the message, as `.\dcsterrain.exe index --terrain Caucasus`.
Then ask your question again.

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
Then add `--install ~/dcs` to every command.
`~` means your home folder.

Download the file for your computer from release 0.3.0:

- `dcsterrain-aarch64-apple-darwin.tar.gz` for a Mac with Apple silicon.
- `dcsterrain-x86_64-apple-darwin.tar.gz` for a Mac with an Intel chip.
- `dcsterrain-x86_64-unknown-linux-gnu.tar.gz` for Linux on a 64-bit PC.

For example, on a Mac with Apple silicon, open Terminal and run:

```bash
curl -LO https://github.com/tritiumgg/dcs-terrain/releases/download/v0.3.0/dcsterrain-aarch64-apple-darwin.tar.gz
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
Every command works the same way:

```bash
./dcsterrain nearest --install ~/dcs --terrain Caucasus --kind airdrome --at town:Zugdidi
```

A DCS update can change a terrain file while a question is reading it.
Then the question may stop with the message "bus error".
Ask the question again.

## For software: `query`

The commands above are for people.
Software, scripts and AI assistants use `query` instead.
`query` takes a question in JSON and answers in JSON.
JSON is a plain-text format for data.
Each command above makes the same question `query` takes, and gets the same
answer.

### One question

A question names its operation in `op` and its details in `request`.
The `|` sign sends it into the tool:

```powershell
'{"op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"nearest","result":{"terrain":"Caucasus","dcs_build":"2.9.31.29434","terrain_fingerprint":"6293651208-203468160-1140069192","cell_size":50.0,"elapsed_ms":1,"results":[{"kind":"airdrome","id":"25","name":"Kutaisi","class":null,"x":-284887.375,"z":683858.71875,"distance":60.0606545632205}]}}
```

- The single quotes `' '` keep the line together.
- `line` counts the lines you sent.
- `result` holds the answer.

An operation's name is its command's name with `_` for `-`: `find-sites`
is `find_sites`.

### From a command to a request

`--show-request` prints the request a command makes, and stops.
It is the easiest way to write a request:

```powershell
.\dcsterrain.exe nearest --terrain Caucasus --kind airdrome --at town:Zugdidi --show-request
```

`--json` on a command prints the answer `query` gives, byte for byte, without
the `line` and `op` around it.

`.\dcsterrain.exe schema` prints every request and answer, field by field.
`.\dcsterrain.exe schema --op find_sites` prints one.
[queries.md](queries.md) describes them in full.

### Many questions at once

A batch is a file with one question per line.
The tool starts once and reads the terrain once, so a batch is faster.
Each line can also have an `id`, so you can tell the answers apart.
A file of JSON lines is called `.jsonl`.
This command makes a file, `questions.jsonl`, with two questions in it:

```powershell
@'
{"id": "kutaisi", "op": "nearest", "request": {"kind": "airdrome", "x": -284900, "z": 683800}}
{"id": "site1", "op": "point_info", "request": {"points": [{"x": -293887.375, "z": 671858.71875}]}}
'@ | Set-Content questions.jsonl
```

Then give the tool the file, and the file to write its answers to:

```powershell
.\dcsterrain.exe query --terrain Caucasus --input questions.jsonl --output answers.jsonl
```

`--output` writes the answers in UTF-8, the encoding other tools expect.
If `answers.jsonl` is already there, it is replaced.
Do not save the answers with `>` instead.
In Windows PowerShell 5.1, `>` saves them as UTF-16, which most other tools cannot read.

A long batch tells you how far it has come, at most once a second:
how many lines it has answered, and how many of them failed.
It tells the count once more when it ends.
That count goes to the PowerShell window, never into `answers.jsonl`.
When a program runs the batch, add `--progress` to get the count too.
Choose the output file yourself: the same file as `--input` is refused.

Each line of `answers.jsonl` answers one line of your file, in the same order,
with the same `id`.
If a line fails, it has `issues` instead of `result`.
The other lines still run.

### Errors in JSON

A failed question has `issues` instead of `result`.
Each issue has a `code`, a `message` and a `path`.
This question misspells Kutaisi:

```powershell
'{"op": "airfield_info", "request": {"airdrome": "Kutaisy"}}' | .\dcsterrain.exe query --terrain Caucasus
```

```text
{"line":1,"op":"airfield_info","issues":[{"code":"unknown_airdrome","message":"no airdrome has the name, name_id or id `Kutaisy`","path":"airdrome"}]}
```

The `path` names the part of the request that is wrong.
`.\dcsterrain.exe schema` lists every code under `issue_codes`.

### Putting the request on the command line

`query` can also take a request after the operation's name:

```powershell
.\dcsterrain.exe query --terrain Caucasus nearest '{"kind": "airdrome", "x": -284900, "z": 683800}'
```

That works in PowerShell 7.
Windows PowerShell 5.1 removes the double quotes before the tool sees them, so
it fails there.
`$PSVersionTable.PSVersion` shows which PowerShell you have.
Send the question with `|`, which works in every PowerShell.

### GeoJSON in an answer

A request with `"geojson": true` adds a `geojson` field to the answer.
It holds what `--geojson-out` writes to a file.

## Ask through an AI assistant

An AI assistant can ask the questions for you.
It talks to `dcsterrain` through MCP, the Model Context Protocol.
MCP is the standard way an assistant calls tools on your computer.
`dcsterrain mcp` is a small program, a server, with one tool per kind of question.
The assistant starts it by itself, when the assistant opens.
You do not type any commands.

The steps below take the program to be `C:\dcs-terrain\dcsterrain.exe`.
Put your own path wherever it appears.
In a command, put a path with a space in it in quotes, such as
`"C:\My Tools\dcsterrain.exe"`.

### Get the skill

The skill teaches the assistant which tool answers which question.
Without it, the assistant knows the tools but not how to use them well.
It is the `skill` folder of the source code.
Go to your work folder:

```powershell
cd C:\dcs-terrain
```

Download the source code:

```powershell
Invoke-WebRequest https://github.com/tritiumgg/dcs-terrain/archive/refs/heads/main.zip -OutFile source.zip
```

```powershell
Expand-Archive -Force source.zip -DestinationPath .
```

The skill is now in `C:\dcs-terrain\dcs-terrain-main\skill`.
Run the steps below in the same window.
In a new window, run `cd C:\dcs-terrain` first.

### Claude Code

Add the server for every folder you work in:

```powershell
claude mcp add --scope user dcsterrain -- C:\dcs-terrain\dcsterrain.exe mcp
```

Everything after `--` is the command the assistant runs.
One server answers about every terrain you have.
The assistant names the terrain in each question it asks.

Copy the skill to where Claude Code looks for skills.
These two commands also update a skill copied before:

```powershell
New-Item -ItemType Directory -Force $HOME\.claude\skills\dcs-terrain
```

```powershell
Copy-Item -Recurse -Force dcs-terrain-main\skill\* $HOME\.claude\skills\dcs-terrain
```

Start `claude`, and type `/mcp`.
`dcsterrain` shows as connected.
On macOS or Linux, `claude mcp add` is the same, with your own path.
The skill goes in `~/.claude/skills/dcs-terrain`:

```bash
mkdir -p ~/.claude/skills/dcs-terrain
```

```bash
cp -R dcs-terrain-main/skill/* ~/.claude/skills/dcs-terrain
```

### Claude Desktop

Open the configuration file.
In Claude Desktop, open Settings, then Developer, and choose Edit Config.
It usually opens `%APPDATA%\Claude\claude_desktop_config.json`.
Edit the file that button opens, wherever it is.
An empty file takes the text below as it stands.
A file that already holds settings keeps them: add the `dcsterrain` entry inside
its `mcpServers`, or add `mcpServers` beside the settings.

```json
{
  "mcpServers": {
    "dcsterrain": {
      "command": "C:\\dcs-terrain\\dcsterrain.exe",
      "args": ["mcp"]
    }
  }
}
```

In this file, every `\` in a path is written twice.
Quit Claude Desktop completely, from its icon beside the clock, and open it again.
In a new chat, open the `+` menu, then Connectors: `dcsterrain` is listed.

On macOS, the file is `~/Library/Application Support/Claude/claude_desktop_config.json`.
On Linux, it is `~/.config/Claude/claude_desktop_config.json`.
There, a path is written once, such as `/Users/you/dcs-terrain/dcsterrain`.

Claude Desktop takes a skill as a zip file.
Make one, with the folder inside named `dcs-terrain`:

```powershell
New-Item -ItemType Directory -Force skill-zip\dcs-terrain
```

```powershell
Copy-Item -Recurse -Force dcs-terrain-main\skill\* skill-zip\dcs-terrain
```

```powershell
tar -a -c -f dcs-terrain-skill.zip -C skill-zip dcs-terrain
```

`tar` comes with Windows 10 and 11.

In Settings, open Capabilities.
Skills need "Code execution and file creation" turned on.
Under Skills, choose Upload skill, and pick `dcs-terrain-skill.zip`.

### Ask

Ask in plain words, such as "Find a FARP site near Kutaisi."
Name the terrain if the place does not make it clear, such as "on Syria".
The assistant can list your terrains and their ids if you ask which you have.
The assistant asks for your approval before it uses a tool, unless you allow it
for good.
It says which terrain and DCS version the answer came from.

A very large answer, such as the GeoJSON of a big area, does not fit in a chat.
The tool writes it to a file instead and gives the assistant its path.
The file is in the `dcsterrain` folder of your temporary folder, which on
Windows is `%TEMP%`.
It is kept for a day.
The assistant gives you the path, and you open the file in a map program.

If the server does not connect, or every tool fails, ask one question by hand
with the same program, here about Caucasus:

```powershell
C:\dcs-terrain\dcsterrain.exe describe --terrain Caucasus
```

If it prints an error, that error is the cause.
[When something goes wrong](#when-something-goes-wrong) explains each one.
Claude Desktop writes what the server printed to
`%APPDATA%\Claude\logs\mcp-server-dcsterrain.log`.

## Where to go next

- `.\dcsterrain.exe <command> --help` shows what one command takes.
- [queries.md](queries.md) describes every operation, test and layer.
- The [skill](../skill/SKILL.md) teaches an AI assistant to ask these questions
  for you.
