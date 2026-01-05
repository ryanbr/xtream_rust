//! Tests for XSPF playlist parsing

#[cfg(test)]
mod tests {
    use crate::xspf_parser::*;

    #[test]
    fn test_basic_xspf() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track><location>http://example.com/song_1.mp3</location></track>
    <track><location>http://example.com/song_2.mp3</location></track>
    <track><location>http://example.com/song_3.mp3</location></track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks.len(), 3);
        assert_eq!(playlist.tracks[0].location, "http://example.com/song_1.mp3");
        assert_eq!(playlist.tracks[1].location, "http://example.com/song_2.mp3");
        assert_eq!(playlist.tracks[2].location, "http://example.com/song_3.mp3");
    }

    #[test]
    fn test_xspf_with_playlist_metadata() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <title>80s Music</title>
  <creator>Jane Doe</creator>
  <annotation>My favorite 80s hits</annotation>
  <info>http://example.com/~jane</info>
  <image>http://example.com/playlist.jpg</image>
  <trackList>
    <track><location>http://example.com/song.mp3</location></track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.title.as_deref(), Some("80s Music"));
        assert_eq!(playlist.creator.as_deref(), Some("Jane Doe"));
        assert_eq!(playlist.annotation.as_deref(), Some("My favorite 80s hits"));
        assert_eq!(playlist.info.as_deref(), Some("http://example.com/~jane"));
        assert_eq!(playlist.image.as_deref(), Some("http://example.com/playlist.jpg"));
    }

    #[test]
    fn test_xspf_with_track_metadata() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track>
      <location>http://example.com/song_1.mp3</location>
      <creator>Led Zeppelin</creator>
      <album>Houses of the Holy</album>
      <title>No Quarter</title>
      <annotation>I love this song</annotation>
      <duration>271066</duration>
      <image>http://images.amazon.com/images/P/B000002J0B.01.MZZZZZZZ.jpg</image>
      <info>http://example.com</info>
      <trackNum>4</trackNum>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks.len(), 1);

        let track = &playlist.tracks[0];
        assert_eq!(track.location, "http://example.com/song_1.mp3");
        assert_eq!(track.creator.as_deref(), Some("Led Zeppelin"));
        assert_eq!(track.album.as_deref(), Some("Houses of the Holy"));
        assert_eq!(track.title.as_deref(), Some("No Quarter"));
        assert_eq!(track.annotation.as_deref(), Some("I love this song"));
        assert_eq!(track.duration, Some(271066));
        assert_eq!(track.image.as_deref(), Some("http://images.amazon.com/images/P/B000002J0B.01.MZZZZZZZ.jpg"));
        assert_eq!(track.info.as_deref(), Some("http://example.com"));
        assert_eq!(track.track_num, Some(4));
    }

    #[test]
    fn test_xspf_file_paths() {
        let content = r#"<?xml version="1.1" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track>
      <title>Windows Path</title>
      <location>file://C:\music\foo.mp3</location>
    </track>
    <track>
      <title>Linux Path</title>
      <location>file:///media/music/foo.mp3</location>
    </track>
    <track>
      <title>Relative Path</title>
      <location>music/foo.mp3</location>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks.len(), 3);
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("Windows Path"));
        assert!(playlist.tracks[0].location.starts_with("file://"));
        assert_eq!(playlist.tracks[1].title.as_deref(), Some("Linux Path"));
        assert_eq!(playlist.tracks[2].location, "music/foo.mp3");
    }

    #[test]
    fn test_xspf_xml_entities() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <title>Rock &amp; Roll</title>
  <trackList>
    <track>
      <location>http://example.com/song.mp3</location>
      <title>&lt;Track&gt; &quot;Test&quot; &amp; &apos;More&apos;</title>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.title.as_deref(), Some("Rock & Roll"));
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("<Track> \"Test\" & 'More'"));
    }

    #[test]
    fn test_xspf_empty_playlist() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks.len(), 0);
    }

    #[test]
    fn test_xspf_various_url_schemes() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track><location>http://example.com/stream.mp3</location></track>
    <track><location>https://secure.example.com/stream.mp3</location></track>
    <track><location>rtsp://camera.local/feed</location></track>
    <track><location>mms://stream.example.com/live</location></track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks.len(), 4);
        assert!(playlist.tracks[0].location.starts_with("http://"));
        assert!(playlist.tracks[1].location.starts_with("https://"));
        assert!(playlist.tracks[2].location.starts_with("rtsp://"));
        assert!(playlist.tracks[3].location.starts_with("mms://"));
    }

    #[test]
    fn test_xspf_unicode() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <title>日本語プレイリスト</title>
  <trackList>
    <track>
      <location>http://example.com/track.mp3</location>
      <title>Привет мир</title>
      <creator>アーティスト</creator>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.title.as_deref(), Some("日本語プレイリスト"));
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("Привет мир"));
        assert_eq!(playlist.tracks[0].creator.as_deref(), Some("アーティスト"));
    }

    #[test]
    fn test_xspf_track_without_location() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track>
      <title>No Location Track</title>
    </track>
    <track>
      <location>http://example.com/valid.mp3</location>
      <title>Valid Track</title>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        // Track without location should be skipped
        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("Valid Track"));
    }

    #[test]
    fn test_xspf_whitespace_handling() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track>
      <location>  http://example.com/song.mp3  </location>
      <title>  Spaced Title  </title>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks[0].location, "http://example.com/song.mp3");
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("Spaced Title"));
    }

    #[test]
    fn test_is_xspf() {
        assert!(is_xspf(r#"<playlist version="1" xmlns="http://xspf.org/ns/0/"><trackList></trackList></playlist>"#));
        assert!(is_xspf(r#"<playlist><trackList></trackList></playlist>"#));
        assert!(!is_xspf("#EXTM3U\n#EXTINF:-1,Test\nhttp://test.com"));
        assert!(!is_xspf("random content"));
    }

    #[test]
    fn test_invalid_xspf() {
        let result = parse_xspf("Not XML at all");
        assert!(result.is_err());

        let result = parse_xspf("<xml>No playlist here</xml>");
        assert!(result.is_err());
    }

    #[test]
    fn test_xspf_vlc_format() {
        // VLC-generated XSPF format
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist xmlns="http://xspf.org/ns/0/" xmlns:vlc="http://www.videolan.org/vlc/playlist/ns/0/" version="1">
	<title>Playlist</title>
	<trackList>
		<track>
			<location>file:///home/user/video.mp4</location>
			<title>My Video</title>
			<duration>120000</duration>
			<extension application="http://www.videolan.org/vlc/playlist/0">
				<vlc:id>0</vlc:id>
			</extension>
		</track>
	</trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.title.as_deref(), Some("Playlist"));
        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("My Video"));
        assert_eq!(playlist.tracks[0].duration, Some(120000));
    }

    #[test]
    fn test_xspf_multiple_locations() {
        // XSPF allows multiple locations per track for fallback
        // We take the first one
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track>
      <location>http://primary.example.com/song.mp3</location>
      <location>http://backup.example.com/song.mp3</location>
      <title>Multi-Location Track</title>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks.len(), 1);
        // Should get the first location
        assert_eq!(playlist.tracks[0].location, "http://primary.example.com/song.mp3");
    }

    #[test]
    fn test_xspf_long_playlist() {
        let mut content = String::from(r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <title>Large Playlist</title>
  <trackList>
"#);
        
        for i in 0..500 {
            content.push_str(&format!(
                "    <track><location>http://example.com/song_{}.mp3</location><title>Song {}</title></track>\n",
                i, i
            ));
        }
        
        content.push_str("  </trackList>\n</playlist>");

        let playlist = parse_xspf(&content).unwrap();
        assert_eq!(playlist.tracks.len(), 500);
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("Song 0"));
        assert_eq!(playlist.tracks[499].title.as_deref(), Some("Song 499"));
    }

    #[test]
    fn test_to_m3u_channels() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <title>Test Playlist</title>
  <trackList>
    <track>
      <location>http://example.com/song.mp3</location>
      <title>Test Song</title>
      <creator>Test Artist</creator>
      <album>Test Album</album>
      <image>http://example.com/cover.jpg</image>
      <trackNum>5</trackNum>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        let channels = to_m3u_channels(&playlist);
        
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Test Song");
        assert_eq!(channels[0].url, "http://example.com/song.mp3");
        assert_eq!(channels[0].group.as_deref(), Some("Test Artist - Test Album"));
        assert_eq!(channels[0].tvg_logo.as_deref(), Some("http://example.com/cover.jpg"));
        assert_eq!(channels[0].tvg_chno, Some(5));
    }

    // ========== Real-World Format Tests ==========

    #[test]
    fn test_init7_tv7_format() {
        // Init7 TV7 Swiss IPTV format with UDP multicast
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist xmlns="http://xspf.org/ns/0/" xmlns:vlc="http://www.videolan.org/vlc/playlist/ns/0/" version="1">
    <title>TV7 Playlist</title>
    <trackList>
        <track>
            <title>SRF 1</title>
            <location>udp://@233.50.230.80:5000</location>
            <image>https://api.tv.init7.net/media/logos/1102_SRF1.ch.png</image>
            <extension application="http://www.videolan.org/vlc/playlist/0">
                <vlc:id>1102</vlc:id>
                <vlc:option>network-caching=1000</vlc:option>
            </extension>
        </track>
        <track>
            <title>SRF zwei</title>
            <location>udp://@233.50.230.212:5000</location>
            <image>https://api.tv.init7.net/media/logos/1104_SRFzwei.ch.png</image>
            <extension application="http://www.videolan.org/vlc/playlist/0">
                <vlc:id>1104</vlc:id>
                <vlc:option>network-caching=1000</vlc:option>
            </extension>
        </track>
    </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.title.as_deref(), Some("TV7 Playlist"));
        assert_eq!(playlist.tracks.len(), 2);

        // Check first track
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("SRF 1"));
        assert_eq!(playlist.tracks[0].location, "udp://@233.50.230.80:5000");
        assert_eq!(playlist.tracks[0].image.as_deref(), Some("https://api.tv.init7.net/media/logos/1102_SRF1.ch.png"));

        // Check second track
        assert_eq!(playlist.tracks[1].title.as_deref(), Some("SRF zwei"));
        assert_eq!(playlist.tracks[1].location, "udp://@233.50.230.212:5000");
    }

    #[test]
    fn test_init7_srg_fhd_format() {
        // Init7 SRG FHD (Full HD Swiss TV)
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist xmlns="http://xspf.org/ns/0/" xmlns:vlc="http://www.videolan.org/vlc/playlist/ns/0/" version="1">
    <title>SRG FHD MC</title>
    <trackList>
        <track>
            <title>SRF 1 FHD</title>
            <location>udp://@233.50.230.1:5000</location>
            <image>https://api.tv.init7.net/media/logos/1102_SRF1.ch.png</image>
            <extension application="http://www.videolan.org/vlc/playlist/0">
                <vlc:id>1102</vlc:id>
                <vlc:option>network-caching=1000</vlc:option>
            </extension>
        </track>
        <track>
            <title>RTS 1 FHD</title>
            <location>udp://@233.50.230.84:5000</location>
            <image>https://api.tv.init7.net/media/logos/2103_RTS1.ch.png</image>
            <extension application="http://www.videolan.org/vlc/playlist/0">
                <vlc:id>2103</vlc:id>
                <vlc:option>network-caching=1000</vlc:option>
            </extension>
        </track>
        <track>
            <title>RSI LA 1 FHD</title>
            <location>udp://@233.50.230.118:5000</location>
            <image>https://api.tv.init7.net/media/logos/3104_RSILa1.ch.png</image>
            <extension application="http://www.videolan.org/vlc/playlist/0">
                <vlc:id>3104</vlc:id>
                <vlc:option>network-caching=1000</vlc:option>
            </extension>
        </track>
    </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.title.as_deref(), Some("SRG FHD MC"));
        assert_eq!(playlist.tracks.len(), 3);

        // Verify all tracks have UDP multicast URLs
        for track in &playlist.tracks {
            assert!(track.location.starts_with("udp://@233.50.230."));
            assert!(track.location.ends_with(":5000"));
            assert!(track.image.is_some());
            assert!(track.title.is_some());
        }
    }

    #[test]
    fn test_init7_to_m3u_channels() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist xmlns="http://xspf.org/ns/0/" xmlns:vlc="http://www.videolan.org/vlc/playlist/ns/0/" version="1">
    <title>TV7 Playlist</title>
    <trackList>
        <track>
            <title>SRF 1</title>
            <location>udp://@233.50.230.80:5000</location>
            <image>https://api.tv.init7.net/media/logos/1102_SRF1.ch.png</image>
            <extension application="http://www.videolan.org/vlc/playlist/0">
                <vlc:id>1102</vlc:id>
            </extension>
        </track>
    </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        let channels = to_m3u_channels(&playlist);
        
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "SRF 1");
        assert_eq!(channels[0].url, "udp://@233.50.230.80:5000");
        assert_eq!(channels[0].tvg_logo.as_deref(), Some("https://api.tv.init7.net/media/logos/1102_SRF1.ch.png"));
        // Group should be playlist title since no creator/album
        assert_eq!(channels[0].group.as_deref(), Some("TV7 Playlist"));
    }

    #[test]
    fn test_udp_multicast_url() {
        let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<playlist version="1" xmlns="http://xspf.org/ns/0/">
  <trackList>
    <track>
      <title>Multicast Stream</title>
      <location>udp://@239.0.0.1:1234</location>
    </track>
    <track>
      <title>RTP Stream</title>
      <location>rtp://@239.0.0.2:5004</location>
    </track>
  </trackList>
</playlist>"#;

        let playlist = parse_xspf(content).unwrap();
        assert_eq!(playlist.tracks.len(), 2);
        assert!(playlist.tracks[0].location.starts_with("udp://"));
        assert!(playlist.tracks[1].location.starts_with("rtp://"));
    }
}
