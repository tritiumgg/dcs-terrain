//! The live validation sortie. `plan` draws stratified samples and records
//! the tool's answer to each; `sortie` turns them into Lua chunks a running
//! DCS evaluates through the bridge; `report` scores DCS's replies and writes
//! `validation.json`.

pub mod plan;
