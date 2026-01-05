//! M3U playlist parser with HTTPS download support
//! Supports both M3U/M3U Plus (IPTV) and M3U8 (HLS) formats

#![allow(dead_code)]

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct M3uChannel {
    pub name: String,
    pub url: String,
    pub group: Option<String>,
    pub tvg_id: Option<String>,
    pub tvg_logo: Option<String>,
    pub tvg_name: Option<String>,      // Alternate name for EPG matching
    pub tvg_chno: Option<u32>,          // Channel number (tvg-chno)
    pub channel_id: Option<String>,     // Channel ID (channel-id)
    pub channel_number: Option<u32>,    // Channel number (channel-number)
    pub catchup: Option<String>,        // Catchup type (default, shift, etc.)
    pub catchup_days: Option<u32>,      // Days of catchup available
}

#[derive(Debug, Clone, Default)]
pub struct M3uPlaylist {
    pub channels: Vec<M3uChannel>,
    pub epg_url: Option<String>,        // From x-tvg-url in header
}

#[derive(Debug, Clone)]
pub struct M3uCredentials {
    pub server: String,
    pub username: String,
    pub password: String,
}

/// Detect if content is M3U8 (HLS) format
fn is_hls_playlist(content: &str) -> bool {
    // HLS playlists contain these tags
    content.contains("#EXT-X-VERSION") ||
    content.contains("#EXT-X-TARGETDURATION") ||
    content.contains("#EXT-X-MEDIA-SEQUENCE") ||
    content.contains("#EXT-X-STREAM-INF") ||
    content.contains("#EXT-X-PLAYLIST-TYPE") ||
    content.contains("#EXT-X-ENDLIST")
}

/// Download and parse M3U from URL (supports HTTP and HTTPS)
pub fn download_and_parse(url: &str, user_agent: &str) -> Result<Vec<M3uChannel>, String> {
    let playlist = download_and_parse_playlist(url, user_agent)?;
    Ok(playlist.channels)
}

/// Download and parse M3U playlist with EPG URL extraction
pub fn download_and_parse_playlist(url: &str, user_agent: &str) -> Result<M3uPlaylist, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .timeout_connect(Some(Duration::from_secs(30)))
        .build()
        .new_agent();

    let mut response = agent
        .get(url)
        .header("User-Agent", user_agent)
        .call()
        .map_err(|e| format!("Request failed: {}", e))?;

    if response.status() != 200 {
        return Err(format!("HTTP error: {}", response.status()));
    }

    let content = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("Read failed: {}", e))?;

    let mut playlist = parse_m3u_playlist(&content);
    
    // For HLS media playlists (segments only), use the original URL as the stream
    if playlist.channels.len() == 1 && playlist.channels[0].url.is_empty() {
        playlist.channels[0].url = url.to_string();
        // Try to extract a name from the URL
        if let Some(name) = url.rsplit('/').next() {
            if !name.is_empty() {
                let name = name.trim_end_matches(".m3u8").trim_end_matches(".m3u");
                if !name.is_empty() {
                    playlist.channels[0].name = name.to_string();
                }
            }
        }
    }
    
    // For HLS master playlists, resolve relative URLs
    if is_hls_playlist(&content) && !playlist.channels.is_empty() {
        let base_url = get_base_url(url);
        for channel in &mut playlist.channels {
            if !channel.url.is_empty() && !channel.url.starts_with("http") {
                channel.url = format!("{}/{}", base_url, channel.url);
            }
        }
    }
    
    Ok(playlist)
}

/// Get base URL for resolving relative paths
fn get_base_url(url: &str) -> String {
    if let Some(pos) = url.rfind('/') {
        url[..pos].to_string()
    } else {
        url.to_string()
    }
}

/// Parse M3U and return playlist with EPG URL
pub fn parse_m3u_playlist(content: &str) -> M3uPlaylist {
    let mut playlist = M3uPlaylist::default();
    
    // Check if this is an HLS (M3U8) playlist
    if is_hls_playlist(content) {
        playlist.channels = parse_m3u8_hls(content);
        return playlist;
    }
    
    // Check first line for EPG URL (M3U Plus format)
    if let Some(first_line) = content.lines().next() {
        if first_line.starts_with("#EXTM3U") {
            // Extract x-tvg-url="..." or url-tvg="..."
            playlist.epg_url = extract_header_attr(first_line, "x-tvg-url")
                .or_else(|| extract_header_attr(first_line, "url-tvg"));
        }
    }
    
    playlist.channels = parse_m3u(content);
    playlist
}

/// Parse M3U8 HLS playlist (master or media playlist)
fn parse_m3u8_hls(content: &str) -> Vec<M3uChannel> {
    let mut channels = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        
        // Master playlist: #EXT-X-STREAM-INF contains stream variants
        if line.starts_with("#EXT-X-STREAM-INF:") {
            // Extract bandwidth and resolution for name
            let bandwidth = extract_hls_attr(line, "BANDWIDTH");
            let resolution = extract_hls_attr(line, "RESOLUTION");
            let name = extract_hls_attr(line, "NAME");
            let video_range = extract_hls_attr(line, "VIDEO-RANGE");
            let codecs = extract_hls_attr(line, "CODECS");
            
            // Next line should be the URL
            if i + 1 < lines.len() {
                let url_line = lines[i + 1].trim();
                if !url_line.is_empty() && !url_line.starts_with('#') {
                    let display_name = name.unwrap_or_else(|| {
                        // Determine HDR type from VIDEO-RANGE and codecs
                        let hdr_label = match video_range.as_deref() {
                            Some("PQ") | Some("HLG") => {
                                // Check for Dolby Vision codecs
                                if codecs.as_ref().map(|c| c.contains("dvh") || c.contains("dvhe")).unwrap_or(false) {
                                    Some("Dolby Vision")
                                } else {
                                    Some("HDR10")
                                }
                            }
                            Some("SDR") => Some("SDR"),
                            _ => None,
                        };
                        
                        // Build name from resolution + bandwidth + HDR type
                        let mut parts = Vec::new();
                        if let Some(res) = &resolution {
                            parts.push(res.clone());
                        }
                        if let Some(bw) = &bandwidth {
                            parts.push(format_bandwidth(bw));
                        }
                        if let Some(hdr) = hdr_label {
                            parts.push(hdr.to_string());
                        }
                        
                        if parts.is_empty() {
                            format!("Stream {}", channels.len() + 1)
                        } else {
                            parts.join(" - ")
                        }
                    });
                    
                    channels.push(M3uChannel {
                        name: display_name,
                        url: url_line.to_string(),
                        group: Some("HLS Streams".to_string()),
                        tvg_id: None,
                        tvg_logo: None,
                        tvg_name: None,
                        tvg_chno: None,
                        channel_id: None,
                        channel_number: None,
                        catchup: None,
                        catchup_days: None,
                    });
                    i += 1;
                }
            }
        }
        // Alternate streams: #EXT-X-MEDIA with URI (multi-angle, audio tracks, etc.)
        else if line.starts_with("#EXT-X-MEDIA:") {
            if let Some(uri) = extract_hls_attr(line, "URI") {
                let media_type = extract_hls_attr(line, "TYPE").unwrap_or_default();
                let name = extract_hls_attr(line, "NAME").unwrap_or_else(|| "Alternate".to_string());
                let group_id = extract_hls_attr(line, "GROUP-ID").unwrap_or_default();
                let is_default = extract_hls_attr(line, "DEFAULT")
                    .map(|v| v.eq_ignore_ascii_case("YES"))
                    .unwrap_or(false);
                
                // Skip default streams (they're usually in EXT-X-STREAM-INF too)
                if !is_default {
                    let display_name = if !group_id.is_empty() {
                        format!("{} ({})", name, group_id)
                    } else {
                        name
                    };
                    
                    let group = match media_type.to_uppercase().as_str() {
                        "VIDEO" => "HLS Video".to_string(),
                        "AUDIO" => "HLS Audio".to_string(),
                        "SUBTITLES" => "HLS Subtitles".to_string(),
                        _ => "HLS Alternate".to_string(),
                    };
                    
                    channels.push(M3uChannel {
                        name: display_name,
                        url: uri,
                        group: Some(group),
                        tvg_id: None,
                        tvg_logo: None,
                        tvg_name: None,
                        tvg_chno: None,
                        channel_id: None,
                        channel_number: None,
                        catchup: None,
                        catchup_days: None,
                    });
                }
            }
        }
        // Media playlist with #EXTINF segments - treat as single stream
        else if line.starts_with("#EXT-X-TARGETDURATION:") && channels.is_empty() {
            // This is a media playlist (segments), not a master playlist
            // Return a single "channel" representing the whole stream
            channels.push(M3uChannel {
                name: "HLS Stream".to_string(),
                url: String::new(), // Will be set by caller with original URL
                group: Some("HLS".to_string()),
                tvg_id: None,
                tvg_logo: None,
                tvg_name: None,
                tvg_chno: None,
                channel_id: None,
                channel_number: None,
                catchup: None,
                catchup_days: None,
            });
            // For media playlists, the original URL is the stream URL
            break;
        }
        
        i += 1;
    }
    
    channels
}

/// Extract attribute from HLS tag (e.g., BANDWIDTH=1280000)
fn extract_hls_attr(line: &str, attr: &str) -> Option<String> {
    let search = format!("{}=", attr);
    if let Some(start) = line.find(&search) {
        let rest = &line[start + search.len()..];
        // Handle quoted and unquoted values
        if rest.starts_with('"') {
            if let Some(end) = rest[1..].find('"') {
                return Some(rest[1..end + 1].to_string());
            }
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Format bandwidth to human-readable (e.g., 1280000 -> "1.3 Mbps")
fn format_bandwidth(bw: &str) -> String {
    if let Ok(bits) = bw.parse::<u64>() {
        if bits >= 1_000_000 {
            format!("{:.1} Mbps", bits as f64 / 1_000_000.0)
        } else if bits >= 1_000 {
            format!("{} Kbps", bits / 1_000)
        } else {
            format!("{} bps", bits)
        }
    } else {
        bw.to_string()
    }
}

/// Extract attribute from #EXTM3U header line
fn extract_header_attr(line: &str, attr_name: &str) -> Option<String> {
    let search = format!("{}=\"", attr_name);
    if let Some(start) = line.to_lowercase().find(&search) {
        let rest = &line[start + search.len()..];
        if let Some(end) = rest.find('"') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Parse M3U content and extract channels
pub fn parse_m3u(content: &str) -> Vec<M3uChannel> {
    // Pre-allocate based on rough estimate (one channel per ~200 bytes)
    let estimated_channels = content.len() / 200;
    let mut channels = Vec::with_capacity(estimated_channels.max(100));
    
    // Reuse buffer to avoid allocations
    let mut current_attrs = AttrBuffer::new();
    let mut current_name: Option<&str> = None;
    
    for line in content.lines() {
        let line = line.trim();
        let bytes = line.as_bytes();
        
        // Fast prefix check using bytes
        let info_part = if bytes.starts_with(b"#EXTINF:") {
            Some(&line[8..])
        } else if bytes.starts_with(b"EXTINF:") {
            Some(&line[7..])
        } else {
            None
        };
        
        if let Some(info_part) = info_part {
            current_attrs.clear();
            
            // Find first and last comma in single pass
            let info_bytes = info_part.as_bytes();
            let mut first_comma = None;
            let mut last_comma = None;
            let mut has_eq_before_comma = false;
            
            for (i, &b) in info_bytes.iter().enumerate() {
                if b == b',' {
                    if first_comma.is_none() {
                        first_comma = Some(i);
                    }
                    last_comma = Some(i);
                } else if b == b'=' && first_comma.is_none() {
                    has_eq_before_comma = true;
                }
            }
            
            if let Some(first) = first_comma {
                if has_eq_before_comma {
                    // Standard format: attrs before comma
                    extract_attrs_fast(info_part, &mut current_attrs);
                    if let Some(last) = last_comma {
                        current_name = Some(info_part[last + 1..].trim());
                    }
                } else {
                    // Alternate format: duration,attrs,name
                    let after_first = &info_part[first + 1..];
                    extract_attrs_fast(after_first, &mut current_attrs);
                    // Find last comma in remaining part
                    if let Some(pos) = after_first.rfind(',') {
                        current_name = Some(after_first[pos + 1..].trim());
                    } else {
                        current_name = Some(after_first.trim());
                    }
                }
            } else {
                extract_attrs_fast(info_part, &mut current_attrs);
            }
        } else if !bytes.is_empty() && bytes[0] != b'#' && !bytes.starts_with(b"EXTM3U") {
            // URL line
            if let Some(name) = current_name.take() {
                // Extract all attrs in one pass using indices
                let (group, tvg_id, tvg_logo, tvg_name, tvg_chno, channel_id, channel_number, catchup, catchup_days) = 
                    current_attrs.get_all();
                
                channels.push(M3uChannel {
                    name: name.to_string(),
                    url: line.to_string(),
                    group: group.map(|s| s.to_string()),
                    tvg_id: tvg_id.map(|s| s.to_string()),
                    tvg_logo: tvg_logo.map(|s| s.to_string()),
                    tvg_name: tvg_name.map(|s| s.to_string()),
                    tvg_chno: tvg_chno.and_then(|s| s.parse().ok()),
                    channel_id: channel_id.map(|s| s.to_string()),
                    channel_number: channel_number.and_then(|s| s.parse().ok()),
                    catchup: catchup.map(|s| s.to_string()),
                    catchup_days: catchup_days.and_then(|s| s.parse().ok()),
                });
            }
        }
    }
    
    channels
}

/// Lightweight attribute buffer - avoids HashMap overhead
struct AttrBuffer<'a> {
    attrs: [Option<(&'a str, &'a str)>; 12], // Increased for more attrs
    len: usize,
}

impl<'a> AttrBuffer<'a> {
    fn new() -> Self {
        Self {
            attrs: [None; 12],
            len: 0,
        }
    }
    
    fn clear(&mut self) {
        self.len = 0;
    }
    
    fn push(&mut self, key: &'a str, value: &'a str) {
        if self.len < 12 {
            self.attrs[self.len] = Some((key, value));
            self.len += 1;
        }
    }
    
    /// Get a single attribute by key (case-insensitive)
    #[allow(dead_code)]
    fn get(&self, key: &str) -> Option<&'a str> {
        for i in 0..self.len {
            if let Some((k, v)) = self.attrs[i] {
                if k.eq_ignore_ascii_case(key) {
                    return Some(v);
                }
            }
        }
        None
    }
    
    /// Get all known attributes in single pass - avoids repeated linear searches
    fn get_all(&self) -> (
        Option<&'a str>, // group-title
        Option<&'a str>, // tvg-id
        Option<&'a str>, // tvg-logo
        Option<&'a str>, // tvg-name
        Option<&'a str>, // tvg-chno
        Option<&'a str>, // channel-id
        Option<&'a str>, // channel-number
        Option<&'a str>, // catchup
        Option<&'a str>, // catchup-days
    ) {
        let mut group = None;
        let mut tvg_id = None;
        let mut tvg_logo = None;
        let mut tvg_name = None;
        let mut tvg_chno = None;
        let mut channel_id = None;
        let mut channel_number = None;
        let mut catchup = None;
        let mut catchup_days = None;
        
        for i in 0..self.len {
            if let Some((k, v)) = self.attrs[i] {
                // Compare lowercase first char for fast rejection
                let k_bytes = k.as_bytes();
                if k_bytes.is_empty() { continue; }
                
                match k_bytes[0].to_ascii_lowercase() {
                    b'g' => if k.eq_ignore_ascii_case("group-title") { group = Some(v); }
                    b't' => {
                        if k.eq_ignore_ascii_case("tvg-id") { tvg_id = Some(v); }
                        else if k.eq_ignore_ascii_case("tvg-logo") { tvg_logo = Some(v); }
                        else if k.eq_ignore_ascii_case("tvg-name") { tvg_name = Some(v); }
                        else if k.eq_ignore_ascii_case("tvg-chno") { tvg_chno = Some(v); }
                    }
                    b'c' => {
                        if k.eq_ignore_ascii_case("channel-id") { channel_id = Some(v); }
                        else if k.eq_ignore_ascii_case("channel-number") { channel_number = Some(v); }
                        else if k.eq_ignore_ascii_case("catchup") { catchup = Some(v); }
                        else if k.eq_ignore_ascii_case("catchup-days") { catchup_days = Some(v); }
                    }
                    _ => {}
                }
            }
        }
        
        (group, tvg_id, tvg_logo, tvg_name, tvg_chno, channel_id, channel_number, catchup, catchup_days)
    }
}

/// Fast attribute extraction using byte scanning
fn extract_attrs_fast<'a>(info: &'a str, attrs: &mut AttrBuffer<'a>) {
    let bytes = info.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    
    // Skip duration (e.g., "-1 ")
    while i < len && (bytes[i].is_ascii_digit() || bytes[i] == b'-' || bytes[i] == b'.') {
        i += 1;
    }
    
    while i < len {
        // Skip whitespace and stray quotes
        while i < len && (bytes[i].is_ascii_whitespace() || bytes[i] == b'"') {
            i += 1;
        }
        
        if i >= len { break; }
        
        // Check for comma (end of attributes, start of name)
        if bytes[i] == b',' { break; }
        
        // Find key (until '=')
        let key_start = i;
        while i < len && bytes[i] != b'=' && bytes[i] != b',' && bytes[i] != b'"' {
            i += 1;
        }
        
        if i >= len || bytes[i] == b',' { break; }
        
        // Skip if we hit a quote before '=' (malformed)
        if bytes[i] == b'"' {
            i += 1;
            continue;
        }
        
        let key = &info[key_start..i];
        i += 1; // skip '='
        
        if i >= len { break; }
        
        // Get value
        let value = if bytes[i] == b'"' {
            i += 1; // skip opening quote
            let value_start = i;
            // Find closing quote (handle escaped quotes)
            while i < len {
                if bytes[i] == b'"' && (i == value_start || bytes[i - 1] != b'\\') {
                    break;
                }
                i += 1;
            }
            let value = &info[value_start..i];
            if i < len { i += 1; } // skip closing quote
            value
        } else {
            // Unquoted value - read until space or comma
            let value_start = i;
            while i < len && !bytes[i].is_ascii_whitespace() && bytes[i] != b',' {
                i += 1;
            }
            &info[value_start..i]
        };
        
        // Store attribute (lowercase key for matching)
        let key_trimmed = key.trim();
        if !key_trimmed.is_empty() && !value.is_empty() {
            attrs.push(key_trimmed, value);
        }
    }
}

/// Extract credentials from M3U Plus URL
/// Format: http://server/get.php?username=XXX&password=YYY&type=m3u_plus
pub fn extract_credentials(url: &str) -> Option<M3uCredentials> {
    extract_from_query(url).or_else(|| extract_from_path(url))
}

/// Extract server base URL and path from a URL
/// Returns (server, path) e.g. ("http://example.com:8080", "/live/user/pass/1.ts")
fn parse_url_parts(url: &str) -> Option<(&str, &str)> {
    let proto_end = url.find("://")?;
    let rest = &url[proto_end + 3..];
    let path_start = rest.find('/').unwrap_or(rest.len());
    Some((&url[..proto_end + 3 + path_start], &rest[path_start..]))
}

fn extract_from_query(url: &str) -> Option<M3uCredentials> {
    let query_start = url.find('?')?;
    let query = &url[query_start + 1..];
    
    let mut username = None;
    let mut password = None;
    
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            match key {
                "username" | "user" => username = Some(value),
                "password" | "pass" => password = Some(value),
                _ => {}
            }
        }
    }
    
    let (server, _) = parse_url_parts(url)?;

    Some(M3uCredentials {
        server: server.to_string(),
        username: username?.to_string(),
        password: password?.to_string(),
    })
}

fn extract_from_path(url: &str) -> Option<M3uCredentials> {
    let (server, path) = parse_url_parts(url)?;
    
    // Need a path with segments
    if path.is_empty() || path == "/" {
        return None;
    }
    
    let segments: Vec<&str> = path[1..].split('/').filter(|s| !s.is_empty()).collect();

    // Pattern: live/username/password/...
    if segments.len() >= 3 {
        let prefixes = ["live", "movie", "series"];
        if prefixes.contains(&segments[0].to_lowercase().as_str()) {
            return Some(M3uCredentials {
                server: server.to_string(),
                username: segments[1].to_string(),
                password: segments[2].to_string(),
            });
        }
    }

    // Pattern: username/password/...
    if segments.len() >= 2 {
        let first = segments[0];
        let second = segments[1];
        
        if !first.contains('.') && !second.contains('.') 
            && first.len() > 1 && second.len() > 1 {
            return Some(M3uCredentials {
                server: server.to_string(),
                username: first.to_string(),
                password: second.to_string(),
            });
        }
    }

    None
}

#[cfg(test)]
#[path = "m3u_parser_tests.rs"]
mod tests;