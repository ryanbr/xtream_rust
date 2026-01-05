//! M3U playlist parser with HTTPS download support

#![allow(dead_code)]

use std::collections::HashMap;
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

    Ok(parse_m3u_playlist(&content))
}

/// Parse M3U and return playlist with EPG URL
pub fn parse_m3u_playlist(content: &str) -> M3uPlaylist {
    let mut playlist = M3uPlaylist::default();
    
    // Check first line for EPG URL
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
    
    // Reuse buffers to avoid allocations
    let mut current_attrs = AttrBuffer::new();
    let mut current_name: Option<&str> = None;
    
    for line in content.lines() {
        let line = line.trim();
        
        // Handle both #EXTINF: and EXTINF: (some malformed M3Us miss the #)
        let info_part = line.strip_prefix("#EXTINF:")
            .or_else(|| line.strip_prefix("EXTINF:"));
        
        if let Some(info_part) = info_part {
            current_attrs.clear();
            
            // Check format: standard has attrs before last comma, alternate has attrs after first comma
            // Standard: "-1 tvg-id="x" group-title="y",Channel Name"
            // Alternate: "10.000000,TVG-ID="x" tvg-name="y",Channel Name"
            
            // Find first comma (after duration)
            if let Some(first_comma) = info_part.find(',') {
                let before_first_comma = &info_part[..first_comma];
                let after_first_comma = &info_part[first_comma + 1..];
                
                // Check if attributes are before or after the first comma
                if before_first_comma.contains('=') {
                    // Standard format: attrs before comma
                    extract_attrs_fast(info_part, &mut current_attrs);
                    // Name is after last comma
                    if let Some(last_comma) = info_part.rfind(',') {
                        current_name = Some(info_part[last_comma + 1..].trim());
                    }
                } else {
                    // Alternate format: duration,attrs,name
                    extract_attrs_fast(after_first_comma, &mut current_attrs);
                    // Name is after last comma
                    if let Some(last_comma) = after_first_comma.rfind(',') {
                        current_name = Some(after_first_comma[last_comma + 1..].trim());
                    } else {
                        // No second comma, entire after_first_comma is the name
                        current_name = Some(after_first_comma.trim());
                    }
                }
            } else {
                // No comma at all - just try to extract attrs
                extract_attrs_fast(info_part, &mut current_attrs);
            }
        } else if !line.is_empty() && !line.starts_with('#') && !line.starts_with("EXTM3U") {
            // This is a URL line (skip EXTM3U without #)
            if let Some(name) = current_name.take() {
                channels.push(M3uChannel {
                    name: name.to_string(),
                    url: line.to_string(),
                    group: current_attrs.get("group-title").map(|s| s.to_string()),
                    tvg_id: current_attrs.get("tvg-id").map(|s| s.to_string()),
                    tvg_logo: current_attrs.get("tvg-logo").map(|s| s.to_string()),
                    tvg_name: current_attrs.get("tvg-name").map(|s| s.to_string()),
                    tvg_chno: current_attrs.get("tvg-chno")
                        .and_then(|s| s.parse().ok()),
                    channel_id: current_attrs.get("channel-id").map(|s| s.to_string()),
                    channel_number: current_attrs.get("channel-number")
                        .and_then(|s| s.parse().ok()),
                    catchup: current_attrs.get("catchup").map(|s| s.to_string()),
                    catchup_days: current_attrs.get("catchup-days")
                        .and_then(|s| s.parse().ok()),
                });
            }
        }
    }
    
    channels
}

/// Lightweight attribute buffer - avoids HashMap overhead
struct AttrBuffer<'a> {
    attrs: [Option<(&'a str, &'a str)>; 8], // Most M3Us have <8 attrs per line
    len: usize,
}

impl<'a> AttrBuffer<'a> {
    fn new() -> Self {
        Self {
            attrs: [None; 8],
            len: 0,
        }
    }
    
    fn clear(&mut self) {
        self.len = 0;
    }
    
    fn push(&mut self, key: &'a str, value: &'a str) {
        if self.len < 8 {
            self.attrs[self.len] = Some((key, value));
            self.len += 1;
        }
    }
    
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
    // Try query parameters first
    if let Some(creds) = extract_from_query(url) {
        return Some(creds);
    }

    // Try path-based credentials
    if let Some(creds) = extract_from_path(url) {
        return Some(creds);
    }

    None
}

fn extract_from_query(url: &str) -> Option<M3uCredentials> {
    let query_start = url.find('?')?;
    let query = &url[query_start + 1..];
    
    let mut params: HashMap<&str, &str> = HashMap::new();
    for pair in query.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            params.insert(key, value);
        }
    }

    let username = params.get("username").or(params.get("user"))?;
    let password = params.get("password").or(params.get("pass"))?;

    // Extract server (up to the path)
    let server = if let Some(proto_end) = url.find("://") {
        let rest = &url[proto_end + 3..];
        if let Some(path_start) = rest.find('/') {
            url[..proto_end + 3 + path_start].to_string()
        } else {
            url.to_string()
        }
    } else {
        return None;
    };

    Some(M3uCredentials {
        server,
        username: username.to_string(),
        password: password.to_string(),
    })
}

fn extract_from_path(url: &str) -> Option<M3uCredentials> {
    // Pattern: http://server/live/username/password/channel.ts
    let proto_end = url.find("://")?;
    let rest = &url[proto_end + 3..];
    
    let path_start = rest.find('/')?;
    let server = url[..proto_end + 3 + path_start].to_string();
    
    let path = &rest[path_start + 1..];
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Pattern: live/username/password/...
    if segments.len() >= 3 {
        let prefixes = ["live", "movie", "series"];
        if prefixes.contains(&segments[0].to_lowercase().as_str()) {
            return Some(M3uCredentials {
                server,
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
                server,
                username: first.to_string(),
                password: second.to_string(),
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_query_credentials() {
        let url = "http://example.com/get.php?username=john&password=secret&type=m3u_plus";
        let creds = extract_credentials(url).unwrap();
        assert_eq!(creds.username, "john");
        assert_eq!(creds.password, "secret");
        assert_eq!(creds.server, "http://example.com");
    }

    #[test]
    fn test_extract_path_credentials() {
        let url = "http://example.com:8080/live/myuser/mypass/123.ts";
        let creds = extract_credentials(url).unwrap();
        assert_eq!(creds.username, "myuser");
        assert_eq!(creds.password, "mypass");
    }

    #[test]
    fn test_parse_m3u() {
        let content = r#"
#EXTM3U
#EXTINF:-1 tvg-id="cnn" group-title="News",CNN
http://example.com/live/user/pass/1.ts
#EXTINF:-1 tvg-id="bbc" group-title="News",BBC
http://example.com/live/user/pass/2.ts
"#;
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "CNN");
        assert_eq!(channels[0].group, Some("News".to_string()));
    }

    #[test]
    fn test_parse_m3u_with_epg_url() {
        let content = r#"#EXTM3U x-tvg-url="http://example.com/epg.xml"
#EXTINF:-1 tvg-id="ch1" tvg-name="Channel One" group-title="General" catchup="default" catchup-days="7",Channel 1
http://example.com/live/user/pass/1.ts
"#;
        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.epg_url, Some("http://example.com/epg.xml".to_string()));
        assert_eq!(playlist.channels.len(), 1);
        assert_eq!(playlist.channels[0].tvg_name, Some("Channel One".to_string()));
        assert_eq!(playlist.channels[0].catchup, Some("default".to_string()));
        assert_eq!(playlist.channels[0].catchup_days, Some(7));
    }

    #[test]
    fn test_parse_attrs_unquoted() {
        let content = r#"#EXTM3U
#EXTINF:-1 tvg-id=unquoted group-title="Quoted Group",Test Channel
http://example.com/stream.ts
"#;
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].tvg_id, Some("unquoted".to_string()));
        assert_eq!(channels[0].group, Some("Quoted Group".to_string()));
    }

    #[test]
    fn test_parse_malformed_stray_quotes() {
        // Real-world format with stray quote before tvg-name
        let content = r#"#EXTM3U
#EXTINF:0 tvg-logo="https://example.com/logo.png" "tvg-name="SRF1.ch" tvg-chno="1108" group-title="Deutsch", SRF 1 FHD
udp://@233.50.230.1:5000
"#;
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "SRF 1 FHD");
        assert_eq!(channels[0].tvg_logo, Some("https://example.com/logo.png".to_string()));
        assert_eq!(channels[0].tvg_name, Some("SRF1.ch".to_string()));
        assert_eq!(channels[0].tvg_chno, Some(1108));
        assert_eq!(channels[0].group, Some("Deutsch".to_string()));
        assert_eq!(channels[0].url, "udp://@233.50.230.1:5000");
    }

    #[test]
    fn test_parse_extinf_without_hash() {
        // Some malformed M3Us have EXTINF without # prefix
        let content = r#"#EXTM3U
#EXTINF:-1 tvg-id="" tvg-name="Channel 1" group-title="Group",Channel 1
http://example.com/1.mp4
EXTINF:-1 tvg-id="" tvg-name="Channel 2" group-title="Group",Channel 2
http://example.com/2.mp4
#EXTINF:-1 tvg-id="" tvg-name="Channel 3" group-title="Group",Channel 3
http://example.com/3.mp4
"#;
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 3);
        assert_eq!(channels[0].name, "Channel 1");
        assert_eq!(channels[1].name, "Channel 2");
        assert_eq!(channels[2].name, "Channel 3");
    }

    #[test]
    fn test_parse_channel_id_and_number() {
        // Channels DVR format with channel-id and channel-number
        let content = r#"#EXTM3U
#EXTINF:-1 channel-id="JPCAM" channel-number="750" tvg-logo="https://example.com/logo.png" tvg-name="JPCAM",JPCAM
https://example.com/stream.m3u8
"#;
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "JPCAM");
        assert_eq!(channels[0].channel_id, Some("JPCAM".to_string()));
        assert_eq!(channels[0].channel_number, Some(750));
        assert_eq!(channels[0].tvg_name, Some("JPCAM".to_string()));
        assert_eq!(channels[0].tvg_logo, Some("https://example.com/logo.png".to_string()));
    }

    #[test]
    fn test_parse_attrs_after_duration_comma() {
        // Alternate format: duration,attrs,name (attrs after first comma)
        let content = r#"#EXTM3U
#EXTINF:10.000000,TVG-ID="Channel1" tvg-name="Channel 1" tvg-logo="http://example.com/channel1.png" group-title="Entertainment",Channel 1
http://example.com/stream1.ts
#EXTINF:10.000000,TVG-ID="Channel2" tvg-name="Channel 2" tvg-logo="http://example.com/channel2.png" group-title="Entertainment",Channel 2
http://example.com/stream2.ts
"#;
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Channel 1");
        assert_eq!(channels[0].tvg_id, Some("Channel1".to_string()));
        assert_eq!(channels[0].tvg_name, Some("Channel 1".to_string()));
        assert_eq!(channels[0].group, Some("Entertainment".to_string()));
        assert_eq!(channels[1].name, "Channel 2");
        assert_eq!(channels[1].tvg_id, Some("Channel2".to_string()));
    }
}
