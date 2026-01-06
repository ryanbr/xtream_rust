//! Xtreme IPTV Player - Rust Edition
//! A cross-platform IPTV player with Xtream Codes API support

// Hide console window on Windows release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Use mimalloc for faster memory allocation (Linux, macOS)
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use eframe::egui;
use std::collections::HashMap;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::io::{BufRead, BufReader};

mod api;
mod config;
mod models;
mod m3u_parser;
mod xspf_parser;
mod epg;
mod ffmpeg_player;

use api::*;
use config::*;
use models::*;
use ffmpeg_player::PlayerWindow;
use epg::{EpgData, EpgAutoUpdate, EpgDownloader, DownloadConfig, Program};

// Re-export ConnectionQuality for use in main

/// Case-insensitive substring check without allocation
fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() { return true; }
    if needle.len() > haystack.len() { return false; }
    
    haystack.as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

/// Get current time as HH:MM:SS (UTC)
fn timestamp_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let secs = now % 86400;
    format!("{:02}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// Load application icon - matches assets/icon.svg design
fn load_icon() -> egui::IconData {
    let size: usize = 64;
    let mut rgba = vec![0u8; size * size * 4];
    
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            
            // Normalize coordinates to 0.0-1.0
            let nx = x as f32 / size as f32;
            let ny = y as f32 / size as f32;
            
            // Rounded rectangle check (background)
            let corner_radius = 0.125; // ~8px on 64px
            let in_rounded_rect = {
                let dx = if nx < corner_radius { corner_radius - nx } 
                         else if nx > 1.0 - corner_radius { nx - (1.0 - corner_radius) } 
                         else { 0.0 };
                let dy = if ny < corner_radius { corner_radius - ny } 
                         else if ny > 1.0 - corner_radius { ny - (1.0 - corner_radius) } 
                         else { 0.0 };
                dx * dx + dy * dy <= corner_radius * corner_radius
            };
            
            if !in_rounded_rect {
                // Transparent outside rounded rect
                rgba[idx] = 0;
                rgba[idx + 1] = 0;
                rgba[idx + 2] = 0;
                rgba[idx + 3] = 0;
                continue;
            }
            
            // Purple gradient background (#667eea to #764ba2)
            let gradient_t = nx * 0.5 + ny * 0.5;
            let r = (102.0 + (118.0 - 102.0) * gradient_t) as u8;  // 102 -> 118
            let g = (126.0 + (75.0 - 126.0) * gradient_t) as u8;   // 126 -> 75
            let b = (234.0 + (162.0 - 234.0) * gradient_t) as u8;  // 234 -> 162
            
            // TV screen area (centered rectangle)
            let screen_left = 0.15;
            let screen_right = 0.85;
            let screen_top = 0.18;
            let screen_bottom = 0.65;
            let in_screen = nx >= screen_left && nx <= screen_right && ny >= screen_top && ny <= screen_bottom;
            
            // Play button triangle (center of screen)
            let play_cx = 0.45;
            let play_cy = 0.40;
            let in_play = {
                let px = nx - play_cx;
                let py = ny - play_cy;
                px >= 0.0 && px <= 0.15 && py.abs() <= px * 0.8
            };
            
            // Stand
            let in_stand_top = nx >= 0.40 && nx <= 0.60 && ny >= 0.68 && ny <= 0.72;
            let in_stand_bottom = nx >= 0.35 && nx <= 0.65 && ny >= 0.73 && ny <= 0.78;
            
            // X letter at bottom
            let x_cx = 0.5;
            let x_cy = 0.88;
            let x_size = 0.08;
            let dx = (nx - x_cx).abs();
            let dy = (ny - x_cy).abs();
            let in_x = dx < x_size && dy < x_size && ((dx - dy).abs() < 0.025 || (dx + dy - x_size).abs() < 0.025);
            
            if in_screen && !in_play {
                // Dark screen (#1a1a2e)
                rgba[idx] = 26;
                rgba[idx + 1] = 26;
                rgba[idx + 2] = 46;
                rgba[idx + 3] = 255;
            } else if in_play {
                // Play button (purple #667eea)
                rgba[idx] = 102;
                rgba[idx + 1] = 126;
                rgba[idx + 2] = 234;
                rgba[idx + 3] = 255;
            } else if in_stand_top || in_stand_bottom {
                // Stand (#2d3748)
                rgba[idx] = 45;
                rgba[idx + 1] = 55;
                rgba[idx + 2] = 72;
                rgba[idx + 3] = 255;
            } else if in_x {
                // White X
                rgba[idx] = 255;
                rgba[idx + 1] = 255;
                rgba[idx + 2] = 255;
                rgba[idx + 3] = 255;
            } else {
                // Background gradient
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = 255;
            }
        }
    }
    
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

/// Background task messages
enum TaskResult {
    CategoriesLoaded {
        live: Vec<Category>,
        movies: Vec<Category>,
        series: Vec<Category>,
    },
    UserInfoLoaded {
        user_info: UserInfo,
        server_info: ServerInfo,
    },
    ChannelsLoaded(Vec<Channel>),
    SeriesListLoaded(Vec<SeriesInfo>),
    SeasonsLoaded(Vec<i32>),
    EpisodesLoaded(Vec<Episode>),
    PlaylistLoaded {
        channels: Vec<Channel>,
        playlist_name: Option<String>,
    },
    PlaylistReloaded {
        channels: Vec<Channel>,
        playlist_name: String,
    },
    // Favorites series viewing
    FavSeasonsLoaded(Vec<i32>),
    FavEpisodesLoaded(Vec<Episode>),
    Error(String),
    PlayerLog(String),
    PlayerExited { code: Option<i32>, stderr: String },
    // EPG loading results
    EpgLoading { progress: String },
    EpgLoaded { data: Box<EpgData> },
    EpgError(String),
}

/// Context for background fetch operations - avoids cloning credentials repeatedly
struct FetchContext {
    server: String,
    username: String,
    password: String,
    user_agent: String,
    use_post: bool,
    sender: std::sync::mpsc::Sender<TaskResult>,
}

impl FetchContext {
    fn client(&self) -> XtreamClient {
        XtreamClient::new(&self.server, &self.username, &self.password)
            .with_user_agent(&self.user_agent)
            .with_post_method(self.use_post)
    }
}

// Predefined user agents
const USER_AGENTS: &[(&str, &str)] = &[
    ("Chrome (Windows)", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36"),
    ("Firefox (Windows)", "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:140.0) Gecko/20100101 Firefox/140.0"),
    ("Safari (macOS)", "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.4 Safari/605.1.15"),
    ("Edge (Windows)", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/138.0.0.0 Safari/537.36 Edg/138.0.3351.83"),
    ("VLC", "VLC/3.0.16 LibVLC/3.0.16"),
    ("Kodi", "Kodi/20.2 (Linux; Android 13; SM-G998B) Android/13 Sys_CPU/armv8a App_Bitness/64 Version/20.2-(20.2.0)-Git:20230626-abc123"),
    ("MX Player", "Dalvik/2.1.0 (Linux; U; Android 13; Pixel 6 Pro Build/TQ2A.230505.002)"),
    // IPTV Smarters variants
    ("IPTV Smarters Pro (Simple)", "IPTVSmartersPro"),
    ("IPTV Smarters Pro (Windows)", "IPTV Smarters Pro/2.2.2.6"),
    ("IPTV Smarters Pro (macOS)", "IPTV Smarters Pro/2.2.2 (Macintosh; Intel Mac OS X)"),
    ("IPTV Smarters (iOS)", "IPTV Smarters/1.0.3 (iPad; iOS 16.6.1; Scale/2.00)"),
    ("IPTV Smarters (Android)", "IPTV Smarters Pro/3.1.5 (Linux; Android 13)"),
    ("IPTV Smarters (Android TV)", "IPTV Smarters Pro/3.0.8 (Linux; Android 10; Android TV)"),
    // Other IPTV apps
    ("TiviMate", "TiviMate/4.7.0 (Linux; Android 13)"),
    ("TiviMate (Fire TV)", "TiviMate/4.7.0 (Linux; Android 9; AFTT Build/NS6264)"),
    ("Wink", "Wink/1.31.1"),
    ("GSE Smart IPTV", "GSE SMART IPTV/8.0 (iOS)"),
    ("Perfect Player", "PerfectPlayer/1.6.1"),
    ("XCIPTV", "XCIPTV/5.0.1 (Linux; Android 12)"),
    ("OTT Navigator", "OTT Navigator/1.6.6 (Linux; Android 11)"),
    ("Ibo Player", "IboPlayer/1.0 (Linux; Android)"),
    // Mobile browsers
    ("Chrome (Android)", "Mozilla/5.0 (Linux; Android 14; Pixel 7 Pro) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36"),
    ("Firefox (Android)", "Mozilla/5.0 (Android 14; Mobile; rv:126.0) Gecko/126.0 Firefox/126.0"),
    ("Safari (iOS)", "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Mobile/15E148 Safari/604.1"),
    // Smart TV
    ("Smart TV (Generic)", "Mozilla/5.0 (SMART-TV; LINUX; Tizen 6.0) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/4.0 Chrome/76.0.3809.146 TV Safari/537.36"),
    ("Fire TV Stick", "Mozilla/5.0 (Linux; Android 7.1.2; AFTMM Build/NS6264) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/59.0.3071.125 Safari/537.36"),
    ("Android TV", "Mozilla/5.0 (Linux; Android 12; Chromecast) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/112.0.0.0 Safari/537.36"),
    // Linux
    ("Firefox (Ubuntu)", "Mozilla/5.0 (X11; Ubuntu; Linux x86_64; rv:126.0) Gecko/20100101 Firefox/126.0"),
    ("Firefox (Fedora)", "Mozilla/5.0 (X11; Fedora; Linux x86_64; rv:126.0) Gecko/20100101 Firefox/126.0"),
    ("Chrome (ChromeOS)", "Mozilla/5.0 (X11; CrOS x86_64 15633.64.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"),
    ("Samsung Internet", "Mozilla/5.0 (Linux; Android 13; SAMSUNG SM-G998B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/24.0 Chrome/124.0.0.0 Mobile Safari/537.36"),
    // Low-level
    ("OkHttp (IPTV)", "okhttp/4.9.3"),
    ("Lavf (FFmpeg)", "Lavf/60.3.100"),
];

fn main() -> Result<(), eframe::Error> {
    // Force X11 backend on Linux before any windowing code runs
    #[cfg(target_os = "linux")]
    {
        std::env::set_var("WINIT_UNIX_BACKEND", "x11");
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    // Load icon from embedded bytes
    let icon = load_icon();

    let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
        .with_inner_size([1250.0, 700.0])
        .with_min_inner_size([1000.0, 550.0])
        .with_icon(icon),
    vsync: true,
    hardware_acceleration: eframe::HardwareAcceleration::Preferred,
    ..Default::default()
};

    eframe::run_native(
        "Xtreme IPTV Player - Rust Edition",
        options,
        Box::new(|cc| {
            // Add emoji font support
            let mut fonts = egui::FontDefinitions::default();
            
            // Load system emoji fonts
            #[cfg(target_os = "windows")]
            {
                // Try to load Segoe UI Emoji (Windows 10/11)
                if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\seguiemj.ttf") {
                    fonts.font_data.insert(
                        "emoji".to_owned(),
                        egui::FontData::from_owned(font_data).into(),
                    );
                    fonts.families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .push("emoji".to_owned());
                }
            }
            
            #[cfg(target_os = "linux")]
            {
                // Try common Linux emoji font paths
                let emoji_paths = [
                    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
                    "/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf",
                    "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
                    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                ];
                
                for path in emoji_paths {
                    if let Ok(font_data) = std::fs::read(path) {
                        fonts.font_data.insert(
                            "emoji".to_owned(),
                            egui::FontData::from_owned(font_data).into(),
                        );
                        fonts.families
                            .entry(egui::FontFamily::Proportional)
                            .or_default()
                            .push("emoji".to_owned());
                        break;
                    }
                }
            }
            
            #[cfg(target_os = "macos")]
            {
                // Try to load Apple Color Emoji
                if let Ok(font_data) = std::fs::read("/System/Library/Fonts/Apple Color Emoji.ttc") {
                    fonts.font_data.insert(
                        "emoji".to_owned(),
                        egui::FontData::from_owned(font_data).into(),
                    );
                    fonts.families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .push("emoji".to_owned());
                }
            }
            
            cc.egui_ctx.set_fonts(fonts);
            
            // Enable dark mode by default
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(IPTVApp::new()))
        }),
    )
}

struct IPTVApp {
    // Login fields
    server: String,
    username: String,
    password: String,
    
    // State
    logged_in: bool,
    current_tab: Tab,
    status_message: String,
    loading: bool,
    
    // Background task channel
    task_receiver: Receiver<TaskResult>,
    task_sender: Sender<TaskResult>,
    
    // Data
    live_categories: Vec<Category>,
    movie_categories: Vec<Category>,
    series_categories: Vec<Category>,
    
    current_channels: Vec<Channel>,
    current_series: Vec<SeriesInfo>,
    current_seasons: Vec<i32>,
    current_episodes: Vec<Episode>,
    
    // Sort settings (persisted)
    live_sort_order: SortOrder,
    movie_sort_order: SortOrder,
    series_sort_order: SortOrder,
    
    // Favorites
    favorites: Vec<FavoriteItem>,
    
    // Favorite series viewing state (for inline seasons/episodes in Favorites tab)
    fav_viewing_series: Option<(i64, String)>, // (series_id, series_name)
    fav_series_seasons: Vec<i32>,
    fav_series_episodes: Vec<Episode>,
    fav_viewing_season: Option<i32>,
    
    // Recently watched (last 20)
    recent_watched: Vec<FavoriteItem>,
    
    navigation_stack: Vec<NavigationLevel>,
    scroll_positions: Vec<f32>,  // Store scroll Y position for each navigation level
    pending_scroll_restore: Option<f32>,  // Scroll position to restore after navigation
    current_scroll_offset: f32,  // Track current scroll offset
    
    // Info
    user_info: UserInfo,
    server_info: ServerInfo,
    
    // Search
    search_query: String,
    
    // Settings
    external_player: String,
    buffer_seconds: u32,
    connection_quality: ConnectionQuality,
    dark_mode: bool,
    use_post_method: bool,
    save_state: bool,
    auto_login: bool,
    auto_login_triggered: bool,
    
    // User Agent
    selected_user_agent: usize,
    custom_user_agent: String,
    use_custom_user_agent: bool,
    pass_user_agent_to_player: bool,
    show_user_agent_dialog: bool,
    
    // Config
    config: AppConfig,
    address_book: Vec<SavedCredential>, // Legacy - kept for migration
    playlist_entries: Vec<PlaylistEntry>, // New unified playlist manager
    show_playlist_manager: bool,
    playlist_name_input: String,
    playlist_url_input: String,
    show_reset_confirm: bool,
    
    // Playlist loading state (M3U/M3U8/XSPF)
    playlist_mode: bool,
    playlist_sources: Vec<(usize, String)>, // (start_index, source_name) for separators
    
    // Console log
    console_log: Vec<String>,
    
    // Player process management
    single_window_mode: bool,
    current_player: Option<std::process::Child>,
    
    // Hardware acceleration
    hw_accel: bool,
    
    // Internal player
    use_internal_player: bool,
    internal_player: PlayerWindow,
    show_internal_player: bool,
    
    // EPG state
    show_epg_dialog: bool,
    epg_url_input: String,
    epg_data: Option<Box<EpgData>>,
    epg_loading: bool,
    epg_status: String,
    epg_progress: f32,
    epg_time_offset: f32,
    epg_auto_update: EpgAutoUpdate,
    epg_last_update: Option<i64>,
    epg_startup_loaded: bool,
    epg_last_ui_refresh: i64,
    epg_show_actual_time: bool, // false = offset mode (Now, +30m), true = actual time (8:00 PM)
    selected_epg_channel: Option<String>,
}

impl Default for IPTVApp {
    fn default() -> Self {
        Self::new()
    }
}

impl IPTVApp {
    fn new() -> Self {
        let config = AppConfig::load();
        let address_book = load_address_book(); // Legacy
        let playlist_entries = load_playlist_entries();
        let (task_sender, task_receiver) = channel();
        
        // Load saved credentials if save_state is enabled
        // Also try to load per-playlist settings from playlist_entries
        let (server, username, password, playlist_settings) = if config.save_state {
            let server = config.saved_server.clone();
            let username = config.saved_username.clone();
            let password = config.saved_password.clone();
            
            // Find matching playlist entry to get per-playlist settings
            let settings = playlist_entries.iter().find(|e| {
                matches!(&e.entry_type, PlaylistType::Xtream { server: s, username: u, .. } 
                    if s == &server && u == &username)
            }).cloned();
            
            (server, username, password, settings)
        } else {
            (String::new(), String::new(), String::new(), None)
        };
        
        // Load favorites from JSON
        let favorites: Vec<FavoriteItem> = if !config.favorites_json.is_empty() {
            serde_json::from_str(&config.favorites_json).unwrap_or_default()
        } else {
            Vec::new()
        };
        
        // Load recent watched from JSON
        let recent_watched: Vec<FavoriteItem> = if !config.recent_watched_json.is_empty() {
            serde_json::from_str(&config.recent_watched_json).unwrap_or_default()
        } else {
            Vec::new()
        };
        
        // Extract values - prefer playlist-specific settings over global config
        let single_window_mode = config.single_window_mode;
        let hw_accel = config.hw_accel;
        
        // Use per-playlist EPG settings if available, otherwise fall back to global config
        let (epg_url, epg_auto_update_index, epg_time_offset, epg_show_actual_time) = 
            if let Some(ref ps) = playlist_settings {
                (
                    if ps.epg_url.is_empty() { config.epg_url.clone() } else { ps.epg_url.clone() },
                    ps.epg_auto_update_index,
                    ps.epg_time_offset,
                    ps.epg_show_actual_time,
                )
            } else {
                (config.epg_url.clone(), config.epg_auto_update_index, config.epg_time_offset, config.epg_show_actual_time)
            };
        
        // Use per-playlist player settings if available
        let (external_player, buffer_seconds, connection_quality) = 
            if let Some(ref ps) = playlist_settings {
                (
                    if ps.external_player.is_empty() { config.external_player.clone() } else { ps.external_player.clone() },
                    ps.buffer_seconds,
                    ps.connection_quality,
                )
            } else {
                (config.external_player.clone(), config.buffer_seconds, config.connection_quality)
            };
        
        // Use per-playlist user agent settings if available
        let (selected_user_agent, custom_user_agent, use_custom_user_agent, pass_user_agent_to_player) = 
            if let Some(ref ps) = playlist_settings {
                (
                    ps.selected_user_agent,
                    ps.custom_user_agent.clone(),
                    ps.use_custom_user_agent,
                    ps.pass_user_agent_to_player,
                )
            } else {
                (config.selected_user_agent, config.custom_user_agent.clone(), config.use_custom_user_agent, config.pass_user_agent_to_player)
            };
        
        Self {
            server,
            username,
            password,
            logged_in: false,
            current_tab: Tab::Live,
            status_message: if config.save_state && config.auto_login { 
                "Auto-login enabled...".to_string() 
            } else { 
                "Ready".to_string() 
            },
            loading: false,
            task_receiver,
            task_sender,
            live_categories: Vec::new(),
            movie_categories: Vec::new(),
            series_categories: Vec::new(),
            current_channels: Vec::new(),
            current_series: Vec::new(),
            current_seasons: Vec::new(),
            current_episodes: Vec::new(),
            live_sort_order: config.live_sort_order,
            movie_sort_order: config.movie_sort_order,
            series_sort_order: config.series_sort_order,
            favorites,
            fav_viewing_series: None,
            fav_series_seasons: Vec::new(),
            fav_series_episodes: Vec::new(),
            fav_viewing_season: None,
            recent_watched,
            navigation_stack: Vec::new(),
            scroll_positions: Vec::new(),
            pending_scroll_restore: None,
            current_scroll_offset: 0.0,
            user_info: UserInfo::default(),
            server_info: ServerInfo::default(),
            search_query: String::new(),
            external_player,
            buffer_seconds,
            connection_quality,
            dark_mode: config.dark_mode,
            use_post_method: false,
            save_state: config.save_state,
            auto_login: config.auto_login,
            auto_login_triggered: false,
            selected_user_agent,
            custom_user_agent,
            use_custom_user_agent,
            pass_user_agent_to_player,
            show_user_agent_dialog: false,
            config,
            address_book,
            playlist_entries,
            show_playlist_manager: false,
            playlist_name_input: String::new(),
            playlist_url_input: String::new(),
            show_reset_confirm: false,
            playlist_mode: false,
            playlist_sources: Vec::new(),
            console_log: vec!["[INFO] Xtreme IPTV Player started".to_string()],
            single_window_mode,
            current_player: None,
            hw_accel,
            use_internal_player: false,
            internal_player: PlayerWindow::new(),
            show_internal_player: false,
            
            // EPG state
            show_epg_dialog: false,
            epg_url_input: epg_url,
            epg_data: None,
            epg_loading: false,
            epg_status: String::new(),
            epg_progress: 0.0,
            epg_time_offset: epg_time_offset,
            epg_auto_update: EpgAutoUpdate::from_index(epg_auto_update_index),
            epg_last_update: None,
            epg_startup_loaded: false,
            epg_last_ui_refresh: 0,
            epg_show_actual_time: epg_show_actual_time,
            selected_epg_channel: None,
        }
    }
    
    fn log(&mut self, message: &str) {
        let timestamp = timestamp_now();
        self.console_log.push(format!("[{}] {}", timestamp, message));
        // Keep last 500 lines
        if self.console_log.len() > 500 {
            self.console_log.remove(0);
        }
    }
    
    fn save_current_state(&mut self) {
        self.config.save_state = self.save_state;
        self.config.auto_login = self.auto_login;
        self.config.external_player = self.external_player.clone();
        self.config.buffer_seconds = self.buffer_seconds;
        self.config.connection_quality = self.connection_quality;
        self.config.dark_mode = self.dark_mode;
        self.config.single_window_mode = self.single_window_mode;
        self.config.hw_accel = self.hw_accel;
        self.config.selected_user_agent = self.selected_user_agent;
        self.config.custom_user_agent = self.custom_user_agent.clone();
        self.config.use_custom_user_agent = self.use_custom_user_agent;
        self.config.pass_user_agent_to_player = self.pass_user_agent_to_player;
        
        // Save EPG settings
        self.config.epg_url = self.epg_url_input.clone();
        self.config.epg_auto_update_index = self.epg_auto_update.to_index();
        self.config.epg_time_offset = self.epg_time_offset;
        self.config.epg_show_actual_time = self.epg_show_actual_time;
        
        // Save favorites
        self.config.favorites_json = serde_json::to_string(&self.favorites).unwrap_or_default();
        
        if self.save_state {
            self.config.saved_server = self.server.clone();
            self.config.saved_username = self.username.clone();
            self.config.saved_password = self.password.clone();
            
            // Also save to playlist_entries if this is an Xtream server
            if !self.server.is_empty() && !self.username.is_empty() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                
                // Create playlist entry with all current settings
                let entry = PlaylistEntry {
                    name: format!("{}@{}", self.username, self.server.split('/').nth(2).unwrap_or(&self.server)),
                    entry_type: PlaylistType::Xtream {
                        server: self.server.clone(),
                        username: self.username.clone(),
                        password: self.password.clone(),
                    },
                    saved_at: now,
                    enabled: true,
                    auto_login: self.auto_login,
                    auto_update_days: 0,
                    last_updated: now,
                    epg_url: self.epg_url_input.clone(),
                    epg_time_offset: self.epg_time_offset,
                    epg_auto_update_index: self.epg_auto_update.to_index(),
                    epg_show_actual_time: self.epg_show_actual_time,
                    epg_last_updated: 0,
                    external_player: self.external_player.clone(),
                    buffer_seconds: self.buffer_seconds,
                    connection_quality: self.connection_quality,
                    selected_user_agent: self.selected_user_agent,
                    custom_user_agent: self.custom_user_agent.clone(),
                    use_custom_user_agent: self.use_custom_user_agent,
                    pass_user_agent_to_player: self.pass_user_agent_to_player,
                };
                
                // Update existing entry or add new one (match by server+username)
                if let Some(existing) = self.playlist_entries.iter_mut().find(|e| {
                    matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                        if server == &self.server && username == &self.username)
                }) {
                    // Keep the existing name, auto_login, auto_update, and epg_last_updated settings
                    let name = existing.name.clone();
                    let auto_login = existing.auto_login;
                    let auto_update_days = existing.auto_update_days;
                    let last_updated = existing.last_updated;
                    let epg_last_updated = existing.epg_last_updated;
                    *existing = entry;
                    existing.name = name;
                    existing.auto_login = auto_login;
                    existing.auto_update_days = auto_update_days;
                    existing.last_updated = last_updated;
                    existing.epg_last_updated = epg_last_updated;
                } else {
                    self.playlist_entries.push(entry);
                }
                save_playlist_entries(&self.playlist_entries);
            }
        } else {
            self.config.saved_server.clear();
            self.config.saved_username.clear();
            self.config.saved_password.clear();
        }
        
        self.config.save();
        self.status_message = "Settings saved".to_string();
    }
    
    /// Reset all settings to defaults
    fn reset_to_defaults(&mut self) {
        // Clear server credentials
        self.server.clear();
        self.username.clear();
        self.password.clear();
        
        // Clear address book (legacy)
        self.address_book.clear();
        save_address_book(&self.address_book);
        
        // Clear playlist entries
        self.playlist_entries.clear();
        save_playlist_entries(&self.playlist_entries);
        
        // Clear loaded playlists
        self.current_channels.clear();
        self.playlist_sources.clear();
        self.playlist_mode = false;
        
        // Clear favorites and recent
        self.favorites.clear();
        self.recent_watched.clear();
        
        // Clear EPG
        self.epg_data = None;
        self.epg_url_input.clear();
        self.epg_last_update = None;
        self.epg_time_offset = 0.0;
        self.epg_auto_update = EpgAutoUpdate::Day1;
        self.epg_show_actual_time = false;
        self.epg_startup_loaded = false;
        self.selected_epg_channel = None;
        
        // Reset player settings to defaults
        self.external_player.clear();
        self.buffer_seconds = 5;
        self.connection_quality = ConnectionQuality::Normal;
        self.hw_accel = true;
        self.single_window_mode = true;
        
        // Reset user agent to defaults
        self.selected_user_agent = 0;
        self.custom_user_agent.clear();
        self.use_custom_user_agent = false;
        self.pass_user_agent_to_player = true;
        self.use_post_method = false;
        
        // Clear current state
        self.live_categories.clear();
        self.movie_categories.clear();
        self.series_categories.clear();
        self.current_channels.clear();
        self.current_series.clear();
        self.current_seasons.clear();
        self.current_episodes.clear();
        self.navigation_stack.clear();
        self.scroll_positions.clear();
        self.playlist_sources.clear();
        self.playlist_mode = false;
        self.logged_in = false;
        
        // Reset config and save
        self.config = AppConfig::default();
        self.config.save();
        
        self.log("All settings reset to defaults");
    }
    
    fn is_favorite(&self, url: &str) -> bool {
        self.favorites.iter().any(|f| f.url == url)
    }
    
    fn toggle_favorite(&mut self, item: FavoriteItem) {
        if let Some(pos) = self.favorites.iter().position(|f| f.url == item.url) {
            let name = self.favorites[pos].name.clone();
            self.favorites.remove(pos);
            self.status_message = format!("Removed '{}' from favorites", name);
        } else {
            self.status_message = format!("Added '{}' to favorites", item.name);
            self.favorites.push(item);
        }
        // Auto-save favorites
        self.config.favorites_json = serde_json::to_string(&self.favorites).unwrap_or_default();
        self.config.save();
    }
    
    fn play_favorite(&mut self, fav: &FavoriteItem) {
        // Series and season favorites are handled inline in favorites tab
        if fav.stream_type == "series" || fav.stream_type == "season" {
            return;
        }
        
        // Handle episode favorites - play directly
        if fav.stream_type == "episode" {
            if let (Some(series_id), Some(stream_id), Some(_season), Some(_ep_num)) = 
                (fav.series_id, fav.stream_id, fav.season_num, fav.episode_num) {
                // Build episode URL directly to avoid navigation dependency
                let container = fav.container_extension.clone().unwrap_or_else(|| "mp4".to_string());
                let url = format!(
                    "{}/series/{}/{}/{}.{}",
                    self.server, self.username, self.password,
                    stream_id, container
                );
                
                // Use the full name stored in favorite (includes series name)
                let channel = Channel {
                    name: fav.name.clone(),
                    url,
                    stream_id: Some(stream_id),
                    category_id: None,
                    epg_channel_id: None,
                    stream_icon: None,
                    series_id: Some(series_id),
                    container_extension: Some(container),
                    playlist_source: fav.playlist_source.clone(),
                };
                
                self.play_channel(&channel);
                return;
            }
        }
        
        // Handle live/movie favorites - play directly
        let channel = Channel {
            name: fav.name.clone(),
            url: fav.url.clone(),
            stream_id: fav.stream_id,
            category_id: None,
            epg_channel_id: None,
            stream_icon: None,
            series_id: fav.series_id,
            container_extension: fav.container_extension.clone(),
            playlist_source: fav.playlist_source.clone(),
        };
        self.play_channel(&channel);
    }
    
    /// Sanitize text by removing unsupported Unicode characters
    /// Keeps ASCII, common Latin, and replaces unsupported chars with spaces
    fn sanitize_text(text: &str) -> String {
        text.chars()
            .map(|c| {
                if c.is_ascii() || 
                   // Common Latin Extended
                   ('\u{00C0}'..='\u{00FF}').contains(&c) ||
                   ('\u{0100}'..='\u{017F}').contains(&c) ||
                   // Common punctuation and symbols that egui supports
                   c == '\u{00B0}' || c == '\u{2122}' || c == '\u{00A9}' || c == '\u{00AE}' ||
                   c == '\u{2013}' || c == '\u{2014}' || c == '\u{2019}' || c == '\u{201C}' || c == '\u{201D}' ||
                   c == '\u{2026}' || c == '\u{2022}' {
                    c
                } else {
                    ' ' // Replace unsupported chars with space
                }
            })
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<&str>>()
            .join(" ") // Collapse multiple spaces
    }

    fn get_effective_buffer(&self) -> u32 {
        self.connection_quality.buffer_seconds(self.buffer_seconds)
    }

    fn get_user_agent(&self) -> String {
        if self.use_custom_user_agent && !self.custom_user_agent.is_empty() {
            self.custom_user_agent.clone()
        } else if self.selected_user_agent < USER_AGENTS.len() {
            USER_AGENTS[self.selected_user_agent].1.to_string()
        } else {
            USER_AGENTS[0].1.to_string()
        }
    }

    fn login(&mut self) {
        if self.server.is_empty() || self.username.is_empty() || self.password.is_empty() {
            self.status_message = "Please fill all fields".to_string();
            return;
        }

        self.status_message = "Logging in...".to_string();
        self.loading = true;
        
        self.log(&format!("[INFO] Attempting login to {}", self.server));
        self.log(&format!("[INFO] User Agent: {}", self.get_user_agent()));

        // Ensure server has protocol
        if !self.server.starts_with("http://") && !self.server.starts_with("https://") {
            self.server = format!("http://{}", self.server);
        }

        // Spawn background thread for login
        let server = self.server.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let user_agent = self.get_user_agent();
        let use_post = self.use_post_method;
        let sender = self.task_sender.clone();

        thread::spawn(move || {
            let client = XtreamClient::new(&server, &username, &password)
                .with_user_agent(&user_agent)
                .with_post_method(use_post);

            // Fetch categories in parallel
            let live_handle = {
                let client = XtreamClient::new(&server, &username, &password)
                    .with_user_agent(&user_agent)
                    .with_post_method(use_post);
                thread::spawn(move || client.get_live_categories())
            };
            
            let movies_handle = {
                let client = XtreamClient::new(&server, &username, &password)
                    .with_user_agent(&user_agent)
                    .with_post_method(use_post);
                thread::spawn(move || client.get_vod_categories())
            };
            
            let series_handle = {
                let client = XtreamClient::new(&server, &username, &password)
                    .with_user_agent(&user_agent)
                    .with_post_method(use_post);
                thread::spawn(move || client.get_series_categories())
            };

            // Wait for all to complete and collect errors
            let live_result = live_handle.join();
            let movies_result = movies_handle.join();
            let series_result = series_handle.join();
            
            // Check for thread panics
            let live = match live_result {
                Ok(Ok(data)) => Some(data),
                Ok(Err(e)) => {
                    let _ = sender.send(TaskResult::Error(format!("Live categories: {}", e)));
                    return;
                }
                Err(_) => {
                    let _ = sender.send(TaskResult::Error("Live categories thread panicked".to_string()));
                    return;
                }
            };
            
            let movies = match movies_result {
                Ok(Ok(data)) => Some(data),
                Ok(Err(e)) => {
                    let _ = sender.send(TaskResult::Error(format!("Movie categories: {}", e)));
                    return;
                }
                Err(_) => {
                    let _ = sender.send(TaskResult::Error("Movie categories thread panicked".to_string()));
                    return;
                }
            };
            
            let series = match series_result {
                Ok(Ok(data)) => Some(data),
                Ok(Err(e)) => {
                    let _ = sender.send(TaskResult::Error(format!("Series categories: {}", e)));
                    return;
                }
                Err(_) => {
                    let _ = sender.send(TaskResult::Error("Series categories thread panicked".to_string()));
                    return;
                }
            };

            if let (Some(live), Some(movies), Some(series)) = (live, movies, series) {
                let _ = sender.send(TaskResult::CategoriesLoaded { live, movies, series });
                
                // Also fetch user info
                if let Ok(info) = client.get_account_info() {
                    let mut user_info = UserInfo::default();
                    let mut server_info = ServerInfo::default();
                    
                    if let Some(user) = info.get("user_info") {
                        user_info.username = user.get("username")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        user_info.password = user.get("password")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        user_info.status = user.get("status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string();
                        user_info.max_connections = user.get("max_connections")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unlimited")
                            .to_string();
                        user_info.active_connections = user.get("active_cons")
                            .and_then(|v| v.as_str())
                            .unwrap_or("0")
                            .to_string();
                        user_info.is_trial = user.get("is_trial")
                            .and_then(|v| v.as_str())
                            .map(|s| s == "1")
                            .unwrap_or(false);
                        
                        if let Some(exp) = user.get("exp_date").and_then(|v| v.as_str()) {
                            if let Ok(ts) = exp.parse::<i64>() {
                                user_info.expiry = format_timestamp(ts);
                            } else {
                                user_info.expiry = "Unlimited".to_string();
                            }
                        }
                    }

                    if let Some(srv) = info.get("server_info") {
                        server_info.url = srv.get("url")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        server_info.port = srv.get("port")
                            .and_then(|v| v.as_str())
                            .unwrap_or("80")
                            .to_string();
                        server_info.timezone = srv.get("timezone")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown")
                            .to_string();
                    }
                    
                    let _ = sender.send(TaskResult::UserInfoLoaded { user_info, server_info });
                }
            }
        });
    }

    /// Helper to create fetch context with all credentials
    fn fetch_context(&self) -> FetchContext {
        FetchContext {
            server: self.server.clone(),
            username: self.username.clone(),
            password: self.password.clone(),
            user_agent: self.get_user_agent(),
            use_post: self.use_post_method,
            sender: self.task_sender.clone(),
        }
    }

    fn fetch_channels(&mut self, category_id: &str, stream_type: &str) {
        self.loading = true;
        self.status_message = "Loading channels...".to_string();
        
        let ctx = self.fetch_context();
        let category_id = category_id.to_string();
        let stream_type = stream_type.to_string();

        thread::spawn(move || {
            let client = ctx.client();
            
            let result = match stream_type.as_str() {
                "live" => client.get_live_streams(&category_id),
                "movie" => client.get_vod_streams(&category_id),
                _ => return,
            };

            if let Ok(streams) = result {
                let channels: Vec<Channel> = streams.into_iter().map(|s| {
                    let ext = s.container_extension.as_deref().unwrap_or(
                        if stream_type == "live" { "ts" } else { "mp4" }
                    );
                    let url = format!(
                        "{}/{}/{}/{}/{}.{}",
                        ctx.server, stream_type, ctx.username, ctx.password,
                        s.stream_id, ext
                    );
                    
                    Channel {
                        name: s.name,
                        url,
                        stream_id: Some(s.stream_id),
                        category_id: s.category_id,
                        epg_channel_id: s.epg_channel_id,
                        stream_icon: s.stream_icon,
                        series_id: None,
                        container_extension: s.container_extension,
                        playlist_source: None, // From Xtream API, not playlist
                    }
                }).collect();
                
                let _ = ctx.sender.send(TaskResult::ChannelsLoaded(channels));
            } else {
                let _ = ctx.sender.send(TaskResult::Error("Failed to load channels".to_string()));
            }
        });
    }

    fn fetch_series_list(&mut self, category_id: &str) {
        self.loading = true;
        self.status_message = "Loading series...".to_string();
        
        let ctx = self.fetch_context();
        let category_id = category_id.to_string();

        thread::spawn(move || {
            let client = ctx.client();
            
            if let Ok(series) = client.get_series(&category_id) {
                let _ = ctx.sender.send(TaskResult::SeriesListLoaded(series));
            } else {
                let _ = ctx.sender.send(TaskResult::Error("Failed to load series".to_string()));
            }
        });
    }

    fn fetch_series_info(&mut self, series_id: i64) {
        self.loading = true;
        self.status_message = "Loading seasons...".to_string();
        
        let ctx = self.fetch_context();

        thread::spawn(move || {
            let client = ctx.client();
            
            if let Ok(info) = client.get_series_info(series_id) {
                if let Some(episodes) = info.get("episodes") {
                    if let Some(obj) = episodes.as_object() {
                        let mut seasons: Vec<i32> = obj.keys()
                            .filter_map(|k| k.parse::<i32>().ok())
                            .collect();
                        seasons.sort();
                        let _ = ctx.sender.send(TaskResult::SeasonsLoaded(seasons));
                        return;
                    }
                }
                let _ = ctx.sender.send(TaskResult::Error("No seasons found".to_string()));
            } else {
                let _ = ctx.sender.send(TaskResult::Error("Failed to load series info".to_string()));
            }
        });
    }

    fn fetch_episodes(&mut self, series_id: i64, season: i32) {
        self.loading = true;
        self.status_message = "Loading episodes...".to_string();
        
        let ctx = self.fetch_context();

        thread::spawn(move || {
            let client = ctx.client();
            
            if let Ok(info) = client.get_series_info(series_id) {
                if let Some(episodes) = info.get("episodes") {
                    if let Some(season_eps) = episodes.get(&season.to_string()) {
                        if let Some(arr) = season_eps.as_array() {
                            let eps: Vec<Episode> = arr.iter().filter_map(|ep| {
                                let id = ep.get("id")?.as_str()?.parse().ok()?;
                                let title = ep.get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Unknown")
                                    .to_string();
                                let episode_num = ep.get("episode_num")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0) as i32;
                                let container = ep.get("container_extension")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("mp4")
                                    .to_string();
                                
                                Some(Episode {
                                    id,
                                    title,
                                    episode_num,
                                    season,
                                    container_extension: container,
                                })
                            }).collect();
                            
                            let _ = ctx.sender.send(TaskResult::EpisodesLoaded(eps));
                            return;
                        }
                    }
                }
                let _ = ctx.sender.send(TaskResult::Error("No episodes found".to_string()));
            } else {
                let _ = ctx.sender.send(TaskResult::Error("Failed to load episodes".to_string()));
            }
        });
    }

    // Fetch series info for favorites tab (doesn't change main navigation)
    fn fetch_fav_series_info(&mut self, series_id: i64) {
        self.loading = true;
        self.status_message = "Loading seasons...".to_string();
        
        let ctx = self.fetch_context();

        thread::spawn(move || {
            let client = ctx.client();
            
            if let Ok(info) = client.get_series_info(series_id) {
                if let Some(episodes) = info.get("episodes") {
                    if let Some(obj) = episodes.as_object() {
                        let mut seasons: Vec<i32> = obj.keys()
                            .filter_map(|k| k.parse().ok())
                            .collect();
                        seasons.sort();
                        let _ = ctx.sender.send(TaskResult::FavSeasonsLoaded(seasons));
                        return;
                    }
                }
                let _ = ctx.sender.send(TaskResult::Error("No seasons found".to_string()));
            } else {
                let _ = ctx.sender.send(TaskResult::Error("Failed to load series".to_string()));
            }
        });
    }

    fn fetch_fav_episodes(&mut self, series_id: i64, season: i32) {
        self.loading = true;
        self.status_message = "Loading episodes...".to_string();
        
        let ctx = self.fetch_context();

        thread::spawn(move || {
            let client = ctx.client();
            
            if let Ok(info) = client.get_series_info(series_id) {
                if let Some(episodes) = info.get("episodes") {
                    if let Some(season_eps) = episodes.get(&season.to_string()) {
                        if let Some(arr) = season_eps.as_array() {
                            let eps: Vec<Episode> = arr.iter().filter_map(|ep| {
                                let id = ep.get("id")?.as_str()?.parse().ok()?;
                                let title = ep.get("title")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Unknown")
                                    .to_string();
                                let episode_num = ep.get("episode_num")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0) as i32;
                                let container = ep.get("container_extension")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("mp4")
                                    .to_string();
                                
                                Some(Episode {
                                    id,
                                    title,
                                    episode_num,
                                    season,
                                    container_extension: container,
                                })
                            }).collect();
                            
                            let _ = ctx.sender.send(TaskResult::FavEpisodesLoaded(eps));
                            return;
                        }
                    }
                }
                let _ = ctx.sender.send(TaskResult::Error("No episodes found".to_string()));
            } else {
                let _ = ctx.sender.send(TaskResult::Error("Failed to load episodes".to_string()));
            }
        });
    }

    fn load_epg(&mut self) {
        let url = self.epg_url_input.trim().to_string();
        if url.is_empty() {
            self.epg_status = "Please enter an EPG URL".to_string();
            return;
        }
        
        self.epg_loading = true;
        self.epg_progress = 0.0;
        self.epg_status = "Starting download...".to_string();
        self.log(&format!("[INFO] Loading EPG from: {}", url));
        
        let sender = self.task_sender.clone();
        let user_agent = self.get_user_agent();
        
        thread::spawn(move || {
            let config = DownloadConfig {
                max_retries: 3,
                retry_delay_ms: 2000,
                connect_timeout_secs: 30,
                read_timeout_secs: 180,
                chunk_size: 64 * 1024,
                user_agent,
            };
            
            // Progress callback sends updates to UI
            let progress_sender = sender.clone();
            let progress_callback: Option<epg::ProgressCallback> = Some(Box::new(move |downloaded, total| {
                let msg = if let Some(total) = total {
                    let pct = (downloaded as f64 / total as f64 * 100.0) as u32;
                    let dl_mb = downloaded as f64 / 1_048_576.0;
                    let total_mb = total as f64 / 1_048_576.0;
                    format!("Downloading: {:.1} / {:.1} MB ({}%)", dl_mb, total_mb, pct)
                } else {
                    let dl_mb = downloaded as f64 / 1_048_576.0;
                    format!("Downloading: {:.1} MB", dl_mb)
                };
                let _ = progress_sender.send(TaskResult::EpgLoading { progress: msg });
            }));
            
            // Download and parse with retry/resume support
            match EpgDownloader::download_and_parse(&url, &config, progress_callback) {
                Ok(epg) => {
                    let _ = sender.send(TaskResult::EpgLoaded { 
                        data: Box::new(epg)
                    });
                }
                Err(e) => {
                    let _ = sender.send(TaskResult::EpgError(e));
                }
            }
        });
    }
    
    fn get_current_program(&self, epg_channel_id: &str) -> Option<&Program> {
        let epg = self.epg_data.as_ref()?;
        let adjusted_now = self.get_adjusted_now();
        
        let programs = epg.programs.get(epg_channel_id)?;
        
        // Binary search for the first program that ends after now
        // Programs are sorted by start time
        let idx = programs.partition_point(|p| p.stop <= adjusted_now);
        
        // Check if this program has started
        programs.get(idx).filter(|p| p.start <= adjusted_now)
    }
    
    /// Get current and next N programs for a channel (with time offset applied)
    fn get_upcoming_programs(&self, epg_channel_id: &str, count: usize) -> Vec<&Program> {
        let Some(epg) = self.epg_data.as_ref() else { return Vec::new() };
        let adjusted_now = self.get_adjusted_now();
        
        let Some(programs) = epg.programs.get(epg_channel_id) else { 
            return Vec::new() 
        };
        
        // Binary search for the first program that ends after now
        let start_idx = programs.partition_point(|p| p.stop <= adjusted_now);
        
        // Take up to 'count' programs from that point
        programs[start_idx..].iter().take(count).collect()
    }
    
    /// Get adjusted "now" timestamp accounting for EPG time offset
    fn get_adjusted_now(&self) -> i64 {
        let offset_secs = (self.epg_time_offset * 3600.0) as i64;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        now - offset_secs
    }
    
    /// Display EPG info inline for a channel (used in Live/Favorites/Recent tabs)
    /// If epg_channel_id is provided, uses it directly. Otherwise looks up by channel name.
    fn show_epg_inline(&self, ui: &mut egui::Ui, channel_name: &str, epg_channel_id: Option<&str>) {
        let Some(ref epg) = self.epg_data else { return };
        
        // Use provided ID or find by name match
        let epg_id: Option<String> = epg_channel_id
            .map(|id| id.to_string())
            .or_else(|| {
                epg.channels.iter()
                    .find(|(_, ch)| {
                        contains_ignore_case(&ch.name, channel_name) ||
                        contains_ignore_case(channel_name, &ch.name)
                    })
                    .map(|(id, _)| id.clone())
            });
        
        let Some(epg_id) = epg_id else { return };
        let Some(program) = self.get_current_program(&epg_id) else { return };
        
        // Truncate title
        let short_title: String = program.title.chars().take(20).collect();
        let display_title = if program.title.len() > 20 {
            format!("{}…", short_title)
        } else {
            short_title
        };
        
        ui.label(" | ");
        ui.label(egui::RichText::new(&display_title)
            .color(egui::Color32::LIGHT_BLUE)
            .italics());
        
        let remaining = (program.stop - self.get_adjusted_now()) / 60;
        if remaining > 0 {
            ui.label(egui::RichText::new(format!("({}m left)", remaining))
                .small()
                .color(egui::Color32::GRAY));
        }
    }

    fn play_channel(&mut self, channel: &Channel) {
        // Add to recently watched
        let category_name = self.navigation_stack.iter().find_map(|n| {
            match n {
                NavigationLevel::Channels(name) => Some(name.clone()),
                NavigationLevel::Series(name) => Some(name.clone()),
                _ => None,
            }
        }).unwrap_or_default();
        
        // Determine stream type
        let stream_type = if channel.series_id.is_some() {
            "series"
        } else if self.current_tab == Tab::Live {
            "live"
        } else {
            "movie"
        };
        
        // Don't reorder if playing from Recent tab
        let reorder = self.current_tab != Tab::Recent;
        
        self.add_to_recent(FavoriteItem {
            name: channel.name.clone(),
            url: channel.url.clone(),
            stream_type: stream_type.to_string(),
            stream_id: channel.stream_id,
            series_id: channel.series_id,
            category_name,
            container_extension: channel.container_extension.clone(),
            season_num: None,
            episode_num: None,
            series_name: None,
            playlist_source: channel.playlist_source.clone(),
        }, reorder);
        
        // Use internal player if enabled OR if user typed "internal" in player field
        let player_lower = self.external_player.to_lowercase();
        let use_internal = self.use_internal_player || player_lower == "internal";
        
        if use_internal {
            return self.play_channel_internal(channel);
        }
        
        // Kill existing player if in single window mode
        if self.single_window_mode {
            if let Some(ref mut child) = self.current_player {
                let _ = child.kill();
                let _ = child.wait(); // Reap the process
            }
            self.current_player = None;
            self.log("[PLAY] Single window mode - closing previous player");
        }
        
        let player = if self.external_player.is_empty() {
            "ffplay".to_string()
        } else {
            self.external_player.clone()
        };
        
        // Auto-detect player paths on Windows
        #[cfg(target_os = "windows")]
        let player = {
            let p = player;
            let p_lower = p.to_lowercase();
            
            if p_lower == "vlc" || p_lower == "vlc.exe" {
                // Check common VLC installation paths
                let paths = [
                    r"C:\Program Files\VideoLAN\VLC\vlc.exe",
                    r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
                ];
                paths.iter()
                    .find(|path| std::path::Path::new(path).exists())
                    .map(|s| s.to_string())
                    .unwrap_or(p)
            } else if p_lower == "mpv" || p_lower == "mpv.exe" {
                let paths = [
                    r"C:\Program Files\mpv\mpv.exe",
                    r"C:\Program Files (x86)\mpv\mpv.exe",
                    r"C:\mpv\mpv.exe",
                ];
                paths.iter()
                    .find(|path| std::path::Path::new(path).exists())
                    .map(|s| s.to_string())
                    .unwrap_or(p)
            } else if p_lower == "ffplay" || p_lower == "ffplay.exe" {
                let paths = [
                    r"C:\ffmpeg\bin\ffplay.exe",
                    r"C:\Program Files\ffmpeg\bin\ffplay.exe",
                ];
                paths.iter()
                    .find(|path| std::path::Path::new(path).exists())
                    .map(|s| s.to_string())
                    .unwrap_or(p)
            } else {
                p
            }
        };
        
        #[cfg(not(target_os = "windows"))]
        let player = player;
        
        self.log(&format!("[PLAY] {} | Player: {}", Self::sanitize_text(&channel.name), player));
        self.log(&format!("[PLAY] URL: {}", channel.url));

        let player_lower = player.to_lowercase();
        let mut cmd = Command::new(&player);
        
        // On Windows, hide the console window for ffplay/ffmpeg
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            if player_lower.contains("ffplay") || player_lower.contains("ffmpeg") {
                cmd.creation_flags(CREATE_NO_WINDOW);
            }
        }
        
        // Get effective buffer based on connection quality
        let buffer_secs = self.get_effective_buffer();
        let buffer_ms = (buffer_secs * 1000) as i64;
        let buffer_bytes = (buffer_secs as i64) * 1024 * 1024; // ~1MB per second
        let buffer_bytes_large = buffer_bytes * 4; // Larger buffer for probing
        let is_slow = matches!(self.connection_quality, ConnectionQuality::Slow | ConnectionQuality::VerySlow);
        
        self.log(&format!("[PLAY] Buffer: {}s | Connection: {:?} | HW Accel: {}", buffer_secs, self.connection_quality, if self.hw_accel { "On" } else { "Off" }));
        
        if player_lower.contains("ffplay") {
            // FFplay settings - simplified for compatibility
            // Note: ffplay takes input directly, not with -i flag
            let mut args = vec![
                channel.url.clone(),  // Input URL first
                "-autoexit".to_string(),
                
                // === BUFFERING ===
                "-probesize".to_string(), format!("{}", buffer_bytes_large),
                "-analyzeduration".to_string(), format!("{}", buffer_ms * 2000), // microseconds
                
                // === SYNC OPTIONS ===
                "-sync".to_string(), "audio".to_string(),
                "-framedrop".to_string(),
            ];
            
            // Window title with stream filename
            let stream_name = channel.url.split('/').last().unwrap_or("stream");
            let title = format!("{} - {}", channel.name, stream_name);
            args.extend(["-window_title".to_string(), title]);
            
            // Add reconnect options for HTTP streams
            if channel.url.starts_with("http") {
                args.extend([
                    "-reconnect".to_string(), "1".to_string(),
                    "-reconnect_streamed".to_string(), "1".to_string(),
                    "-reconnect_delay_max".to_string(), if is_slow { "30".to_string() } else { "10".to_string() },
                ]);
            }
            
            // Infinite buffer for slow connections
            if is_slow {
                args.push("-infbuf".to_string());
            }
            
            // User agent (optional)
            if self.pass_user_agent_to_player {
                args.extend([
                    "-user_agent".to_string(), self.get_user_agent(),
                ]);
            }
            
            // Hardware acceleration - disabled on Windows (black screen with Vulkan renderer)
            // Works on Linux/Mac
            if self.hw_accel {
                #[cfg(target_os = "macos")]
                {
                    args.insert(0, "videotoolbox".to_string());
                    args.insert(0, "-hwaccel".to_string());
                }
                #[cfg(target_os = "linux")]
                {
                    args.insert(0, "auto".to_string());
                    args.insert(0, "-hwaccel".to_string());
                }
                // Windows: skip hwaccel - causes black screen
            }
            
            for arg in args {
                cmd.arg(arg);
            }
        } else if player_lower.contains("mpv") {
            // MPV buffer settings - aggressive for IPTV
            let cache_secs = buffer_secs * 2; // Double cache
            let cache_mb = buffer_secs * 4;   // 4MB per buffer second
            
            // Title with stream filename
            let stream_name = channel.url.split('/').last().unwrap_or("stream");
            let title = format!("{} - {}", channel.name, stream_name);
            
            let mut args = vec![
                channel.url.clone(),
                format!("--title={}", title),
                
                // === CACHE SETTINGS (most important) ===
                "--cache=yes".to_string(),
                format!("--cache-secs={}", cache_secs),
                format!("--demuxer-readahead-secs={}", cache_secs),
                format!("--demuxer-max-bytes={}M", cache_mb),
                format!("--demuxer-max-back-bytes={}M", cache_mb / 2),
                "--cache-pause=yes".to_string(),
                format!("--cache-pause-wait={}", buffer_secs),
                "--cache-pause-initial=yes".to_string(),
                
                // === NETWORK OPTIONS ===
                format!("--network-timeout={}", if is_slow { 120 } else { 60 }),
                "--stream-lavf-o=reconnect=1".to_string(),
                "--stream-lavf-o=reconnect_streamed=1".to_string(),
                "--stream-lavf-o=reconnect_delay_max=30".to_string(),
                format!("--stream-buffer-size={}MiB", buffer_secs * 2),
                
                // === DEMUXER OPTIONS ===
                "--demuxer-lavf-probe-info=yes".to_string(),
                format!("--demuxer-lavf-analyzeduration={}", buffer_ms / 1000),
                format!("--demuxer-lavf-probesize={}", buffer_bytes_large),
                "--demuxer-lavf-o=fflags=+genpts+discardcorrupt".to_string(),
                
                // === PLAYBACK OPTIONS ===
                "--keep-open=yes".to_string(),
                "--force-seekable=yes".to_string(),
                "--hr-seek=yes".to_string(),
                "--reset-on-next-file=pause".to_string(),
                
                // === VIDEO/AUDIO SYNC ===
                "--video-sync=audio".to_string(),
                "--interpolation=no".to_string(),
                
                // === ERROR HANDLING ===
                "--ytdl=no".to_string(), // Don't use youtube-dl
            ];
            
            // Hardware acceleration
            if self.hw_accel {
                args.push("--hwdec=auto-safe".to_string());
                args.push("--vo=gpu".to_string());
            } else {
                args.push("--hwdec=no".to_string());
            }
            
            // User agent
            if self.pass_user_agent_to_player {
                args.push(format!("--user-agent={}", self.get_user_agent()));
            }
            
            // Slow connection optimizations
            if is_slow {
                args.extend([
                    "--vd-lavc-threads=0".to_string(),        // Auto threads
                    "--vd-lavc-skiploopfilter=all".to_string(), // Skip loop filter
                    "--vd-lavc-skipframe=nonref".to_string(), // Skip non-reference frames
                    "--framedrop=vo".to_string(),             // Drop frames at VO
                    "--video-latency-hacks=yes".to_string(),  // Latency hacks
                    "--untimed=no".to_string(),
                    "--audio-buffer=1".to_string(),           // Larger audio buffer
                ]);
            } else {
                args.extend([
                    "--framedrop=no".to_string(),
                ]);
            }
            
            for arg in args {
                cmd.arg(arg);
            }
        } else if player_lower.contains("vlc") {
            // VLC buffer settings - simple and reliable
            let cache_ms = buffer_ms * 2;
            
            // Extract filename from URL for title
            let stream_name = channel.url.split('/').last().unwrap_or("stream");
            let title = format!("{} - {}", channel.name, stream_name);
            
            let mut args = vec![
                channel.url.clone(),
                format!("--meta-title={}", title),
                format!("--network-caching={}", cache_ms),
                format!("--live-caching={}", cache_ms),
                "--http-reconnect".to_string(),
            ];
            
            // Hardware acceleration
            if self.hw_accel {
                args.push("--avcodec-hw=any".to_string());
            }
            
            // User agent
            if self.pass_user_agent_to_player {
                args.push(format!("--http-user-agent={}", self.get_user_agent()));
            }
            
            for arg in args {
                cmd.arg(arg);
            }
        } else if player_lower.contains("potplayer") {
            // PotPlayer (Windows)
            let stream_name = channel.url.split('/').last().unwrap_or("stream");
            let title = format!("{} - {}", channel.name, stream_name);
            cmd.arg(&channel.url);
            cmd.arg(format!("/title={}", title));
        } else if player_lower.contains("mpc-hc") || player_lower.contains("mpc-be") {
            // MPC-HC / MPC-BE (Windows)
            cmd.arg(&channel.url);
            // MPC doesn't have a direct title arg, but we can try
        } else if player_lower.contains("mplayer") {
            // MPlayer settings
            let cache_min = if is_slow { "50" } else { "20" };
            let stream_name = channel.url.split('/').last().unwrap_or("stream");
            let title = format!("{} - {}", channel.name, stream_name);
            let mut args = vec![
                channel.url.clone(),
                "-cache".to_string(), format!("{}", buffer_secs * 1024),
                "-cache-min".to_string(), cache_min.to_string(),
                "-title".to_string(), title,
            ];
            
            if self.pass_user_agent_to_player {
                args.extend(["-user-agent".to_string(), self.get_user_agent()]);
            }
            
            for arg in args {
                cmd.arg(arg);
            }
        } else if player_lower.contains("celluloid") || player_lower.contains("gnome-mpv") {
            // Celluloid (GNOME MPV frontend) - passes args to mpv
            let stream_name = channel.url.split('/').last().unwrap_or("stream");
            let title = format!("{} - {}", channel.name, stream_name);
            cmd.args([
                &channel.url,
                &format!("--mpv-title={}", title),
                &format!("--mpv-cache-secs={}", buffer_secs),
            ]);
        } else {
            // Generic player - just pass URL
            cmd.arg(&channel.url);
        }

        // Set user agent environment variable for some players
        cmd.env("USER_AGENT", self.get_user_agent());
        
        // Capture stderr for error logging
        cmd.stderr(Stdio::piped());
        cmd.stdout(Stdio::null()); // Ignore stdout

        match cmd.spawn() {
            Ok(mut child) => {
                let pid = child.id();
                self.log(&format!("[PLAY] Player launched successfully (PID: {})", pid));
                
                // Take stderr before potentially moving child
                let stderr = child.stderr.take();
                
                // Spawn stderr reader thread
                if let Some(stderr) = stderr {
                    let sender = self.task_sender.clone();
                    thread::spawn(move || {
                        let reader = BufReader::new(stderr);
                        for line in reader.lines() {
                            if let Ok(line) = line {
                                if !line.trim().is_empty() {
                                    let _ = sender.send(TaskResult::PlayerLog(format!("[PLAYER] {}", line)));
                                }
                            }
                        }
                    });
                }
                
                if self.single_window_mode {
                    self.current_player = Some(child);
                } else {
                    // Spawn monitoring thread for non-single-window mode to track exit
                    let sender = self.task_sender.clone();
                    let channel_name = channel.name.clone();
                    thread::spawn(move || {
                        // Wait for process and get exit code
                        match child.wait() {
                            Ok(status) => {
                                if !status.success() {
                                    let _ = sender.send(TaskResult::PlayerExited {
                                        code: status.code(),
                                        stderr: format!("Player exited with error for '{}'", channel_name),
                                    });
                                }
                            }
                            Err(e) => {
                                let _ = sender.send(TaskResult::PlayerLog(format!("[ERROR] Failed to wait for player: {}", e)));
                            }
                        }
                    });
                }
            }
            Err(e) => {
                self.log(&format!("[ERROR] Failed to launch player '{}': {}", player, e));
                eprintln!("Failed to launch player '{}': {}", player, e);
            }
        }
    }
    
    /// Play using internal FFmpeg player
    fn play_channel_internal(&mut self, channel: &Channel) {
        self.log(&format!("[PLAY] {} | Internal Player", Self::sanitize_text(&channel.name)));
        self.log(&format!("[PLAY] URL: {}", channel.url));
        
        let buffer_secs = self.get_effective_buffer();
        let user_agent = self.get_user_agent();
        
        self.internal_player.play(&channel.name, &channel.url, buffer_secs, &user_agent);
        self.show_internal_player = true;
    }

    fn play_episode(&mut self, episode: &Episode, series_id: i64) {
        // Get series name from navigation or use generic name
        let series_name = self.navigation_stack.iter().find_map(|n| {
            if let NavigationLevel::Series(name) = n { Some(name.clone()) } else { None }
        }).or_else(|| {
            // Check if viewing from favorites
            self.fav_viewing_series.as_ref().map(|(_, name)| name.clone())
        }).unwrap_or_else(|| "Series".to_string());
        
        let url = format!(
            "{}/series/{}/{}/{}.{}",
            self.server, self.username, self.password,
            episode.id, episode.container_extension
        );
        
        let channel = Channel {
            name: format!("{} - {}", series_name, episode.title),
            url,
            stream_id: Some(episode.id),
            category_id: None,
            epg_channel_id: None,
            stream_icon: None,
            series_id: Some(series_id),
            container_extension: Some(episode.container_extension.clone()),
            playlist_source: None,
        };
        
        self.play_channel(&channel);
    }

    fn go_back(&mut self) {
        if self.navigation_stack.pop().is_some() {
            // Restore scroll position for the previous level
            if let Some(scroll_y) = self.scroll_positions.pop() {
                self.pending_scroll_restore = Some(scroll_y);
            }
            
            // Handle what to show based on remaining stack
            if let Some(level) = self.navigation_stack.last() {
                match level {
                    NavigationLevel::Categories => {
                        self.current_channels.clear();
                        self.current_series.clear();
                    }
                    NavigationLevel::Channels(_cat) => {
                        // Refetch channels
                    }
                    _ => {}
                }
            } else {
                self.current_channels.clear();
                self.current_series.clear();
                self.current_seasons.clear();
                self.current_episodes.clear();
            }
        }
        self.search_query.clear();
    }
    
    /// Save current scroll position before navigating into a folder
    fn save_scroll_position(&mut self, _ctx: &egui::Context) {
        // Save the current scroll offset tracked from the scroll area
        self.scroll_positions.push(self.current_scroll_offset);
    }

    fn extract_m3u_credentials(&mut self, url: &str) {
        // Parse m3u_plus URL format:
        // http://server.com/get.php?username=XXX&password=YYY&type=m3u_plus
        if let Some(query_start) = url.find('?') {
            let query = &url[query_start + 1..];
            let mut params: HashMap<&str, &str> = HashMap::new();
            
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    params.insert(key, value);
                }
            }
            
            if let (Some(&user), Some(&pass)) = (params.get("username"), params.get("password")) {
                self.username = user.to_string();
                self.password = pass.to_string();
                
                // Extract server
                if let Some(proto_end) = url.find("://") {
                    let rest = &url[proto_end + 3..];
                    if let Some(path_start) = rest.find('/') {
                        self.server = url[..proto_end + 3 + path_start].to_string();
                    }
                }
            }
        }
    }
    
    /// Parse Xtream credentials from M3U Plus URL - returns (server, username, password)
    fn parse_xtream_url(url: &str) -> Option<(String, String, String)> {
        if let Some(query_start) = url.find('?') {
            let query = &url[query_start + 1..];
            let mut params: HashMap<&str, &str> = HashMap::new();
            
            for pair in query.split('&') {
                if let Some((key, value)) = pair.split_once('=') {
                    params.insert(key, value);
                }
            }
            
            if let (Some(&user), Some(&pass)) = (params.get("username"), params.get("password")) {
                // Extract server
                if let Some(proto_end) = url.find("://") {
                    let rest = &url[proto_end + 3..];
                    if let Some(path_start) = rest.find('/') {
                        let server = url[..proto_end + 3 + path_start].to_string();
                        return Some((server, user.to_string(), pass.to_string()));
                    }
                }
            }
        }
        None
    }
    
    /// Load playlist with a specific name (for saved playlists)
    fn load_playlist_with_name(&mut self, url: &str, name: &str) {
        let url = url.to_string();
        let name = name.to_string();
        let sender = self.task_sender.clone();
        let user_agent = self.get_user_agent().to_string();
        
        self.loading = true;
        self.status_message = format!("Loading {}...", name);
        self.log(&format!("[INFO] Loading playlist: {} ({})", name, url));
        
        std::thread::spawn(move || {
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(60)))
                .build()
                .new_agent();
            
            let result = agent.get(&url)
                .header("User-Agent", &user_agent)
                .call();
            
            match result {
                Ok(mut response) => {
                    if let Ok(content) = response.body_mut().read_to_string() {
                        let (channels, playlist_name) = if xspf_parser::is_xspf(&content) {
                            match xspf_parser::parse_xspf(&content) {
                                Ok(playlist) => {
                                    let pname = playlist.title.clone().unwrap_or_else(|| name.clone());
                                    let m3u_channels = xspf_parser::to_m3u_channels(&playlist);
                                    let channels: Vec<Channel> = m3u_channels.into_iter().map(|c| {
                                        Channel {
                                            stream_id: None,
                                            name: c.name,
                                            url: c.url,
                                            epg_channel_id: c.tvg_id,
                                            stream_icon: c.tvg_logo,
                                            category_id: None,
                                            series_id: None,
                                            container_extension: None,
                                            playlist_source: Some(name.clone()),
                                        }
                                    }).collect();
                                    (channels, Some(pname))
                                }
                                Err(e) => {
                                    let _ = sender.send(TaskResult::Error(format!("XSPF parse error: {}", e)));
                                    return;
                                }
                            }
                        } else {
                            let playlist = m3u_parser::parse_m3u_playlist(&content);
                            let channels: Vec<Channel> = playlist.channels.into_iter().map(|c| {
                                Channel {
                                    stream_id: None,
                                    name: c.name,
                                    url: c.url,
                                    epg_channel_id: c.tvg_id,
                                    stream_icon: c.tvg_logo,
                                    category_id: None,
                                    series_id: None,
                                    container_extension: None,
                                    playlist_source: Some(name.clone()),
                                }
                            }).collect();
                            (channels, Some(name.clone()))
                        };
                        
                        let _ = sender.send(TaskResult::PlaylistLoaded { channels, playlist_name });
                    } else {
                        let _ = sender.send(TaskResult::Error("Failed to read playlist content".to_string()));
                    }
                }
                Err(e) => {
                    let _ = sender.send(TaskResult::Error(format!("Failed to fetch playlist: {}", e)));
                }
            }
        });
    }
    
    /// Reload a playlist in background (for auto-update)
    fn reload_playlist(&mut self, url: &str, name: &str) {
        let url = url.to_string();
        let name = name.to_string();
        let sender = self.task_sender.clone();
        let user_agent = self.get_user_agent().to_string();
        
        self.status_message = format!("Updating {}...", name);
        
        std::thread::spawn(move || {
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(60)))
                .build()
                .new_agent();
            
            let result = agent.get(&url)
                .header("User-Agent", &user_agent)
                .call();
            
            match result {
                Ok(mut response) => {
                    if let Ok(content) = response.body_mut().read_to_string() {
                        let channels = if xspf_parser::is_xspf(&content) {
                            match xspf_parser::parse_xspf(&content) {
                                Ok(playlist) => {
                                    let m3u_channels = xspf_parser::to_m3u_channels(&playlist);
                                    m3u_channels.into_iter().map(|c| {
                                        Channel {
                                            stream_id: None,
                                            name: c.name,
                                            url: c.url,
                                            epg_channel_id: c.tvg_id,
                                            stream_icon: c.tvg_logo,
                                            category_id: None,
                                            series_id: None,
                                            container_extension: None,
                                            playlist_source: Some(name.clone()),
                                        }
                                    }).collect()
                                }
                                Err(e) => {
                                    let _ = sender.send(TaskResult::Error(format!("XSPF parse error: {}", e)));
                                    return;
                                }
                            }
                        } else {
                            let playlist = m3u_parser::parse_m3u_playlist(&content);
                            playlist.channels.into_iter().map(|c| {
                                Channel {
                                    stream_id: None,
                                    name: c.name,
                                    url: c.url,
                                    epg_channel_id: c.tvg_id,
                                    stream_icon: c.tvg_logo,
                                    category_id: None,
                                    series_id: None,
                                    container_extension: None,
                                    playlist_source: Some(name.clone()),
                                }
                            }).collect()
                        };
                        
                        let _ = sender.send(TaskResult::PlaylistReloaded { channels, playlist_name: name });
                    } else {
                        let _ = sender.send(TaskResult::Error("Failed to read playlist content".to_string()));
                    }
                }
                Err(e) => {
                    let _ = sender.send(TaskResult::Error(format!("Failed to fetch playlist: {}", e)));
                }
            }
        });
    }
    
    /// Unload a specific playlist by index
    fn unload_playlist(&mut self, idx: usize) {
        if idx >= self.playlist_sources.len() {
            return;
        }
        
        let (start_idx, name) = self.playlist_sources[idx].clone();
        let end_idx = self.playlist_sources.get(idx + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(self.current_channels.len());
        
        let channels_to_remove = end_idx - start_idx;
        
        // Remove channels
        if start_idx < self.current_channels.len() {
            let actual_end = end_idx.min(self.current_channels.len());
            self.current_channels.drain(start_idx..actual_end);
        }
        
        // Remove from sources
        self.playlist_sources.remove(idx);
        
        // Update indices
        for (start, _) in self.playlist_sources.iter_mut().skip(idx) {
            *start = start.saturating_sub(channels_to_remove);
        }
        
        // Remove related favorites/recent
        self.favorites.retain(|f| f.playlist_source.as_ref() != Some(&name));
        self.recent_watched.retain(|f| f.playlist_source.as_ref() != Some(&name));
        self.config.favorites_json = serde_json::to_string(&self.favorites).unwrap_or_default();
        self.config.recent_watched_json = serde_json::to_string(&self.recent_watched).unwrap_or_default();
        self.config.save();
        
        if self.playlist_sources.is_empty() {
            self.playlist_mode = false;
        }
        
        self.status_message = format!("Unloaded '{}' ({} channels)", name, channels_to_remove);
    }

    fn load_playlist(&mut self, url: &str) {
        let url = url.to_string();
        let sender = self.task_sender.clone();
        let user_agent = self.get_user_agent().to_string();
        
        // Extract a short name from URL for source tracking
        let url_for_name = url.split('/').last()
            .unwrap_or(&url)
            .split('?').next()
            .unwrap_or(&url)
            .to_string();
        
        self.loading = true;
        self.status_message = "Loading playlist...".to_string();
        self.log(&format!("[INFO] Loading playlist: {}", url));
        
        std::thread::spawn(move || {
            let agent = ureq::Agent::config_builder()
                .timeout_global(Some(std::time::Duration::from_secs(60)))
                .build()
                .new_agent();
            
            let result = agent.get(&url)
                .header("User-Agent", &user_agent)
                .call();
            
            match result {
                Ok(mut response) => {
                    if let Ok(content) = response.body_mut().read_to_string() {
                        let (channels, playlist_name) = if xspf_parser::is_xspf(&content) {
                            // Parse as XSPF
                            match xspf_parser::parse_xspf(&content) {
                                Ok(playlist) => {
                                    let name = playlist.title.clone();
                                    let source_name = name.clone().unwrap_or_else(|| url_for_name.clone());
                                    let m3u_channels = xspf_parser::to_m3u_channels(&playlist);
                                    let channels: Vec<Channel> = m3u_channels.into_iter().map(|c| {
                                        Channel {
                                            stream_id: None,
                                            name: c.name,
                                            url: c.url,
                                            epg_channel_id: c.tvg_id,
                                            stream_icon: c.tvg_logo,
                                            category_id: None,
                                            series_id: None,
                                            container_extension: None,
                                            playlist_source: Some(source_name.clone()),
                                        }
                                    }).collect();
                                    (channels, name)
                                }
                                Err(e) => {
                                    let _ = sender.send(TaskResult::Error(format!("XSPF parse error: {}", e)));
                                    return;
                                }
                            }
                        } else {
                            // Parse as M3U/M3U8
                            let playlist = m3u_parser::parse_m3u_playlist(&content);
                            let source_name = url_for_name.clone();
                            let channels: Vec<Channel> = playlist.channels.into_iter().map(|c| {
                                Channel {
                                    stream_id: None,
                                    name: c.name,
                                    url: c.url,
                                    epg_channel_id: c.tvg_id,
                                    stream_icon: c.tvg_logo,
                                    category_id: None,
                                    series_id: None,
                                    container_extension: None,
                                    playlist_source: Some(source_name.clone()),
                                }
                            }).collect();
                            (channels, None)
                        };
                        
                        let _ = sender.send(TaskResult::PlaylistLoaded { channels, playlist_name });
                    } else {
                        let _ = sender.send(TaskResult::Error("Failed to read playlist content".to_string()));
                    }
                }
                Err(e) => {
                    let _ = sender.send(TaskResult::Error(format!("Failed to fetch playlist: {}", e)));
                }
            }
        });
    }
}

impl eframe::App for IPTVApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process background task results (non-blocking)
        while let Ok(result) = self.task_receiver.try_recv() {
            match result {
                TaskResult::CategoriesLoaded { live, movies, series } => {
                    self.log(&format!("[INFO] Login successful - Live: {}, Movies: {}, Series: {} categories", 
                        live.len(), movies.len(), series.len()));
                    self.live_categories = live;
                    self.movie_categories = movies;
                    self.series_categories = series;
                    self.logged_in = true;
                    self.loading = false;
                    self.status_message = "Logged in successfully".to_string();
                    
                    // Auto-save to playlist_entries if save_state is enabled
                    if self.save_state && !self.server.is_empty() && !self.username.is_empty() {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        
                        let entry = PlaylistEntry {
                            name: format!("{}@{}", self.username, self.server.split('/').nth(2).unwrap_or(&self.server)),
                            entry_type: PlaylistType::Xtream {
                                server: self.server.clone(),
                                username: self.username.clone(),
                                password: self.password.clone(),
                            },
                            saved_at: now,
                            enabled: true,
                            auto_login: false, // Default off for new entries
                            auto_update_days: 0,
                            last_updated: now,
                            epg_url: self.epg_url_input.clone(),
                            epg_time_offset: self.epg_time_offset,
                            epg_auto_update_index: self.epg_auto_update.to_index(),
                            epg_show_actual_time: self.epg_show_actual_time,
                            epg_last_updated: 0,
                            external_player: self.external_player.clone(),
                            buffer_seconds: self.buffer_seconds,
                            connection_quality: self.connection_quality,
                            selected_user_agent: self.selected_user_agent,
                            custom_user_agent: self.custom_user_agent.clone(),
                            use_custom_user_agent: self.use_custom_user_agent,
                            pass_user_agent_to_player: self.pass_user_agent_to_player,
                        };
                        
                        // Update existing or add new
                        if let Some(existing) = self.playlist_entries.iter_mut().find(|e| {
                            matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                                if server == &self.server && username == &self.username)
                        }) {
                            // Keep existing name, auto_login, auto_update, and epg_last_updated settings
                            let name = existing.name.clone();
                            let auto_login = existing.auto_login;
                            let auto_update_days = existing.auto_update_days;
                            let last_updated = existing.last_updated;
                            let epg_last_updated = existing.epg_last_updated;
                            *existing = entry;
                            existing.name = name;
                            existing.auto_login = auto_login;
                            existing.auto_update_days = auto_update_days;
                            existing.last_updated = last_updated;
                            existing.epg_last_updated = epg_last_updated;
                        } else {
                            self.playlist_entries.push(entry);
                        }
                        save_playlist_entries(&self.playlist_entries);
                    }
                    
                    // Load EPG cache from disk if available
                    if !self.epg_url_input.is_empty() && self.epg_data.is_none() {
                        // Try to load cached EPG data
                        if let Some(cached_epg) = load_epg_cache::<EpgData>(&self.server, &self.username) {
                            let channel_count = cached_epg.channels.len();
                            let program_count = cached_epg.program_count();
                            self.log(&format!("[INFO] Loaded EPG from cache: {} channels, {} programs", channel_count, program_count));
                            self.epg_data = Some(Box::new(cached_epg));
                            self.epg_status = format!("Cached: {} channels, {} programs", channel_count, program_count);
                            
                            // Get persistent epg_last_updated from playlist entry
                            let epg_last_updated = self.playlist_entries.iter().find(|e| {
                                matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                                    if server == &self.server && username == &self.username)
                            }).map(|e| e.epg_last_updated).unwrap_or(0);
                            
                            // Set in-memory timestamp from persistent storage
                            if epg_last_updated > 0 {
                                self.epg_last_update = Some(epg_last_updated);
                            }
                            
                            // Check if cache is stale and needs refresh
                            if let Some(interval_secs) = self.epg_auto_update.as_secs() {
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                
                                if epg_last_updated > 0 && (now - epg_last_updated) >= interval_secs {
                                    self.log(&format!("[INFO] EPG cache is stale (last updated {} hours ago), will refresh", 
                                        (now - epg_last_updated) / 3600));
                                    // Trigger refresh - the periodic check will handle it
                                }
                            }
                        } else {
                            self.log("[INFO] No EPG cache found - use EPG button to load");
                        }
                    }
                }
                TaskResult::UserInfoLoaded { user_info, server_info } => {
                    self.log(&format!("[INFO] User: {} | Status: {} | Expiry: {}", 
                        user_info.username, user_info.status, user_info.expiry));
                    self.user_info = user_info;
                    self.server_info = server_info;
                }
                TaskResult::ChannelsLoaded(channels) => {
                    self.log(&format!("[INFO] Loaded {} channels", channels.len()));
                    self.current_channels = channels;
                    self.loading = false;
                    self.status_message = format!("Loaded {} channels", self.current_channels.len());
                }
                TaskResult::SeriesListLoaded(series) => {
                    self.log(&format!("[INFO] Loaded {} series", series.len()));
                    self.current_series = series;
                    self.loading = false;
                    self.status_message = format!("Loaded {} series", self.current_series.len());
                }
                TaskResult::SeasonsLoaded(seasons) => {
                    self.log(&format!("[INFO] Loaded {} seasons", seasons.len()));
                    self.current_seasons = seasons;
                    self.loading = false;
                    self.status_message = format!("Loaded {} seasons", self.current_seasons.len());
                }
                TaskResult::EpisodesLoaded(episodes) => {
                    self.log(&format!("[INFO] Loaded {} episodes", episodes.len()));
                    self.current_episodes = episodes;
                    self.loading = false;
                    self.status_message = format!("Loaded {} episodes", self.current_episodes.len());
                }
                TaskResult::FavSeasonsLoaded(seasons) => {
                    self.log(&format!("[INFO] Loaded {} seasons for favorite", seasons.len()));
                    self.fav_series_seasons = seasons;
                    self.loading = false;
                    self.status_message = format!("Loaded {} seasons", self.fav_series_seasons.len());
                }
                TaskResult::FavEpisodesLoaded(episodes) => {
                    self.log(&format!("[INFO] Loaded {} episodes for favorite", episodes.len()));
                    self.fav_series_episodes = episodes;
                    self.loading = false;
                    self.status_message = format!("Loaded {} episodes", self.fav_series_episodes.len());
                }
                TaskResult::Error(msg) => {
                    self.log(&format!("[ERROR] {}", msg));
                    self.loading = false;
                    self.status_message = format!("Error: {}", msg);
                }
                TaskResult::PlayerLog(msg) => {
                    self.log(&msg);
                }
                TaskResult::PlayerExited { code, stderr } => {
                    let exit_msg = match code {
                        Some(c) => format!("[WARN] Player exited with code {}: {}", c, stderr),
                        None => format!("[WARN] Player terminated by signal: {}", stderr),
                    };
                    self.log(&exit_msg);
                    self.status_message = stderr;
                }
                TaskResult::EpgLoading { progress } => {
                    self.epg_status = progress.clone();
                    // Extract percentage from status like "Downloading: 45.2 / 80.0 MB (56%)"
                    if let Some(start) = progress.rfind('(') {
                        if let Some(end) = progress.rfind('%') {
                            if let Ok(pct) = progress[start+1..end].parse::<f32>() {
                                self.epg_progress = pct / 100.0;
                            }
                        }
                    } else if progress.contains("Parsing") {
                        self.epg_progress = 0.95; // Parsing is near the end
                    }
                }
                TaskResult::EpgLoaded { data } => {
                    let channel_count = data.channels.len();
                    let program_count = data.program_count();
                    
                    // Log completion details
                    self.log("[INFO] ========== EPG LOAD COMPLETE ==========");
                    self.log(&format!("[INFO] EPG URL: {}", self.epg_url_input));
                    self.log(&format!("[INFO] Channels parsed: {}", channel_count));
                    self.log(&format!("[INFO] Programs parsed: {}", program_count));
                    
                    // Log any parse errors
                    if data.parse_error_count > 0 {
                        self.log(&format!("[WARN] Parse errors encountered: {} (recovered)", data.parse_error_count));
                        for err in &data.parse_errors {
                            self.log(&format!("[WARN]   {}", err));
                        }
                        if data.parse_error_count > data.parse_errors.len() {
                            self.log(&format!("[WARN]   ... and {} more errors", 
                                data.parse_error_count - data.parse_errors.len()));
                        }
                    } else {
                        self.log("[INFO] No parse errors - EPG file was clean");
                    }
                    self.log("[INFO] =========================================");
                    
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    
                    // Save EPG cache to disk for persistence across restarts
                    if !self.server.is_empty() && !self.username.is_empty() {
                        self.log("[INFO] Saving EPG cache to disk...");
                        save_epg_cache(&self.server, &self.username, data.as_ref());
                    }
                    
                    self.epg_data = Some(data);
                    self.epg_loading = false;
                    self.epg_progress = 1.0;
                    self.epg_last_update = Some(now);
                    self.epg_status = format!("Loaded {} channels, {} programs", channel_count, program_count);
                    
                    // Save epg_last_updated to playlist entry for persistence
                    if let Some(entry) = self.playlist_entries.iter_mut().find(|e| {
                        matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                            if server == &self.server && username == &self.username)
                    }) {
                        entry.epg_last_updated = now;
                        save_playlist_entries(&self.playlist_entries);
                    }
                }
                TaskResult::EpgError(msg) => {
                    self.log(&format!("[ERROR] EPG: {}", msg));
                    self.epg_loading = false;
                    self.epg_progress = 0.0;
                    self.epg_status = format!("Error: {}", msg);
                }
                TaskResult::PlaylistLoaded { channels, playlist_name } => {
                    let count = channels.len();
                    let source_name = playlist_name.clone().unwrap_or_else(|| "Playlist".to_string());
                    self.log(&format!("[INFO] Loaded {} with {} channels", source_name, count));
                    
                    // Track source for separator display
                    let start_idx = self.current_channels.len();
                    self.playlist_sources.push((start_idx, source_name.clone()));
                    
                    // Append channels (don't replace)
                    self.current_channels.extend(channels);
                    
                    self.playlist_mode = true;
                    self.logged_in = true;
                    self.loading = false;
                    
                    // Set navigation to show channels (only on first playlist load)
                    if self.playlist_sources.len() == 1 {
                        self.navigation_stack.clear();
                        self.navigation_stack.push(NavigationLevel::Channels("Playlist".to_string()));
                    }
                    
                    let total = self.current_channels.len();
                    if self.playlist_sources.len() > 1 {
                        self.status_message = format!("Total: {} channels from {} playlists", total, self.playlist_sources.len());
                    } else {
                        self.status_message = format!("Loaded: {} ({} channels)", source_name, count);
                    }
                    
                    // Check if this playlist needs immediate auto-update (time elapsed while app was closed)
                    if let Some(playlist_name) = playlist_name {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64;
                        
                        if let Some((idx, entry)) = self.playlist_entries.iter().enumerate()
                            .find(|(_, e)| e.name == playlist_name && e.enabled && e.auto_update_days > 0)
                        {
                            if let PlaylistType::M3U { url } = &entry.entry_type {
                                let interval_secs = (entry.auto_update_days as i64) * 24 * 60 * 60;
                                if entry.last_updated > 0 && (now - entry.last_updated) >= interval_secs {
                                    let url = url.clone();
                                    let name = playlist_name.clone();
                                    self.log(&format!("[INFO] Playlist '{}' needs update (last updated {} days ago)", 
                                        name, (now - entry.last_updated) / 86400));
                                    
                                    // Update timestamp and trigger reload
                                    self.playlist_entries[idx].last_updated = now;
                                    save_playlist_entries(&self.playlist_entries);
                                    self.reload_playlist(&url, &name);
                                }
                            }
                        }
                    }
                }
                TaskResult::PlaylistReloaded { channels, playlist_name } => {
                    // Find and replace channels for this playlist source
                    if let Some(idx) = self.playlist_sources.iter().position(|(_, name)| name == &playlist_name) {
                        let (start_idx, _) = self.playlist_sources[idx];
                        let end_idx = self.playlist_sources.get(idx + 1)
                            .map(|(i, _)| *i)
                            .unwrap_or(self.current_channels.len());
                        
                        let old_count = end_idx - start_idx;
                        let new_count = channels.len();
                        let diff = new_count as i32 - old_count as i32;
                        
                        // Remove old channels
                        self.current_channels.drain(start_idx..end_idx);
                        
                        // Insert new channels at the same position
                        for (i, channel) in channels.into_iter().enumerate() {
                            self.current_channels.insert(start_idx + i, channel);
                        }
                        
                        // Update indices for subsequent playlists
                        for (start, _) in self.playlist_sources.iter_mut().skip(idx + 1) {
                            *start = (*start as i32 + diff) as usize;
                        }
                        
                        self.log(&format!("[INFO] Updated '{}': {} → {} channels", playlist_name, old_count, new_count));
                        self.status_message = format!("Updated '{}' ({} channels)", playlist_name, new_count);
                    }
                }
            }
        }
        
        // Request repaint while loading or when player might be outputting
        if self.loading || self.epg_loading || self.current_player.is_some() {
            ctx.request_repaint();
        }
        
        // EPG UI refresh every 5 minutes (to update current program, time remaining, etc.)
        if self.epg_data.is_some() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            
            if now - self.epg_last_ui_refresh >= 300 { // 5 minutes = 300 seconds
                self.epg_last_ui_refresh = now;
                ctx.request_repaint();
            }
        }
        
        // Auto-login on startup - check playlist_entries for auto_login flag (must be enabled)
        if !self.auto_login_triggered && !self.logged_in && !self.loading {
            // Find first entry with auto_login enabled AND playlist enabled
            let auto_login_idx = self.playlist_entries.iter().position(|e| {
                e.enabled && e.auto_login && matches!(e.entry_type, PlaylistType::Xtream { .. })
            });
            
            if let Some(idx) = auto_login_idx {
                self.auto_login_triggered = true;
                let entry = &self.playlist_entries[idx];
                if let PlaylistType::Xtream { server, username, password } = &entry.entry_type {
                    // Load settings from entry
                    self.server = server.clone();
                    self.username = username.clone();
                    self.password = password.clone();
                    if !entry.epg_url.is_empty() {
                        self.epg_url_input = entry.epg_url.clone();
                    }
                    self.epg_time_offset = entry.epg_time_offset;
                    self.epg_auto_update = EpgAutoUpdate::from_index(entry.epg_auto_update_index);
                    self.epg_show_actual_time = entry.epg_show_actual_time;
                    if !entry.external_player.is_empty() {
                        self.external_player = entry.external_player.clone();
                    }
                    self.buffer_seconds = entry.buffer_seconds;
                    self.connection_quality = entry.connection_quality;
                    self.selected_user_agent = entry.selected_user_agent;
                    self.custom_user_agent = entry.custom_user_agent.clone();
                    self.use_custom_user_agent = entry.use_custom_user_agent;
                    self.pass_user_agent_to_player = entry.pass_user_agent_to_player;
                    self.login();
                }
            } else {
                // Fall back to legacy auto_login behavior
                if self.save_state && self.auto_login {
                    if !self.server.is_empty() && !self.username.is_empty() && !self.password.is_empty() {
                        self.auto_login_triggered = true;
                        self.login();
                    } else {
                        self.auto_login_triggered = true;
                    }
                } else {
                    self.auto_login_triggered = true;
                }
            }
        }
        
        // === Auto-update checks (EPG and Playlist) ===
        // Share timestamp to avoid duplicate syscalls
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        
        // EPG auto-update check (periodic refresh) - only when logged in and interval elapsed
        if self.logged_in && !self.epg_loading && !self.epg_url_input.is_empty() {
            if let Some(interval_secs) = self.epg_auto_update.as_secs() {
                // Get persistent epg_last_updated from playlist entry if available
                let persistent_last_update = self.playlist_entries.iter().find(|e| {
                    matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                        if server == &self.server && username == &self.username)
                }).map(|e| e.epg_last_updated).unwrap_or(0);
                
                // Use in-memory timestamp if set, otherwise use persistent
                let last_update = self.epg_last_update.unwrap_or(persistent_last_update);
                
                // Only update if interval has elapsed (last_update > 0 means it was loaded before)
                if last_update > 0 && (now - last_update) >= interval_secs {
                    self.log("[INFO] EPG auto-update triggered");
                    self.load_epg();
                }
            }
        }
        
        // Playlist auto-update check (periodic refresh for M3U playlists)
        // Skip if loading/EPG loading, and add 30min stagger after EPG updates
        if !self.loading && !self.epg_loading {
            // 30 minute stagger after EPG update to avoid simultaneous requests
            let stagger_ok = self.epg_last_update.map_or(true, |epg_last| (now - epg_last) >= 1800);
            
            if stagger_ok {
                // Find first M3U playlist that needs updating
                let playlist_to_update = self.playlist_entries.iter().enumerate()
                    .filter(|(_, e)| e.enabled && e.auto_update_days > 0)
                    .filter_map(|(i, entry)| {
                        if let PlaylistType::M3U { url } = &entry.entry_type {
                            // Check if loaded and interval elapsed
                            let is_loaded = self.playlist_sources.iter().any(|(_, name)| name == &entry.name);
                            let interval_secs = (entry.auto_update_days as i64) * 24 * 60 * 60;
                            if is_loaded && entry.last_updated > 0 && (now - entry.last_updated) >= interval_secs {
                                return Some((i, url.clone(), entry.name.clone()));
                            }
                        }
                        None
                    })
                    .next();
                
                // Trigger background update
                if let Some((idx, url, name)) = playlist_to_update {
                    self.log(&format!("[INFO] Playlist auto-update triggered for '{}'", name));
                    self.playlist_entries[idx].last_updated = now;
                    save_playlist_entries(&self.playlist_entries);
                    self.reload_playlist(&url, &name);
                }
            }
        }

        // Apply theme
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Top panel - Controls
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(5.0);
            
            ui.horizontal(|ui| {
                // Unified Playlists button - primary action
                let playlist_count = self.playlist_entries.len();
                let loaded_count = self.playlist_sources.len();
                let btn_text = if self.logged_in {
                    // Find current playlist name by reference to avoid clone
                    let current_name = self.playlist_entries.iter().find(|e| {
                        matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                            if server == &self.server && username == &self.username)
                    }).map(|e| e.name.as_str());
                    
                    if let Some(name) = current_name {
                        format!("📺 {} ▼  ", name)
                    } else {
                        format!("📺 {}@{} ▼  ", self.username, self.server.split('/').nth(2).unwrap_or(&self.server))
                    }
                } else if loaded_count > 0 {
                    format!("📺 Playlists ({}/{})  ", loaded_count, playlist_count)
                } else if playlist_count > 0 {
                    format!("📺 Playlists ({})  ", playlist_count)
                } else {
                    "📺 Playlists  ".to_string()
                };
                if ui.button(btn_text).on_hover_text("Manage playlists - Add Xtream/M3U sources").clicked() {
                    self.show_playlist_manager = true;
                }
                
                // Logout button when logged in
                if self.logged_in {
                    if ui.button("🚪 Logout").clicked() {
                        self.logged_in = false;
                        self.live_categories.clear();
                        self.movie_categories.clear();
                        self.series_categories.clear();
                        self.current_channels.clear();
                        self.current_series.clear();
                        self.status_message = "Logged out".to_string();
                    }
                }
                
                ui.separator();
                
                if ui.button("🌐 User Agent").clicked() {
                    self.show_user_agent_dialog = true;
                }
                
                if ui.button("📡 EPG").on_hover_text("Load Electronic Program Guide").clicked() {
                    self.show_epg_dialog = true;
                }
                
                ui.separator();
                
                ui.checkbox(&mut self.dark_mode, "🌙 Dark");
                ui.checkbox(&mut self.single_window_mode, "Single Window")
                    .on_hover_text("Close previous player when opening new stream");
                
                ui.separator();
                
                ui.checkbox(&mut self.save_state, "💾 Auto-Save")
                    .on_hover_text("Auto-save logins to Playlist Manager");
                
                if ui.button("💾 Save").on_hover_text("Save current settings").clicked() {
                    self.save_current_state();
                }
            });
            
            ui.horizontal(|ui| {
                ui.label("🎬 Player:");
                ui.add(egui::TextEdit::singleline(&mut self.external_player)
                    .hint_text("mpv, vlc, ffplay, internal...")
                    .desired_width(260.0))
                    .on_hover_text("Enter media player command/path:\n• mpv\n• vlc\n• ffplay\n• internal (built-in player)\n• /usr/bin/mpv\n• C:\\Program Files\\VLC\\vlc.exe\n\nLeave empty for ffplay (default)");
                
                if ui.button("📁").on_hover_text("Browse for player executable").clicked() {
                    #[cfg(target_os = "windows")]
                    {
                        // Native Windows file dialog
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Select Media Player")
                            .add_filter("Executables", &["exe", "com", "bat", "cmd"])
                            .add_filter("All Files", &["*"])
                            .pick_file() 
                        {
                            self.external_player = path.display().to_string();
                        }
                    }
                    
                    #[cfg(target_os = "linux")]
                    {
                        // Try zenity first (GTK), then kdialog (KDE), then rfd
                        let mut found = false;
                        
                        if let Ok(output) = std::process::Command::new("zenity")
                            .args(["--file-selection", "--title=Select Media Player"])
                            .output()
                        {
                            if output.status.success() {
                                if let Ok(path) = String::from_utf8(output.stdout) {
                                    let path = path.trim();
                                    if !path.is_empty() {
                                        self.external_player = path.to_string();
                                        found = true;
                                    }
                                }
                            }
                        }
                        
                        if !found {
                            if let Ok(output) = std::process::Command::new("kdialog")
                                .args(["--getopenfilename", ".", "All Files (*)"])
                                .output()
                            {
                                if output.status.success() {
                                    if let Ok(path) = String::from_utf8(output.stdout) {
                                        let path = path.trim();
                                        if !path.is_empty() {
                                            self.external_player = path.to_string();
                                            found = true;
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Fallback to rfd if available
                        if !found {
                            if let Some(path) = rfd::FileDialog::new()
                                .set_title("Select Media Player")
                                .pick_file() 
                            {
                                self.external_player = path.display().to_string();
                            }
                        }
                    }
                    
                    #[cfg(target_os = "macos")]
                    {
                        // Native macOS file dialog
                        if let Some(path) = rfd::FileDialog::new()
                            .set_title("Select Media Player")
                            .add_filter("Applications", &["app"])
                            .add_filter("All Files", &["*"])
                            .pick_file() 
                        {
                            self.external_player = path.display().to_string();
                        }
                    }
                }
                
                ui.separator();
                
                ui.label("📶 Connection:");
                egui::ComboBox::from_id_salt("connection_quality")
                    .selected_text(match self.connection_quality {
                        ConnectionQuality::Fast => "Fast",
                        ConnectionQuality::Normal => "Normal",
                        ConnectionQuality::Slow => "Slow",
                        ConnectionQuality::VerySlow => "Very Slow",
                        ConnectionQuality::Custom => "⚙️ Custom",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.connection_quality, ConnectionQuality::Fast, "⚡ Fast (2s)");
                        ui.selectable_value(&mut self.connection_quality, ConnectionQuality::Normal, "📶 Normal (5s)");
                        ui.selectable_value(&mut self.connection_quality, ConnectionQuality::Slow, "🐢 Slow (15s)");
                        ui.selectable_value(&mut self.connection_quality, ConnectionQuality::VerySlow, "🦥 Very Slow (30s)");
                        ui.selectable_value(&mut self.connection_quality, ConnectionQuality::Custom, "⚙️ Custom");
                    });
                
                if self.connection_quality == ConnectionQuality::Custom {
                    ui.label("Buffer:");
                    ui.add(egui::DragValue::new(&mut self.buffer_seconds)
                        .range(1..=120)
                        .suffix("s"));
                }
                
                // Show effective buffer
                ui.label(format!("({}s)", self.get_effective_buffer()));
                
                ui.separator();
                
                ui.checkbox(&mut self.hw_accel, "HW Acceleration")
                    .on_hover_text("GPU Decoding\n\nEnable GPU hardware acceleration for video decoding\nDisable if you experience playback issues");
            });
            
            ui.add_space(5.0);
        });

        // Bottom panel - Status
        egui::TopBottomPanel::bottom("bottom_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.loading {
                    ui.spinner();
                }
                ui.label(&self.status_message);
            });
        });

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.logged_in && !self.playlist_mode {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.0);
                    ui.heading("📺 Xtreme IPTV Player");
                    ui.add_space(20.0);
                    
                    let enabled_count = self.playlist_entries.iter().filter(|e| e.enabled).count();
                    
                    if enabled_count == 0 {
                        if self.playlist_entries.is_empty() {
                            ui.label("Click 'Playlists' to add your first playlist");
                        } else {
                            ui.label("All playlists are disabled");
                            ui.label(egui::RichText::new("Enable playlists in Playlist Manager").weak());
                        }
                        ui.add_space(10.0);
                        if ui.button("📺 Playlist Manager").clicked() {
                            self.show_playlist_manager = true;
                        }
                    } else {
                        ui.label("Select a playlist to get started:");
                        ui.add_space(10.0);
                        
                        // Show quick access to enabled playlists - use index to avoid clone
                        let mut to_load_idx: Option<usize> = None;
                        for (i, entry) in self.playlist_entries.iter().enumerate() {
                            if !entry.enabled { continue; }
                            let btn_text = match &entry.entry_type {
                                PlaylistType::Xtream { .. } => format!("🔑 {}", entry.name),
                                PlaylistType::M3U { .. } => format!("📺 {}", entry.name),
                            };
                            if ui.button(&btn_text).clicked() {
                                to_load_idx = Some(i);
                            }
                        }
                        
                        if let Some(idx) = to_load_idx {
                            let entry = &self.playlist_entries[idx];
                            match &entry.entry_type {
                                PlaylistType::Xtream { server, username, password } => {
                                    self.server = server.clone();
                                    self.username = username.clone();
                                    self.password = password.clone();
                                    if !entry.epg_url.is_empty() {
                                        self.epg_url_input = entry.epg_url.clone();
                                    }
                                    self.epg_time_offset = entry.epg_time_offset;
                                    self.epg_auto_update = EpgAutoUpdate::from_index(entry.epg_auto_update_index);
                                    self.epg_show_actual_time = entry.epg_show_actual_time;
                                    if !entry.external_player.is_empty() {
                                        self.external_player = entry.external_player.clone();
                                    }
                                    self.buffer_seconds = entry.buffer_seconds;
                                    self.connection_quality = entry.connection_quality;
                                    self.selected_user_agent = entry.selected_user_agent;
                                    self.custom_user_agent = entry.custom_user_agent.clone();
                                    self.use_custom_user_agent = entry.use_custom_user_agent;
                                    self.pass_user_agent_to_player = entry.pass_user_agent_to_player;
                                    self.login();
                                }
                                PlaylistType::M3U { url } => {
                                    // Clone before calling mutable method
                                    let url = url.clone();
                                    let name = entry.name.clone();
                                    self.load_playlist_with_name(&url, &name);
                                }
                            }
                        }
                        
                        ui.add_space(20.0);
                        if ui.button("📺 Manage Playlists").clicked() {
                            self.show_playlist_manager = true;
                        }
                    }
                });
                return;
            }

            // Tab bar
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::Live, "📺 LIVE");
                ui.selectable_value(&mut self.current_tab, Tab::Movies, "🎬 MOVIES");
                ui.selectable_value(&mut self.current_tab, Tab::Series, "📺 SERIES");
                ui.selectable_value(&mut self.current_tab, Tab::Favorites, "⭐ FAVORITES");
                ui.selectable_value(&mut self.current_tab, Tab::Recent, "🕐 RECENT");
                ui.selectable_value(&mut self.current_tab, Tab::Info, "ℹ️ INFO");
                
                // Push Console to the right
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.selectable_value(&mut self.current_tab, Tab::Console, "🖥 CONSOLE");
                });
            });
            
            ui.separator();

            // Search bar (not for Info, Favorites, Recent, or Console tab)
            if self.current_tab != Tab::Info && self.current_tab != Tab::Favorites && self.current_tab != Tab::Recent && self.current_tab != Tab::Console {
                ui.horizontal(|ui| {
                    if !self.navigation_stack.is_empty() {
                        if ui.button("⬅ Back").clicked() {
                            self.go_back();
                        }
                    }
                    
                    ui.label("");
                    ui.add(egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Search...")
                        .desired_width(150.0));
                    
                    // Sort dropdown - show for Live, Movies, Series tabs
                    match self.current_tab {
                        Tab::Live => {
                            let item_count = if !self.current_channels.is_empty() && 
                               matches!(self.navigation_stack.last(), Some(NavigationLevel::Channels(_))) {
                                self.current_channels.len()
                            } else {
                                self.live_categories.len()
                            };
                            if item_count > 0 {
                                ui.separator();
                                egui::ComboBox::from_id_salt("live_sort_top")
                                    .selected_text(format!("{} {}", self.live_sort_order.icon(), self.live_sort_order.label()))
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_value(&mut self.live_sort_order, SortOrder::Default, "⇅ Default").changed() {
                                            self.config.live_sort_order = self.live_sort_order;
                                            self.config.save();
                                        }
                                        if ui.selectable_value(&mut self.live_sort_order, SortOrder::NameAsc, "↑ Name A-Z").changed() {
                                            self.config.live_sort_order = self.live_sort_order;
                                            self.config.save();
                                        }
                                        if ui.selectable_value(&mut self.live_sort_order, SortOrder::NameDesc, "↓ Name Z-A").changed() {
                                            self.config.live_sort_order = self.live_sort_order;
                                            self.config.save();
                                        }
                                    });
                                ui.label(format!("({})", item_count));
                            }
                        }
                        Tab::Movies => {
                            let item_count = if !self.current_channels.is_empty() && 
                               matches!(self.navigation_stack.last(), Some(NavigationLevel::Channels(_))) {
                                self.current_channels.len()
                            } else {
                                self.movie_categories.len()
                            };
                            if item_count > 0 {
                                ui.separator();
                                egui::ComboBox::from_id_salt("movie_sort_top")
                                    .selected_text(format!("{} {}", self.movie_sort_order.icon(), self.movie_sort_order.label()))
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_value(&mut self.movie_sort_order, SortOrder::Default, "⇅ Default").changed() {
                                            self.config.movie_sort_order = self.movie_sort_order;
                                            self.config.save();
                                        }
                                        if ui.selectable_value(&mut self.movie_sort_order, SortOrder::NameAsc, "↑ Name A-Z").changed() {
                                            self.config.movie_sort_order = self.movie_sort_order;
                                            self.config.save();
                                        }
                                        if ui.selectable_value(&mut self.movie_sort_order, SortOrder::NameDesc, "↓ Name Z-A").changed() {
                                            self.config.movie_sort_order = self.movie_sort_order;
                                            self.config.save();
                                        }
                                    });
                                ui.label(format!("({})", item_count));
                            }
                        }
                        Tab::Series => {
                            let item_count = if !self.current_series.is_empty() {
                                self.current_series.len()
                            } else {
                                self.series_categories.len()
                            };
                            // Only show when not viewing episodes/seasons
                            let show_sort = self.current_episodes.is_empty() && self.current_seasons.is_empty() && item_count > 0;
                            if show_sort {
                                ui.separator();
                                egui::ComboBox::from_id_salt("series_sort_top")
                                    .selected_text(format!("{} {}", self.series_sort_order.icon(), self.series_sort_order.label()))
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_value(&mut self.series_sort_order, SortOrder::Default, "⇅ Default").changed() {
                                            self.config.series_sort_order = self.series_sort_order;
                                            self.config.save();
                                        }
                                        if ui.selectable_value(&mut self.series_sort_order, SortOrder::NameAsc, "↑ Name A-Z").changed() {
                                            self.config.series_sort_order = self.series_sort_order;
                                            self.config.save();
                                        }
                                        if ui.selectable_value(&mut self.series_sort_order, SortOrder::NameDesc, "↓ Name Z-A").changed() {
                                            self.config.series_sort_order = self.series_sort_order;
                                            self.config.save();
                                        }
                                    });
                                ui.label(format!("({})", item_count));
                            }
                        }
                        _ => {}
                    }
                });
                ui.separator();
            }

            // Content area - split into channels (left) and EPG grid (right)
            let has_epg = self.epg_data.is_some();
            let show_epg_panel = has_epg && 
                (self.current_tab == Tab::Live || 
                 self.current_tab == Tab::Favorites || 
                 self.current_tab == Tab::Recent);
            
            if show_epg_panel {
                // Two-column layout: channels fixed on left, EPG fills remaining space
                egui::SidePanel::left("channels_panel")
                    .resizable(true)
                    .default_width(300.0)
                    .min_width(200.0)
                    .max_width(450.0)
                    .show_inside(ui, |ui| {
                        // Restore scroll position if pending
                        let scroll_offset = self.pending_scroll_restore.take();
                        
                        let mut scroll_area = egui::ScrollArea::both()
                            .id_salt("channels_scroll")
                            .auto_shrink([false, false]);
                        
                        if let Some(offset) = scroll_offset {
                            scroll_area = scroll_area.vertical_scroll_offset(offset);
                        }
                        
                        let scroll_output = scroll_area.show(ui, |ui| {
                                match self.current_tab {
                                    Tab::Live => self.show_live_tab(ui),
                                    Tab::Movies => self.show_movies_tab(ui),
                                    Tab::Series => self.show_series_tab(ui),
                                    Tab::Favorites => self.show_favorites_tab(ui),
                                    Tab::Recent => self.show_recent_tab(ui),
                                    Tab::Info => self.show_info_tab(ui),
                                    Tab::Console => self.show_console_tab(ui),
                                }
                            });
                        
                        // Track current scroll position
                        self.current_scroll_offset = scroll_output.state.offset.y;
                    });
                
                // EPG grid fills remaining space on right
                egui::CentralPanel::default()
                    .show_inside(ui, |ui| {
                        self.show_epg_grid_panel(ui);
                    });
            } else {
                // No EPG - full width for content
                // Restore scroll position if pending
                let scroll_offset = self.pending_scroll_restore.take();
                
                let mut scroll_area = egui::ScrollArea::vertical()
                    .id_salt("channels_scroll")
                    .auto_shrink([false, false]);
                
                if let Some(offset) = scroll_offset {
                    scroll_area = scroll_area.vertical_scroll_offset(offset);
                }
                
                let scroll_output = scroll_area.show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        match self.current_tab {
                            Tab::Live => self.show_live_tab(ui),
                            Tab::Movies => self.show_movies_tab(ui),
                            Tab::Series => self.show_series_tab(ui),
                            Tab::Favorites => self.show_favorites_tab(ui),
                            Tab::Recent => self.show_recent_tab(ui),
                            Tab::Info => self.show_info_tab(ui),
                            Tab::Console => self.show_console_tab(ui),
                        }
                    });
                
                // Track current scroll position
                self.current_scroll_offset = scroll_output.state.offset.y;
            }
        });

        // Address Book Window
        // Unified Playlist Manager Window
        if self.show_playlist_manager {
            egui::Window::new("📺 Playlist Manager")
                .collapsible(false)
                .resizable(true)
                .min_width(550.0)
                .show(ctx, |ui| {
                    // Add new playlist section
                    ui.heading("Add Playlist");
                    
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.playlist_name_input)
                            .hint_text("My Playlist")
                            .desired_width(150.0));
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("URL:");
                        ui.add(egui::TextEdit::singleline(&mut self.playlist_url_input)
                            .hint_text("http://server.com/playlist.m3u or Xtream URL")
                            .desired_width(400.0));
                    });
                    
                    ui.horizontal(|ui| {
                        // Add as M3U playlist
                        if ui.button("➕ Add M3U/XSPF").on_hover_text("Add as M3U/M3U8/XSPF playlist").clicked() {
                            if !self.playlist_url_input.is_empty() {
                                let name = if self.playlist_name_input.is_empty() {
                                    self.playlist_url_input.split('/').last()
                                        .unwrap_or("Playlist")
                                        .split('?').next()
                                        .unwrap_or("Playlist")
                                        .to_string()
                                } else {
                                    self.playlist_name_input.clone()
                                };
                                
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                
                                let entry = PlaylistEntry {
                                    name: name.clone(),
                                    entry_type: PlaylistType::M3U { url: self.playlist_url_input.clone() },
                                    saved_at: now,
                                    enabled: true,
                                    auto_login: false,
                                    auto_update_days: 0,
                                    last_updated: now,
                                    epg_url: String::new(),
                                    epg_time_offset: 0.0,
                                    epg_auto_update_index: 3,
                                    epg_show_actual_time: false,
                                    epg_last_updated: 0,
                                    external_player: String::new(),
                                    buffer_seconds: 5,
                                    connection_quality: ConnectionQuality::Normal,
                                    selected_user_agent: 0,
                                    custom_user_agent: String::new(),
                                    use_custom_user_agent: false,
                                    pass_user_agent_to_player: true,
                                };
                                
                                // Add if not duplicate
                                if !self.playlist_entries.iter().any(|e| {
                                    matches!(&e.entry_type, PlaylistType::M3U { url } if url == &self.playlist_url_input)
                                }) {
                                    self.playlist_entries.push(entry);
                                    save_playlist_entries(&self.playlist_entries);
                                    self.status_message = format!("Added playlist '{}'", name);
                                }
                                
                                self.playlist_name_input.clear();
                                self.playlist_url_input.clear();
                            }
                        }
                        
                        // Add as Xtream
                        if ui.button("➕ Add Xtream").on_hover_text("Extract Xtream credentials from M3U Plus URL").clicked() {
                            if !self.playlist_url_input.is_empty() {
                                // Try to extract Xtream credentials
                                if let Some((server, username, password)) = Self::parse_xtream_url(&self.playlist_url_input) {
                                    let name = if self.playlist_name_input.is_empty() {
                                        format!("{}@{}", username, server.split('/').nth(2).unwrap_or(&server))
                                    } else {
                                        self.playlist_name_input.clone()
                                    };
                                    
                                    let now = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs() as i64;
                                    
                                    let entry = PlaylistEntry {
                                        name: name.clone(),
                                        entry_type: PlaylistType::Xtream { server, username, password },
                                        saved_at: now,
                                        enabled: true,
                                        auto_login: false,
                                        auto_update_days: 0,
                                        last_updated: now,
                                        epg_url: String::new(),
                                        epg_time_offset: 0.0,
                                        epg_auto_update_index: 3,
                                        epg_show_actual_time: false,
                                        epg_last_updated: 0,
                                        external_player: String::new(),
                                        buffer_seconds: 5,
                                        connection_quality: ConnectionQuality::Normal,
                                        selected_user_agent: 0,
                                        custom_user_agent: String::new(),
                                        use_custom_user_agent: false,
                                        pass_user_agent_to_player: true,
                                    };
                                    
                                    // Add if not duplicate
                                    if !self.playlist_entries.iter().any(|e| {
                                        matches!(&e.entry_type, PlaylistType::Xtream { server: s, username: u, .. } 
                                            if matches!(&entry.entry_type, PlaylistType::Xtream { server: s2, username: u2, .. } 
                                                if s == s2 && u == u2))
                                    }) {
                                        self.playlist_entries.push(entry);
                                        save_playlist_entries(&self.playlist_entries);
                                        self.status_message = format!("Added Xtream '{}'", name);
                                    }
                                    
                                    self.playlist_name_input.clear();
                                    self.playlist_url_input.clear();
                                } else {
                                    self.status_message = "Could not extract Xtream credentials from URL".to_string();
                                }
                            }
                        }
                        
                        // Save current Xtream session
                        if !self.server.is_empty() && self.logged_in {
                            if ui.button("💾 Save Current").on_hover_text("Save current Xtream session with all settings").clicked() {
                                let name = if self.playlist_name_input.is_empty() {
                                    format!("{}@{}", self.username, self.server.split('/').nth(2).unwrap_or(&self.server))
                                } else {
                                    self.playlist_name_input.clone()
                                };
                                
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                
                                // Check if entry exists to preserve auto_login, enabled, and auto_update settings
                                let existing_entry = self.playlist_entries.iter().find(|e| {
                                    matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                                        if server == &self.server && username == &self.username)
                                });
                                let existing_auto_login = existing_entry.map(|e| e.auto_login).unwrap_or(false);
                                let existing_enabled = existing_entry.map(|e| e.enabled).unwrap_or(true);
                                let existing_auto_update_days = existing_entry.map(|e| e.auto_update_days).unwrap_or(0);
                                let existing_last_updated = existing_entry.map(|e| e.last_updated).unwrap_or(now);
                                let existing_epg_last_updated = existing_entry.map(|e| e.epg_last_updated).unwrap_or(0);
                                
                                let entry = PlaylistEntry {
                                    name: name.clone(),
                                    entry_type: PlaylistType::Xtream {
                                        server: self.server.clone(),
                                        username: self.username.clone(),
                                        password: self.password.clone(),
                                    },
                                    saved_at: now,
                                    enabled: existing_enabled,
                                    auto_login: existing_auto_login,
                                    auto_update_days: existing_auto_update_days,
                                    last_updated: existing_last_updated,
                                    epg_url: self.epg_url_input.clone(),
                                    epg_time_offset: self.epg_time_offset,
                                    epg_auto_update_index: self.epg_auto_update.to_index(),
                                    epg_show_actual_time: self.epg_show_actual_time,
                                    epg_last_updated: existing_epg_last_updated,
                                    external_player: self.external_player.clone(),
                                    buffer_seconds: self.buffer_seconds,
                                    connection_quality: self.connection_quality,
                                    selected_user_agent: self.selected_user_agent,
                                    custom_user_agent: self.custom_user_agent.clone(),
                                    use_custom_user_agent: self.use_custom_user_agent,
                                    pass_user_agent_to_player: self.pass_user_agent_to_player,
                                };
                                
                                // Update existing or add new
                                if let Some(existing) = self.playlist_entries.iter_mut().find(|e| {
                                    matches!(&e.entry_type, PlaylistType::Xtream { server, username, .. } 
                                        if server == &self.server && username == &self.username)
                                }) {
                                    *existing = entry;
                                } else {
                                    self.playlist_entries.push(entry);
                                }
                                save_playlist_entries(&self.playlist_entries);
                                self.status_message = format!("Saved '{}'", name);
                                self.playlist_name_input.clear();
                            }
                        }
                    });
                    
                    ui.separator();
                    
                    // Saved playlists section
                    ui.heading("Saved Playlists");
                    
                    if self.playlist_entries.is_empty() {
                        ui.label(egui::RichText::new("No saved playlists").weak());
                    } else {
                        let mut to_delete: Option<usize> = None;
                        let mut to_load_xtream_idx: Option<usize> = None;
                        let mut to_load_m3u: Option<(String, String)> = None; // url, name
                        let mut to_toggle_auto_login: Option<usize> = None;
                        let mut to_toggle_enabled: Option<usize> = None;
                        let mut to_change_auto_update: Option<(usize, u8)> = None; // (index, new_days)
                        
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for (i, entry) in self.playlist_entries.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        match &entry.entry_type {
                                            PlaylistType::Xtream { .. } => {
                                                // Enabled toggle
                                                let enabled_text = if entry.enabled { "✓" } else { "○" };
                                                let enabled_color = if entry.enabled { egui::Color32::from_rgb(100, 200, 100) } else { egui::Color32::GRAY };
                                                if ui.button(egui::RichText::new(enabled_text).color(enabled_color))
                                                    .on_hover_text(if entry.enabled { "Enabled - click to disable" } else { "Disabled - click to enable" })
                                                    .clicked() 
                                                {
                                                    to_toggle_enabled = Some(i);
                                                }
                                                
                                                // Play button (only if enabled)
                                                if entry.enabled {
                                                    if ui.button("▶").on_hover_text("Login to this Xtream server").clicked() {
                                                        to_load_xtream_idx = Some(i);
                                                    }
                                                }
                                                ui.label("🔑");
                                                let name_text = if entry.enabled {
                                                    egui::RichText::new(&entry.name).strong()
                                                } else {
                                                    egui::RichText::new(&entry.name).weak().strikethrough()
                                                };
                                                ui.label(name_text);
                                            }
                                            PlaylistType::M3U { url } => {
                                                // Enabled toggle
                                                let enabled_text = if entry.enabled { "✓" } else { "○" };
                                                let enabled_color = if entry.enabled { egui::Color32::from_rgb(100, 200, 100) } else { egui::Color32::GRAY };
                                                if ui.button(egui::RichText::new(enabled_text).color(enabled_color))
                                                    .on_hover_text(if entry.enabled { "Enabled - click to disable" } else { "Disabled - click to enable" })
                                                    .clicked() 
                                                {
                                                    to_toggle_enabled = Some(i);
                                                }
                                                
                                                // Play button (only if enabled)
                                                if entry.enabled {
                                                    if ui.button("▶").on_hover_text("Load this playlist").clicked() {
                                                        to_load_m3u = Some((url.clone(), entry.name.clone()));
                                                    }
                                                }
                                                ui.label("📺");
                                                let name_text = if entry.enabled {
                                                    egui::RichText::new(&entry.name).strong()
                                                } else {
                                                    egui::RichText::new(&entry.name).weak().strikethrough()
                                                };
                                                ui.label(name_text);
                                            }
                                        }
                                        
                                        // Auto-login toggle for Xtream entries (only if enabled)
                                        if entry.enabled && matches!(entry.entry_type, PlaylistType::Xtream { .. }) {
                                            let auto_text = if entry.auto_login { "🔄" } else { "⏸" };
                                            let hover = if entry.auto_login { "Auto-login ON - click to disable" } else { "Auto-login OFF - click to enable" };
                                            if ui.button(egui::RichText::new(auto_text).size(14.0)).on_hover_text(hover).clicked() {
                                                to_toggle_auto_login = Some(i);
                                            }
                                        }
                                        
                                        // Auto-update dropdown (for enabled entries)
                                        if entry.enabled {
                                            let update_text = match entry.auto_update_days {
                                                0 => "⏰Off",
                                                1 => "⏰1d",
                                                2 => "⏰2d",
                                                3 => "⏰3d",
                                                4 => "⏰4d",
                                                5 => "⏰5d",
                                                _ => "⏰?",
                                            };
                                            egui::ComboBox::from_id_salt(format!("auto_update_{}", i))
                                                .selected_text(update_text)
                                                .width(55.0)
                                                .show_ui(ui, |ui| {
                                                    if ui.selectable_label(entry.auto_update_days == 0, "Off").clicked() {
                                                        to_change_auto_update = Some((i, 0));
                                                    }
                                                    if ui.selectable_label(entry.auto_update_days == 1, "1 day").clicked() {
                                                        to_change_auto_update = Some((i, 1));
                                                    }
                                                    if ui.selectable_label(entry.auto_update_days == 2, "2 days").clicked() {
                                                        to_change_auto_update = Some((i, 2));
                                                    }
                                                    if ui.selectable_label(entry.auto_update_days == 3, "3 days").clicked() {
                                                        to_change_auto_update = Some((i, 3));
                                                    }
                                                    if ui.selectable_label(entry.auto_update_days == 4, "4 days").clicked() {
                                                        to_change_auto_update = Some((i, 4));
                                                    }
                                                    if ui.selectable_label(entry.auto_update_days == 5, "5 days").clicked() {
                                                        to_change_auto_update = Some((i, 5));
                                                    }
                                                });
                                        }
                                        
                                        // Saved date
                                        if entry.saved_at > 0 {
                                            ui.label(egui::RichText::new(Self::format_datetime(entry.saved_at)).small().weak());
                                        }
                                        
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.button("🗑").on_hover_text("Delete").clicked() {
                                                to_delete = Some(i);
                                            }
                                        });
                                    });
                                    ui.separator();
                                }
                            });
                        
                        // Handle enabled toggle
                        if let Some(i) = to_toggle_enabled {
                            self.playlist_entries[i].enabled = !self.playlist_entries[i].enabled;
                            // If disabling, also disable auto-login
                            if !self.playlist_entries[i].enabled {
                                self.playlist_entries[i].auto_login = false;
                            }
                            save_playlist_entries(&self.playlist_entries);
                        }
                        
                        // Handle auto-login toggle
                        if let Some(i) = to_toggle_auto_login {
                            self.playlist_entries[i].auto_login = !self.playlist_entries[i].auto_login;
                            save_playlist_entries(&self.playlist_entries);
                        }
                        
                        // Handle auto-update change
                        if let Some((i, days)) = to_change_auto_update {
                            self.playlist_entries[i].auto_update_days = days;
                            // Reset last_updated when enabling to prevent immediate update
                            if days > 0 && self.playlist_entries[i].last_updated == 0 {
                                self.playlist_entries[i].last_updated = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                            }
                            save_playlist_entries(&self.playlist_entries);
                        }
                        
                        // Handle actions
                        if let Some(idx) = to_load_xtream_idx {
                            let entry = &self.playlist_entries[idx];
                            if let PlaylistType::Xtream { server, username, password } = &entry.entry_type {
                                // Server credentials
                                self.server = server.clone();
                                self.username = username.clone();
                                self.password = password.clone();
                                // EPG settings
                                if !entry.epg_url.is_empty() {
                                    self.epg_url_input = entry.epg_url.clone();
                                }
                                self.epg_time_offset = entry.epg_time_offset;
                                self.epg_auto_update = EpgAutoUpdate::from_index(entry.epg_auto_update_index);
                                self.epg_show_actual_time = entry.epg_show_actual_time;
                                // Clear EPG data for new provider
                                self.epg_data = None;
                                self.epg_last_update = None;
                                self.epg_startup_loaded = false;
                                // Player settings
                                if !entry.external_player.is_empty() {
                                    self.external_player = entry.external_player.clone();
                                }
                                self.buffer_seconds = entry.buffer_seconds;
                                self.connection_quality = entry.connection_quality;
                                // User agent settings
                                self.selected_user_agent = entry.selected_user_agent;
                                self.custom_user_agent = entry.custom_user_agent.clone();
                                self.use_custom_user_agent = entry.use_custom_user_agent;
                                self.pass_user_agent_to_player = entry.pass_user_agent_to_player;
                                
                                self.show_playlist_manager = false;
                                self.login();
                            }
                        }
                        
                        if let Some((url, name)) = to_load_m3u {
                            self.load_playlist_with_name(&url, &name);
                            self.show_playlist_manager = false;
                        }
                        
                        if let Some(i) = to_delete {
                            let entry = &self.playlist_entries[i];
                            let name = entry.name.clone();
                            
                            // Remove related favorites/recent for M3U playlists
                            if matches!(entry.entry_type, PlaylistType::M3U { .. }) {
                                self.favorites.retain(|f| f.playlist_source.as_ref() != Some(&name));
                                self.recent_watched.retain(|f| f.playlist_source.as_ref() != Some(&name));
                                self.config.favorites_json = serde_json::to_string(&self.favorites).unwrap_or_default();
                                self.config.recent_watched_json = serde_json::to_string(&self.recent_watched).unwrap_or_default();
                                self.config.save();
                            }
                            
                            self.playlist_entries.remove(i);
                            save_playlist_entries(&self.playlist_entries);
                            self.status_message = format!("Removed '{}'", name);
                        }
                    }
                    
                    // Currently loaded playlists
                    if !self.playlist_sources.is_empty() {
                        ui.separator();
                        ui.heading("Currently Loaded");
                        
                        let mut to_unload: Option<usize> = None;
                        
                        let playlist_infos: Vec<_> = self.playlist_sources.iter().enumerate().map(|(i, (start_idx, name))| {
                            let end_idx = self.playlist_sources.get(i + 1)
                                .map(|(next_start, _)| *next_start)
                                .unwrap_or(self.current_channels.len());
                            (i, name.clone(), end_idx - start_idx)
                        }).collect();
                        
                        for (idx, name, count) in &playlist_infos {
                            ui.horizontal(|ui| {
                                ui.label(format!("📺 {} ({} channels)", name, count));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("✕").on_hover_text("Unload").clicked() {
                                        to_unload = Some(*idx);
                                    }
                                });
                            });
                        }
                        
                        if let Some(idx) = to_unload {
                            self.unload_playlist(idx);
                        }
                    }
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            self.show_playlist_manager = false;
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(egui::RichText::new("⚠ Reset All").color(egui::Color32::from_rgb(200, 80, 80)))
                                .on_hover_text("Clear all settings, playlists, favorites")
                                .clicked() 
                            {
                                self.show_reset_confirm = true;
                            }
                        });
                    });
                });
        }

        // Reset Settings Confirmation Dialog
        if self.show_reset_confirm {
            egui::Window::new("⚠ Reset All Settings")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("Are you sure you want to reset all settings?").strong());
                    ui.add_space(10.0);
                    
                    ui.label("This will permanently delete:");
                    ui.label("  • Server credentials");
                    ui.label("  • Saved playlists");
                    ui.label("  • Favorites");
                    ui.label("  • Recent watch history");
                    ui.label("  • EPG settings");
                    ui.label("  • Player settings");
                    
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new("This action cannot be undone!").color(egui::Color32::from_rgb(200, 80, 80)));
                    ui.add_space(10.0);
                    
                    ui.horizontal(|ui| {
                        if ui.button("Cancel").clicked() {
                            self.show_reset_confirm = false;
                        }
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(egui::RichText::new("Reset Everything").color(egui::Color32::from_rgb(200, 80, 80))).clicked() {
                                self.reset_to_defaults();
                                self.playlist_entries.clear();
                                save_playlist_entries(&self.playlist_entries);
                                self.show_reset_confirm = false;
                                self.show_playlist_manager = false;
                                self.status_message = "All settings have been reset".to_string();
                            }
                        });
                    });
                });
        }

        // User Agent Dialog
        if self.show_user_agent_dialog {
            egui::Window::new("🌐 User Agent Settings")
                .collapsible(false)
                .resizable(true)
                .min_width(500.0)
                .show(ctx, |ui| {
                    ui.heading("Select User Agent");
                    ui.separator();
                    
                    // Preset user agents
                    ui.label("Preset User Agents:");
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for (i, (name, _ua)) in USER_AGENTS.iter().enumerate() {
                                let is_selected = !self.use_custom_user_agent && self.selected_user_agent == i;
                                if ui.selectable_label(is_selected, *name).clicked() {
                                    self.selected_user_agent = i;
                                    self.use_custom_user_agent = false;
                                }
                            }
                        });
                    
                    ui.separator();
                    
                    // Custom user agent
                    ui.checkbox(&mut self.use_custom_user_agent, "Use custom User Agent");
                    
                    if self.use_custom_user_agent {
                        ui.add(egui::TextEdit::multiline(&mut self.custom_user_agent)
                            .hint_text("Enter custom user agent string...")
                            .desired_width(f32::INFINITY)
                            .desired_rows(2));
                    }
                    
                    ui.separator();
                    
                    // Pass user agent to player option
                    ui.checkbox(&mut self.pass_user_agent_to_player, "Pass User Agent to media player");
                    ui.label("[i] Disable if your player doesn't support user agent arguments (e.g. MPC-HC, PotPlayer)");
                    
                    ui.separator();
                    
                    // Current user agent display
                    ui.label("Current User Agent:");
                    let current_ua = self.get_user_agent();
                    ui.add(egui::TextEdit::multiline(&mut current_ua.clone())
                        .desired_width(f32::INFINITY)
                        .desired_rows(2)
                        .interactive(false));
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Save & Close").clicked() {
                            // Save to config
                            self.config.selected_user_agent = self.selected_user_agent;
                            self.config.custom_user_agent = self.custom_user_agent.clone();
                            self.config.use_custom_user_agent = self.use_custom_user_agent;
                            self.config.pass_user_agent_to_player = self.pass_user_agent_to_player;
                            self.config.save();
                            self.show_user_agent_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            // Revert changes
                            self.selected_user_agent = self.config.selected_user_agent;
                            self.custom_user_agent = self.config.custom_user_agent.clone();
                            self.use_custom_user_agent = self.config.use_custom_user_agent;
                            self.pass_user_agent_to_player = self.config.pass_user_agent_to_player;
                            self.show_user_agent_dialog = false;
                        }
                    });
                });
        }
        
        // EPG Dialog Window
        if self.show_epg_dialog {
            egui::Window::new("📺 EPG - Electronic Program Guide")
                .collapsible(false)
                .resizable(true)
                .min_width(450.0)
                .show(ctx, |ui| {
                    ui.heading("Load Program Guide");
                    ui.separator();
                    
                    ui.label("Enter XMLTV EPG URL:");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.epg_url_input)
                            .hint_text("http://provider.com/xmltv.php?username=...&password=...")
                            .desired_width(350.0));
                        
                        let button_text = if self.epg_loading { "⏳" } else { "📥" };
                        if ui.button(button_text)
                            .on_hover_text("Load EPG")
                            .clicked() && !self.epg_loading 
                        {
                            self.load_epg();
                        }
                        
                        // Reload button - force re-download
                        if ui.button("🔄")
                            .on_hover_text("Force reload EPG")
                            .clicked() && !self.epg_loading && !self.epg_url_input.is_empty()
                        {
                            self.epg_last_update = None; // Reset last update to force reload
                            self.load_epg();
                        }
                    });
                    
                    // Auto-update dropdown
                    ui.horizontal(|ui| {
                        ui.label("Auto-update:");
                        egui::ComboBox::from_id_salt("epg_auto_update")
                            .selected_text(self.epg_auto_update.label())
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Off, "Off");
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Hours6, "6 Hours");
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Hours12, "12 Hours");
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Day1, "1 Day");
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Days2, "2 Days");
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Days3, "3 Days");
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Days4, "4 Days");
                                ui.selectable_value(&mut self.epg_auto_update, EpgAutoUpdate::Days5, "5 Days");
                            });
                        
                        // Show last update time
                        if let Some(last) = self.epg_last_update {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            let ago = now - last;
                            let ago_str = if ago < 3600 {
                                format!("{}m ago", ago / 60)
                            } else if ago < 86400 {
                                format!("{}h ago", ago / 3600)
                            } else {
                                format!("{}d ago", ago / 86400)
                            };
                            ui.label(egui::RichText::new(format!("(Last: {})", ago_str)).small().color(egui::Color32::GRAY));
                        }
                    });
                    
                    // Time offset slider
                    ui.horizontal(|ui| {
                        ui.label("Time Offset:");
                        if ui.button("−").clicked() {
                            self.epg_time_offset = (self.epg_time_offset - 0.5).max(-60.0);
                        }
                        ui.add(egui::Slider::new(&mut self.epg_time_offset, -60.0..=60.0)
                            .step_by(0.5)
                            .show_value(false)
                            .trailing_fill(true));
                        if ui.button("+").clicked() {
                            self.epg_time_offset = (self.epg_time_offset + 0.5).min(60.0);
                        }
                        let sign = if self.epg_time_offset >= 0.0 { "+" } else { "" };
                        ui.label(format!("{}{:.1} hours", sign, self.epg_time_offset));
                        if self.epg_time_offset != 0.0 {
                            if ui.small_button("Reset").clicked() {
                                self.epg_time_offset = 0.0;
                            }
                        }
                    });
                    
                    // EPG Grid display mode
                    ui.horizontal(|ui| {
                        ui.label("Grid Header:");
                        ui.selectable_value(&mut self.epg_show_actual_time, false, "Offset (Now, +30m...)")
                            .on_hover_text("Show relative time offsets");
                        ui.selectable_value(&mut self.epg_show_actual_time, true, "Time (8:00, 8:30...)")
                            .on_hover_text("Show actual times");
                    });
                    
                    if !self.epg_status.is_empty() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            if self.epg_loading {
                                ui.spinner();
                            }
                            let color = if self.epg_status.starts_with("Error") {
                                egui::Color32::RED
                            } else if self.epg_status.starts_with("Loaded") {
                                egui::Color32::GREEN
                            } else {
                                egui::Color32::YELLOW
                            };
                            ui.label(egui::RichText::new(&self.epg_status).color(color));
                        });
                    }
                    
                    if let Some(ref epg) = self.epg_data {
                        ui.separator();
                        ui.heading("EPG Statistics");
                        
                        egui::Grid::new("epg_stats")
                            .num_columns(2)
                            .spacing([20.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Channels:");
                                ui.label(format!("{}", epg.channels.len()));
                                ui.end_row();
                                
                                ui.label("Programs:");
                                ui.label(format!("{}", epg.program_count()));
                                ui.end_row();
                            });
                    }
                    
                    ui.separator();
                    
                    // Close on left, Clear EPG Data on right - same row
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            self.show_epg_dialog = false;
                        }
                        
                        if self.epg_data.is_some() {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("🗑 Clear EPG Data").clicked() {
                                    self.epg_data = None;
                                    self.epg_last_update = None;
                                    self.epg_status = "EPG data cleared".to_string();
                                    self.log("[INFO] EPG data cleared");
                                }
                            });
                        }
                    });
                });
        }
        
        // Internal Player Window
        if self.show_internal_player {
            let mut open = self.show_internal_player;
            egui::Window::new("🎬 Internal Player")
                .open(&mut open)
                .resizable(true)
                .default_size([860.0, 540.0])
                .show(ctx, |ui| {
                    self.internal_player.show(ctx, ui);
                });
            
            if !open {
                self.show_internal_player = false;
                self.internal_player.stop();
            }
        }
    }
}

impl IPTVApp {
    fn show_live_tab(&mut self, ui: &mut egui::Ui) {
        self.show_category_tab(ui, "live");
    }

    fn show_movies_tab(&mut self, ui: &mut egui::Ui) {
        self.show_category_tab(ui, "movie");
    }

    fn show_category_tab(&mut self, ui: &mut egui::Ui, stream_type: &str) {
        let categories = match stream_type {
            "live" => &self.live_categories,
            "movie" => &self.movie_categories,
            _ => return,
        };

        // If we have channels loaded, show them
        if !self.current_channels.is_empty() && 
           matches!(self.navigation_stack.last(), Some(NavigationLevel::Channels(_))) {
            let search = self.search_query.to_lowercase();
            let category_name = if let Some(NavigationLevel::Channels(name)) = self.navigation_stack.last() {
                name.clone()
            } else {
                String::new()
            };
            
            // Clone and sort channels
            let mut channels: Vec<_> = self.current_channels.clone();
            
            // Apply sort order based on stream type
            let sort_order = match stream_type {
                "live" => self.live_sort_order,
                "movie" => self.movie_sort_order,
                _ => SortOrder::Default,
            };
            
            match sort_order {
                SortOrder::NameAsc => channels.sort_by_cached_key(|c| c.name.to_lowercase()),
                SortOrder::NameDesc => {
                    channels.sort_by_cached_key(|c| c.name.to_lowercase());
                    channels.reverse();
                }
                SortOrder::Default => {} // Keep server order
            }
            
            let playlist_sources = &self.playlist_sources;
            let mut toggle_fav: Option<FavoriteItem> = None;
            let mut to_play: Option<Channel> = None;
            
            for (idx, channel) in channels.iter().enumerate() {
                // Show separator header for playlist sources (only in playlist mode)
                if self.playlist_mode && !playlist_sources.is_empty() {
                    for (start_idx, source_name) in playlist_sources {
                        if *start_idx == idx {
                            ui.add_space(8.0);
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new(format!("📺 {}", source_name))
                                    .strong()
                                    .size(14.0)
                                    .color(egui::Color32::from_rgb(100, 149, 237)));
                            });
                            ui.separator();
                            ui.add_space(4.0);
                        }
                    }
                }
                
                let display_name = Self::sanitize_text(&channel.name);
                if !search.is_empty() && !display_name.to_lowercase().contains(&search) {
                    continue;
                }
                
                let is_fav = self.is_favorite(&channel.url);
                
                ui.horizontal(|ui| {
                    // Favorite checkbox - use colored text for better visibility
                    let fav_text = if is_fav { 
                        egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)
                    } else { 
                        egui::RichText::new("☆").size(18.0).color(egui::Color32::GRAY)
                    };
                    if ui.button(fav_text).on_hover_text(if is_fav { "Remove from favorites" } else { "Add to favorites" }).clicked() {
                        toggle_fav = Some(FavoriteItem {
                            name: channel.name.clone(),
                            url: channel.url.clone(),
                            stream_type: stream_type.to_string(),
                            stream_id: channel.stream_id,
                            series_id: None,
                            category_name: category_name.clone(),
                            container_extension: channel.container_extension.clone(),
                            season_num: None,
                            episode_num: None,
                            series_name: None,
                            playlist_source: channel.playlist_source.clone(),
                        });
                    }
                    
                    if ui.button("▶").clicked() {
                        to_play = Some(channel.clone());
                    }
                    
                    ui.label(egui::RichText::new(&display_name).strong());
                    
                    // Show EPG info if available (only for live streams)
                    if stream_type == "live" {
                        self.show_epg_inline(ui, &channel.name, channel.epg_channel_id.as_deref());
                    }
                });
            }
            
            if let Some(channel) = to_play {
                self.play_channel(&channel);
            }
            
            if let Some(fav) = toggle_fav {
                self.toggle_favorite(fav);
            }
            return;
        }

        // Show categories (sorted)
        let search = self.search_query.to_lowercase();
        let mut clicked_category: Option<(String, String)> = None;
        
        // Clone and sort categories
        let mut sorted_categories: Vec<_> = categories.clone();
        let sort_order = match stream_type {
            "live" => self.live_sort_order,
            "movie" => self.movie_sort_order,
            _ => SortOrder::Default,
        };
        
        match sort_order {
            SortOrder::NameAsc => sorted_categories.sort_by_cached_key(|c| c.category_name.to_lowercase()),
            SortOrder::NameDesc => {
                sorted_categories.sort_by_cached_key(|c| c.category_name.to_lowercase());
                sorted_categories.reverse();
            }
            SortOrder::Default => {} // Keep server order
        }
        
        for cat in &sorted_categories {
            let display_name = Self::sanitize_text(&cat.category_name);
            if !search.is_empty() && !display_name.to_lowercase().contains(&search) {
                continue;
            }
            
            if ui.button(&display_name).clicked() {
                clicked_category = Some((cat.category_id.clone(), cat.category_name.clone()));
            }
        }
        
        if let Some((cat_id, cat_name)) = clicked_category {
            self.save_scroll_position(ui.ctx());
            self.navigation_stack.push(NavigationLevel::Channels(cat_name));
            self.fetch_channels(&cat_id, stream_type);
        }
    }

    fn show_series_tab(&mut self, ui: &mut egui::Ui) {
        let search = self.search_query.to_lowercase();

        // Episodes level
        if !self.current_episodes.is_empty() {
            if let Some(NavigationLevel::Episodes(series_id, _)) = self.navigation_stack.last() {
                let sid = *series_id;
                let episodes: Vec<_> = self.current_episodes.clone();
                let mut to_play: Option<(Episode, i64)> = None;
                
                for ep in &episodes {
                    let display_title = Self::sanitize_text(&ep.title);
                    if !search.is_empty() && !display_title.to_lowercase().contains(&search) {
                        continue;
                    }
                    
                    ui.horizontal(|ui| {
                        if ui.button("▶").clicked() {
                            to_play = Some((ep.clone(), sid));
                        }
                        ui.label(format!("E{}: {}", ep.episode_num, display_title));
                    });
                }
                
                if let Some((ep, series_id)) = to_play {
                    self.play_episode(&ep, series_id);
                }
                return;
            }
        }

        // Seasons level
        if !self.current_seasons.is_empty() {
            if let Some(NavigationLevel::Seasons(series_id)) = self.navigation_stack.last() {
                let sid = *series_id;
                let mut clicked_season: Option<i32> = None;
                
                for season in &self.current_seasons {
                    if ui.button(format!("Season {}", season)).clicked() {
                        clicked_season = Some(*season);
                    }
                }
                
                if let Some(s) = clicked_season {
                    self.save_scroll_position(ui.ctx());
                    self.navigation_stack.push(NavigationLevel::Episodes(sid, s));
                    self.fetch_episodes(sid, s);
                }
                return;
            }
        }

        // Series list
        if !self.current_series.is_empty() {
            // Get category name for favorites
            let category_name = self.navigation_stack.iter()
                .find_map(|n| if let NavigationLevel::Series(name) = n { Some(name.clone()) } else { None })
                .unwrap_or_default();
            
            // Clone and sort series
            let mut series_list: Vec<_> = self.current_series.clone();
            match self.series_sort_order {
                SortOrder::NameAsc => series_list.sort_by_cached_key(|s| s.name.to_lowercase()),
                SortOrder::NameDesc => {
                    series_list.sort_by_cached_key(|s| s.name.to_lowercase());
                    series_list.reverse();
                }
                SortOrder::Default => {} // Keep server order
            }
            
            let mut clicked_series: Option<i64> = None;
            let mut toggle_fav: Option<FavoriteItem> = None;
            
            for series in &series_list {
                let display_name = Self::sanitize_text(&series.name);
                if !search.is_empty() && !display_name.to_lowercase().contains(&search) {
                    continue;
                }
                
                // Create a unique URL for series favorite (using series_id)
                let series_url = format!("series://{}", series.series_id);
                let is_fav = self.is_favorite(&series_url);
                
                ui.horizontal(|ui| {
                    // Favorite star
                    let fav_text = if is_fav { 
                        egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)
                    } else { 
                        egui::RichText::new("☆").size(18.0).color(egui::Color32::GRAY)
                    };
                    if ui.button(fav_text).on_hover_text(if is_fav { "Remove from favorites" } else { "Add to favorites" }).clicked() {
                        toggle_fav = Some(FavoriteItem {
                            name: series.name.clone(),
                            url: series_url,
                            stream_type: "series".to_string(),
                            stream_id: None,
                            series_id: Some(series.series_id),
                            category_name: category_name.clone(),
                            container_extension: None,
                            season_num: None,
                            episode_num: None,
                            series_name: None,
                            playlist_source: None,
                        });
                    }
                    
                    if ui.button(&display_name).clicked() {
                        clicked_series = Some(series.series_id);
                    }
                });
            }
            
            if let Some(fav) = toggle_fav {
                self.toggle_favorite(fav);
            }
            
            if let Some(sid) = clicked_series {
                self.save_scroll_position(ui.ctx());
                self.navigation_stack.push(NavigationLevel::Seasons(sid));
                self.fetch_series_info(sid);
            }
            return;
        }

        // Categories (sorted)
        let mut clicked_category: Option<(String, String)> = None;
        
        // Clone and sort categories
        let mut sorted_categories: Vec<_> = self.series_categories.clone();
        match self.series_sort_order {
            SortOrder::NameAsc => sorted_categories.sort_by_cached_key(|c| c.category_name.to_lowercase()),
            SortOrder::NameDesc => {
                sorted_categories.sort_by_cached_key(|c| c.category_name.to_lowercase());
                sorted_categories.reverse();
            }
            SortOrder::Default => {} // Keep server order
        }
        
        for cat in &sorted_categories {
            let display_name = Self::sanitize_text(&cat.category_name);
            if !search.is_empty() && !display_name.to_lowercase().contains(&search) {
                continue;
            }
            
            if ui.button(&display_name).clicked() {
                clicked_category = Some((cat.category_id.clone(), cat.category_name.clone()));
            }
        }
        
        if let Some((cat_id, cat_name)) = clicked_category {
            self.save_scroll_position(ui.ctx());
            self.navigation_stack.push(NavigationLevel::Series(cat_name));
            self.fetch_series_list(&cat_id);
        }
    }

    fn show_favorites_tab(&mut self, ui: &mut egui::Ui) {
        // Check if we're viewing a favorite series inline
        if let Some((series_id, ref series_name)) = self.fav_viewing_series.clone() {
            // Back button
            ui.horizontal(|ui| {
                if ui.button("⬅ Back").clicked() {
                    self.fav_viewing_series = None;
                    self.fav_series_seasons.clear();
                    self.fav_series_episodes.clear();
                    self.fav_viewing_season = None;
                }
                ui.label(egui::RichText::new(series_name.clone()).strong().size(16.0));
            });
            ui.separator();
            
            // Show episodes if viewing a season
            if let Some(season) = self.fav_viewing_season {
                ui.horizontal(|ui| {
                    if ui.button("⬅ Seasons").clicked() {
                        self.fav_viewing_season = None;
                        self.fav_series_episodes.clear();
                    }
                    ui.label(format!("Season {}", season));
                    
                    // Favorite the season
                    let season_url = format!("season://{}:{}", series_id, season);
                    let is_season_fav = self.is_favorite(&season_url);
                    let fav_text = if is_season_fav { 
                        egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)
                    } else { 
                        egui::RichText::new("☆").size(18.0).color(egui::Color32::GRAY)
                    };
                    if ui.button(fav_text).on_hover_text(if is_season_fav { "Remove season from favorites" } else { "Add season to favorites" }).clicked() {
                        self.toggle_favorite(FavoriteItem {
                            name: format!("{} - Season {}", series_name, season),
                            url: season_url,
                            stream_type: "season".to_string(),
                            stream_id: None,
                            series_id: Some(series_id),
                            category_name: series_name.clone(),
                            container_extension: None,
                            season_num: Some(season),
                            episode_num: None,
                            series_name: Some(series_name.clone()),
                            playlist_source: None,
                        });
                    }
                });
                ui.separator();
                
                let episodes = self.fav_series_episodes.clone();
                let mut to_play: Option<Episode> = None;
                let mut toggle_ep_fav: Option<FavoriteItem> = None;
                
                for ep in &episodes {
                    let ep_url = format!("episode://{}:{}:{}", series_id, season, ep.id);
                    let is_ep_fav = self.is_favorite(&ep_url);
                    
                    ui.horizontal(|ui| {
                        // Favorite star for episode
                        let fav_text = if is_ep_fav { 
                            egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)
                        } else { 
                            egui::RichText::new("☆").size(18.0).color(egui::Color32::GRAY)
                        };
                        if ui.button(fav_text).on_hover_text(if is_ep_fav { "Remove from favorites" } else { "Add to favorites" }).clicked() {
                            toggle_ep_fav = Some(FavoriteItem {
                                name: format!("{} S{}E{}: {}", series_name, season, ep.episode_num, ep.title),
                                url: ep_url,
                                stream_type: "episode".to_string(),
                                stream_id: Some(ep.id),
                                series_id: Some(series_id),
                                category_name: series_name.clone(),
                                container_extension: Some(ep.container_extension.clone()),
                                season_num: Some(season),
                                episode_num: Some(ep.episode_num),
                                series_name: Some(series_name.clone()),
                                playlist_source: None,
                            });
                        }
                        
                        if ui.button("▶").clicked() {
                            to_play = Some(ep.clone());
                        }
                        ui.label(format!("E{}: {}", ep.episode_num, Self::sanitize_text(&ep.title)));
                    });
                }
                
                if let Some(fav) = toggle_ep_fav {
                    self.toggle_favorite(fav);
                }
                
                if let Some(ep) = to_play {
                    self.play_episode(&ep, series_id);
                }
                return;
            }
            
            // Show seasons
            let seasons = self.fav_series_seasons.clone();
            let mut clicked_season: Option<i32> = None;
            let mut toggle_season_fav: Option<FavoriteItem> = None;
            
            for season in &seasons {
                let season_url = format!("season://{}:{}", series_id, season);
                let is_season_fav = self.is_favorite(&season_url);
                
                ui.horizontal(|ui| {
                    // Favorite star for season
                    let fav_text = if is_season_fav { 
                        egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)
                    } else { 
                        egui::RichText::new("☆").size(18.0).color(egui::Color32::GRAY)
                    };
                    if ui.button(fav_text).on_hover_text(if is_season_fav { "Remove from favorites" } else { "Add to favorites" }).clicked() {
                        toggle_season_fav = Some(FavoriteItem {
                            name: format!("{} - Season {}", series_name, season),
                            url: season_url,
                            stream_type: "season".to_string(),
                            stream_id: None,
                            series_id: Some(series_id),
                            category_name: series_name.clone(),
                            container_extension: None,
                            season_num: Some(*season),
                            episode_num: None,
                            series_name: Some(series_name.clone()),
                            playlist_source: None,
                        });
                    }
                    
                    if ui.button(format!("Season {}", season)).clicked() {
                        clicked_season = Some(*season);
                    }
                });
            }
            
            if let Some(fav) = toggle_season_fav {
                self.toggle_favorite(fav);
            }
            
            if let Some(s) = clicked_season {
                self.fav_viewing_season = Some(s);
                self.fetch_fav_episodes(series_id, s);
            }
            return;
        }
        
        if self.favorites.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.heading("No favorites yet");
                ui.label("Click ☆ next to any channel, movie, series, season, or episode");
            });
            return;
        }
        
        // Clone favorites to avoid borrow issues
        let live_favs: Vec<_> = self.favorites.iter()
            .filter(|f| f.stream_type == "live")
            .cloned()
            .collect();
        let movie_favs: Vec<_> = self.favorites.iter()
            .filter(|f| f.stream_type == "movie")
            .cloned()
            .collect();
        let series_favs: Vec<_> = self.favorites.iter()
            .filter(|f| f.stream_type == "series")
            .cloned()
            .collect();
        let season_favs: Vec<_> = self.favorites.iter()
            .filter(|f| f.stream_type == "season")
            .cloned()
            .collect();
        let episode_favs: Vec<_> = self.favorites.iter()
            .filter(|f| f.stream_type == "episode")
            .cloned()
            .collect();
        
        let mut to_remove: Option<String> = None;
        let mut to_play: Option<FavoriteItem> = None;
        let mut to_view_series: Option<(i64, String)> = None;
        let mut to_view_season: Option<(i64, i32, String)> = None; // series_id, season, series_name
        
        if !live_favs.is_empty() {
            egui::CollapsingHeader::new(format!("📡 Live Channels ({})", live_favs.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for fav in &live_favs {
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)).on_hover_text("Remove from favorites").clicked() {
                                to_remove = Some(fav.url.clone());
                            }
                            if ui.button("▶").clicked() {
                                to_play = Some(fav.clone());
                            }
                            ui.label(Self::sanitize_text(&fav.name));
                            self.show_epg_inline(ui, &fav.name, None);
                            if let Some(ref src) = fav.playlist_source {
                                ui.label(egui::RichText::new(format!("[{}]", src)).small().color(egui::Color32::from_rgb(100, 149, 237)));
                            } else {
                                ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&fav.category_name))).weak());
                            }
                        });
                    }
                });
        }
        
        if !movie_favs.is_empty() {
            egui::CollapsingHeader::new(format!("🎬 Movies ({})", movie_favs.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for fav in &movie_favs {
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)).on_hover_text("Remove from favorites").clicked() {
                                to_remove = Some(fav.url.clone());
                            }
                            if ui.button("▶").clicked() {
                                to_play = Some(fav.clone());
                            }
                            ui.label(Self::sanitize_text(&fav.name));
                            if let Some(ref src) = fav.playlist_source {
                                ui.label(egui::RichText::new(format!("[{}]", src)).small().color(egui::Color32::from_rgb(100, 149, 237)));
                            } else {
                                ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&fav.category_name))).weak());
                            }
                        });
                    }
                });
        }
        
        if !series_favs.is_empty() {
            egui::CollapsingHeader::new(format!("📺 Series ({})", series_favs.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for fav in &series_favs {
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)).on_hover_text("Remove from favorites").clicked() {
                                to_remove = Some(fav.url.clone());
                            }
                            if ui.button("📺").on_hover_text("View seasons").clicked() {
                                if let Some(series_id) = fav.series_id {
                                    to_view_series = Some((series_id, fav.name.clone()));
                                }
                            }
                            ui.label(Self::sanitize_text(&fav.name));
                            ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&fav.category_name))).weak());
                        });
                    }
                });
        }
        
        if !season_favs.is_empty() {
            egui::CollapsingHeader::new(format!("📂 Seasons ({})", season_favs.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for fav in &season_favs {
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)).on_hover_text("Remove from favorites").clicked() {
                                to_remove = Some(fav.url.clone());
                            }
                            if ui.button("📂").on_hover_text("View episodes").clicked() {
                                if let (Some(series_id), Some(season)) = (fav.series_id, fav.season_num) {
                                    let series_name = fav.series_name.clone().unwrap_or_else(|| fav.category_name.clone());
                                    to_view_season = Some((series_id, season, series_name));
                                }
                            }
                            ui.label(Self::sanitize_text(&fav.name));
                        });
                    }
                });
        }
        
        if !episode_favs.is_empty() {
            egui::CollapsingHeader::new(format!("🎞 Episodes ({})", episode_favs.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for fav in &episode_favs {
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)).on_hover_text("Remove from favorites").clicked() {
                                to_remove = Some(fav.url.clone());
                            }
                            if ui.button("▶").clicked() {
                                to_play = Some(fav.clone());
                            }
                            ui.label(Self::sanitize_text(&fav.name));
                        });
                    }
                });
        }
        
        // Handle view season action (stay in favorites)
        if let Some((series_id, season, series_name)) = to_view_season {
            self.fav_viewing_series = Some((series_id, series_name));
            self.fav_viewing_season = Some(season);
            self.fav_series_seasons.clear();
            self.fav_series_episodes.clear();
            self.fetch_fav_episodes(series_id, season);
        }
        
        // Handle view series action (stay in favorites)
        if let Some((series_id, name)) = to_view_series {
            self.fav_viewing_series = Some((series_id, name));
            self.fav_series_seasons.clear();
            self.fav_series_episodes.clear();
            self.fav_viewing_season = None;
            self.fetch_fav_series_info(series_id);
        }
        
        // Handle play action (for live/movies/episodes - all play directly)
        if let Some(fav) = to_play {
            self.play_favorite(&fav);
        }
        
        // Handle removal
        if let Some(url) = to_remove {
            if let Some(pos) = self.favorites.iter().position(|f| f.url == url) {
                let name = self.favorites[pos].name.clone();
                self.favorites.remove(pos);
                self.status_message = format!("Removed '{}' from favorites", name);
                // Auto-save
                self.config.favorites_json = serde_json::to_string(&self.favorites).unwrap_or_default();
                self.config.save();
            }
        }
        
        ui.add_space(20.0);
        ui.separator();
        
        if ui.button("🗑 Clear All Favorites").clicked() {
            self.favorites.clear();
            self.config.favorites_json.clear();
            self.config.save();
            self.status_message = "All favorites cleared".to_string();
        }
    }

    fn show_recent_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Recently Watched");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.recent_watched.is_empty() && ui.button("🗑 Clear History").clicked() {
                    self.recent_watched.clear();
                    self.config.recent_watched_json.clear();
                    self.config.save();
                    self.status_message = "Watch history cleared".to_string();
                }
            });
        });
        ui.separator();
        
        if self.recent_watched.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.heading("No watch history");
                ui.label("Streams you play will appear here");
            });
            return;
        }
        
        // Clone to avoid borrow issues
        let recent: Vec<_> = self.recent_watched.iter().cloned().collect();
        let mut to_play: Option<FavoriteItem> = None;
        let mut to_remove: Option<usize> = None;
        let mut to_toggle_fav: Option<FavoriteItem> = None;
        
        for (idx, item) in recent.iter().enumerate() {
            ui.horizontal(|ui| {
                // Favorite toggle button
                let is_fav = self.favorites.iter().any(|f| f.url == item.url);
                if is_fav {
                    if ui.button(egui::RichText::new("★").size(16.0).color(egui::Color32::GOLD))
                        .on_hover_text("Remove from favorites")
                        .clicked() 
                    {
                        to_toggle_fav = Some(item.clone());
                    }
                } else {
                    if ui.button(egui::RichText::new("☆").size(16.0).color(egui::Color32::GRAY))
                        .on_hover_text("Add to favorites")
                        .clicked() 
                    {
                        to_toggle_fav = Some(item.clone());
                    }
                }
                
                if ui.button("▶").clicked() {
                    to_play = Some(item.clone());
                }
                
                // Type icon
                let type_icon = match item.stream_type.as_str() {
                    "live" => "📺",
                    "movie" => "🎬",
                    "series" => "📺",
                    _ => "▶",
                };
                ui.label(type_icon);
                
                ui.label(Self::sanitize_text(&item.name));
                
                // Show EPG info (will only display if EPG match found)
                self.show_epg_inline(ui, &item.name, None);
                
                // Show playlist source or category
                if let Some(ref src) = item.playlist_source {
                    ui.label(egui::RichText::new(format!("[{}]", src)).small().color(egui::Color32::from_rgb(100, 149, 237)));
                } else {
                    ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&item.category_name))).weak());
                }
                
                // Remove from history button
                if ui.small_button("✕").on_hover_text("Remove from history").clicked() {
                    to_remove = Some(idx);
                }
            });
        }
        
        // Handle favorite toggle
        if let Some(item) = to_toggle_fav {
            self.toggle_favorite(item);
        }
        
        // Handle play
        if let Some(item) = to_play {
            self.play_favorite(&item);
        }
        
        // Handle removal
        if let Some(idx) = to_remove {
            self.recent_watched.remove(idx);
            self.config.recent_watched_json = serde_json::to_string(&self.recent_watched).unwrap_or_default();
            self.config.save();
        }
    }

    fn add_to_recent(&mut self, item: FavoriteItem, reorder: bool) {
        if reorder {
            // Remove if already in list (to move to top)
            self.recent_watched.retain(|r| r.url != item.url);
            
            // Add to front (newest first)
            self.recent_watched.insert(0, item);
        } else {
            // Don't reorder - only add if not already in list
            if !self.recent_watched.iter().any(|r| r.url == item.url) {
                self.recent_watched.insert(0, item);
            } else {
                // Already in list, don't change anything
                return;
            }
        }
        
        // Keep only last 25
        self.recent_watched.truncate(25);
        
        // Save
        self.config.recent_watched_json = serde_json::to_string(&self.recent_watched).unwrap_or_default();
        self.config.save();
    }

    fn show_info_tab(&self, ui: &mut egui::Ui) {
        ui.heading("Account Information");
        ui.separator();
        
        egui::Grid::new("info_grid")
            .num_columns(2)
            .spacing([20.0, 8.0])
            .show(ui, |ui| {
                ui.label("Host:");
                ui.label(format!("{}:{}", self.server_info.url, self.server_info.port));
                ui.end_row();
                
                ui.label("Username:");
                ui.label(&self.user_info.username);
                ui.end_row();
                
                ui.label("Password:");
                ui.label(&self.user_info.password);
                ui.end_row();
                
                ui.label("Status:");
                ui.label(&self.user_info.status);
                ui.end_row();
                
                ui.label("Max Connections:");
                ui.label(&self.user_info.max_connections);
                ui.end_row();
                
                ui.label("Active Connections:");
                ui.label(&self.user_info.active_connections);
                ui.end_row();
                
                ui.label("Trial:");
                ui.label(if self.user_info.is_trial { "Yes" } else { "No" });
                ui.end_row();
                
                ui.label("Timezone:");
                ui.label(&self.server_info.timezone);
                ui.end_row();
                
                ui.label("Expiry:");
                ui.label(&self.user_info.expiry);
                ui.end_row();
            });
    }
    
    fn show_console_tab(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Console Log");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("🗑 Clear").clicked() {
                    self.console_log.clear();
                    self.console_log.push(format!("[{}] Console cleared", timestamp_now()));
                }
            });
        });
        ui.separator();
        
        // Display log entries with monospace font
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.console_log {
                    let color = if line.contains("[ERROR]") {
                        egui::Color32::RED
                    } else if line.contains("[WARN]") {
                        egui::Color32::YELLOW
                    } else if line.contains("[INFO]") {
                        egui::Color32::LIGHT_BLUE
                    } else if line.contains("[PLAY]") {
                        egui::Color32::GREEN
                    } else {
                        egui::Color32::GRAY
                    };
                    
                    ui.label(egui::RichText::new(line).monospace().color(color));
                }
            });
    }
    
    fn show_epg_grid_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("📺 EPG Guide");
        ui.separator();
        
        let adjusted_now = self.get_adjusted_now();
        
        // Fixed layout for scrollable grid
        let channel_col_width = 137.0;  // Channel column width
        let prog_col_width = 130.0;
        let num_progs = 7; // Show 7 programs (current + 6 upcoming), user scrolls to see more
        
        // Time header labels - either offset or actual time
        let time_labels: Vec<String> = if self.epg_show_actual_time {
            // Calculate actual times based on adjusted_now
            let offsets_mins = [0, 30, 60, 90, 120, 150, 180];
            offsets_mins.iter().map(|&offset| {
                let ts = adjusted_now + (offset * 60);
                Self::format_time(ts)
            }).collect()
        } else {
            // Offset mode
            vec![
                "Now".to_string(),
                "+30m".to_string(),
                "+60m".to_string(),
                "+90m".to_string(),
                "+2h".to_string(),
                "+2.5h".to_string(),
                "+3h".to_string(),
            ]
        };
        
        // Get channels to display based on current view
        let channels_to_show: Vec<(String, Option<String>)> = match self.current_tab {
            Tab::Live => {
                self.current_channels.iter()
                    .take(20) // Limit for performance
                    .filter_map(|c| c.epg_channel_id.as_ref().map(|id| (c.name.clone(), Some(id.clone()))))
                    .collect()
            }
            Tab::Favorites | Tab::Recent => {
                let items = if self.current_tab == Tab::Favorites {
                    &self.favorites
                } else {
                    &self.recent_watched
                };
                items.iter()
                    .filter(|f| f.stream_type == "live")
                    .take(20)
                    .map(|f| (f.name.clone(), None))
                    .collect()
            }
            _ => Vec::new(),
        };
        
        // Fixed time header row (outside scroll area)
        ui.horizontal(|ui| {
            ui.add_sized([channel_col_width, 20.0], egui::Label::new("")); // Channel column spacer
            for label in &time_labels {
                ui.add_sized([prog_col_width, 20.0], egui::Label::new(egui::RichText::new(label).strong()));
            }
        });
        
        ui.separator();
        
        // Vertical scroll area for channel rows only
        egui::ScrollArea::vertical()
            .id_salt("epg_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Channel rows
                for (channel_name, epg_id_opt) in &channels_to_show {
                    // Try to find EPG ID - first from provided, then from current_channels, then from EPG data
                    let epg_id = epg_id_opt.as_ref().cloned()
                        .or_else(|| {
                            self.current_channels.iter()
                                .find(|c| c.name == *channel_name)
                                .and_then(|c| c.epg_channel_id.clone())
                        })
                        .or_else(|| {
                            // Search EPG data for matching channel name (case-insensitive)
                            if let Some(ref epg) = self.epg_data {
                                epg.channels.iter()
                                    .find(|(_id, ch)| {
                                        // Use case-insensitive contains without allocation
                                        contains_ignore_case(&ch.name, channel_name) ||
                                        contains_ignore_case(channel_name, &ch.name)
                                    })
                                    .map(|(id, _)| id.clone())
                            } else {
                                None
                            }
                        });
                    
                    let is_selected = self.selected_epg_channel.as_ref() == Some(channel_name);
                    
                    ui.horizontal(|ui| {
                        // Channel name (clickable) - fixed width
                        let name_text = Self::sanitize_text(channel_name);
                        let short_name: String = name_text.chars().take(14).collect();
                        
                        let response = ui.add_sized([channel_col_width - 5.0, 20.0], 
                            egui::Button::new(egui::RichText::new(&short_name).strong())
                                .selected(is_selected)
                        );
                        
                        if response.clicked() {
                            self.selected_epg_channel = Some(channel_name.clone());
                        }
                        
                        if response.double_clicked() {
                            // Find and play the channel - check current_channels first, then favorites/recent
                            let channel_opt = self.current_channels.iter()
                                .find(|c| c.name == *channel_name)
                                .cloned()
                                .or_else(|| {
                                    // Check favorites
                                    self.favorites.iter()
                                        .find(|f| f.name == *channel_name && f.stream_type == "live")
                                        .map(|f| Channel {
                                            name: f.name.clone(),
                                            url: f.url.clone(),
                                            stream_id: f.stream_id,
                                            category_id: None,
                                            epg_channel_id: None,
                                            stream_icon: None,
                                            series_id: None,
                                            container_extension: None,
                                            playlist_source: f.playlist_source.clone(),
                                        })
                                })
                                .or_else(|| {
                                    // Check recent
                                    self.recent_watched.iter()
                                        .find(|f| f.name == *channel_name && f.stream_type == "live")
                                        .map(|f| Channel {
                                            name: f.name.clone(),
                                            url: f.url.clone(),
                                            stream_id: f.stream_id,
                                            category_id: None,
                                            epg_channel_id: None,
                                            stream_icon: None,
                                            series_id: None,
                                            container_extension: None,
                                            playlist_source: f.playlist_source.clone(),
                                        })
                                });
                            
                            if let Some(channel) = channel_opt {
                                self.play_channel(&channel);
                            }
                        }
                        
                        response.on_hover_text(channel_name);
                        
                        // Program blocks - fixed width each
                        if let Some(ref id) = epg_id {
                            let programs = self.get_upcoming_programs(id, num_progs);
                            
                            for (idx, prog) in programs.iter().enumerate() {
                                let is_current = prog.start <= adjusted_now && prog.stop > adjusted_now;
                                let duration_mins = (prog.stop - prog.start) / 60;
                                
                                // Fixed width for each program block
                                let width = prog_col_width - 6.0;
                                
                                // Truncate title to fit - allow more chars (roughly 6px per char)
                                let max_chars = ((width - 8.0) / 5.5) as usize;
                                let title: String = prog.title.chars().take(max_chars).collect();
                                let display = if prog.title.len() > max_chars {
                                    format!("{}…", title)
                                } else {
                                    title
                                };
                                
                                let bg_color = if is_current {
                                    egui::Color32::from_rgb(60, 100, 60)
                                } else if idx % 2 == 0 {
                                    egui::Color32::from_rgb(50, 50, 70)
                                } else {
                                    egui::Color32::from_rgb(40, 40, 60)
                                };
                                
                                let text_color = if is_current {
                                    egui::Color32::WHITE
                                } else {
                                    egui::Color32::LIGHT_GRAY
                                };
                                
                                egui::Frame::new()
                                    .fill(bg_color)
                                    .inner_margin(egui::Margin::symmetric(4, 3))
                                    .corner_radius(3.0)
                                    .show(ui, |ui| {
                                        ui.set_min_width(width);
                                        ui.set_max_width(width);
                                        let response = ui.label(
                                            egui::RichText::new(&display)
                                                .color(text_color)
                                        );
                                        response.on_hover_text(format!(
                                            "{}\n{} - {}\n{}m",
                                            prog.title,
                                            Self::format_time(prog.start),
                                            Self::format_time(prog.stop),
                                            duration_mins
                                        ));
                                    });
                            }
                            
                            if programs.is_empty() {
                                ui.label(egui::RichText::new("No EPG data").weak().small());
                            }
                        } else {
                            ui.label(egui::RichText::new("No EPG ID").weak().small());
                        }
                    });
                }
                
                if channels_to_show.is_empty() {
                    ui.label("Select a category to view EPG");
                }
            });
        
        ui.separator();
        
        // Selected program details
        if let Some(ref channel_name) = self.selected_epg_channel.clone() {
            let epg_id = self.current_channels.iter()
                .find(|c| c.name == *channel_name)
                .and_then(|c| c.epg_channel_id.clone());
            
            if let Some(ref id) = epg_id {
                if let Some(prog) = self.get_current_program(id) {
                    ui.group(|ui| {
                        ui.heading(egui::RichText::new(&prog.title).size(14.0));
                        
                        let duration_mins = (prog.stop - prog.start) / 60;
                        let elapsed = (adjusted_now - prog.start).max(0) / 60;
                        let remaining = duration_mins - elapsed;
                        
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!(
                                "{} - {} ({}m remaining)",
                                Self::format_time(prog.start),
                                Self::format_time(prog.stop),
                                remaining
                            )).small());
                        });
                        
                        // Progress bar
                        let progress = if duration_mins > 0 {
                            elapsed as f32 / duration_mins as f32
                        } else {
                            0.0
                        };
                        ui.add(egui::ProgressBar::new(progress.clamp(0.0, 1.0))
                            .show_percentage());
                        
                        if let Some(ref desc) = prog.description {
                            ui.separator();
                            ui.label(egui::RichText::new(desc).small());
                        }
                        
                        if let Some(ref cat) = prog.category {
                            ui.label(egui::RichText::new(format!("Category: {}", cat)).weak().small());
                        }
                        
                        if let Some(ref ep) = prog.episode {
                            ui.label(egui::RichText::new(format!("Episode: {}", ep)).weak().small());
                        }
                    });
                }
            }
        }
    }
    
    fn format_time(ts: i64) -> String {
        epg::format_time(ts)
    }
    
    fn format_datetime(ts: i64) -> String {
        epg::format_datetime(ts)
    }
}

fn format_timestamp(ts: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    
    let d = UNIX_EPOCH + Duration::from_secs(ts as u64);
    // Simple formatting
    format!("{:?}", d)
}
