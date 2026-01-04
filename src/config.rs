//! Configuration management

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum ConnectionQuality {
    Fast,
    #[default]
    Normal,
    Slow,
    VerySlow,
    Custom,
}

impl ConnectionQuality {
    pub fn buffer_seconds(&self, custom: u32) -> u32 {
        match self {
            ConnectionQuality::Fast => 2,
            ConnectionQuality::Normal => 5,
            ConnectionQuality::Slow => 15,
            ConnectionQuality::VerySlow => 30,
            ConnectionQuality::Custom => custom,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub external_player: String,
    #[serde(default = "default_buffer")]
    pub buffer_seconds: u32,
    #[serde(default)]
    pub connection_quality: ConnectionQuality,
    #[serde(default = "default_true")]
    pub dark_mode: bool,
    #[serde(default = "default_font_size")]
    pub font_size: u32,
    #[serde(default)]
    pub selected_user_agent: usize,
    #[serde(default)]
    pub custom_user_agent: String,
    #[serde(default)]
    pub use_custom_user_agent: bool,
    #[serde(default = "default_true")]
    pub pass_user_agent_to_player: bool,
    #[serde(default = "default_true")]
    pub single_window_mode: bool,
    // Saved state
    #[serde(default)]
    pub save_state: bool,
    #[serde(default)]
    pub saved_server: String,
    #[serde(default)]
    pub saved_username: String,
    #[serde(default)]
    pub saved_password: String,
    #[serde(default)]
    pub auto_login: bool,
    // Hardware acceleration
    #[serde(default = "default_true")]
    pub hw_accel: bool,
    // Favorites (stored as JSON)
    #[serde(default)]
    pub favorites_json: String,
    // Recently watched (stored as JSON)
    #[serde(default)]
    pub recent_watched_json: String,
    // EPG settings
    #[serde(default)]
    pub epg_url: String,
    #[serde(default = "default_epg_auto_update")]
    pub epg_auto_update_index: u8,
    #[serde(default)]
    pub epg_time_offset: f32,
}

fn default_buffer() -> u32 { 5 }
fn default_font_size() -> u32 { 12 }
fn default_true() -> bool { true }
fn default_epg_auto_update() -> u8 { 3 } // 1 Day

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            external_player: String::new(),
            buffer_seconds: 5,
            connection_quality: ConnectionQuality::Normal,
            dark_mode: true,
            font_size: 12,
            selected_user_agent: 0,
            custom_user_agent: String::new(),
            use_custom_user_agent: false,
            pass_user_agent_to_player: true,
            single_window_mode: true,
            // Saved state defaults
            save_state: false,
            saved_server: String::new(),
            saved_username: String::new(),
            saved_password: String::new(),
            auto_login: false,
            hw_accel: true,
            favorites_json: String::new(),
            recent_watched_json: String::new(),
            epg_url: String::new(),
            epg_auto_update_index: 3, // 1 Day
            epg_time_offset: 0.0,
        }
    }
}

impl AppConfig {
    fn config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("xtreme_iptv");
        fs::create_dir_all(&path).ok();
        path.push("config.json");
        path
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str(&content) {
                    return config;
                }
            }
        }
        
        Self::default()
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(content) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, content);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedCredential {
    // Server credentials
    pub server: String,
    pub username: String,
    pub password: String,
    // When this entry was saved (Unix timestamp)
    #[serde(default)]
    pub saved_at: i64,
    // Player settings
    #[serde(default)]
    pub external_player: String,
    #[serde(default = "default_buffer")]
    pub buffer_seconds: u32,
    #[serde(default)]
    pub connection_quality: ConnectionQuality,
    // User agent settings
    #[serde(default)]
    pub selected_user_agent: usize,
    #[serde(default)]
    pub custom_user_agent: String,
    #[serde(default)]
    pub use_custom_user_agent: bool,
    #[serde(default = "default_true")]
    pub pass_user_agent_to_player: bool,
    // EPG settings
    #[serde(default)]
    pub epg_url: String,
    #[serde(default)]
    pub epg_time_offset: f32,
    #[serde(default = "default_epg_auto_update")]
    pub epg_auto_update_index: u8,
}

fn address_book_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("xtreme_iptv");
    fs::create_dir_all(&path).ok();
    path.push("address_book.json");
    path
}

pub fn load_address_book() -> Vec<SavedCredential> {
    let path = address_book_path();
    
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(book) = serde_json::from_str(&content) {
                return book;
            }
        }
    }
    
    Vec::new()
}

pub fn save_address_book(book: &[SavedCredential]) {
    let path = address_book_path();
    if let Ok(content) = serde_json::to_string_pretty(book) {
        let _ = fs::write(path, content);
    }
}
