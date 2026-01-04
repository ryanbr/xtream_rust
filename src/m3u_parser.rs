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
    let mut channels = Vec::new();
    let mut current_attrs: HashMap<String, String> = HashMap::new();
    let mut current_name: Option<String> = None;

    for line in content.lines() {
        let line = line.trim();

        if line.starts_with("#EXTINF:") {
            // Parse EXTINF line
            let info_part = &line[8..];
            current_attrs.clear();

            // Extract attributes using robust parser
            extract_attrs(info_part, &mut current_attrs);

            // Extract channel name (after the last comma)
            if let Some(comma_pos) = info_part.rfind(',') {
                current_name = Some(info_part[comma_pos + 1..].trim().to_string());
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            // This is a URL line
            if let Some(name) = current_name.take() {
                channels.push(M3uChannel {
                    name,
                    url: line.to_string(),
                    group: current_attrs.get("group-title").cloned(),
                    tvg_id: current_attrs.get("tvg-id").cloned(),
                    tvg_logo: current_attrs.get("tvg-logo").cloned(),
                    tvg_name: current_attrs.get("tvg-name").cloned(),
                    catchup: current_attrs.get("catchup").cloned(),
                    catchup_days: current_attrs.get("catchup-days")
                        .and_then(|s| s.parse().ok()),
                });
            }
        }
    }

    channels
}

/// Extract attributes from EXTINF line - handles quoted and unquoted values
fn extract_attrs(info: &str, attrs: &mut HashMap<String, String>) {
    let mut chars = info.chars().peekable();
    
    while chars.peek().is_some() {
        // Skip whitespace and commas
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == ',' || c == '-' && attrs.is_empty() {
                chars.next();
            } else {
                break;
            }
        }
        
        // Skip the duration number at the start (e.g., "-1")
        if attrs.is_empty() {
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() || c == '-' || c == '.' {
                    chars.next();
                } else {
                    break;
                }
            }
            // Skip whitespace after duration
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    chars.next();
                } else {
                    break;
                }
            }
        }
        
        // Collect key until '='
        let mut key = String::new();
        while let Some(&c) = chars.peek() {
            if c == '=' {
                chars.next(); // consume '='
                break;
            }
            if c == ',' {
                // No more attributes, rest is channel name
                return;
            }
            key.push(chars.next().unwrap());
        }
        
        let key = key.trim().to_lowercase();
        if key.is_empty() { 
            continue; 
        }
        
        // Get value - check if quoted
        match chars.peek() {
            Some(&'"') => {
                chars.next(); // consume opening quote
                let mut value = String::new();
                while let Some(c) = chars.next() {
                    if c == '"' { break; }
                    // Handle escaped quotes
                    if c == '\\' {
                        if let Some(&next) = chars.peek() {
                            if next == '"' {
                                value.push(chars.next().unwrap());
                                continue;
                            }
                        }
                    }
                    value.push(c);
                }
                attrs.insert(key, value);
            }
            Some(_) => {
                // Unquoted value - read until space or comma
                let mut value = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == ',' { break; }
                    value.push(chars.next().unwrap());
                }
                if !value.is_empty() {
                    attrs.insert(key, value);
                }
            }
            None => {}
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
}
