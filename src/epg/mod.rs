//! EPG (Electronic Program Guide) module
//! 
//! Contains the XMLTV parser and related types.

mod parser;

// Re-export public types
pub use parser::{
    EpgData,
    Program,
    EpgDownloader,
    DownloadConfig,
    ProgressCallback,
};

/// EPG auto-update interval settings
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
    /// Get interval in seconds, or None if auto-update is off
    pub fn as_secs(&self) -> Option<i64> {
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
    
    /// Get human-readable label
    pub fn label(&self) -> &'static str {
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
    
    /// Convert to index for storage
    pub fn to_index(&self) -> u8 {
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
    
    /// Create from storage index
    pub fn from_index(i: u8) -> Self {
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

/// Format a Unix timestamp as local time HH:MM
pub fn format_time(ts: i64) -> String {
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

/// Format a Unix timestamp as local datetime YYYY-MM-DD HH:MM
pub fn format_datetime(ts: i64) -> String {
    use chrono::{TimeZone, Local};
    
    if let Some(dt) = Local.timestamp_opt(ts, 0).single() {
        dt.format("%Y-%m-%d %H:%M").to_string()
    } else {
        // Fallback for invalid timestamp
        let secs_per_day = 86400;
        let days_since_epoch = ts / secs_per_day;
        let secs_today = ts % secs_per_day;
        
        let hours = secs_today / 3600;
        let mins = (secs_today % 3600) / 60;
        
        // Simple date calculation
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
