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
mod epg_parser;
mod ffmpeg_player;

use api::*;
use config::*;
use models::*;
use ffmpeg_player::PlayerWindow;
use epg_parser::EpgData;

// Re-export ConnectionQuality for use in main

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
    Error(String),
    PlayerLog(String),
    PlayerExited { code: Option<i32>, stderr: String },
    // EPG loading results
    EpgLoading { progress: String },
    EpgLoaded { data: Box<EpgData> },
    EpgError(String),
}

/// EPG auto-update interval
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum EpgAutoUpdate {
    Off,
    Hours6,
    Hours12,
    #[default]
    Day1,
    Days2,
    Days3,
    Days4,
    Days5,
}

impl EpgAutoUpdate {
    fn as_secs(&self) -> Option<i64> {
        match self {
            EpgAutoUpdate::Off => None,
            EpgAutoUpdate::Hours6 => Some(6 * 3600),
            EpgAutoUpdate::Hours12 => Some(12 * 3600),
            EpgAutoUpdate::Day1 => Some(24 * 3600),
            EpgAutoUpdate::Days2 => Some(2 * 24 * 3600),
            EpgAutoUpdate::Days3 => Some(3 * 24 * 3600),
            EpgAutoUpdate::Days4 => Some(4 * 24 * 3600),
            EpgAutoUpdate::Days5 => Some(5 * 24 * 3600),
        }
    }
    
    fn label(&self) -> &'static str {
        match self {
            EpgAutoUpdate::Off => "Off",
            EpgAutoUpdate::Hours6 => "6 Hours",
            EpgAutoUpdate::Hours12 => "12 Hours",
            EpgAutoUpdate::Day1 => "1 Day",
            EpgAutoUpdate::Days2 => "2 Days",
            EpgAutoUpdate::Days3 => "3 Days",
            EpgAutoUpdate::Days4 => "4 Days",
            EpgAutoUpdate::Days5 => "5 Days",
        }
    }
    
    fn to_index(&self) -> u8 {
        match self {
            EpgAutoUpdate::Off => 0,
            EpgAutoUpdate::Hours6 => 1,
            EpgAutoUpdate::Hours12 => 2,
            EpgAutoUpdate::Day1 => 3,
            EpgAutoUpdate::Days2 => 4,
            EpgAutoUpdate::Days3 => 5,
            EpgAutoUpdate::Days4 => 6,
            EpgAutoUpdate::Days5 => 7,
        }
    }
    
    fn from_index(i: u8) -> Self {
        match i {
            0 => EpgAutoUpdate::Off,
            1 => EpgAutoUpdate::Hours6,
            2 => EpgAutoUpdate::Hours12,
            3 => EpgAutoUpdate::Day1,
            4 => EpgAutoUpdate::Days2,
            5 => EpgAutoUpdate::Days3,
            6 => EpgAutoUpdate::Days4,
            7 => EpgAutoUpdate::Days5,
            _ => EpgAutoUpdate::Day1,
        }
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
    
    // Favorites
    favorites: Vec<FavoriteItem>,
    
    // Recently watched (last 20)
    recent_watched: Vec<FavoriteItem>,
    
    navigation_stack: Vec<NavigationLevel>,
    
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
    address_book: Vec<SavedCredential>,
    show_address_book: bool,
    show_m3u_dialog: bool,
    m3u_url_input: String,
    
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
        let address_book = load_address_book();
        let (task_sender, task_receiver) = channel();
        
        // Load saved credentials if save_state is enabled
        let (server, username, password) = if config.save_state {
            (
                config.saved_server.clone(),
                config.saved_username.clone(),
                config.saved_password.clone(),
            )
        } else {
            (String::new(), String::new(), String::new())
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
        
        // Extract values before moving config
        let single_window_mode = config.single_window_mode;
        let hw_accel = config.hw_accel;
        let epg_url = config.epg_url.clone();
        let epg_auto_update_index = config.epg_auto_update_index;
        let epg_time_offset = config.epg_time_offset;
        let epg_show_actual_time = config.epg_show_actual_time;
        
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
            favorites,
            recent_watched,
            navigation_stack: Vec::new(),
            user_info: UserInfo::default(),
            server_info: ServerInfo::default(),
            search_query: String::new(),
            external_player: config.external_player.clone(),
            buffer_seconds: config.buffer_seconds,
            connection_quality: config.connection_quality,
            dark_mode: config.dark_mode,
            use_post_method: false,
            save_state: config.save_state,
            auto_login: config.auto_login,
            auto_login_triggered: false,
            selected_user_agent: config.selected_user_agent,
            custom_user_agent: config.custom_user_agent.clone(),
            use_custom_user_agent: config.use_custom_user_agent,
            pass_user_agent_to_player: config.pass_user_agent_to_player,
            show_user_agent_dialog: false,
            config,
            address_book,
            show_address_book: false,
            show_m3u_dialog: false,
            m3u_url_input: String::new(),
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
            
            // Also save to Address Book if server is set
            if !self.server.is_empty() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                
                // Create credential entry
                let cred = SavedCredential {
                    server: self.server.clone(),
                    username: self.username.clone(),
                    password: self.password.clone(),
                    saved_at: now,
                    external_player: self.external_player.clone(),
                    buffer_seconds: self.buffer_seconds,
                    connection_quality: self.connection_quality,
                    selected_user_agent: self.selected_user_agent,
                    custom_user_agent: self.custom_user_agent.clone(),
                    use_custom_user_agent: self.use_custom_user_agent,
                    pass_user_agent_to_player: self.pass_user_agent_to_player,
                    epg_url: self.epg_url_input.clone(),
                    epg_time_offset: self.epg_time_offset,
                    epg_auto_update_index: self.epg_auto_update.to_index(),
                    epg_show_actual_time: self.epg_show_actual_time,
                };
                
                // Update existing entry or add new one (match by server+username)
                if let Some(existing) = self.address_book.iter_mut().find(|c| c.server == cred.server && c.username == cred.username) {
                    *existing = cred;
                } else {
                    self.address_book.push(cred);
                }
                save_address_book(&self.address_book);
            }
        } else {
            self.config.saved_server.clear();
            self.config.saved_username.clear();
            self.config.saved_password.clear();
        }
        
        self.config.save();
        self.status_message = "Settings saved".to_string();
    }
    
    fn is_favorite(&self, url: &str) -> bool {
        self.favorites.iter().any(|f| f.url == url)
    }
    
    fn toggle_favorite(&mut self, item: FavoriteItem) {
        if let Some(pos) = self.favorites.iter().position(|f| f.url == item.url) {
            self.favorites.remove(pos);
            self.status_message = format!("Removed '{}' from favorites", item.name);
        } else {
            self.favorites.push(item.clone());
            self.status_message = format!("Added '{}' to favorites", item.name);
        }
        // Auto-save favorites
        self.config.favorites_json = serde_json::to_string(&self.favorites).unwrap_or_default();
        self.config.save();
    }
    
    fn play_favorite(&mut self, fav: &FavoriteItem) {
        let channel = Channel {
            name: fav.name.clone(),
            url: fav.url.clone(),
            stream_id: fav.stream_id,
            category_id: None,
            epg_channel_id: None,
            stream_icon: None,
            series_id: fav.series_id,
            container_extension: fav.container_extension.clone(),
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

    fn fetch_channels(&mut self, category_id: &str, stream_type: &str) {
        self.loading = true;
        self.status_message = "Loading channels...".to_string();
        
        let server = self.server.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let user_agent = self.get_user_agent();
        let use_post = self.use_post_method;
        let category_id = category_id.to_string();
        let stream_type = stream_type.to_string();
        let sender = self.task_sender.clone();

        thread::spawn(move || {
            let client = XtreamClient::new(&server, &username, &password)
                .with_user_agent(&user_agent)
                .with_post_method(use_post);
            
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
                        server, stream_type, username, password,
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
                    }
                }).collect();
                
                let _ = sender.send(TaskResult::ChannelsLoaded(channels));
            } else {
                let _ = sender.send(TaskResult::Error("Failed to load channels".to_string()));
            }
        });
    }

    fn fetch_series_list(&mut self, category_id: &str) {
        self.loading = true;
        self.status_message = "Loading series...".to_string();
        
        let server = self.server.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let user_agent = self.get_user_agent();
        let use_post = self.use_post_method;
        let category_id = category_id.to_string();
        let sender = self.task_sender.clone();

        thread::spawn(move || {
            let client = XtreamClient::new(&server, &username, &password)
                .with_user_agent(&user_agent)
                .with_post_method(use_post);
            
            if let Ok(series) = client.get_series(&category_id) {
                let _ = sender.send(TaskResult::SeriesListLoaded(series));
            } else {
                let _ = sender.send(TaskResult::Error("Failed to load series".to_string()));
            }
        });
    }

    fn fetch_series_info(&mut self, series_id: i64) {
        self.loading = true;
        self.status_message = "Loading seasons...".to_string();
        
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
            
            if let Ok(info) = client.get_series_info(series_id) {
                if let Some(episodes) = info.get("episodes") {
                    if let Some(obj) = episodes.as_object() {
                        let mut seasons: Vec<i32> = obj.keys()
                            .filter_map(|k| k.parse::<i32>().ok())
                            .collect();
                        seasons.sort();
                        let _ = sender.send(TaskResult::SeasonsLoaded(seasons));
                        return;
                    }
                }
                let _ = sender.send(TaskResult::Error("No seasons found".to_string()));
            } else {
                let _ = sender.send(TaskResult::Error("Failed to load series info".to_string()));
            }
        });
    }

    fn fetch_episodes(&mut self, series_id: i64, season: i32) {
        self.loading = true;
        self.status_message = "Loading episodes...".to_string();
        
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
                            
                            let _ = sender.send(TaskResult::EpisodesLoaded(eps));
                            return;
                        }
                    }
                }
                let _ = sender.send(TaskResult::Error("No episodes found".to_string()));
            } else {
                let _ = sender.send(TaskResult::Error("Failed to load episodes".to_string()));
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
            use epg_parser::{DownloadConfig, EpgDownloader};
            
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
            let progress_callback: Option<epg_parser::ProgressCallback> = Some(Box::new(move |downloaded, total| {
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
    
    fn get_current_program(&self, epg_channel_id: &str) -> Option<&epg_parser::Program> {
        let epg = self.epg_data.as_ref()?;
        let offset_secs = (self.epg_time_offset * 3600.0) as i64;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let adjusted_now = now - offset_secs;
        
        epg.programs
            .get(epg_channel_id)?
            .iter()
            .find(|p| p.start <= adjusted_now && p.stop > adjusted_now)
    }
    
    /// Get current and next N programs for a channel (with time offset applied)
    fn get_upcoming_programs(&self, epg_channel_id: &str, count: usize) -> Vec<&epg_parser::Program> {
        let Some(epg) = self.epg_data.as_ref() else { return Vec::new() };
        let offset_secs = (self.epg_time_offset * 3600.0) as i64;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let adjusted_now = now - offset_secs;
        
        epg.programs
            .get(epg_channel_id)
            .map(|progs| {
                progs.iter()
                    .filter(|p| p.stop > adjusted_now)
                    .take(count)
                    .collect()
            })
            .unwrap_or_default()
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
        
        self.add_to_recent(FavoriteItem {
            name: channel.name.clone(),
            url: channel.url.clone(),
            stream_type: stream_type.to_string(),
            stream_id: channel.stream_id,
            series_id: channel.series_id,
            category_name,
            container_extension: channel.container_extension.clone(),
        });
        
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
        
        // Get effective buffer based on connection quality
        let buffer_secs = self.get_effective_buffer();
        let buffer_ms = (buffer_secs * 1000) as i64;
        let buffer_bytes = (buffer_secs as i64) * 1024 * 1024; // ~1MB per second
        let buffer_bytes_large = buffer_bytes * 4; // Larger buffer for probing
        let is_slow = matches!(self.connection_quality, ConnectionQuality::Slow | ConnectionQuality::VerySlow);
        
        self.log(&format!("[PLAY] Buffer: {}s | Connection: {:?} | HW Accel: {}", buffer_secs, self.connection_quality, if self.hw_accel { "On" } else { "Off" }));
        
        if player_lower.contains("ffplay") {
            // FFplay buffer settings - aggressive for IPTV
            let mut args = vec![
                "-i".to_string(), channel.url.clone(),
                "-autoexit".to_string(),
                
                // === BUFFERING (most important) ===
                "-probesize".to_string(), format!("{}", buffer_bytes_large),
                "-analyzeduration".to_string(), format!("{}", buffer_ms * 2000), // microseconds, 2x buffer
                
                // === STREAM FLAGS ===
                "-fflags".to_string(), "+genpts+discardcorrupt+igndts+nobuffer".to_string(),
                "-flags".to_string(), "low_delay".to_string(),
                
                // === BUFFER SIZES ===
                "-rtbufsize".to_string(), format!("{}M", buffer_secs * 4),
                "-max_delay".to_string(), format!("{}", buffer_ms * 1000),
                "-reorder_queue_size".to_string(), format!("{}", buffer_secs * 100),
                
                // === SYNC OPTIONS ===
                "-sync".to_string(), "audio".to_string(),
                "-framedrop".to_string(),
                "-avioflags".to_string(), "direct".to_string(),
                
                // === THREADS ===
                "-threads".to_string(), "0".to_string(), // Auto
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
                    "-reconnect_at_eof".to_string(), "1".to_string(),
                    "-reconnect_delay_max".to_string(), if is_slow { "30".to_string() } else { "10".to_string() },
                    // Timeout settings
                    "-timeout".to_string(), format!("{}", buffer_ms * 2000), // microseconds
                    "-rw_timeout".to_string(), format!("{}", buffer_ms * 2000),
                ]);
            }
            
            // Slow connection optimizations
            if is_slow {
                args.extend([
                    "-infbuf".to_string(),  // Infinite buffer mode
                    "-fflags".to_string(), "+genpts+discardcorrupt+igndts".to_string(),
                    "-err_detect".to_string(), "ignore_err".to_string(),
                    "-ec".to_string(), "favor_inter".to_string(), // Error concealment
                ]);
            }
            
            // User agent (optional)
            if self.pass_user_agent_to_player {
                args.extend([
                    "-user_agent".to_string(), self.get_user_agent(),
                ]);
            }
            
            // Hardware acceleration
            if self.hw_accel {
                args.insert(0, "auto".to_string());
                args.insert(0, "-hwaccel".to_string());
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
        // Get series name from navigation
        let series_name = self.navigation_stack.iter().find_map(|n| {
            if let NavigationLevel::Series(name) = n { Some(name.clone()) } else { None }
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
        };
        
        self.play_channel(&channel);
    }

    fn go_back(&mut self) {
        if self.navigation_stack.pop().is_some() {
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
                    
                    self.epg_data = Some(data);
                    self.epg_loading = false;
                    self.epg_progress = 1.0;
                    self.epg_last_update = Some(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64);
                    self.epg_status = format!("Loaded {} channels, {} programs", channel_count, program_count);
                }
                TaskResult::EpgError(msg) => {
                    self.log(&format!("[ERROR] EPG: {}", msg));
                    self.epg_loading = false;
                    self.epg_progress = 0.0;
                    self.epg_status = format!("Error: {}", msg);
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
        
        // Auto-login on startup if enabled
        if self.save_state && self.auto_login && !self.auto_login_triggered && !self.logged_in && !self.loading {
            if !self.server.is_empty() && !self.username.is_empty() && !self.password.is_empty() {
                self.auto_login_triggered = true;
                self.login();
            } else {
                self.auto_login_triggered = true;
            }
        }
        
        // EPG auto-load on startup if URL is saved
        if !self.epg_loading && !self.epg_url_input.is_empty() && self.epg_data.is_none() && !self.epg_startup_loaded {
            self.epg_startup_loaded = true;
            self.log("[INFO] Loading saved EPG on startup");
            self.load_epg();
        }
        
        // EPG auto-update check (periodic refresh)
        if !self.epg_loading && !self.epg_url_input.is_empty() && self.epg_data.is_some() {
            if let Some(interval_secs) = self.epg_auto_update.as_secs() {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                
                let should_update = match self.epg_last_update {
                    Some(last) => (now - last) >= interval_secs,
                    None => false,
                };
                
                if should_update {
                    self.log("[INFO] EPG auto-update triggered");
                    self.load_epg();
                }
            }
        }

        // Apply theme
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Top panel - Login controls
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.add_space(5.0);
            
            ui.horizontal(|ui| {
                ui.label("Server:");
                ui.add(egui::TextEdit::singleline(&mut self.server)
                    .hint_text("http://server.com:port")
                    .desired_width(180.0));
                
                ui.label("Username:");
                ui.add(egui::TextEdit::singleline(&mut self.username)
                    .hint_text("username")
                    .desired_width(100.0));
                
                ui.label("Password:");
                ui.add(egui::TextEdit::singleline(&mut self.password)
                    .password(true)
                    .hint_text("password")
                    .desired_width(100.0));
                
                if ui.button("🔑 Login").clicked() {
                    self.login();
                }
            });

            ui.horizontal(|ui| {
                if ui.button("📚 Address Book").clicked() {
                    self.show_address_book = true;
                }
                
                if ui.button("📋 M3U URL").clicked() {
                    self.show_m3u_dialog = true;
                }
                
                if ui.button("🌐 User Agent").clicked() {
                    self.show_user_agent_dialog = true;
                }
                
                if ui.button("📺 EPG").on_hover_text("Load Electronic Program Guide").clicked() {
                    self.show_epg_dialog = true;
                }
                
                ui.separator();
                
                ui.checkbox(&mut self.use_post_method, "Use POST");
                ui.checkbox(&mut self.dark_mode, "🌙 Dark");
                ui.checkbox(&mut self.single_window_mode, "Single Window")
                    .on_hover_text("Close previous player when opening new stream");
                
                ui.separator();
                
                ui.checkbox(&mut self.save_state, "💾 Save State")
                    .on_hover_text("Remember server, username, password, and settings");
                
                if self.save_state {
                    ui.checkbox(&mut self.auto_login, "Auto-Login")
                        .on_hover_text("Automatically login when app starts");
                }
                
                if ui.button("💾 Save").on_hover_text("Save current settings").clicked() {
                    self.save_current_state();
                }
            });
            
            ui.horizontal(|ui| {
                ui.label("🎬 Player:");
                ui.add(egui::TextEdit::singleline(&mut self.external_player)
                    .hint_text("mpv, vlc, ffplay, internal...")
                    .desired_width(200.0))
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
            if !self.logged_in {
                ui.centered_and_justified(|ui| {
                    ui.heading("Enter credentials and click Login");
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
                        .desired_width(200.0));
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
                        egui::ScrollArea::vertical()
                            .id_salt("channels_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width() - 15.0); // Leave room for scrollbar
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
                    });
                
                // EPG grid fills remaining space on right
                egui::CentralPanel::default()
                    .show_inside(ui, |ui| {
                        self.show_epg_grid_panel(ui);
                    });
            } else {
                // No EPG - full width for content
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
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
            }
        });

        // Address Book Window
        if self.show_address_book {
            egui::Window::new("📚 Address Book")
                .collapsible(false)
                .resizable(true)
                .min_width(380.0)
                .show(ctx, |ui| {
                    // Save current credentials section
                    ui.heading("Save Current Settings");
                    
                    // Show what will be saved
                    if !self.server.is_empty() {
                        ui.label(egui::RichText::new(format!("{}@{}", self.username, self.server)).weak());
                    }
                    
                    let can_save = !self.server.is_empty();
                    ui.add_enabled_ui(can_save, |ui| {
                        if ui.button("💾 Save to Address Book").clicked() {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs() as i64;
                            
                            let cred = SavedCredential {
                                server: self.server.clone(),
                                username: self.username.clone(),
                                password: self.password.clone(),
                                saved_at: now,
                                external_player: self.external_player.clone(),
                                buffer_seconds: self.buffer_seconds,
                                connection_quality: self.connection_quality,
                                selected_user_agent: self.selected_user_agent,
                                custom_user_agent: self.custom_user_agent.clone(),
                                use_custom_user_agent: self.use_custom_user_agent,
                                pass_user_agent_to_player: self.pass_user_agent_to_player,
                                epg_url: self.epg_url_input.clone(),
                                epg_time_offset: self.epg_time_offset,
                                epg_auto_update_index: self.epg_auto_update.to_index(),
                                epg_show_actual_time: self.epg_show_actual_time,
                            };
                            
                            // Update existing or add new
                            if let Some(existing) = self.address_book.iter_mut().find(|c| c.server == cred.server && c.username == cred.username) {
                                *existing = cred;
                            } else {
                                self.address_book.push(cred);
                            }
                            save_address_book(&self.address_book);
                            self.status_message = "Saved to address book".to_string();
                        }
                    });
                    
                    if !can_save {
                        ui.label(egui::RichText::new("Enter server credentials first").small().weak());
                    }
                    
                    ui.separator();
                    
                    // Saved credentials list
                    ui.heading("Saved Credentials");
                    
                    if self.address_book.is_empty() {
                        ui.label(egui::RichText::new("No saved credentials").weak());
                    }
                    
                    let mut to_delete: Option<usize> = None;
                    let mut to_load: Option<SavedCredential> = None;
                    
                    egui::ScrollArea::vertical()
                        .max_height(250.0)
                        .show(ui, |ui| {
                            for (i, cred) in self.address_book.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    // Load button
                                    if ui.button("▶").on_hover_text("Load credentials").clicked() {
                                        to_load = Some(cred.clone());
                                    }
                                    
                                    // Server info and saved date
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(format!("{}@{}", cred.username, cred.server)).strong());
                                        // Format saved_at timestamp
                                        let saved_str = if cred.saved_at > 0 {
                                            Self::format_datetime(cred.saved_at)
                                        } else {
                                            "Unknown".to_string()
                                        };
                                        ui.label(egui::RichText::new(format!("Saved: {}", saved_str)).small().weak());
                                    });
                                    
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button("🗑").on_hover_text("Delete").clicked() {
                                            to_delete = Some(i);
                                        }
                                    });
                                });
                                ui.separator();
                            }
                        });
                    
                    if let Some(cred) = to_load {
                        let cred_label = format!("{}@{}", cred.username, cred.server);
                        // Server credentials
                        self.server = cred.server;
                        self.username = cred.username;
                        self.password = cred.password;
                        // Player settings
                        self.external_player = cred.external_player;
                        self.buffer_seconds = cred.buffer_seconds;
                        self.connection_quality = cred.connection_quality;
                        // User agent settings
                        self.selected_user_agent = cred.selected_user_agent;
                        self.custom_user_agent = cred.custom_user_agent;
                        self.use_custom_user_agent = cred.use_custom_user_agent;
                        self.pass_user_agent_to_player = cred.pass_user_agent_to_player;
                        // EPG settings
                        self.epg_url_input = cred.epg_url;
                        self.epg_time_offset = cred.epg_time_offset;
                        self.epg_auto_update = EpgAutoUpdate::from_index(cred.epg_auto_update_index);
                        self.epg_show_actual_time = cred.epg_show_actual_time;
                        // Clear EPG data since we're loading new provider
                        self.epg_data = None;
                        self.epg_last_update = None;
                        self.epg_startup_loaded = false; // Allow auto-load for new provider
                        
                        self.status_message = format!("Loaded '{}'", cred_label);
                        self.show_address_book = false;
                    }
                    
                    if let Some(i) = to_delete {
                        let cred = &self.address_book[i];
                        let cred_label = format!("{}@{}", cred.username, cred.server);
                        self.address_book.remove(i);
                        save_address_book(&self.address_book);
                        self.status_message = format!("Removed '{}'", cred_label);
                    }
                    
                    ui.separator();
                    
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            self.show_address_book = false;
                        }
                    });
                });
        }

        // M3U URL Dialog
        if self.show_m3u_dialog {
            egui::Window::new("M3U Plus URL")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Enter M3U Plus URL:");
                    ui.add(egui::TextEdit::singleline(&mut self.m3u_url_input)
                        .hint_text("http://server/get.php?username=...&password=...&type=m3u_plus")
                        .desired_width(400.0));
                    
                    ui.horizontal(|ui| {
                        if ui.button("Extract & Login").clicked() {
                            self.extract_m3u_credentials(&self.m3u_url_input.clone());
                            self.show_m3u_dialog = false;
                            self.login();
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_m3u_dialog = false;
                        }
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
                        
                        ui.separator();
                        
                        if ui.button("🗑 Clear EPG Data").clicked() {
                            self.epg_data = None;
                            self.epg_last_update = None;
                            self.epg_status = "EPG data cleared".to_string();
                            self.log("[INFO] EPG data cleared");
                        }
                    }
                    
                    ui.separator();
                    
                    // Progress section
                    if self.epg_loading {
                        ui.label("Downloading and parsing...");
                        ui.add(egui::ProgressBar::new(self.epg_progress).show_percentage().animate(true));
                        ui.label(egui::RichText::new(&self.epg_status).color(egui::Color32::YELLOW));
                    } else if self.epg_status.starts_with("Loaded") {
                        ui.label(egui::RichText::new("✓ Completed").color(egui::Color32::GREEN).strong());
                        ui.add(egui::ProgressBar::new(1.0).show_percentage());
                        ui.label(egui::RichText::new(&self.epg_status).color(egui::Color32::GREEN));
                    } else if self.epg_status.starts_with("Error") {
                        ui.label(egui::RichText::new("✗ Failed").color(egui::Color32::RED).strong());
                        ui.add(egui::ProgressBar::new(0.0).show_percentage());
                        ui.label(egui::RichText::new(&self.epg_status).color(egui::Color32::RED));
                    }
                    
                    ui.separator();
                    
                    if ui.button("Close").clicked() {
                        self.show_epg_dialog = false;
                    }
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
            
            // Clone channels to avoid borrow issues
            let channels: Vec<_> = self.current_channels.clone();
            let mut toggle_fav: Option<FavoriteItem> = None;
            let mut to_play: Option<Channel> = None;
            
            for channel in &channels {
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
                        });
                    }
                    
                    if ui.button("▶").clicked() {
                        to_play = Some(channel.clone());
                    }
                    
                    ui.label(egui::RichText::new(&display_name).strong());
                    
                    // Show EPG info if available (only for live streams) - truncated to avoid overlap
                    if stream_type == "live" {
                        if let Some(epg_id) = &channel.epg_channel_id {
                            if let Some(program) = self.get_current_program(epg_id) {
                                // Truncate title for inline display
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
                                
                                let offset_secs = (self.epg_time_offset * 3600.0) as i64;
                                let now = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs() as i64;
                                let adjusted_now = now - offset_secs;
                                let remaining = (program.stop - adjusted_now) / 60;
                                if remaining > 0 {
                                    ui.label(egui::RichText::new(format!("({}m left)", remaining))
                                        .small()
                                        .color(egui::Color32::GRAY));
                                }
                            }
                        }
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

        // Show categories
        let search = self.search_query.to_lowercase();
        let mut clicked_category: Option<(String, String)> = None;
        
        for cat in categories {
            let display_name = Self::sanitize_text(&cat.category_name);
            if !search.is_empty() && !display_name.to_lowercase().contains(&search) {
                continue;
            }
            
            if ui.button(&display_name).clicked() {
                clicked_category = Some((cat.category_id.clone(), cat.category_name.clone()));
            }
        }
        
        if let Some((cat_id, cat_name)) = clicked_category {
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
                    self.navigation_stack.push(NavigationLevel::Episodes(sid, s));
                    self.fetch_episodes(sid, s);
                }
                return;
            }
        }

        // Series list
        if !self.current_series.is_empty() {
            let mut clicked_series: Option<i64> = None;
            
            for series in &self.current_series {
                let display_name = Self::sanitize_text(&series.name);
                if !search.is_empty() && !display_name.to_lowercase().contains(&search) {
                    continue;
                }
                
                if ui.button(&display_name).clicked() {
                    clicked_series = Some(series.series_id);
                }
            }
            
            if let Some(sid) = clicked_series {
                self.navigation_stack.push(NavigationLevel::Seasons(sid));
                self.fetch_series_info(sid);
            }
            return;
        }

        // Categories
        let mut clicked_category: Option<(String, String)> = None;
        
        for cat in &self.series_categories {
            let display_name = Self::sanitize_text(&cat.category_name);
            if !search.is_empty() && !display_name.to_lowercase().contains(&search) {
                continue;
            }
            
            if ui.button(&display_name).clicked() {
                clicked_category = Some((cat.category_id.clone(), cat.category_name.clone()));
            }
        }
        
        if let Some((cat_id, cat_name)) = clicked_category {
            self.navigation_stack.push(NavigationLevel::Series(cat_name));
            self.fetch_series_list(&cat_id);
        }
    }

    fn show_favorites_tab(&mut self, ui: &mut egui::Ui) {
        if self.favorites.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.heading("No favorites yet");
                ui.label("Click ☆ next to any channel or movie to add it to favorites");
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
        
        let mut to_remove: Option<String> = None;
        let mut to_play: Option<FavoriteItem> = None;
        
        if !live_favs.is_empty() {
            egui::CollapsingHeader::new(format!("Live Channels ({})", live_favs.len()))
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
                            ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&fav.category_name))).weak());
                        });
                    }
                });
        }
        
        if !movie_favs.is_empty() {
            egui::CollapsingHeader::new(format!("Movies ({})", movie_favs.len()))
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
                            ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&fav.category_name))).weak());
                        });
                    }
                });
        }
        
        if !series_favs.is_empty() {
            egui::CollapsingHeader::new(format!("Series ({})", series_favs.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for fav in &series_favs {
                        ui.horizontal(|ui| {
                            if ui.button(egui::RichText::new("★").size(18.0).color(egui::Color32::GOLD)).on_hover_text("Remove from favorites").clicked() {
                                to_remove = Some(fav.url.clone());
                            }
                            if ui.button("▶").clicked() {
                                to_play = Some(fav.clone());
                            }
                            ui.label(Self::sanitize_text(&fav.name));
                            ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&fav.category_name))).weak());
                        });
                    }
                });
        }
        
        // Handle play action
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
                ui.label(egui::RichText::new(format!("({})", Self::sanitize_text(&item.category_name))).weak());
                
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

    fn add_to_recent(&mut self, item: FavoriteItem) {
        // Remove if already in list (to move to top)
        self.recent_watched.retain(|r| r.url != item.url);
        
        // Add to front (newest first)
        self.recent_watched.insert(0, item);
        
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
        let channel_col_width = 125.0;  // Wider channel column for longer names
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
        
        // Horizontal + Vertical scroll area for the entire grid
        egui::ScrollArea::both()
            .id_salt("epg_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Time header row
                ui.horizontal(|ui| {
                    ui.add_sized([channel_col_width, 20.0], egui::Label::new("")); // Channel column spacer
                    for label in &time_labels {
                        ui.add_sized([prog_col_width, 20.0], egui::Label::new(egui::RichText::new(label).strong()));
                    }
                });
                
                ui.separator();
                
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
                            // Search EPG data for matching channel name
                            if let Some(ref epg) = self.epg_data {
                                let channel_name_lower = channel_name.to_lowercase();
                                epg.channels.iter()
                                    .find(|(_id, ch)| {
                                        let ch_name_lower = ch.name.to_lowercase();
                                        ch_name_lower.contains(&channel_name_lower) ||
                                        channel_name_lower.contains(&ch_name_lower)
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
                        
                        let response = ui.add_sized([channel_col_width - 5.0, 20.0], egui::SelectableLabel::new(
                            is_selected, 
                            egui::RichText::new(&short_name).strong()
                        ));
                        
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
                                
                                egui::Frame::none()
                                    .fill(bg_color)
                                    .inner_margin(egui::Margin::symmetric(4.0, 3.0))
                                    .rounding(3.0)
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
        // Convert UTC timestamp to local time display using chrono
        use chrono::{TimeZone, Local};
        
        if let Some(dt) = Local.timestamp_opt(ts, 0).single() {
            dt.format("%H:%M").to_string()
        } else {
            // Fallback for invalid timestamp
            let secs = ts % 86400;
            let hours = (secs / 3600) % 24;
            let mins = (secs % 3600) / 60;
            format!("{:02}:{:02}", hours, mins)
        }
    }
    
    fn format_datetime(ts: i64) -> String {
        // Convert Unix timestamp to readable date/time
        let secs_per_day = 86400;
        let days_since_epoch = ts / secs_per_day;
        let secs_today = ts % secs_per_day;
        
        let hours = secs_today / 3600;
        let mins = (secs_today % 3600) / 60;
        
        // Simple date calculation (approximate, doesn't account for leap years perfectly)
        let mut year = 1970;
        let mut remaining_days = days_since_epoch;
        
        loop {
            let days_in_year = if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 366 } else { 365 };
            if remaining_days < days_in_year {
                break;
            }
            remaining_days -= days_in_year;
            year += 1;
        }
        
        let days_in_months = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut month = 1;
        for &days in &days_in_months {
            let days = if month == 2 && year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { days };
            if remaining_days < days {
                break;
            }
            remaining_days -= days;
            month += 1;
        }
        let day = remaining_days + 1;
        
        format!("{:04}-{:02}-{:02} {:02}:{:02}", year, month, day, hours, mins)
    }
}

fn format_timestamp(ts: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    
    let d = UNIX_EPOCH + Duration::from_secs(ts as u64);
    // Simple formatting
    format!("{:?}", d)
}
