//! EPG (Electronic Program Guide) Parser
//! Fast streaming parser for XMLTV format - handles 100MB+ files efficiently
//! Supports both plain XML and gzip-compressed (.xml.gz) files

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::collections::HashMap;
use std::io::{BufRead, Read};
use flate2::read::GzDecoder;

/// A single TV program/show
#[derive(Debug, Clone)]
pub struct Program {
    /// Channel ID this program belongs to
    pub channel_id: String,
    /// Program title
    pub title: String,
    /// Program description (optional)
    pub description: Option<String>,
    /// Start time as Unix timestamp
    pub start: i64,
    /// End time as Unix timestamp
    pub stop: i64,
    /// Category/genre (optional)
    pub category: Option<String>,
    /// Episode info (optional) e.g., "S01E05"
    pub episode: Option<String>,
    /// Program icon/poster URL (optional)
    pub icon: Option<String>,
}

/// Channel information from EPG
#[derive(Debug, Clone)]
pub struct EpgChannel {
    /// Channel ID (matches epg_channel_id in streams)
    pub id: String,
    /// Display name
    pub name: String,
    /// Channel icon/logo URL (optional)
    pub icon: Option<String>,
}

/// Parsed EPG data
#[derive(Debug, Clone, Default)]
pub struct EpgData {
    /// Channel information indexed by channel ID
    pub channels: HashMap<String, EpgChannel>,
    /// Programs indexed by channel ID
    pub programs: HashMap<String, Vec<Program>>,
    /// Parse errors encountered (up to 50)
    pub parse_errors: Vec<String>,
    /// Total count of parse errors
    pub parse_error_count: usize,
}

impl EpgData {
    /// Create empty EPG data
    pub fn new() -> Self {
        Self::default()
    }

    /// Get current program for a channel
    pub fn current_program(&self, channel_id: &str) -> Option<&Program> {
        let now = current_timestamp();
        self.programs
            .get(channel_id)?
            .iter()
            .find(|p| p.start <= now && p.stop > now)
    }

    /// Get next program for a channel
    pub fn next_program(&self, channel_id: &str) -> Option<&Program> {
        let now = current_timestamp();
        self.programs
            .get(channel_id)?
            .iter()
            .find(|p| p.start > now)
    }

    /// Get programs for a channel within a time range
    pub fn programs_in_range(&self, channel_id: &str, start: i64, end: i64) -> Vec<&Program> {
        self.programs
            .get(channel_id)
            .map(|progs| progs.iter().filter(|p| p.stop > start && p.start < end).collect())
            .unwrap_or_default()
    }

    /// Get all programs for today for a channel
    pub fn today_programs(&self, channel_id: &str) -> Vec<&Program> {
        let now = current_timestamp();
        let today_start = (now / 86400) * 86400;
        let today_end = today_start + 86400;
        self.programs_in_range(channel_id, today_start, today_end)
    }

    /// Total number of programs
    pub fn program_count(&self) -> usize {
        self.programs.values().map(|v| v.len()).sum()
    }
}

fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Parser state
#[derive(Debug, Clone, Copy, PartialEq)]
enum ParserState {
    Root,
    Channel,
    Programme,
    Title,
    Desc,
    Category,
    DisplayName,
    EpisodeNum,
}

/// EPG Parser for XMLTV format - streaming, memory efficient
pub struct EpgParser;

impl EpgParser {
    /// Parse EPG from XMLTV string (for smaller files)
    pub fn parse(xml: &str) -> Result<EpgData, String> {
        Self::parse_reader(xml.as_bytes())
    }

    /// Parse EPG from a reader - streaming, handles large files
    pub fn parse_reader<R: BufRead>(reader: R) -> Result<EpgData, String> {
        let mut xml_reader = Reader::from_reader(reader);
        xml_reader.config_mut().trim_text(true);

        let mut epg = EpgData::new();
        let mut buf = Vec::with_capacity(8192);
        
        let mut state = ParserState::Root;
        let mut current_channel: Option<EpgChannel> = None;
        let mut current_program: Option<Program> = None;
        let mut text_buf = String::new();
        let mut error_count = 0;
        let mut errors: Vec<String> = Vec::new();

        loop {
            let position = xml_reader.buffer_position();
            match xml_reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) | Ok(Event::Empty(ref e)) => {
                    let name = e.name();
                    let name_bytes = name.as_ref();

                    match name_bytes {
                        b"channel" => {
                            state = ParserState::Channel;
                            let id = get_attribute(e, b"id").unwrap_or_default();
                            current_channel = Some(EpgChannel {
                                id,
                                name: String::new(),
                                icon: None,
                            });
                        }
                        b"programme" => {
                            state = ParserState::Programme;
                            let channel_id = get_attribute(e, b"channel").unwrap_or_default();
                            let start = get_attribute(e, b"start")
                                .map(|s| parse_xmltv_time(&s))
                                .unwrap_or(0);
                            let stop = get_attribute(e, b"stop")
                                .map(|s| parse_xmltv_time(&s))
                                .unwrap_or(0);

                            current_program = Some(Program {
                                channel_id,
                                title: String::new(),
                                description: None,
                                start,
                                stop,
                                category: None,
                                episode: None,
                                icon: None,
                            });
                        }
                        b"title" if state == ParserState::Programme => {
                            state = ParserState::Title;
                            text_buf.clear();
                        }
                        b"desc" if state == ParserState::Programme => {
                            state = ParserState::Desc;
                            text_buf.clear();
                        }
                        b"category" if state == ParserState::Programme => {
                            state = ParserState::Category;
                            text_buf.clear();
                        }
                        b"display-name" if state == ParserState::Channel => {
                            state = ParserState::DisplayName;
                            text_buf.clear();
                        }
                        b"episode-num" if state == ParserState::Programme => {
                            state = ParserState::EpisodeNum;
                            text_buf.clear();
                        }
                        b"icon" => {
                            if let Some(src) = get_attribute(e, b"src") {
                                match state {
                                    ParserState::Channel => {
                                        if let Some(ref mut chan) = current_channel {
                                            chan.icon = Some(src);
                                        }
                                    }
                                    ParserState::Programme => {
                                        if let Some(ref mut prog) = current_program {
                                            prog.icon = Some(src);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
                Ok(Event::Text(e)) => {
                    // Get raw bytes and convert to string
                    let raw = String::from_utf8_lossy(e.as_ref()).to_string();
                    // Decode common XML entities
                    let text = decode_xml_entities(&raw);
                    match state {
                        ParserState::Title
                        | ParserState::Desc
                        | ParserState::Category
                        | ParserState::DisplayName
                        | ParserState::EpisodeNum => {
                            text_buf.push_str(&text);
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let name = e.name();
                    let name_bytes = name.as_ref();

                    match name_bytes {
                        b"channel" => {
                            if let Some(channel) = current_channel.take() {
                                if !channel.id.is_empty() {
                                    epg.channels.insert(channel.id.clone(), channel);
                                }
                            }
                            state = ParserState::Root;
                        }
                        b"programme" => {
                            if let Some(program) = current_program.take() {
                                if !program.channel_id.is_empty() && !program.title.is_empty() {
                                    epg.programs
                                        .entry(program.channel_id.clone())
                                        .or_default()
                                        .push(program);
                                }
                            }
                            state = ParserState::Root;
                        }
                        b"title" => {
                            if let Some(ref mut prog) = current_program {
                                prog.title = text_buf.trim().to_string();
                            }
                            state = ParserState::Programme;
                        }
                        b"desc" => {
                            if let Some(ref mut prog) = current_program {
                                let desc = text_buf.trim().to_string();
                                if !desc.is_empty() {
                                    prog.description = Some(desc);
                                }
                            }
                            state = ParserState::Programme;
                        }
                        b"category" => {
                            if let Some(ref mut prog) = current_program {
                                let cat = text_buf.trim().to_string();
                                if !cat.is_empty() {
                                    prog.category = Some(cat);
                                }
                            }
                            state = ParserState::Programme;
                        }
                        b"display-name" => {
                            if let Some(ref mut chan) = current_channel {
                                if chan.name.is_empty() {
                                    chan.name = text_buf.trim().to_string();
                                }
                            }
                            state = ParserState::Channel;
                        }
                        b"episode-num" => {
                            if let Some(ref mut prog) = current_program {
                                let ep = format_episode(text_buf.trim());
                                if !ep.is_empty() {
                                    prog.episode = Some(ep);
                                }
                            }
                            state = ParserState::Programme;
                        }
                        _ => {}
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    error_count += 1;
                    // Log error with position/details
                    let error_msg = format!(
                        "XML error at byte {}: {}",
                        position,
                        e
                    );
                    if errors.len() < 50 { // Limit stored errors
                        errors.push(error_msg);
                    }
                    
                    // Reset state to root to skip malformed element
                    if current_program.is_some() {
                        current_program = None;
                    }
                    if current_channel.is_some() {
                        current_channel = None;
                    }
                    state = ParserState::Root;
                    text_buf.clear();
                    // Continue to next element
                }
                _ => {}
            }
            buf.clear();
        }

        // Sort programs by start time
        for programs in epg.programs.values_mut() {
            programs.sort_by_key(|p| p.start);
        }

        // Store errors in epg for reporting
        epg.parse_errors = errors;
        epg.parse_error_count = error_count;

        // Return what we got, even if partially parsed
        Ok(epg)
    }

    /// Parse EPG from file path - streams from disk
    /// Parse EPG from file - auto-detects gzip compression
    pub fn parse_file(path: &str) -> Result<EpgData, String> {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
        
        // Read first 2 bytes to check for gzip magic number (1f 8b)
        let mut magic = [0u8; 2];
        reader.read_exact(&mut magic).map_err(|e| e.to_string())?;
        
        // Seek back to start
        use std::io::Seek;
        reader.seek(std::io::SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        
        // Check for gzip magic bytes
        if magic[0] == 0x1f && magic[1] == 0x8b {
            // Gzip compressed - decompress first
            let decoder = GzDecoder::new(reader);
            let buf_reader = std::io::BufReader::with_capacity(64 * 1024, decoder);
            let sanitizing_reader = SanitizingBufReader::new(buf_reader);
            Self::parse_reader(sanitizing_reader)
        } else {
            // Plain XML
            let sanitizing_reader = SanitizingBufReader::new(reader);
            Self::parse_reader(sanitizing_reader)
        }
    }
}

/// BufReader wrapper that filters out illegal XML 1.0 characters on read
/// Legal XML 1.0: #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]
/// Also handles some common XML issues like invalid UTF-8 and unescaped ampersands
struct SanitizingBufReader<R> {
    inner: R,
    buffer: Vec<u8>,
    out_buffer: Vec<u8>,
    pos: usize,
    filled: usize,
}

impl<R: std::io::Read> SanitizingBufReader<R> {
    fn new(inner: R) -> Self {
        Self { 
            inner,
            buffer: vec![0u8; 64 * 1024],
            out_buffer: Vec::with_capacity(96 * 1024), // Slightly larger for escapes
            pos: 0,
            filled: 0,
        }
    }
    
    fn sanitize_byte(b: u8) -> u8 {
        // For single-byte chars, only allow: tab(0x9), newline(0xA), carriage return(0xD), and >= 0x20
        // Also filter out DEL (0x7F) and high control chars that cause issues
        match b {
            0x09 | 0x0A | 0x0D => b,  // Tab, LF, CR - keep
            0x00..=0x1F => 0x20,       // Control chars -> space
            0x7F => 0x20,              // DEL -> space
            _ => b,                     // Everything else keep
        }
    }
    
    fn refill_buffer(&mut self) -> std::io::Result<()> {
        let n = self.inner.read(&mut self.buffer)?;
        self.out_buffer.clear();
        
        let mut i = 0;
        while i < n {
            let b = Self::sanitize_byte(self.buffer[i]);
            
            // Check for bare & that's not a valid entity
            if b == b'&' {
                // Look ahead to see if this is a valid entity
                let remaining = &self.buffer[i..n];
                if !Self::is_valid_entity_start(remaining) {
                    // Replace bare & with &amp;
                    self.out_buffer.extend_from_slice(b"&amp;");
                    i += 1;
                    continue;
                }
            }
            
            self.out_buffer.push(b);
            i += 1;
        }
        
        self.pos = 0;
        self.filled = self.out_buffer.len();
        Ok(())
    }
    
    /// Check if bytes starting with & look like a valid XML entity
    fn is_valid_entity_start(bytes: &[u8]) -> bool {
        if bytes.len() < 2 {
            return false;
        }
        
        // Check for numeric entity &#
        if bytes.len() >= 2 && bytes[1] == b'#' {
            return true; // Assume numeric entities are valid
        }
        
        // Check for named entities - look for pattern &name;
        // Valid entity names: amp, lt, gt, quot, apos, nbsp, etc.
        let mut end = 1;
        while end < bytes.len() && end < 10 {
            match bytes[end] {
                b';' => return end > 1, // Found valid entity end
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => end += 1,
                _ => return false, // Invalid char in entity name
            }
        }
        
        false // No semicolon found within reasonable distance
    }
}

impl<R: std::io::Read> std::io::Read for SanitizingBufReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.filled {
            self.refill_buffer()?;
            if self.filled == 0 {
                return Ok(0); // EOF
            }
        }
        
        let available = self.filled - self.pos;
        let to_copy = available.min(buf.len());
        buf[..to_copy].copy_from_slice(&self.out_buffer[self.pos..self.pos + to_copy]);
        self.pos += to_copy;
        Ok(to_copy)
    }
}

impl<R: std::io::Read> std::io::BufRead for SanitizingBufReader<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if self.pos >= self.filled {
            self.refill_buffer()?;
        }
        Ok(&self.out_buffer[self.pos..self.filled])
    }
    
    fn consume(&mut self, amt: usize) {
        self.pos = (self.pos + amt).min(self.filled);
    }
}

/// Decode XML entities back to normal characters
fn decode_xml_entities(s: &str) -> String {
    let mut result = s.to_string();
    
    // Decode named entities
    result = result.replace("&amp;", "&");
    result = result.replace("&lt;", "<");
    result = result.replace("&gt;", ">");
    result = result.replace("&quot;", "\"");
    result = result.replace("&apos;", "'");
    result = result.replace("&nbsp;", " ");
    
    // Decode numeric entities (decimal)
    while let Some(start) = result.find("&#") {
        if let Some(end) = result[start..].find(';') {
            let entity = &result[start..start + end + 1];
            let num_str = &entity[2..entity.len() - 1];
            
            // Check for hex (&#x...) or decimal (&#...)
            let decoded = if num_str.starts_with('x') || num_str.starts_with('X') {
                u32::from_str_radix(&num_str[1..], 16).ok()
            } else {
                num_str.parse::<u32>().ok()
            };
            
            if let Some(code) = decoded {
                if let Some(c) = char::from_u32(code) {
                    result = result.replace(entity, &c.to_string());
                    continue;
                }
            }
        }
        break; // Malformed entity, stop processing
    }
    
    result
}

/// Get attribute value from XML element
fn get_attribute(e: &quick_xml::events::BytesStart, name: &[u8]) -> Option<String> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == name {
            let raw = String::from_utf8(attr.value.as_ref().to_vec()).ok()?;
            return Some(decode_xml_entities(&raw));
        }
    }
    None
}

/// Parse XMLTV time format: "20240115120000 +0000" -> Unix timestamp
fn parse_xmltv_time(time_str: &str) -> i64 {
    let time_str = time_str.trim();

    // Split off timezone if present
    let (datetime, tz_offset) = if let Some(space_pos) = time_str.find(' ') {
        let (dt, tz) = time_str.split_at(space_pos);
        (dt, parse_tz_offset(tz.trim()))
    } else if time_str.len() > 14 {
        (&time_str[..14], parse_tz_offset(&time_str[14..]))
    } else {
        (time_str, 0)
    };

    if datetime.len() < 14 {
        return 0;
    }

    let year: i64 = datetime[0..4].parse().unwrap_or(2024);
    let month: i64 = datetime[4..6].parse().unwrap_or(1);
    let day: i64 = datetime[6..8].parse().unwrap_or(1);
    let hour: i64 = datetime[8..10].parse().unwrap_or(0);
    let minute: i64 = datetime[10..12].parse().unwrap_or(0);
    let second: i64 = datetime[12..14].parse().unwrap_or(0);

    // Days from year
    let mut days: i64 = 0;
    for y in 1970..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }

    // Days from month
    const DAYS_IN_MONTH: [i64; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for m in 1..month {
        days += DAYS_IN_MONTH[(m - 1) as usize];
        if m == 2 && is_leap_year(year) {
            days += 1;
        }
    }

    // Days in current month
    days += day - 1;

    days * 86400 + hour * 3600 + minute * 60 + second - tz_offset
}

/// Parse timezone offset like "+0100" or "-0530" to seconds
fn parse_tz_offset(tz: &str) -> i64 {
    let tz = tz.trim();
    if tz.is_empty() {
        return 0;
    }

    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let tz = tz.trim_start_matches(['+', '-']);

    if tz.len() >= 4 {
        let hours: i64 = tz[0..2].parse().unwrap_or(0);
        let minutes: i64 = tz[2..4].parse().unwrap_or(0);
        sign * (hours * 3600 + minutes * 60)
    } else if tz.len() >= 2 {
        let hours: i64 = tz[0..2].parse().unwrap_or(0);
        sign * hours * 3600
    } else {
        0
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Format episode number (e.g., "0.4." -> "S01E05")
fn format_episode(episode: &str) -> String {
    let episode = episode.trim();

    // XMLTV format: "season.episode.part" (0-indexed)
    let parts: Vec<&str> = episode.split('.').collect();

    if parts.len() >= 2 {
        let season: i32 = parts[0].parse().unwrap_or(-1) + 1;
        let ep: i32 = parts[1].parse().unwrap_or(-1) + 1;

        if season > 0 && ep > 0 {
            return format!("S{:02}E{:02}", season, ep);
        }
    }

    episode.to_string()
}

/// Download configuration
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// Maximum retry attempts
    pub max_retries: u32,
    /// Delay between retries in milliseconds
    pub retry_delay_ms: u64,
    /// Connection timeout in seconds
    pub connect_timeout_secs: u64,
    /// Read timeout in seconds  
    pub read_timeout_secs: u64,
    /// Chunk size for reading (bytes)
    pub chunk_size: usize,
    /// User agent string
    pub user_agent: String,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_delay_ms: 2000,
            connect_timeout_secs: 30,
            read_timeout_secs: 120,
            chunk_size: 64 * 1024, // 64KB chunks
            user_agent: "XtremeIPTV/1.0".to_string(),
        }
    }
}

/// Download progress callback: (downloaded_bytes, total_bytes)
pub type ProgressCallback = Box<dyn Fn(u64, Option<u64>) + Send>;

/// EPG Downloader with HTTPS support
pub struct EpgDownloader;

impl EpgDownloader {
    /// Create a configured ureq agent
    fn create_agent(config: &DownloadConfig) -> ureq::Agent {
        use std::time::Duration;
        
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(config.read_timeout_secs)))
            .timeout_connect(Some(Duration::from_secs(config.connect_timeout_secs)))
            .max_idle_connections(4)
            .max_idle_connections_per_host(2)
            .build()
            .new_agent()
    }

    /// Download EPG to file with retry support (supports HTTP and HTTPS)
    pub fn download_to_file(
        url: &str,
        output_path: &str,
        config: &DownloadConfig,
        progress: Option<ProgressCallback>,
    ) -> Result<String, String> {
        use std::time::Duration;

        let agent = Self::create_agent(config);
        let mut attempts = 0;

        loop {
            attempts += 1;

            match Self::try_download(&agent, url, output_path, config, &progress) {
                Ok(total) => {
                    if let Some(ref cb) = progress {
                        cb(total, Some(total));
                    }
                    return Ok(output_path.to_string());
                }
                Err(e) => {
                    if attempts >= config.max_retries {
                        return Err(format!("Download failed after {} attempts: {}", attempts, e));
                    }

                    // Wait before retry
                    std::thread::sleep(Duration::from_millis(config.retry_delay_ms));
                }
            }
        }
    }

    fn try_download(
        agent: &ureq::Agent,
        url: &str,
        output_path: &str,
        config: &DownloadConfig,
        progress: &Option<ProgressCallback>,
    ) -> Result<u64, String> {
        use std::io::{Read, Write};

        let response = agent
            .get(url)
            .header("User-Agent", &config.user_agent)
            .call()
            .map_err(|e| format!("Request failed: {}", e))?;

        let status = response.status();
        if status != 200 && status != 206 {
            return Err(format!("HTTP error: {}", status));
        }

        // Get content length if available
        let total_size: Option<u64> = response
            .headers()
            .get("Content-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse().ok());

        // Open file for writing
        let mut file = std::fs::File::create(output_path)
            .map_err(|e| format!("Create file failed: {}", e))?;

        // Stream the response body
        let mut reader = response.into_body().into_reader();
        let mut buffer = vec![0u8; config.chunk_size];
        let mut downloaded: u64 = 0;

        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    file.write_all(&buffer[..n])
                        .map_err(|e| format!("Write failed: {}", e))?;
                    downloaded += n as u64;

                    if let Some(ref cb) = progress {
                        cb(downloaded, total_size);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(format!("Read failed: {}", e)),
            }
        }

        file.flush().map_err(|e| format!("Flush failed: {}", e))?;
        Ok(downloaded)
    }

    /// Download and parse EPG in one step with retry support
    pub fn download_and_parse(
        url: &str,
        config: &DownloadConfig,
        progress: Option<ProgressCallback>,
    ) -> Result<EpgData, String> {
        // Create temp file - use appropriate extension based on URL
        let ext = if url.ends_with(".gz") { "xml.gz" } else { "xml" };
        let temp_path = std::env::temp_dir().join(format!("xtreme_iptv_epg.{}", ext));
        let temp_path_str = temp_path.to_string_lossy().to_string();

        // Download with retry
        Self::download_to_file(url, &temp_path_str, config, progress)?;

        // Parse the downloaded file (auto-detects gzip)
        let result = EpgParser::parse_file(&temp_path_str);

        // Clean up temp file
        let _ = std::fs::remove_file(&temp_path);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_xmltv_time() {
        let ts = parse_xmltv_time("20240115120000 +0000");
        assert!(ts > 0);

        let ts1 = parse_xmltv_time("20240115120000 +0100");
        let ts2 = parse_xmltv_time("20240115120000 +0000");
        assert_eq!(ts2 - ts1, 3600);
    }

    #[test]
    fn test_format_episode() {
        assert_eq!(format_episode("0.4."), "S01E05");
        assert_eq!(format_episode("1.9.0"), "S02E10");
    }

    #[test]
    fn test_parse_simple_epg() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<tv>
  <channel id="bbc1">
    <display-name>BBC One</display-name>
    <icon src="http://example.com/bbc1.png"/>
  </channel>
  <programme start="20240115120000 +0000" stop="20240115130000 +0000" channel="bbc1">
    <title>News at Noon</title>
    <desc>Daily news broadcast</desc>
    <category>News</category>
  </programme>
</tv>"#;

        let epg = EpgParser::parse(xml).unwrap();

        assert_eq!(epg.channels.len(), 1);
        assert_eq!(epg.channels.get("bbc1").unwrap().name, "BBC One");
        assert_eq!(epg.programs.get("bbc1").unwrap().len(), 1);
        assert_eq!(epg.programs.get("bbc1").unwrap()[0].title, "News at Noon");
    }

    #[test]
    fn test_program_count() {
        let xml = r#"<tv>
  <programme start="20240115120000" stop="20240115130000" channel="ch1"><title>Show 1</title></programme>
  <programme start="20240115130000" stop="20240115140000" channel="ch1"><title>Show 2</title></programme>
  <programme start="20240115120000" stop="20240115130000" channel="ch2"><title>Show 3</title></programme>
</tv>"#;

        let epg = EpgParser::parse(xml).unwrap();
        assert_eq!(epg.program_count(), 3);
    }
}
