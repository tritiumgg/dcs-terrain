//! An install tree the test builds: headers only, but for a surface file of
//! one block of two triangles, a road across it, and on one terrain a scene
//! over it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use dcsterrain_core::decode::rn4::build as rn4;
use dcsterrain_core::decode::scn5::build as scene;
use dcsterrain_core::decode::surface5::build::{Block, file, tri, u32s, vec3};

/// A fresh directory under the system temp, removed when dropped.
pub struct Tree(pub PathBuf);

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn put(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// A container header as every terrain file opens with: container version
/// 2, header size 0x30, a data base, sixteen zero bytes, then the class name
/// with its length and the class's format version.
pub fn header(class: &str, version: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&2u32.to_le_bytes());
    b.extend_from_slice(&0x30u32.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(&[0; 16]);
    b.extend_from_slice(&(class.len() as u32).to_le_bytes());
    b.extend_from_slice(class.as_bytes());
    b.extend_from_slice(&version.to_le_bytes());
    b
}

pub fn terrain(root: &Path, folder: &str, id: &str, scene_version: u32) {
    let m = format!("Mods/terrains/{folder}");
    let entry = format!(
        "theatre =\n{{\n\t['update_id'] = \"{}\";\n\t['id'] = \"{id}\";\n}}\n",
        id.to_uppercase()
    );
    put(root, &format!("{m}/entry.lua"), entry.as_bytes());
    let files = [
        ("Surface", "surface5", "landscape5::Surface5File", 5),
        ("roads", "rn4", "landscape4::lRoadNetwork", 5),
        ("Scenes", "scn5", "landscape5::Scene5File", scene_version),
    ];
    for (dir, ext, class, version) in files {
        let bytes = match ext {
            "surface5" => surface(),
            "scn5" if version == 9 => scenery(),
            "rn4" => roads(),
            _ => header(class, version),
        };
        put(root, &format!("{m}/{dir}/{folder}.{ext}"), &bytes);
    }
}

/// A road network of one road across the mesh.
pub fn roads() -> Vec<u8> {
    let road = rn4::Edge {
        kind: dcsterrain_core::decode::rn4::KIND_ROAD,
        class: 0,
        a: 1,
        b: 2,
        points: vec![[100.0, 10.0, 500.0], [900.0, 10.0, 500.0]],
    };
    rn4::file(&["road"], &[road])
}

/// A scene of one house over the mesh, in the objects pool, and nothing in
/// the others but a tree and a pole beside it.
pub fn scenery() -> Vec<u8> {
    scene::Scene {
        version: 9,
        models: vec![
            ("house", [-5.0, 0.0, -5.0, 5.0, 8.0, 5.0]),
            ("oak", [0.0; 6]),
        ],
        nodes: vec![scene::Node {
            t: [500.0, 0.0, 500.0],
            p: vec![[0.0, 10.0, 0.0], [20.0, 10.0, 0.0], [0.0, 10.0, 20.0]],
            model: vec![0, 1, 1],
            axis: vec![[1.0, 0.0]; 3],
        }],
        group: Vec::new(),
        pools: [vec![(0, 0)], vec![(0, 2)], vec![(0, 1)]],
        shift: None,
    }
    .file()
}

/// A mesh of one block: two triangles over a kilometre square at the
/// origin, flat at 10 m.
pub fn surface() -> Vec<u8> {
    let v = [
        [0.0, 10.0, 0.0],
        [1000.0, 10.0, 0.0],
        [0.0, 10.0, 1000.0],
        [1000.0, 10.0, 1000.0],
    ];
    let block = Block {
        t: [0.0; 3],
        bbox: [0.0, 0.0, 0.0, 1000.0, 100.0, 1000.0],
        arrays: vec![
            u32s("TRITYPE", &[0, 0]),
            tri(&[[0, 1, 2], [1, 3, 2]], None),
            vec3("Pbase", &v),
        ],
    };
    file(&[block], 1)
}

/// A fresh, empty directory under the system temp.
pub fn scratch() -> Tree {
    static N: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dcsterrain-cli-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    Tree(dir)
}

/// An install holding Caucasus, and Germany under a folder that is not its
/// id, whose scene file states a version no decoder reads when `bad`.
pub fn install(bad_scene: bool) -> Tree {
    let tree = scratch();
    let dir = tree.0.clone();
    put(
        &dir,
        "autoupdate.cfg",
        br#"{"version": "0.0.0.1", "timestamp": "20000101-000000"}"#,
    );
    terrain(&dir, "Caucasus", "Caucasus", 9);
    terrain(
        &dir,
        "GermanyColdWar",
        "GermanyCW",
        if bad_scene { 10 } else { 9 },
    );
    tree
}
