# ADR 0048: A coarse cell takes the largest code and the mean of its covered cells

## Status

Accepted

## Context

**Affects:** `core.md` "Grids" (the resampling sentence of the pack config,
and `omit_sea_tiles`); ADR 0011 (omitting sea tiles is lossless); ADR 0036;
`plan.md` C5b.

At `cell_size` 100 every packed cell covers 2 × 2 extract cells. The core spec
gives the rule in one sentence:

> A `cell_size` of 100 resamples the extract by nearest-neighbour for
> `water`/`surface` and mean for `height`.

The spec leaves three things open.

- **Nearest to what.** The coarse cell's centre is the corner its four cells
  share, so all four are equally near, and "nearest" needs a tie rule.
- **The mesh edge.** Some of the four cells may be outside the mesh and
  `nodata`. The spec does not say whether the coarse cell is then `nodata`,
  or is made from the cells that are covered.
- **Sea tiles.** `omit_sea_tiles` stores no tile but `water` where `water` is
  entirely sea, and the reader returns `height` 0 and `surface` 3 there. ADR
  0011 keeps the rule because omitting is lossless: at 50 m a sea cell's
  height is the water surface, 0. At 100 m a coarse cell can be sea and still
  have land among its four cells, and its mean height is then not 0.

Both per-cell precedences already exist. ADR 0041 classes a cell's water as
river, else sea, else lake, else land, which is codes 3, 2, 1, 0 in
descending order. ADR 0039 sets `surface` to the first that holds of runway 5,
road 4, water 3 and land 1, also in descending order. `nodata` is 255 for
`water` and 0 for `surface`.

## Decision

A coarse cell is covered when any of the extract cells it spans is covered.
It is `nodata` in every base grid only when none of them is. Off-grid cells,
past the far edge where the extract's count is odd, count as `nodata`.
Its `height` is the mean of the covered cells' heights, as
`floor(mean + 0.5)`, the extract's rounding. Its `water` and `surface` are the
largest code among the covered cells, which is the precedence that decides
each layer at a single cell. At 50 m each rule returns the cell unchanged.

Under `omit_sea_tiles` a packed tile whose `water` is entirely sea stores
`height` or `surface` only when that tile differs from what the reader
rebuilds in its place: `height` all 0, `surface` all 3. A tile whose `water`
has no covered cell is stored in no grid.

Each base grid's `params` is `{}` at 50 m. At 100 m it is
`{"resampled_from_m": 50, "rule": "mean"}` for `height`, and the same with
`"rule": "largest"` for `water` and `surface`.

### Alternatives considered

**One fixed cell of the four** (plain decimation, such as the south-west
cell). It is the cheapest rule. It loses because it drops a 50 m river or
road strip half the time, and it moves every code 25 m off its cell's centre.

**Majority vote.** Four cells often split two against two, so it still needs
a tie rule. It also erases a narrow river or road every time.

**`nodata` when any of the four is `nodata`.** This is what bilinear `sample`
returns at that point. It loses because it pulls coverage in by up to 50 m at
the mesh edge. It also leaves `height` disagreeing with `water` there unless
both layers use it.

**Omit every sea tile's `height` and `surface`, as the spec says.** At 100 m
this silently loses the height of islands and coastline that fall inside an
all-sea tile. The spec's own reason for omitting, that it costs nothing, does
not hold there.

## Consequences

At 100 m any water among a coarse cell's four cells makes it water, and any
road or runway among them sets its `surface`. Coastlines and rivers grow by up
to 50 m, and so do the cells that C6 marks `valid = 0` at sea.

At 100 m a few coastal tiles keep `height` and `surface` under an all-sea
`water` tile. A reader uses a stored tile wherever one exists, so the reader's
rules do not change. At 50 m every all-sea tile is still omitted.

Nothing changes at the default 50 m. C5b's done test reads the 100 m grids
back against this rule applied to the synthetic theatre's closed form.
