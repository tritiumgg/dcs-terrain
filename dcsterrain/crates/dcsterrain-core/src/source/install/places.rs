//! An install terrain's places: `places`, built from every settlement
//! building of the objects pool at low priority, which takes up to a minute
//! on the largest terrains.

use super::{Scenery, Tables, refusal};
use crate::cache::{Entry, TerrainCache};
use crate::decode::scn5::Pool;
use crate::extract::FileEntry;
use crate::places::{self, Building, Places};
use crate::source::Refusal;

/// Every place of the terrain, named from its towns.
pub struct PlacesEntry;

pub static PLACES: PlacesEntry = PlacesEntry;

/// Wider than any terrain: the walk takes every object of the pool.
const EVERYWHERE: [f64; 4] = [-1e8, -1e8, 1e8, 1e8];

impl Entry for PlacesEntry {
    fn name(&self) -> &str {
        "places"
    }

    /// Raised past 1 with the layout; the classes' rules are folded in, so a
    /// change to which models settle rebuilds it too.
    fn version(&self) -> u32 {
        1 ^ crate::classes::rules_digest()
    }

    /// The scene for the buildings, and the Lua tables for the towns that
    /// name them: a place stores its names as indices into the towns.
    fn roles(&self) -> &[&str] {
        &["scene", "lua"]
    }

    fn build(&self, cache: &TerrainCache, _: &[FileEntry]) -> Result<Vec<u8>, Refusal> {
        let tables = Tables::open(cache)?;
        let towns: Vec<(f64, f64)> = tables.towns.iter().map(|t| (t.x, t.z)).collect();
        // On a thread of its own at low priority, so a build beside a DCS
        // server leaves it the CPU, and the caller's priority is untouched.
        std::thread::scope(|s| {
            s.spawn(|| {
                crate::threads::low_priority();
                let scenery = Scenery::open(cache.install(), cache)?;
                let models = &scenery.index().models;
                let mut all = Vec::new();
                scenery.each_structure_in(Pool::Objects, EVERYWHERE, |i, a| {
                    if let Some(f) = a.family {
                        let m = &models[i.model as usize];
                        all.extend(Building::new(i.id, i.pos[0], i.pos[2], f, &m.bbox));
                    }
                })?;
                all.sort_by_key(|b| b.id);
                all.dedup_by_key(|b| b.id);
                Ok(places::group(&all, &towns).encode())
            })
            .join()
            .unwrap_or_else(|_| Err(Refusal("the places build failed".into())))
        })
    }
}

/// The places, from the cache, built there first if missing or stale.
pub fn read(cache: &TerrainCache) -> Result<Places, Refusal> {
    let opened = cache.get(&PLACES)?;
    let path = opened.path.display().to_string();
    let b = opened.payload().map_err(|e| refusal(&path, e))?;
    Places::decode(&b).map_err(|e| {
        Refusal(format!(
            "{path} cannot be read ({e}); `dcsterrain cache clear --terrain {}` rebuilds it",
            cache.map().theatre
        ))
    })
}
