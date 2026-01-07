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

/// Layout for content lists (Movies, Series)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum ListLayout {
    #[default]
    Single,     // One column
    Double,     // Two columns
    Triple,     // Three columns
    Quad,       // Four columns
}

impl ListLayout {
    pub fn label(&self) -> &'static str {
        match self {
            ListLayout::Single => "1 Column",
            ListLayout::Double => "2 Columns",
            ListLayout::Triple => "3 Columns",
            ListLayout::Quad => "4 Columns",
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            ListLayout::Single => "▤",
            ListLayout::Double => "▥",
            ListLayout::Triple => "▦",
            ListLayout::Quad => "▩",
        }
    }
}

/// Font size options
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum FontSize {
    #[default]
    Default,    // 13px
    Medium,     // 15px
    Large,      // 16px
    XLarge,     // 18px
}

impl FontSize {
    pub fn label(&self) -> &'static str {
        match self {
            FontSize::Default => "Default",
            FontSize::Medium => "Medium",
            FontSize::Large => "Large",
            FontSize::XLarge => "X-Large",
        }
    }
    
    pub fn size(&self) -> f32 {
        match self {
            FontSize::Default => 13.0,
            FontSize::Medium => 15.0,
            FontSize::Large => 16.0,
            FontSize::XLarge => 18.0,
        }
    }
}

/// Sort order for content lists
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum SortOrder {
    #[default]
    Default,      // Server order (as received)
    NameAsc,      // A-Z
    NameDesc,     // Z-A
}

impl SortOrder {
    pub fn label(&self) -> &'static str {
        match self {
            SortOrder::Default => "Default",
            SortOrder::NameAsc => "Name A-Z",
            SortOrder::NameDesc => "Name Z-A",
        }
    }
    
    pub fn cycle(&self) -> Self {
        match self {
            SortOrder::Default => SortOrder::NameAsc,
            SortOrder::NameAsc => SortOrder::NameDesc,
            SortOrder::NameDesc => SortOrder::Default,
        }
    }
    
    pub fn icon(&self) -> &'static str {
        match self {
            SortOrder::Default => "⇅",
            SortOrder::NameAsc => "↑",
            SortOrder::NameDesc => "↓",
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
    #[serde(default)]
    pub epg_show_actual_time: bool,
    #[serde(default = "default_true")]
    pub epg_load_on_startup: bool,
    // Sort settings
    #[serde(default)]
    pub live_sort_order: SortOrder,
    #[serde(default)]
    pub movie_sort_order: SortOrder,
    #[serde(default)]
    pub series_sort_order: SortOrder,
    // UI settings
    #[serde(default = "default_channel_name_width")]
    pub channel_name_width: f32,
    #[serde(default)]
    pub list_layout: ListLayout,
    #[serde(default)]
    pub font_size_setting: FontSize,
}

fn default_buffer() -> u32 { 5 }
fn default_font_size() -> u32 { 12 }
fn default_true() -> bool { true }
fn default_channel_name_width() -> f32 { 200.0 }
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
            epg_show_actual_time: false,
            epg_load_on_startup: true,
            live_sort_order: SortOrder::Default,
            movie_sort_order: SortOrder::Default,
            series_sort_order: SortOrder::Default,
            channel_name_width: 200.0,
            list_layout: ListLayout::Single,
            font_size_setting: FontSize::Default,
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
    #[serde(default)]
    pub epg_show_actual_time: bool,
}

/// Unified playlist entry - can be Xtream API or M3U/XSPF playlist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub name: String,
    pub entry_type: PlaylistType,
    #[serde(default)]
    pub saved_at: i64,
    // Enabled/disabled state
    #[serde(default = "default_true")]
    pub enabled: bool,
    // Auto-login on startup
    #[serde(default)]
    pub auto_login: bool,
    // Playlist auto-update settings (0=Off, 1=1day, 2=2days, etc.)
    #[serde(default)]
    pub auto_update_days: u8,
    #[serde(default)]
    pub last_updated: i64,
    // EPG settings
    #[serde(default)]
    pub epg_url: String,
    #[serde(default)]
    pub epg_time_offset: f32,
    #[serde(default = "default_epg_auto_update")]
    pub epg_auto_update_index: u8,
    #[serde(default)]
    pub epg_show_actual_time: bool,
    #[serde(default)]
    pub epg_last_updated: i64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PlaylistType {
    Xtream {
        server: String,
        username: String,
        password: String,
    },
    M3U {
        url: String,
    },
}

impl PlaylistEntry {
    /// Create a new M3U playlist entry with default settings
    pub fn new_m3u(name: String, url: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        
        Self {
            name,
            entry_type: PlaylistType::M3U { url },
            saved_at: now,
            enabled: true,
            auto_login: false,
            auto_update_days: 0,
            last_updated: now,
            epg_url: String::new(),
            epg_time_offset: 0.0,
            epg_auto_update_index: 3, // default_epg_auto_update
            epg_show_actual_time: false,
            epg_last_updated: 0,
            external_player: String::new(),
            buffer_seconds: 5, // default_buffer
            connection_quality: ConnectionQuality::Normal,
            selected_user_agent: 0,
            custom_user_agent: String::new(),
            use_custom_user_agent: false,
            pass_user_agent_to_player: true,
        }
    }
    
    /// Create a new Xtream playlist entry with default settings
    pub fn new_xtream(name: String, server: String, username: String, password: String) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        
        Self {
            name,
            entry_type: PlaylistType::Xtream { server, username, password },
            saved_at: now,
            enabled: true,
            auto_login: false,
            auto_update_days: 0,
            last_updated: now,
            epg_url: String::new(),
            epg_time_offset: 0.0,
            epg_auto_update_index: 3, // default_epg_auto_update
            epg_show_actual_time: false,
            epg_last_updated: 0,
            external_player: String::new(),
            buffer_seconds: 5, // default_buffer
            connection_quality: ConnectionQuality::Normal,
            selected_user_agent: 0,
            custom_user_agent: String::new(),
            use_custom_user_agent: false,
            pass_user_agent_to_player: true,
        }
    }
}

fn playlist_manager_path() -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("xtreme_iptv");
    fs::create_dir_all(&path).ok();
    path.push("playlists.json");
    path
}

pub fn load_playlist_entries() -> Vec<PlaylistEntry> {
    let path = playlist_manager_path();
    
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(entries) = serde_json::from_str(&content) {
                return entries;
            }
        }
    }
    
    Vec::new()
}

pub fn save_playlist_entries(entries: &[PlaylistEntry]) {
    let path = playlist_manager_path();
    if let Ok(content) = serde_json::to_string_pretty(entries) {
        let _ = fs::write(path, content);
    }
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

fn epg_cache_path(server: &str, username: &str) -> PathBuf {
    let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("xtreme_iptv");
    path.push("epg_cache");
    fs::create_dir_all(&path).ok();
    // Create filename from server+username hash to avoid path issues
    let key = format!("{}_{}", username, server.replace(['/', ':', '.'], "_"));
    path.push(format!("{}.json", key));
    path
}

pub fn save_epg_cache<T: serde::Serialize>(server: &str, username: &str, data: &T) {
    let path = epg_cache_path(server, username);
    // Use non-pretty JSON for smaller file size (EPG can be large)
    if let Ok(content) = serde_json::to_string(data) {
        let _ = fs::write(path, content);
    }
}

pub fn load_epg_cache<T: serde::de::DeserializeOwned>(server: &str, username: &str) -> Option<T> {
    let path = epg_cache_path(server, username);
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(data) = serde_json::from_str(&content) {
                return Some(data);
            }
        }
    }
    None
}
