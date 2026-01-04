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
}

#[derive(Debug, Clone)]
pub struct M3uCredentials {
    pub server: String,
    pub username: String,
    pub password: String,
}

/// Download and parse M3U from URL (supports HTTP and HTTPS)
pub fn download_and_parse(url: &str, user_agent: &str) -> Result<Vec<M3uChannel>, String> {
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

    Ok(parse_m3u(&content))
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

            // Extract attributes like tvg-id="...", group-title="..."
            let mut remaining = info_part;
            while let Some(eq_pos) = remaining.find('=') {
                // Find the key (word before =)
                let key_start = remaining[..eq_pos]
                    .rfind(|c: char| c.is_whitespace())
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let key = &remaining[key_start..eq_pos];

                // Find the value (quoted string after =)
                let value_start = eq_pos + 1;
                if remaining[value_start..].starts_with('"') {
                    if let Some(end) = remaining[value_start + 1..].find('"') {
                        let value = &remaining[value_start + 1..value_start + 1 + end];
                        current_attrs.insert(key.to_string(), value.to_string());
                        remaining = &remaining[value_start + 2 + end..];
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

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
                });
            }
        }
    }

    channels
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
}
