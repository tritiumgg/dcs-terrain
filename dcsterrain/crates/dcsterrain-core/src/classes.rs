//! Scenery classes over a terrain's model table: every model gets a class,
//! and a building its settlement family, each with what decided it.
//!
//! The class, in order: a hand override for the terrain, else one for every
//! terrain; `effect` for a `smoke#` model, a smoke column 200–600 m tall that
//! is not a structure; `wall` for a box under 1 m thin and 4 m or more long;
//! `misc` under 4 m²; `industrial` by name; else `building`.
//!
//! The family groups settlements: houses, apartments, shops, civic and
//! industrial buildings make a place, and garages, sheds, farm and military
//! buildings do not. It is decided by name, first match wins, and a family
//! of house-like models the name cannot split is split by its box.
//!
//! A terrain's models number from a few hundred to a few thousand, so the
//! classes are made when first asked for, in milliseconds, and never cached.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::decode::scn5::Model;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    Building,
    Industrial,
    Wall,
    Misc,
    Effect,
}

impl Class {
    pub const ALL: [Class; 5] = [
        Class::Building,
        Class::Industrial,
        Class::Wall,
        Class::Misc,
        Class::Effect,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Class::Building => "building",
            Class::Industrial => "industrial",
            Class::Wall => "wall",
            Class::Misc => "misc",
            Class::Effect => "effect",
        }
    }

    /// Whether the object stands on the ground. An `effect` is a volume
    /// drawn in the air, and no height, clearing, placement, count or place
    /// answer reads one.
    pub fn is_structure(self) -> bool {
        self != Class::Effect
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    House,
    Apartment,
    Shop,
    Civic,
    Industrial,
    Outbuilding,
    Farm,
    Military,
    /// A building no rule names.
    Unknown,
}

impl Family {
    /// Whether a building of this family is grouped into places.
    pub fn settles(self) -> bool {
        matches!(
            self,
            Family::House | Family::Apartment | Family::Shop | Family::Civic | Family::Industrial
        )
    }
}

/// What decided a class or a family.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// An override file, by its terrain id or `common`, with its reason.
    Override { file: String, reason: String },
    /// A built-in rule, by what it says of the model.
    Rule(String),
}

impl Source {
    fn rule(s: &str) -> Source {
        Source::Rule(s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Assignment {
    pub class: Class,
    pub class_by: Source,
    /// None for a model that is not a building: a wall, a small or flat
    /// thing, an effect, or an industrial or building class whose name
    /// says it is a vehicle, a bridge, a mast or the like.
    pub family: Option<Family>,
    pub family_by: Source,
}

impl Assignment {
    /// Whether an object of this model is grouped into places.
    pub fn settles(&self) -> bool {
        self.class.is_structure() && self.family.is_some_and(Family::settles)
    }
}

/// The override files, by terrain id; `common` applies to every terrain,
/// and a terrain's own file wins over it.
const OVERRIDES: &[(&str, &str)] = &[
    ("common", include_str!("../classes/common.toml")),
    ("Caucasus", include_str!("../classes/Caucasus.toml")),
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Override {
    class: Class,
    family: Option<Family>,
    reason: String,
}

/// One override file's entries, keyed by the model's lowercase name.
fn overrides(file: &str, text: &str) -> Result<BTreeMap<String, Override>, String> {
    let raw: BTreeMap<String, Override> =
        toml::from_str(text).map_err(|e| format!("{file}.toml: {e}"))?;
    let mut out = BTreeMap::new();
    for (k, v) in raw {
        let key = k.to_ascii_lowercase();
        if out.insert(key, v).is_some() {
            return Err(format!("{file}.toml names {k} twice, in another case"));
        }
    }
    Ok(out)
}

/// Every override file's terrain id.
pub fn override_files() -> impl Iterator<Item = &'static str> {
    OVERRIDES.iter().map(|(t, _)| *t)
}

/// Width across the model's local x, depth across its local z, height.
pub fn dims(bbox: &[f32; 6]) -> (f64, f64, f64) {
    let e = |k: usize| f64::from(bbox[k + 3] - bbox[k]);
    (e(0), e(2), e(1))
}

// The class rules after the overrides. Each name is what the rule says of
// the model, which the catalog shows as the reason.
const EFFECT: &str = "a smoke volume";
const WALL: &str = "thinner than 1 m and 4 m or longer";
const MISC: &str = "under 4 m²";
const INDUSTRIAL: &str = "an industrial name";
const BUILDING: &str = "no other rule";

static INDUSTRIAL_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        "kernkraftwerk|wasserturm|sklad|^angar|batumi_hangar|airbase_airport_tbilisi_hangar|",
        "airbase_hungar|ceh_|ceh$|kotelnaya|boiler|him_bak|^bak_new|toplivo|wodokachka|",
        "water_tower|^tec_|tec_a|gradir|cooling_tower|powerplant|power_plant|power_hub|",
        "power_trans_station|silo|industrial|elevator|zavod|factory|refinery|oilgas|tuapse|",
        "tank|nasos|pump|fuel_storage|liq_tank|concreet_can|korpus_atom|cement|coalmine|",
        "mining_factory|car_plant|bucket_wheel|warehouse|stock$|train_depot|bakery|hangar|",
        "hungar|stancija|tech_new|^f_sklad|safshungar|safsconcreet|dagon|coolingtowers|",
        "akkuyu|aleppo_electrostation|lafarge|limasol_kean",
    ))
    .unwrap()
});

/// A family rule: the family a match gives, none for a model that is not
/// a building, or `None` in the outer option for the size split.
struct FamilyRule {
    family: Option<Option<Family>>,
    says: &'static str,
    pattern: &'static str,
}

const fn rule(family: Option<Family>, says: &'static str, pattern: &'static str) -> FamilyRule {
    FamilyRule {
        family: Some(family),
        says,
        pattern,
    }
}

use Family::*;

/// In order, first match wins: what is not a building comes first, so a
/// bridge named for a town or a tank truck never settles.
const FAMILY_RULES: &[FamilyRule] = &[
    rule(None, "a smoke volume", "^smoke#"),
    rule(
        None,
        "a wreck or debris of a non-building",
        "(small_crash|car_crash|jeep_crash|board_crash|container_crash|truck_crash|road_ramp|\
         beams_crash|generator_crash|t-55_crash|leopard|uhf_|sockpole|vishka|l_39c|btr60|\
         mitsubishi|toyota|hilux|power_trans_line|blast_wall|borderfence|wall_conc|\
         tankcover2_crash|crashcolumm|debris)",
    ),
    rule(Some(Industrial), "a destroyed fuel tank", "azal_oil_tank"),
    rule(
        None,
        "a vehicle, aircraft or vessel",
        "^(gaz[-_][0-9]|kamaz|ural|atz|btr|prw|rsp|rls|vaz|mi-8|an-2|an_26|il76|il-62|ikarus|\
         trolebus|laz-|liaz|teplowoz|elektrowoz|kovatch|tu-134|boeing|airbus|learjet|hawker|\
         yak-52|f-4e|f-5e|mystereiv|vautour|meteor$|md500|a4skyhawk|su17|mig15|gannet|vw[-_]|\
         mrs[-_]|bmw|trabant|wartburg|ifa_|ursus|zil_|maz-|belaz|uaz_|faun_|man_|neoplan|\
         bussing|hanomag|mercedes|kraz|liebher|fire_truck|taxi_omni|hyundai|matiz|honda|\
         ford_bbc|baggage|mhu_|mj1_|prmg|grm_army|t-55|t-34|tank_t|tld_|ikrs|brks|unex|\
         rectifier|schopf|gaz2402|l13_blanik|case_blanik|man_sd|cyprus_cabriolet|\
         racing_yacht|fishing_?boat|yacht|sailingboat|motorboat|tanker_|ro-ro|cruise_ship|\
         passenger|boat|barge|ship)",
    ),
    rule(
        None,
        "a tree, shrub or hedge",
        "^(trees?_|norwayspruce|americanbeech|green_ash|european_beech|lombardypoplar|\
         honey_mesquite|italiancypress|shrub|cypress_oak|caucasus_fir|greenash|junipertree|\
         palm[0-9]|platan|orange$|pine|salsola|mandal|juniper|europeanlinden|europeanaspen|\
         kousa|reddelicious|riverbirch|blackelder|blackgum|azalea|hedgerow|poplar|fir$|spruce|\
         leafwall)",
    ),
    rule(
        None,
        "a bridge, viaduct or tunnel portal",
        "(^|_)(most|mst|mostik|mstik|perehod|bridge|brucke|viaduct|tunnel|tank_bridge|\
         wooden_support|stone_support|rokitunnel|hilltunnel)|^most|^mst",
    ),
    rule(
        None,
        "a crane, derrick, rig or site feature",
        "(^mby_|grober_feldberg|cemetry|directional_approach|kran|crane|gantry|derrick|\
         neftevyshka|oilrig|gas_torch)",
    ),
    rule(
        None,
        "a light or lamp",
        "(light_pole|stop_light|approach_lights|direct_light|taxiway_light|in_pavement|spot_|\
         spotlight|^light|street_lamp|lamp|floodlight|flood_light|light_tower|searchlight|\
         rw_light|road_lights|app_flash|halo_omni|runway_omni|helipad_omni|traffic_light|\
         signaling|rw_semaphore)",
    ),
    rule(
        None,
        "a pole, wire, antenna, navaid, small prop or container",
        "(powertranspole|power_pole|pole_railway|^pole$|tram_pole|polegate|^wire|^tros|stolb|\
         power_line_pole|wooden_pole|^vyshka$|pylon|^mast|antenna|anten|lokator|radar|rsbn|\
         vhf_|vor_dme|^vor|tacan|papisystem|ndb_|direction_finder|localizer|vortac|\
         gsm_sattelite|^course|^zont|canopy_beach|umbrella|^barrel|wood_box|ventilation|\
         conditioner|electric_box|electrical_box|transformer_box|box_new|konteiner|container|\
         cargo|^can$|^bowl$)",
    ),
    rule(
        None,
        "an airfield marking, light or cone",
        "^[a-z0-9]_(block|left|right|frwd|black)$|^(yellow|arrow|course|1_black)|\
         ^(app_|afb_|emb_|stop_|chock|comp_cone|aerfield|airfield_cone|afb_cone)",
    ),
    rule(
        None,
        "a wall, fence, gate or barrier",
        "(wall|safsgate|(^|[^e])fence|vorota|barbed|enclosure|inclosure|bollard|barrier|\
         sandbag|brunofence|concertina|roadblock|road_block|highwayb|antitank|gate_small|\
         ^gate_|irongate|wooden_gate|brick_gate|fortwall|townwall|leaf_wall|pillar|^stone_|\
         checkpoint_road|rw_gate|bordergate|deflector|trap)",
    ),
    rule(
        None,
        "a flag, sign or banner",
        "(flagpole|flag_big|flag$|banner|sign|advertis|logo_|billboard|big_board|dsisign|\
         street_clock|weltzeituhr)",
    ),
    rule(
        None,
        "a monument, statue, grave, pitch or play item",
        "(monument|memorial|statue|sculpture|lenin|eagle|stela|obelisk|column$|victory_column|\
         jesus|brandenburg_gate|bismarck|grave|fountain|well$|bascetball|tennis|football|\
         playground|pool[0-9]*$|aqua[0-9]|swing|slide|carousel|looping|sling_shot|booster|\
         giant_wheel|waterslide|sword)",
    ),
    rule(
        None,
        "a maritime, rail, tower, ruin or dam structure",
        "(volnorez|prichal|breakwater|dok_s|dry_dock|floating_dock|flooded_dock|port_wooden|\
         pier|cargo_platform|platforma_(track|tank|tunguska|btr|msta|ikarus)|wagon|locomotive|\
         railway_carriage|elnya|noria|windgen|solar_panel|bashnja|^bordertower|un_tower|\
         watchtower|military_tower|ohr-vyshka|tele_bash|majak|lighthouse|batumi_fonar|\
         vyshka_airport|berlin_tv|fernsehturm|fernmeldeturm|funkturm|europaturm|\
         transmitter_tower|brocken_transmitter|radio_tower|lattice_tower|colonius|\
         kernkraftwerk_mulheim_karlich_pipe|ruin|ruined|(^|_)dam_|_dam$|^dam1|damm_|sperre|\
         ataturka|aksifat|berdan|truboprovod|haystack|platforma_akacya|railway_platform|\
         mobile_elevator|^tank$|mby_tank|target_|tank_bridge)",
    ),
    rule(
        Some(Military),
        "a military building",
        "^(kazarma|aviashtab|ukrytie|barrack|kaserne|military_building|valentin|bunker|\
         caponier|tankcover|tank_hangar|hangar_millitary|dot_ussr|tent_ussr|wuensdorf|\
         teufelsberg|bunkerhill|concretecover|checkpoint_post|us_border|brocken_radar|\
         brocken_small|airbase_tbilisi_tent|airbase_airport_tbilisi_checkpoint|\
         par_apparatnaya|par_agregatnaya|par_gsm|spandau_wall)",
    ),
    rule(
        Some(Outbuilding),
        "a hut, kiosk, shed or small garage",
        "(tr_budka|transformer_booth|^wc_|toilet|skladik|hozdomik3|garagh-small|^garage_0|\
         saray|cabin|^s[124]_new|rw_shack|closedguardhouse|gatehouse|rw_tower|hozdomik2|\
         airbase_warehouse_01|safsairbase_warehouse|german_rural_21|bunker_small)",
    ),
    rule(
        Some(Outbuilding),
        "a garage block or small shed",
        "(^garage_[ab]_new|industrial_garage|garage_|weather_station|^s3_new|gatehouse_eu_sm)",
    ),
    rule(
        Some(Farm),
        "a farm building",
        "(ferma|zoofarm|barn|greenhouse|granary|korovnik|hozdomik1)",
    ),
    rule(
        Some(Industrial),
        "an industrial or warehouse building",
        "(kernkraftwerk|wasserturm|sklad|^angar|batumi_hangar|airbase_airport_tbilisi_hangar|\
         airbase_hungar|ceh_|ceh$|kotelnaya|boiler|him_bak|^bak_new|toplivo|wodokachka|\
         water_tower|^tec_|tec_a|gradir|cooling_tower|powerplant|power_plant|power_hub|\
         power_trans_station|silo|industrial|elevator|zavod|factory|refinery|oilgas|tuapse|\
         tank|nasos|pump|fuel_storage|liq_tank|concreet_can|bengal_industrial|\
         nevad_vegas_industrial|bagram_industrial|korpus_atom|aleppo_electrostation|akkuyu|\
         cement|lafarge|kean_factory|coalmine|mining_factory|car_plant|bucket_wheel|\
         warehouse|stock$|train_depot|bakery|hangar|hungar|port_hangar|stancija|tech_new|\
         ^f_sklad|safshungar|safsconcreet|limasol_kean|konya_cement)",
    ),
    rule(
        None,
        "a prop, equipment, pad or minor structure",
        "(rotunda|dome_of_the_rock_(well|arch)|tetrapylon|ruined_column|^arc1|konya_park|\
         akrotiri_entrance|street_market|market_big_pillar|parking|fn_support|oldcity_gate|\
         jaffa_gate|golden_gate|damascus_gate|kinghusseingate|hangar_box|^bak12|tesla_rp3f|\
         grm_sp80|heliport|helipad|barrels|tbilisi_block|airbase_vor|ayia_napa|scoreboard|\
         ^sock|^canopy$|ladder|stairway|bed_|interior_|chair|table|shelves|trash|garbage|\
         bench|flower|cart|firewood|heap_|plank|pile|pallet|crate|wheelbarrow|gas_cylinder|\
         vent_|roof_air|air_system|compressor|generator|mobile_|liq_oxygen|liquid_|missile|\
         ammocrate|bomb|nuclear|aircraft_jack|gasoilpipe|ski_grid|slalom|(^|_)tent|balloon|\
         scaffold|lathing|armenian_x|groundcover|surface|oilgas_x|toilet_x)",
    ),
    rule(
        Some(Industrial),
        "a silo or cooling towers",
        "(dagon|haifa_coolingtowers|firetraining_x)",
    ),
    rule(
        Some(Shop),
        "a shop, hotel, office, market or petrol station",
        "(magazin|uniwersam|cafe|bufet|restoran|stolovaya|hotel|dansing|market|gaz_station|\
         gas_station|bengal_comercial|^mall|_mall|mymall|^bank|bank_of|galleria|optimum|avm|\
         burda|shopping|pavilyon|casino|canopy_market|supermarket|kiosk|sheraton|hilton|\
         radisson|intercontinental|park_inn|europa-center|silberturm|westend|city_haus|\
         merkur|anzeigerhochhaus|city_hochhaus|skyscraper|matcal|marganit|hayovel|azrieli|\
         haifa_tower|platinum|beirut_skygate|sama_beirut|fortyfour|sabanchi|damascus_tower|\
         bank|ski_resort|leonardo_plaza|canopy_01_x)",
    ),
    rule(
        Some(Apartment),
        "an apartment block or residential tower",
        "(olympic_residence|^home3_|^f_home3|home5m|home9a|home16|home5[123]_|lap-house|\
         syria_tower|beirut_building|aleppo_tower_building|korpus_?[abd]|^korpus$|^korpus_|\
         f_korpus)",
    ),
    rule(Some(House), "a house", "^(domik|home1|home2_|dom2)"),
    rule(Some(House), "probably a stone house", "^kam_h"),
    rule(
        Some(Outbuilding),
        "a small shelter",
        "(canopy_01|weather_site|gatehouse_x)",
    ),
    rule(
        Some(Unknown),
        "a building no name tells",
        "^(komplex|section_main|star$|triangle|skp_|construction|constraction|cg_building|\
         leipziger_m|fn_support_chair|firetraining)",
    ),
    rule(
        Some(Industrial),
        "an engineering building",
        "^(inz|concord_facil)",
    ),
    FamilyRule {
        family: None,
        says: "a house-like family split by its box",
        pattern: "(balticurban|danish_house|german_(0|rural|town)|german_red_roof|suburb_baltic|\
                  baltic_01|east_city|danish_city|german_city|germanurban|frgurban|israel_|\
                  village_house|syria_house|syria_city_house|syria_block_building|turkey_|\
                  arabian_block|bengal_city_house|house_village|nevada_vegas_house|\
                  red_roof_house|^house_[0-9])",
    },
    rule(
        Some(Industrial),
        "an airfield support building",
        "(airfields_building|airfields_hangar|airfield_hangar|building_23)",
    ),
    rule(
        Some(Civic),
        "a public, religious, transport or cultural building",
        "(school|shkola|bolnica|hospital|polyklinika|klub_|cinema|d_sad|mechet|mosque|minaret|\
         church|kirche|cathedral|basilika|chantry|synagogue|monastery|temple|vokz|vok_sim|\
         r_vok|vokzal|vok|railstation|rw_station|station$|_station$|terminal|airport|\
         city_hall|town_hall|rathaus|stadthaus|stadium|stadion|tribune|tribun|^kdp|_kdp|kdp|\
         control_tower|air_base_tower|airport_building|_tower$|museum|palace|schloss|\
         stadtschloss|castle|citadel|burg_|schwerin|opera|theatre|library|university|\
         ministry|meteo|reichstag|hammershus|fire_station|haus_des|congress|prison|galerie|\
         gallery|kunsthalle|paulskirche|holstentor|hochhaus_rathaus|herrenhausen|stadel|\
         alte_oper|charlottenburg|potsdam|cecilienhof|kassel_|achilleion|leipzig_history|\
         sanctuaire|our_lady|saint_|^st_|armenian|mohammad|orthodox|amphitheatre|mausoleum|\
         bahai|dome_of_the_rock|al_aqsa|old_?town|acre_old|oldcity|abdullah|chukurova|\
         diyarbakir|gaziantep_terminal|adiyaman|ben_gurion|kastrup|tegel|tempelhof|\
         schonefeld|frankfurt_airoport|hamburg_airport|hannover_airport|leipzig-halle|\
         hahn_air|spangdahlem|bitburg|ramstein|tower_frg|hardthohe|bonn_|bremen_|fulda_|\
         hoher_dom|funerary|baalshamin|palmyra_|nicosia_|cyprus_ministry|cyprus_telecom|\
         larnaca_|limassol_general|limasol_new_port|oval$|haifa_|sourasky|rambam|jerusalem|\
         aleppo_city|aleppo_citadel|famagusta|kyrenia|kizkalesi|tripoly|krak|latakia|\
         nevatim_tower|atc_|heichal|habima|tel_aviv_herbert|migdal|tower_david|\
         universityhouse|alalbayt|paphos_(general|terminal)|beirut_deir|amalienborg|\
         hauptbahnhof|central_station|railway_roofing|marienburg|arena|stadyum|observ|\
         damascus_|asad|omejadov|beslan)",
    ),
];

static FAMILY_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    FAMILY_RULES
        .iter()
        .map(|r| Regex::new(r.pattern).unwrap())
        .collect()
});

/// A house-like family the name cannot split, by its footprint and height:
/// tall or large is apartments, low and small houses, and an urban name
/// tips a mid-size box to apartments.
fn split(name: &str, fp: f64, h: f64) -> (Family, &'static str) {
    let urban = ["city", "block", "urban", "town", "east_"]
        .iter()
        .any(|w| name.contains(w));
    if fp < 30.0 {
        (Outbuilding, "a box under 30 m² in a house-like family")
    } else if fp < 100.0 && h >= 14.0 {
        (House, "a tall narrow box in a house-like family")
    } else if h >= 14.0 || (fp > 400.0 && h > 10.0) {
        (Apartment, "a tall or large box in a house-like family")
    } else if urban && h >= 8.5 && fp >= 150.0 {
        (Apartment, "three stories or more in an urban family")
    } else if h <= 9.5 && fp <= 400.0 {
        (House, "a low, small box in a house-like family")
    } else if urban {
        (Apartment, "a mid-size box in an urban family")
    } else {
        (House, "a mid-size box in a house family")
    }
}

/// The first family rule `name` matches.
fn family_by_name(name: &str, fp: f64, h: f64) -> Option<(Option<Family>, &'static str)> {
    let hit = FAMILY_RULES
        .iter()
        .zip(FAMILY_PATTERNS.iter())
        .find(|(_, p)| p.is_match(name))?;
    Some(match hit.0.family {
        Some(f) => (f, hit.0.says),
        None => {
            let (f, says) = split(name, fp, h);
            (Some(f), says)
        }
    })
}

/// A building's family, from its name, else from the name of what it is a
/// wreck of, else from its size.
pub(crate) fn family(name: &str, bbox: &[f32; 6]) -> (Option<Family>, &'static str) {
    let (w, d, h) = dims(bbox);
    let fp = w * d;
    if let Some(f) = family_by_name(name, fp, h) {
        return f;
    }
    if name.contains("crash") {
        let wrecked = name.replace("_crash", "").replace("crash", "");
        if let Some(f) = family_by_name(&wrecked, fp, h) {
            return f;
        }
    }
    if h <= 1.5 && fp <= 400.0 {
        (None, "flat or small, and no name tells")
    } else {
        (Some(Unknown), "no name tells")
    }
}

/// The class the rules give a model no override names.
pub(crate) fn class(name: &str, bbox: &[f32; 6]) -> (Class, &'static str) {
    let (w, d, _) = dims(bbox);
    if name.starts_with("smoke#") {
        (Class::Effect, EFFECT)
    } else if w.min(d) < 1.0 && w.max(d) >= 4.0 {
        (Class::Wall, WALL)
    } else if w * d < 4.0 {
        (Class::Misc, MISC)
    } else if INDUSTRIAL_NAME.is_match(name) {
        (Class::Industrial, INDUSTRIAL)
    } else {
        (Class::Building, BUILDING)
    }
}

/// Every model's class and family, for one terrain.
#[derive(Debug, Clone)]
pub struct Classes {
    pub terrain: String,
    /// By model index.
    pub models: Vec<Assignment>,
    /// The terrain's own override keys naming no model in the table, with
    /// the file of each; a `common` key may name a model only some have.
    pub unmatched: Vec<(String, String)>,
}

impl Classes {
    /// Classes for `terrain`'s model table. The override files are part of
    /// the build and their tests parse them, so a failure here is a bug.
    pub fn new(terrain: &str, models: &[Model]) -> Classes {
        Classes::with(OVERRIDES, terrain, models)
    }

    fn with(files: &[(&str, &str)], terrain: &str, models: &[Model]) -> Classes {
        let mut by_name: HashMap<String, (&str, Override)> = HashMap::new();
        // `common` is read first, so the terrain's own entry replaces it.
        let common = files.iter().filter(|(f, _)| *f == "common");
        let own = files
            .iter()
            .filter(|(f, _)| *f == terrain && *f != "common");
        for (file, text) in common.chain(own) {
            for (k, v) in overrides(file, text).unwrap() {
                by_name.insert(k, (file, v));
            }
        }
        let names: HashSet<String> = models.iter().map(|m| m.name.to_ascii_lowercase()).collect();
        let mut unmatched: Vec<(String, String)> = by_name
            .iter()
            // A common key names a model some terrains lack, so only a
            // terrain's own keys must each name one of its models.
            .filter(|(k, (file, _))| *file != "common" && !names.contains(*k))
            .map(|(k, (file, _))| (file.to_string(), k.clone()))
            .collect();
        unmatched.sort();
        let models = models
            .iter()
            .map(|m| {
                let name = m.name.to_ascii_lowercase();
                let (family, says) = family(&name, &m.bbox);
                let mut a = Assignment {
                    class: Class::Building,
                    class_by: Source::rule(BUILDING),
                    family,
                    family_by: Source::rule(says),
                };
                if let Some((file, o)) = by_name.get(&name) {
                    let by = Source::Override {
                        file: file.to_string(),
                        reason: o.reason.clone(),
                    };
                    a.class = o.class;
                    a.class_by = by.clone();
                    if o.family.is_some() {
                        a.family = o.family;
                        a.family_by = by;
                    }
                } else {
                    let (c, says) = class(&name, &m.bbox);
                    a.class = c;
                    a.class_by = Source::rule(says);
                }
                // Only a building has a family; an override that names one
                // says the model is a building whatever its class.
                if !matches!(a.class, Class::Building | Class::Industrial)
                    && !matches!(a.family_by, Source::Override { .. })
                {
                    a.family = None;
                    a.family_by = Source::rule("not a building class");
                }
                a
            })
            .collect();
        Classes {
            terrain: terrain.to_string(),
            models,
            unmatched,
        }
    }

    pub fn of(&self, model: u32) -> &Assignment {
        &self.models[model as usize]
    }
}

/// One model in the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Row {
    pub model: String,
    /// Objects placing it, group records included.
    pub count: u64,
    #[serde(flatten)]
    pub assignment: Assignment,
    pub obb_w: f64,
    pub obb_d: f64,
    pub height: f64,
}

/// A terrain's models with their classes, most placed first, so a new
/// terrain's top models can be reviewed in minutes.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Catalog {
    pub terrain: String,
    pub rows: Vec<Row>,
    /// Objects of each class.
    pub counts_by_class: BTreeMap<Class, u64>,
    /// The terrain's own override keys naming no model: a model renamed or
    /// removed.
    pub unmatched: Vec<(String, String)>,
}

impl Catalog {
    /// `counts` by model index, as the objects pool lists them.
    pub fn new(classes: &Classes, models: &[Model], counts: &[u64]) -> Catalog {
        let mut rows: Vec<Row> = models
            .iter()
            .zip(&classes.models)
            .zip(counts)
            .map(|((m, a), &count)| {
                let (obb_w, obb_d, height) = dims(&m.bbox);
                Row {
                    model: m.name.clone(),
                    count,
                    assignment: a.clone(),
                    obb_w,
                    obb_d,
                    height,
                }
            })
            .collect();
        rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.model.cmp(&b.model)));
        let mut counts_by_class = BTreeMap::new();
        for r in &rows {
            *counts_by_class.entry(r.assignment.class).or_insert(0) += r.count;
        }
        Catalog {
            terrain: classes.terrain.clone(),
            rows,
            counts_by_class,
            unmatched: classes.unmatched.clone(),
        }
    }

    /// The `n` tallest models placed at least once, tallest first.
    pub fn tallest(&self, n: usize) -> Vec<&Row> {
        let mut v: Vec<&Row> = self.rows.iter().filter(|r| r.count > 0).collect();
        v.sort_by(|a, b| {
            b.height
                .total_cmp(&a.height)
                .then_with(|| a.model.cmp(&b.model))
        });
        v.truncate(n);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model `w` across, `d` deep and `h` tall, centered on its origin.
    fn model(name: &str, w: f32, d: f32, h: f32) -> Model {
        Model {
            name: name.to_string(),
            bbox: [-w / 2.0, 0.0, -d / 2.0, w / 2.0, h, d / 2.0],
        }
    }

    /// The catalog row of one model alone on `terrain`.
    fn row(terrain: &str, m: Model) -> Row {
        let models = [m];
        let c = Classes::new(terrain, &models);
        Catalog::new(&c, &models, &[1]).rows.remove(0)
    }

    fn by_rule(r: &Row, says: &str) -> bool {
        r.assignment.class_by == Source::rule(says)
    }

    #[test]
    fn each_class_rule_takes_its_models_and_leaves_the_rest() {
        // (model, the rule's class and reason, a near miss)
        let cases = [
            (
                model("smoke#column_big", 60.0, 60.0, 300.0),
                Class::Effect,
                EFFECT,
                model("smokestack", 60.0, 60.0, 300.0),
            ),
            (
                model("plain_a", 0.5, 10.0, 2.0),
                Class::Wall,
                WALL,
                model("plain_a", 1.0, 10.0, 2.0),
            ),
            (
                model("plain_b", 1.9, 2.0, 2.0),
                Class::Misc,
                MISC,
                model("plain_b", 2.0, 2.0, 2.0),
            ),
            (
                model("sklad_new", 41.4, 43.3, 12.0),
                Class::Industrial,
                INDUSTRIAL,
                model("home1_a_new", 41.4, 43.3, 12.0),
            ),
            (
                model("home1_a_new", 7.7, 15.1, 8.0),
                Class::Building,
                BUILDING,
                model("home1_a_new", 0.5, 15.1, 8.0),
            ),
        ];
        for (yes, class, says, no) in cases {
            let r = row("Nowhere", yes.clone());
            assert_eq!(r.assignment.class, class, "{}", yes.name);
            assert!(by_rule(&r, says), "{}: {:?}", yes.name, r.assignment);
            let r = row("Nowhere", no.clone());
            assert!(!by_rule(&r, says), "{}: {:?}", no.name, r.assignment);
            assert_ne!(r.assignment.class, class, "{}", no.name);
        }
    }

    #[test]
    fn a_short_thin_box_is_misc_not_wall() {
        let r = row("Nowhere", model("plain", 0.5, 3.9, 2.0));
        assert_eq!(r.assignment.class, Class::Misc);
    }

    #[test]
    fn a_terrains_override_applies_on_that_terrain_alone() {
        // A Caucasus bridge, misc there; a building by its size elsewhere.
        let bridge = model("most(road)big", 15.0, 141.0, 9.0);
        let r = row("Caucasus", bridge.clone());
        assert_eq!(r.assignment.class, Class::Misc);
        let Source::Override { file, reason } = &r.assignment.class_by else {
            panic!("{:?}", r.assignment);
        };
        assert_eq!(
            (file.as_str(), reason.as_str()),
            ("Caucasus", "road bridge 15 x 141 m")
        );
        let r = row("Syria", bridge);
        assert_eq!(r.assignment.class, Class::Building);
        assert!(by_rule(&r, BUILDING), "{:?}", r.assignment);
        // Its name still says it is no settlement building.
        assert_eq!(r.assignment.family, None);
    }

    #[test]
    fn an_override_matches_any_case_and_a_terrain_wins_over_common() {
        let files = [
            (
                "common",
                r#"
                "Hut" = { class = "misc", reason = "everywhere" }
                "Shed" = { class = "misc", reason = "everywhere" }
                "Barn" = { class = "misc", reason = "not on every terrain" }
                "#,
            ),
            (
                "There",
                r#"
                "HUT" = { class = "industrial", family = "farm", reason = "here" }
                "GONE" = { class = "misc", reason = "renamed" }
                "#,
            ),
        ];
        let models = [
            model("hut", 10.0, 10.0, 4.0),
            model("shed", 10.0, 10.0, 4.0),
        ];
        let c = Classes::with(&files, "There", &models);
        assert_eq!(c.of(0).class, Class::Industrial);
        assert_eq!(c.of(0).family, Some(Family::Farm));
        assert!(matches!(&c.of(0).family_by, Source::Override { file, .. } if file == "There"));
        assert_eq!(c.of(1).class, Class::Misc);
        assert!(matches!(&c.of(1).class_by, Source::Override { file, .. } if file == "common"));
        assert_eq!(c.unmatched, [("There".to_string(), "gone".to_string())]);
        // Elsewhere only common applies.
        let c = Classes::with(&files, "Elsewhere", &models);
        assert_eq!(c.of(0).class, Class::Misc);
        assert!(c.unmatched.is_empty());
    }

    #[test]
    fn every_override_file_parses_and_names_no_smoke_volume() {
        for (file, text) in OVERRIDES {
            let o = overrides(file, text).unwrap();
            for (k, v) in &o {
                assert!(!k.starts_with("smoke#"), "{file}: {k}");
                assert!(!v.reason.is_empty(), "{file}: {k}");
            }
            if *file == "Caucasus" {
                assert!(o.len() > 80, "{}", o.len());
            }
        }
        let e = overrides("X", r#""A" = { class = "tower", reason = "r" }"#).unwrap_err();
        assert!(e.contains("X.toml") && e.contains("tower"), "{e}");
        let twice = "\"A\" = { class = \"misc\", reason = \"r\" }\n\
                     \"a\" = { class = \"misc\", reason = \"r\" }\n";
        let e = overrides("X", twice).unwrap_err();
        assert!(e.contains("twice"), "{e}");
    }

    #[test]
    fn each_family_rule_takes_its_models_and_leaves_the_rest() {
        // A name each rule takes, and one it leaves, in the rules' order.
        let names = [
            ("smoke#a", "smoker_house"),
            ("car_crash", "car_wash"),
            ("azal_oil_tank", "azal_oil"),
            ("gaz-66", "gaz_station"),
            ("pine_01", "apine"),
            ("most(road)big", "almost"),
            ("port_crane", "cranberry"),
            ("street_lamp", "lam_house"),
            ("power_pole", "polesie"),
            ("a_block", "ab_block"),
            ("fence_01", "defence_hq"),
            ("road_sign", "sig"),
            ("lenin_monument", "lemon"),
            ("prichal", "pricha"),
            ("kazarma2", "a_kazarma"),
            ("tr_budka_new", "budka"),
            ("garage_a_new", "garag"),
            ("ferma_a_new", "ferm"),
            ("sklad_new", "skla"),
            ("parking_lot", "park"),
            ("dagon_1", "dago"),
            ("magazin_new", "magaz"),
            ("home3_a", "home4_a"),
            ("home1_a_new", "ahome1"),
            ("kam_h1", "akam_h"),
            ("canopy_01", "canopy_02"),
            ("komplex_1", "akomplex"),
            ("inz_1", "binz"),
            ("german_town_house", "german_cake"),
            ("airfields_building_01", "airfields_b"),
            ("school_1", "schoo"),
        ];
        assert_eq!(names.len(), FAMILY_RULES.len());
        let bbox = model("", 10.0, 10.0, 6.0).bbox;
        // The size split names the branch of the split, not the rule.
        let low = "a low, small box in a house-like family";
        for ((yes, no), rule) in names.iter().zip(FAMILY_RULES) {
            let (f, says) = family(yes, &bbox);
            let decided = match rule.family {
                Some(want) => {
                    assert_eq!((f, says), (want, rule.says), "{yes}");
                    rule.says
                }
                None => {
                    assert_eq!((f, says), (Some(Family::House), low), "{yes}");
                    low
                }
            };
            let (_, says) = family(no, &bbox);
            assert_ne!(says, decided, "{no}");
        }
        assert_eq!(family("german_cake", &bbox).1, "no name tells");
    }

    #[test]
    fn a_wreck_takes_the_family_of_what_it_was() {
        let bbox = model("", 2.0, 2.0, 4.0).bbox;
        assert_eq!(
            family("column_crash", &bbox).1,
            "a monument, statue, grave, pitch or play item"
        );
        assert_eq!(family("column_cras", &bbox).1, "no name tells");
    }

    #[test]
    fn a_building_no_name_tells_is_unknown_unless_flat_and_small() {
        let tall = model("", 10.0, 10.0, 6.0).bbox;
        let flat = model("", 10.0, 10.0, 1.5).bbox;
        let wide = model("", 30.0, 30.0, 1.5).bbox;
        assert_eq!(
            family("zzz", &tall),
            (Some(Family::Unknown), "no name tells")
        );
        assert_eq!(family("zzz", &flat).0, None);
        assert_eq!(family("zzz", &wide).0, Some(Family::Unknown));
    }

    #[test]
    fn a_house_like_family_splits_by_its_box() {
        // (name, footprint side, height, family)
        let cases = [
            ("german_town_a", 5.0, 6.0, Family::Outbuilding),
            ("german_town_a", 9.0, 14.0, Family::House),
            ("german_town_a", 15.0, 14.0, Family::Apartment),
            ("german_town_a", 21.0, 10.5, Family::Apartment),
            ("german_town_a", 13.0, 8.5, Family::Apartment),
            ("german_rural_a", 13.0, 8.5, Family::House),
            ("german_rural_a", 20.0, 9.5, Family::House),
            ("german_rural_a", 21.0, 9.5, Family::House),
            ("german_city_a", 21.0, 9.5, Family::Apartment),
        ];
        for (name, side, h, want) in cases {
            let (f, says) = family(name, &model("", side, side, h).bbox);
            assert_eq!(f, Some(want), "{name} {side} {h}: {says}");
        }
    }

    #[test]
    fn only_a_building_class_has_a_family() {
        let c = Classes::new(
            "Nowhere",
            &[
                model("home1_a_new", 0.5, 15.0, 8.0),
                model("smoke#home1", 60.0, 60.0, 300.0),
                model("home1_a_new", 8.0, 15.0, 8.0),
                model("sklad_new", 41.0, 43.0, 12.0),
            ],
        );
        assert_eq!(c.of(0).family, None);
        assert_eq!(c.of(1).family, None);
        assert!(!c.of(1).settles());
        assert_eq!(c.of(2).family, Some(Family::House));
        assert!(c.of(2).settles());
        assert_eq!(c.of(3).family, Some(Family::Industrial));
        assert!(c.of(3).settles());
    }

    #[test]
    fn the_catalog_lists_the_most_placed_first_and_the_tallest() {
        let models = [
            model("home1_a_new", 8.0, 15.0, 8.0),
            model("smoke#a", 60.0, 60.0, 300.0),
            model("blk_light_pole", 0.5, 0.5, 10.0),
            model("tele_bash_new", 20.0, 20.0, 180.0),
        ];
        let c = Classes::new("Nowhere", &models);
        let cat = Catalog::new(&c, &models, &[50, 7, 900, 0]);
        let order: Vec<&str> = cat.rows.iter().map(|r| r.model.as_str()).collect();
        assert_eq!(
            order,
            ["blk_light_pole", "home1_a_new", "smoke#a", "tele_bash_new"]
        );
        // A model placed nowhere is not among the tallest.
        let tall: Vec<&str> = cat.tallest(2).iter().map(|r| r.model.as_str()).collect();
        assert_eq!(tall, ["smoke#a", "blk_light_pole"]);
        assert_eq!(cat.counts_by_class[&Class::Misc], 900);
        assert_eq!(cat.counts_by_class[&Class::Building], 50);
        assert_eq!(cat.counts_by_class[&Class::Effect], 7);
        let v = serde_json::to_value(&cat).unwrap();
        assert_eq!(v["rows"][0]["class"], "misc");
        assert_eq!(v["rows"][0]["class_by"]["rule"], MISC);
        assert_eq!(v["counts_by_class"]["effect"], 7);
    }
}
