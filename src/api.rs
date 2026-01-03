//! Xtream Codes API client

#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Category {
    pub category_id: String,
    pub category_name: String,
    #[serde(default)]
    pub parent_id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stream {
    pub stream_id: i64,
    pub name: String,
    #[serde(default)]
    pub category_id: Option<String>,
    #[serde(default)]
    pub epg_channel_id: Option<String>,
    #[serde(default)]
    pub stream_icon: Option<String>,
    #[serde(default)]
    pub container_extension: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeriesInfo {
    pub series_id: i64,
    pub name: String,
    #[serde(default)]
    pub cover: Option<String>,
    #[serde(default)]
    pub plot: Option<String>,
    #[serde(default)]
    pub cast: Option<String>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub rating: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Episode {
    pub id: i64,
    pub title: String,
    pub episode_num: i32,
    pub season: i32,
    pub container_extension: String,
}

pub struct XtreamClient {
    server: String,
    username: String,
    password: String,
    user_agent: String,
}

impl XtreamClient {
    pub fn new(server: &str, username: &str, password: &str) -> Self {
        Self {
            server: server.to_string(),
            username: username.to_string(),
            password: password.to_string(),
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".to_string(),
        }
    }

    pub fn with_user_agent(mut self, user_agent: &str) -> Self {
        self.user_agent = user_agent.to_string();
        self
    }

    fn api_url(&self, action: &str) -> String {
        format!(
            "{}/player_api.php?username={}&password={}&action={}",
            self.server, self.username, self.password, action
        )
    }

    fn api_url_with_param(&self, action: &str, param_name: &str, param_value: &str) -> String {
        format!(
            "{}/player_api.php?username={}&password={}&action={}&{}={}",
            self.server, self.username, self.password, action, param_name, param_value
        )
    }

    fn make_request(&self, url: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        // Parse URL
        let url = url.trim();
        let (host, port, path) = parse_http_url(url)?;

        // Connect with timeout
        let addr = format!("{}:{}", host, port);
        let mut stream = TcpStream::connect(&addr)?;
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;

        // Send HTTP GET request with configurable user agent
        let request = format!(
            "GET {} HTTP/1.1\r\n\
             Host: {}\r\n\
             Connection: close\r\n\
             User-Agent: {}\r\n\
             Accept: application/json\r\n\
             \r\n",
            path, host, self.user_agent
        );
        stream.write_all(request.as_bytes())?;

        // Read response
        let mut response = Vec::new();
        stream.read_to_end(&mut response)?;
        
        let response_str = String::from_utf8_lossy(&response);

        // Skip HTTP headers
        if let Some(body_start) = response_str.find("\r\n\r\n") {
            let body = &response_str[body_start + 4..];
            
            // Handle chunked encoding
            if response_str.to_lowercase().contains("transfer-encoding: chunked") {
                return Ok(decode_chunked(body));
            }
            
            Ok(body.to_string())
        } else {
            Err("Invalid HTTP response".into())
        }
    }

    pub fn get_account_info(&self) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "{}/player_api.php?username={}&password={}",
            self.server, self.username, self.password
        );
        let response = self.make_request(&url)?;
        let json: Value = serde_json::from_str(&response)?;
        Ok(json)
    }

    pub fn get_live_categories(&self) -> Result<Vec<Category>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("get_live_categories");
        let response = self.make_request(&url)?;
        let categories: Vec<Category> = serde_json::from_str(&response)?;
        Ok(categories)
    }

    pub fn get_vod_categories(&self) -> Result<Vec<Category>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("get_vod_categories");
        let response = self.make_request(&url)?;
        let categories: Vec<Category> = serde_json::from_str(&response)?;
        Ok(categories)
    }

    pub fn get_series_categories(&self) -> Result<Vec<Category>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url("get_series_categories");
        let response = self.make_request(&url)?;
        let categories: Vec<Category> = serde_json::from_str(&response)?;
        Ok(categories)
    }

    pub fn get_live_streams(&self, category_id: &str) -> Result<Vec<Stream>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url_with_param("get_live_streams", "category_id", category_id);
        let response = self.make_request(&url)?;
        let streams: Vec<Stream> = serde_json::from_str(&response)?;
        Ok(streams)
    }

    pub fn get_vod_streams(&self, category_id: &str) -> Result<Vec<Stream>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url_with_param("get_vod_streams", "category_id", category_id);
        let response = self.make_request(&url)?;
        let streams: Vec<Stream> = serde_json::from_str(&response)?;
        Ok(streams)
    }

    pub fn get_series(&self, category_id: &str) -> Result<Vec<SeriesInfo>, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url_with_param("get_series", "category_id", category_id);
        let response = self.make_request(&url)?;
        let series: Vec<SeriesInfo> = serde_json::from_str(&response)?;
        Ok(series)
    }

    pub fn get_series_info(&self, series_id: i64) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url_with_param("get_series_info", "series_id", &series_id.to_string());
        let response = self.make_request(&url)?;
        let info: Value = serde_json::from_str(&response)?;
        Ok(info)
    }

    pub fn get_vod_info(&self, vod_id: i64) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url_with_param("get_vod_info", "vod_id", &vod_id.to_string());
        let response = self.make_request(&url)?;
        let info: Value = serde_json::from_str(&response)?;
        Ok(info)
    }

    pub fn get_epg(&self, stream_id: i64) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = self.api_url_with_param("get_short_epg", "stream_id", &stream_id.to_string());
        let response = self.make_request(&url)?;
        let epg: Value = serde_json::from_str(&response)?;
        Ok(epg)
    }

    pub fn get_xmltv(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let url = format!(
            "{}/xmltv.php?username={}&password={}",
            self.server, self.username, self.password
        );
        self.make_request(&url)
    }
}

fn parse_http_url(url: &str) -> Result<(String, u16, String), Box<dyn std::error::Error + Send + Sync>> {
    let url = url.strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or("Invalid URL scheme")?;
    
    let (host_port, path) = if let Some(slash_pos) = url.find('/') {
        (&url[..slash_pos], &url[slash_pos..])
    } else {
        (url, "/")
    };

    let (host, port) = if let Some(colon_pos) = host_port.rfind(':') {
        let port: u16 = host_port[colon_pos + 1..].parse()?;
        (&host_port[..colon_pos], port)
    } else {
        (host_port, 80)
    };

    Ok((host.to_string(), port, path.to_string()))
}

fn decode_chunked(body: &str) -> String {
    let mut result = String::new();
    let mut remaining = body;

    loop {
        let size_end = match remaining.find("\r\n") {
            Some(pos) => pos,
            None => break,
        };

        let size_str = &remaining[..size_end];
        let chunk_size = match usize::from_str_radix(size_str.trim(), 16) {
            Ok(s) => s,
            Err(_) => break,
        };

        if chunk_size == 0 {
            break;
        }

        let data_start = size_end + 2;
        let data_end = data_start + chunk_size;

        if data_end <= remaining.len() {
            result.push_str(&remaining[data_start..data_end]);
            remaining = &remaining[data_end..];
            
            if remaining.starts_with("\r\n") {
                remaining = &remaining[2..];
            }
        } else {
            break;
        }
    }

    result
}
