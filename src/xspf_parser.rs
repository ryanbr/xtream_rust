//! XSPF (XML Shareable Playlist Format) parser - Optimized
//!
//! Parses XSPF playlists (pronounced "spiff") - an XML-based playlist format
//! supported by VLC, Audacious, Clementine, and other media players.
//!
//! Reference: https://xspf.org/spec

use std::time::Duration;

/// Represents a track in an XSPF playlist
#[derive(Debug, Clone, Default)]
pub struct XspfTrack {
    /// URI of the resource to be rendered
    pub location: String,
    /// Human-readable name of the track
    pub title: Option<String>,
    /// Human-readable name of the artist/creator
    pub creator: Option<String>,
    /// Human-readable name of the album
    pub album: Option<String>,
    /// Human-readable comment/description
    pub annotation: Option<String>,
    /// Duration in milliseconds
    pub duration: Option<u64>,
    /// URI of an image for the track (album art)
    pub image: Option<String>,
    /// URI of a web page about the track
    pub info: Option<String>,
    /// Track number
    pub track_num: Option<u32>,
}

/// Represents an XSPF playlist
#[derive(Debug, Clone, Default)]
pub struct XspfPlaylist {
    /// Human-readable title of the playlist
    pub title: Option<String>,
    /// Human-readable name of the playlist creator
    pub creator: Option<String>,
    /// Human-readable comment/description
    pub annotation: Option<String>,
    /// URI of a web page about the playlist
    pub info: Option<String>,
    /// URI of an image for the playlist
    pub image: Option<String>,
    /// List of tracks
    pub tracks: Vec<XspfTrack>,
}

/// Download and parse XSPF from URL
pub fn download_and_parse(url: &str, user_agent: &str) -> Result<XspfPlaylist, String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
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

    parse_xspf(&content)
}

/// Parse XSPF content from string - optimized single-pass parser
pub fn parse_xspf(content: &str) -> Result<XspfPlaylist, String> {
    // Quick validation using byte search (faster than str methods)
    let bytes = content.as_bytes();
    if !contains_bytes(bytes, b"<playlist") || !contains_bytes(bytes, b"<trackList") {
        return Err("Not a valid XSPF playlist".to_string());
    }

    let mut playlist = XspfPlaylist::default();
    
    // Find trackList boundaries
    let tracklist_start = find_bytes(bytes, b"<trackList").unwrap_or(content.len());
    let tracklist_end = find_bytes(bytes, b"</trackList>").unwrap_or(content.len());
    
    // Parse playlist-level metadata (before trackList) - single pass
    let header = &content[..tracklist_start];
    playlist.title = extract_tag_fast(header, "title");
    playlist.creator = extract_tag_fast(header, "creator");
    playlist.annotation = extract_tag_fast(header, "annotation");
    playlist.info = extract_tag_fast(header, "info");
    playlist.image = extract_tag_fast(header, "image");

    // Pre-allocate tracks vector (estimate ~1 track per 200 bytes)
    let estimated_tracks = (tracklist_end - tracklist_start) / 200;
    playlist.tracks = Vec::with_capacity(estimated_tracks.max(10));

    // Parse tracks - optimized iteration
    if tracklist_start < tracklist_end {
        let tracklist = &content[tracklist_start..tracklist_end];
        let mut pos = 0;
        
        while pos < tracklist.len() {
            // Find next <track> tag
            if let Some(track_start) = find_bytes(&tracklist.as_bytes()[pos..], b"<track") {
                let abs_start = pos + track_start;
                
                // Find </track> end
                if let Some(track_end_rel) = find_bytes(&tracklist.as_bytes()[abs_start..], b"</track>") {
                    let abs_end = abs_start + track_end_rel + 8;
                    let track_content = &tracklist[abs_start..abs_end];
                    
                    if let Some(track) = parse_track_fast(track_content) {
                        playlist.tracks.push(track);
                    }
                    
                    pos = abs_end;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    Ok(playlist)
}

/// Parse a single track element - optimized
#[inline]
fn parse_track_fast(track_content: &str) -> Option<XspfTrack> {
    // Location is required - early exit if not found
    let location = extract_tag_fast(track_content, "location")?;
    
    Some(XspfTrack {
        location,
        title: extract_tag_fast(track_content, "title"),
        creator: extract_tag_fast(track_content, "creator"),
        album: extract_tag_fast(track_content, "album"),
        annotation: extract_tag_fast(track_content, "annotation"),
        duration: extract_tag_fast(track_content, "duration")
            .and_then(|d| d.parse().ok()),
        image: extract_tag_fast(track_content, "image"),
        info: extract_tag_fast(track_content, "info"),
        track_num: extract_tag_fast(track_content, "trackNum")
            .and_then(|n| n.parse().ok()),
    })
}

/// Fast tag extraction - avoids format! allocations
#[inline]
fn extract_tag_fast(content: &str, tag: &str) -> Option<String> {
    // Find opening tag
    let open_pattern = format!("<{}", tag);
    let start_pos = content.find(&open_pattern)?;
    
    // Find > after tag name
    let tag_end = start_pos + content[start_pos..].find('>')?;
    
    // Check for self-closing tag
    if content[start_pos..=tag_end].contains("/>") {
        return None;
    }
    
    let content_start = tag_end + 1;
    
    // Find closing tag
    let close_pattern = format!("</{}>", tag);
    let end_pos = content[content_start..].find(&close_pattern)? + content_start;
    
    let value = content[content_start..end_pos].trim();
    
    if value.is_empty() {
        None
    } else {
        Some(decode_xml_entities_fast(value))
    }
}

/// Fast XML entity decoding - only allocates if entities present
#[inline]
fn decode_xml_entities_fast(s: &str) -> String {
    // Fast path: no entities
    if !s.contains('&') {
        return s.to_string();
    }
    
    // Check which entities are present to minimize work
    let has_amp = s.contains("&amp;");
    let has_lt = s.contains("&lt;");
    let has_gt = s.contains("&gt;");
    let has_quot = s.contains("&quot;");
    let has_apos = s.contains("&apos;") || s.contains("&#39;");
    
    // Only do replacements that are needed
    let mut result = s.to_string();
    if has_amp { result = result.replace("&amp;", "&"); }
    if has_lt { result = result.replace("&lt;", "<"); }
    if has_gt { result = result.replace("&gt;", ">"); }
    if has_quot { result = result.replace("&quot;", "\""); }
    if has_apos { 
        result = result.replace("&apos;", "'").replace("&#39;", "'"); 
    }
    
    result
}

/// Fast byte search - avoids str overhead
#[inline]
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len())
        .position(|window| window == needle)
}

/// Fast contains check for bytes
#[inline]
fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    find_bytes(haystack, needle).is_some()
}

/// Check if content is an XSPF playlist - optimized
#[inline]
pub fn is_xspf(content: &str) -> bool {
    let bytes = content.as_bytes();
    contains_bytes(bytes, b"<playlist") && 
    (contains_bytes(bytes, b"xmlns=\"http://xspf.org/ns/0/\"") ||
     contains_bytes(bytes, b"xmlns='http://xspf.org/ns/0/'") ||
     contains_bytes(bytes, b"<trackList"))
}

/// Convert XSPF playlist to M3U channel format for compatibility
pub fn to_m3u_channels(playlist: &XspfPlaylist) -> Vec<super::m3u_parser::M3uChannel> {
    let mut channels = Vec::with_capacity(playlist.tracks.len());
    
    for track in &playlist.tracks {
        if track.location.is_empty() {
            continue;
        }

        let name = track.title.clone()
            .or_else(|| {
                // Extract filename from URL as fallback
                track.location.rsplit('/').next()
                    .map(|s| {
                        // Strip common extensions
                        let s = s.strip_suffix(".mp3").unwrap_or(s);
                        let s = s.strip_suffix(".ogg").unwrap_or(s);
                        let s = s.strip_suffix(".m4a").unwrap_or(s);
                        let s = s.strip_suffix(".flac").unwrap_or(s);
                        let s = s.strip_suffix(".ts").unwrap_or(s);
                        let s = s.strip_suffix(".m3u8").unwrap_or(s);
                        s.to_string()
                    })
            })
            .unwrap_or_else(|| "Unknown".to_string());

        // Build group from creator/album
        let group = match (&track.creator, &track.album) {
            (Some(creator), Some(album)) => Some(format!("{} - {}", creator, album)),
            (Some(creator), None) => Some(creator.clone()),
            (None, Some(album)) => Some(album.clone()),
            (None, None) => playlist.title.clone(),
        };

        channels.push(super::m3u_parser::M3uChannel {
            name,
            url: track.location.clone(),
            group,
            tvg_id: None,
            tvg_logo: track.image.clone(),
            tvg_name: track.title.clone(),
            tvg_chno: track.track_num,
            channel_id: None,
            channel_number: track.track_num,
            catchup: None,
            catchup_days: None,
        });
    }
    
    channels
}

#[cfg(test)]
#[path = "xspf_parser_tests.rs"]
mod tests;
