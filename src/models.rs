//! Data models for Xtreme IPTV Player

use serde::{Deserialize, Serialize};

/// UI Tab selection
#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    Live,
    Movies,
    Series,
    Favorites,
    Recent,
    Info,
    Console,
}

/// Navigation breadcrumb levels
#[derive(Debug, Clone)]
pub enum NavigationLevel {
    Categories,
    Channels(String),   // category name
    Series(String),     // series name
    Seasons(i64),       // series_id
    Episodes(i64, i32), // series_id, season_num
}

/// Channel/Stream information
#[derive(Debug, Clone)]
pub struct Channel {
    pub name: String,
    pub url: String,
    pub stream_id: Option<i64>,
    pub category_id: Option<String>,
    pub epg_channel_id: Option<String>,
    pub stream_icon: Option<String>,
    pub series_id: Option<i64>,
    pub container_extension: Option<String>,
}

/// User account information
#[derive(Debug, Clone, Default)]
pub struct UserInfo {
    pub username: String,
    pub password: String,
    pub status: String,
    pub max_connections: String,
    pub active_connections: String,
    pub is_trial: bool,
    pub expiry: String,
    pub created_at: String,
}

/// Server information
#[derive(Debug, Clone, Default)]
pub struct ServerInfo {
    pub url: String,
    pub port: String,
    pub timezone: String,
}

/// Favorite item (persisted to JSON)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FavoriteItem {
    pub name: String,
    pub url: String,
    pub stream_type: String, // "live", "movie", "series"
    pub stream_id: Option<i64>,
    pub series_id: Option<i64>,
    pub category_name: String,
    pub container_extension: Option<String>,
}
