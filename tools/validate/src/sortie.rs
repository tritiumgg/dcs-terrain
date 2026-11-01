//! Writes the Lua chunks a running DCS evaluates, one batch to a chunk. The
//! bridge bounds a request and a reply, so a chunk writes its reply to a file
//! and returns one line.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use crate::plan::Plan;

const HEIGHTS: usize = 250;
const SIGHTS: usize = 100;
const ROUTES: usize = 25;
const MGRS: usize = 100;

/// What every chunk starts with. Mission-state calls crash DCS unless a
/// terrain is loaded, so nothing runs before the terrain is the planned one,
/// and a figure measured on another build would be stamped with the wrong
/// one.
fn prelude(plan: &Plan, out: &str) -> String {
    format!(
        r#"local TERRAIN, BUILD, OUT = {terrain:?}, {build:?}, {out:?}
local terrain = rawget(_G, "terrain") or require("terrain")
local loaded = terrain.GetTerrainConfig("id")
if loaded ~= TERRAIN then
  error("terrain " .. tostring(loaded) .. " is loaded, not " .. TERRAIN, 0)
end
local build = rawget(_G, "_APP_VERSION")
if build ~= BUILD then
  error("DCS is " .. tostring(build) .. ", the plan is " .. BUILD, 0)
end
local function num(v)
  if type(v) == "number" and v == v and v ~= 1 / 0 and v ~= -1 / 0 then
    return string.format("%.17g", v)
  end
  return "null"
end
local function str(v)
  if v == nil then
    return "null"
  end
  return '"' .. string.gsub(tostring(v), '[%c"\\]', "?") .. '"'
end
local function reply(name, values)
  local f = assert(io.open(OUT .. name, "w"))
  f:write('{{"terrain":', str(loaded), ',"build":', str(build), ',"values":[')
  f:write(table.concat(values, ","), "]}}")
  f:close()
  return name .. ": " .. #values
end
"#,
        terrain = plan.terrain,
        build = plan.dcs_build,
    )
}

fn height_chunk(cases: &[[f64; 2]], name: &str) -> String {
    let mut s = String::from("local cases = {\n");
    for [x, z] in cases {
        let _ = writeln!(s, "{{{x:?},{z:?}}},");
    }
    let _ = write!(
        s,
        r#"}}
local out = {{}}
for i, c in ipairs(cases) do
  out[i] = "[" .. num(terrain.GetHeight(c[1], c[2])) .. ","
    .. str(terrain.GetSurfaceType(c[1], c[2])) .. "]"
end
return reply({name:?}, out)
"#
    );
    s
}

fn sight_chunk(cases: &[([f64; 3], [f64; 3])], name: &str) -> String {
    let mut s = String::from("local cases = {\n");
    for (a, b) in cases {
        let _ = writeln!(
            s,
            "{{{:?},{:?},{:?},{:?},{:?},{:?}}},",
            a[0], a[1], a[2], b[0], b[1], b[2]
        );
    }
    let _ = write!(
        s,
        r#"}}
local out = {{}}
for i, c in ipairs(cases) do
  out[i] = tostring(terrain.isVisible(c[1], c[2], c[3], c[4], c[5], c[6]) and true or false)
end
return reply({name:?}, out)
"#
    );
    s
}

fn route_chunk(cases: &[(String, [f64; 2], [f64; 2])], name: &str) -> String {
    let mut s = String::from("local cases = {\n");
    for (network, a, b) in cases {
        let _ = writeln!(
            s,
            "{{{network:?},{:?},{:?},{:?},{:?}}},",
            a[0], a[1], b[0], b[1]
        );
    }
    let _ = write!(
        s,
        r#"}}
local out = {{}}
for i, c in ipairs(cases) do
  local path = terrain.findPathOnRoads(c[1], c[2], c[3], c[4], c[5])
  if path and #path > 0 then
    local length = 0
    for j = 2, #path do
      local dx, dy = path[j].x - path[j - 1].x, path[j].y - path[j - 1].y
      length = length + math.sqrt(dx * dx + dy * dy)
    end
    local a, b = path[1], path[#path]
    out[i] = "[" .. num(length) .. "," .. #path .. "," .. num(a.x) .. "," .. num(a.y) .. ","
      .. num(b.x) .. "," .. num(b.y) .. "]"
  else
    out[i] = "null"
  end
end
return reply({name:?}, out)
"#
    );
    s
}

/// `coord` lives in the mission state, reached from the hook by
/// `net.dostring_in`, which answers one string. The prelude has already
/// proved a terrain is loaded.
fn mgrs_chunk(cases: &[[f64; 4]], name: &str) -> String {
    let mut server = String::from("local cases = {\n");
    for [x, z, lat, lon] in cases {
        let _ = writeln!(server, "{{{x:?},{z:?},{lat:?},{lon:?}}},");
    }
    server.push_str(
        r#"}
local function ref(m)
  return string.format("%q,%q,%.3f,%.3f", m.UTMZone, m.MGRSDigraph, m.Easting, m.Northing)
end
local out = {}
for i, c in ipairs(cases) do
  local lat, lon = coord.LOtoLL({x = c[1], y = 0, z = c[2]})
  out[i] = string.format("[%.12f,%.12f,", lat, lon) .. ref(coord.LLtoMGRS(lat, lon)) .. ","
    .. ref(coord.LLtoMGRS(c[3], c[4])) .. "]"
end
return table.concat(out, "\n")
"#,
    );
    format!(
        r#"local answer = net.dostring_in("server", [==[
{server}]==])
if type(answer) ~= "string" or string.sub(answer, 1, 1) ~= "[" then
  error("the mission state refused: " .. tostring(answer), 0)
end
local out = {{}}
for line in string.gmatch(answer, "[^\n]+") do
  out[#out + 1] = line
end
return reply({name:?}, out)
"#
    )
}

/// Writes every chunk under `dir`, and returns their names in the order they
/// run.
pub fn write(plan: &Plan, dir: &Path) -> Result<Vec<String>, String> {
    let chunks = dir.join("chunks");
    let replies = dir.join("replies");
    for d in [&chunks, &replies] {
        fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let absolute = fs::canonicalize(&replies).map_err(|e| format!("{}: {e}", replies.display()))?;
    let out = format!(
        "{}/",
        absolute
            .display()
            .to_string()
            .trim_start_matches(r"\\?\")
            .replace('\\', "/")
    );
    let head = prelude(plan, &out);
    let mut names = Vec::new();
    let mut emit = |kind: &str, i: usize, body: &dyn Fn(&str) -> String| -> Result<(), String> {
        let stem = format!("{kind}-{i:03}");
        let path = chunks.join(format!("{stem}.lua"));
        fs::write(&path, format!("{head}{}", body(&format!("{stem}.json"))))
            .map_err(|e| format!("{}: {e}", path.display()))?;
        names.push(stem);
        Ok(())
    };
    for (i, c) in plan.heights.chunks(HEIGHTS).enumerate() {
        let cases: Vec<_> = c.iter().map(|c| [c.x, c.z]).collect();
        emit("heights", i, &|n| height_chunk(&cases, n))?;
    }
    for (i, c) in plan.sights.chunks(SIGHTS).enumerate() {
        let cases: Vec<_> = c.iter().map(|c| (c.a, c.b)).collect();
        emit("sights", i, &|n| sight_chunk(&cases, n))?;
    }
    for (i, c) in plan.routes.chunks(ROUTES).enumerate() {
        let cases: Vec<_> = c
            .iter()
            .map(|c| (c.network.clone(), c.from, c.to))
            .collect();
        emit("routes", i, &|n| route_chunk(&cases, n))?;
    }
    for (i, c) in plan.mgrs.chunks(MGRS).enumerate() {
        let cases: Vec<_> = c.iter().map(|c| [c.x, c.z, c.lat, c.lon]).collect();
        emit("mgrs", i, &|n| mgrs_chunk(&cases, n))?;
    }
    Ok(names)
}
