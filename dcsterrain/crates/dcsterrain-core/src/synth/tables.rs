//! The synthetic terrain's tables, as an install's `tables.json` would hold
//! them: one airdrome with its runway, stands, two beacons and a radio, a
//! line of towns and a line of node pairs.

use std::collections::BTreeMap;

use super::*;
use crate::decode::airfield::{Params, RunwayRow, StandRow};
use crate::decode::lua::tables::{BeaconRow, Crs, Frequencies, NodeRow, RadioRow, TownRow, Xz};
use crate::source::Tables;

/// Town `k`'s name, counting from 1.
pub fn town_name(k: u32) -> String {
    format!("Synth Town {k}")
}

/// Node `k`'s name, counting from 1.
pub fn node_name(k: u32) -> String {
    format!("Synth Node {k}")
}

/// Beacon `k`'s id, counting from 1, labeled as `Radio.lua` labels an
/// airdrome's.
pub fn beacon_id(k: u32) -> String {
    format!("airfield{AIRDROME_ID}_{}", k - 1)
}

impl Theatre {
    /// The two runway ends, west (`09`) then east (`27`), in DCS meters.
    pub fn runway_ends(&self) -> [(f64, f64); 2] {
        let half = RUNWAY_LENGTH_M / 2.0;
        [
            self.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M - half),
            self.abs(RUNWAY_NORTH_M, RUNWAY_EAST_M + half),
        ]
    }

    /// Town `k`'s position, counting from 0: a line running north.
    pub fn town(&self, k: u32) -> (f64, f64) {
        let n = TOWNS_FIRST_NORTH_M + f64::from(k) * TOWNS_SPACING_M;
        self.abs(n, TOWNS_FIRST_EAST_M)
    }

    /// Node `k`'s red and blue sides, counting from 0: a line running
    /// north, red to the west and blue to the east.
    pub fn node(&self, k: u32) -> [(f64, f64); 2] {
        let n = NODES_FIRST_NORTH_M + f64::from(k) * NODES_SPACING_M;
        [
            self.abs(n, NODES_FIRST_EAST_M - NODES_SIDE_OFFSET_M),
            self.abs(n, NODES_FIRST_EAST_M + NODES_SIDE_OFFSET_M),
        ]
    }

    pub fn tables(&self) -> Tables {
        let tmerc = crate::crs::Tmerc {
            lon_0: CRS_LON_0_DEG,
            easting: CRS_EASTING_OFFSET_M,
            northing: CRS_NORTHING_OFFSET_M,
        };
        let latlon = |(x, z): (f64, f64)| tmerc.to_latlon(x, z);
        let [w, e] = self.runway_ends();
        let (rx, rz) = ((w.0 + e.0) / 2.0, (w.1 + e.1) / 2.0);
        let (rlat, rlon) = latlon((rx, rz));
        let airdrome = crate::source::install::AirdromeRow {
            id: Some(AIRDROME_ID.into()),
            name_id: AIRDROME_NAME_ID.into(),
            display_name: Some(AIRDROME_DISPLAY_NAME.into()),
            x: Some(rx),
            z: Some(rz),
            lat: Some(rlat),
            lon: Some(rlon),
            runway_names: vec![RUNWAY_EDGE1_NAME.into(), RUNWAY_EDGE2_NAME.into()],
            beacon_ids: (1..=BEACON_COUNT).map(beacon_id).collect(),
            radio_ids: vec![format!("airfield{AIRDROME_ID}_0")],
            roadnet: format!("./{FILE_AIRFIELD_RN4_PATH}"),
            roadnet5: Some(format!("./{FILE_AIRFIELD_RN5_PATH}")),
        };
        let runway = RunwayRow {
            airdrome_id: Some(AIRDROME_ID.into()),
            airdrome_name_id: AIRDROME_NAME_ID.into(),
            name: format!("{RUNWAY_EDGE1_NAME}-{RUNWAY_EDGE2_NAME}"),
            edge1_name: RUNWAY_EDGE1_NAME.into(),
            edge1_x: w.0,
            edge1_z: w.1,
            edge2_name: RUNWAY_EDGE2_NAME.into(),
            edge2_x: e.0,
            edge2_z: e.1,
            course: RUNWAY_COURSE_RAD,
        };
        let stands = (0..STAND_COUNT)
            .map(|k| {
                let (x, z) = self.abs(
                    RUNWAY_NORTH_M + STAND_NORTH_OFFSET_M,
                    RUNWAY_EAST_M - RUNWAY_LENGTH_M / 2.0 + f64::from(k) * STAND_SPACING_M,
                );
                StandRow {
                    airdrome_id: Some(AIRDROME_ID.into()),
                    airdrome_name_id: AIRDROME_NAME_ID.into(),
                    crossroad_index: k,
                    name: format!("{:02}", k + 1),
                    flag: None,
                    x,
                    z,
                    stand_class: Some(STAND_CLASS.into()),
                    heading: STAND_HEADING_RAD,
                    params: Params {
                        SHELTER: None,
                        FOR_HELICOPTERS: Some(true),
                        FOR_AIRPLANES: Some(true),
                        WIDTH: Some(STAND_WIDTH_M),
                        LENGTH: Some(STAND_LENGTH_M),
                        HEIGHT: None,
                    },
                }
            })
            .collect();
        let beacon = |k: u32, (x, z): (f64, f64), hz: u64, channel: Option<u32>| {
            let (lat, lon) = latlon((x, z));
            BeaconRow {
                beacon_id: Some(beacon_id(k)),
                callsign: Some(format!("SY{k}")),
                display_name: Some(AIRDROME_DISPLAY_NAME.into()),
                kind: None,
                frequency_hz: Some(hz as f64),
                channel: channel.map(i64::from),
                direction: None,
                x: Some(x),
                alt: Some(self.ground(x - f64::from(ORIGIN_X_M), z - f64::from(ORIGIN_Z_M))),
                z: Some(z),
                lat: Some(lat),
                lon: Some(lon),
                scene_objects: Vec::new(),
            }
        };
        let beacons = vec![
            beacon(1, w, BEACON_1_FREQUENCY_HZ, Some(BEACON_1_CHANNEL)),
            beacon(2, e, BEACON_2_FREQUENCY_HZ, None),
        ];
        let radio = vec![RadioRow {
            radio_id: Some(format!("airfield{AIRDROME_ID}_0")),
            callsigns: BTreeMap::from([("en".to_string(), AIRDROME_DISPLAY_NAME.to_string())]),
            roles: vec!["ground".into(), "tower".into(), "approach".into()],
            frequencies_hz: Frequencies {
                hf: Some(RADIO_HF_HZ as f64),
                fm: Some(RADIO_FM_HZ as f64),
                vhf: Some(RADIO_VHF_HZ as f64),
                uhf: Some(RADIO_UHF_HZ as f64),
            },
            scene_objects: Vec::new(),
        }];
        let towns = (0..TOWNS_COUNT)
            .map(|k| {
                let (x, z) = self.town(k);
                let (lat, lon) = latlon((x, z));
                TownRow {
                    name: town_name(k + 1),
                    display_name: Some(town_name(k + 1)),
                    lat,
                    lon,
                    x,
                    z,
                }
            })
            .collect();
        let nodes = (0..NODES_COUNT)
            .map(|k| {
                let [r, b] = self.node(k);
                NodeRow {
                    id: Some(i64::from(k + 1)),
                    name: Some(node_name(k + 1)),
                    red: Some(Xz { x: r.0, z: r.1 }),
                    blue: Some(Xz { x: b.0, z: b.1 }),
                }
            })
            .collect();
        Tables {
            airdromes: vec![airdrome],
            runways: vec![runway],
            stands,
            beacons,
            radio,
            towns,
            nodes,
            crs: Crs {
                proj4: CRS_PROJ4.into(),
                lon_0: CRS_LON_0_DEG,
                easting: CRS_EASTING_OFFSET_M,
                northing: CRS_NORTHING_OFFSET_M,
                k_0: CRS_K_0,
                n: None,
                rms_m: None,
                max_m: None,
                source: CRS_SOURCE.into(),
            },
            lua_included: Vec::new(),
            lua_unresolved: Vec::new(),
        }
    }
}
