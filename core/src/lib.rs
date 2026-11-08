//! PvP fighter bots for Horizon servers: the parts that do not need the engine.
//!
//! Settings and presets, the data files (plain YAML), the command
//! grammar, factions, kits, scripts, paths, stats, skins, ghosts and the bot's brain, which turns
//! what a bot perceives into the inputs a player's client would send. The Horizon glue in
//! `../mixins/horizonbot/` feeds it and carries out what it decides.
pub mod settings;
mod settings_table;
pub mod yaml;
pub mod math;
pub mod physics;
pub mod world;
pub mod factions;
pub mod items;
pub mod kits;
pub mod paths;
pub mod scripts;
pub mod skins;
pub mod stats;
pub mod store;
pub mod config;
pub mod command;
pub mod selector;
pub mod brain;
pub mod arena;
pub mod companion;
pub mod ghost;
pub mod messages;
pub mod teleport;
pub mod plugin;
pub mod uuid;
pub mod menu;
