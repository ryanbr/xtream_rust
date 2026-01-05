//! Tests for M3U and M3U8 playlist parsing

#[cfg(test)]
mod tests {
    use crate::m3u_parser::*;

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

    // ========== M3U8 HLS Media Playlist Tests ==========

    #[test]
    fn test_hls_media_playlist() {
        let content = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:10
#EXT-X-MEDIA-SEQUENCE:0
#EXTINF:9.6,
segment0.ts
#EXTINF:10.0,
segment1.ts
#EXTINF:9.8,
segment2.ts
#EXT-X-ENDLIST"#;

        let playlist = parse_m3u_playlist(content);
        // Media playlist should return single channel with empty URL (caller fills it)
        assert_eq!(playlist.channels.len(), 1);
        assert_eq!(playlist.channels[0].name, "HLS Stream");
        assert_eq!(playlist.channels[0].url, "");
        assert_eq!(playlist.channels[0].group.as_deref(), Some("HLS"));
    }

    // ========== M3U8 HLS Master Playlist Tests ==========

    #[test]
    fn test_hls_master_basic() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1280000,RESOLUTION=640x360
low_quality.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2560000,RESOLUTION=854x480
medium_quality.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5120000,RESOLUTION=1280x720
high_quality.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=8192000,RESOLUTION=1920x1080
ultra_quality.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 4);

        assert!(playlist.channels[0].name.contains("640x360"));
        assert!(playlist.channels[0].name.contains("1.3 Mbps"));
        assert_eq!(playlist.channels[0].url, "low_quality.m3u8");

        assert!(playlist.channels[3].name.contains("1920x1080"));
        assert!(playlist.channels[3].name.contains("8.2 Mbps"));
        assert_eq!(playlist.channels[3].url, "ultra_quality.m3u8");
    }

    #[test]
    fn test_hls_master_bandwidth_only() {
        let content = r#"#EXTM3U
#EXT-X-VERSION:3
#JUST A COMMENT
#CUSTOM-PLAYLIST-TAG:42
#EXT-X-STREAM-INF:PROGRAM-ID=1,BANDWIDTH=300000
chunklist-b300000.m3u8
#EXT-X-STREAM-INF:PROGRAM-ID=1,BANDWIDTH=600000
chunklist-b600000.m3u8
#EXT-X-STREAM-INF:PROGRAM-ID=1,BANDWIDTH=1500000
chunklist-b1500000.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 3);

        assert_eq!(playlist.channels[0].name, "300 Kbps");
        assert_eq!(playlist.channels[1].name, "600 Kbps");
        assert_eq!(playlist.channels[2].name, "1.5 Mbps");
    }

    #[test]
    fn test_hls_master_with_codecs() {
        let content = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-STREAM-INF:PROGRAM-ID=1,BANDWIDTH=300000,CODECS="avc1.42c015,mp4a.40.2"
chunklist-b300000.m3u8
#EXT-X-STREAM-INF:PROGRAM-ID=1,BANDWIDTH=600000,CODECS="avc1.42c015,mp4a.40.2"
chunklist-b600000.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 2);
        // Codecs should be ignored, just bandwidth in name
        assert_eq!(playlist.channels[0].name, "300 Kbps");
    }

    #[test]
    fn test_hls_master_multi_angle() {
        let content = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID="low",NAME="Main",DEFAULT=YES,URI="low/main/audio-video.m3u8"
#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID="low",NAME="Centerfield",DEFAULT=NO,URI="low/centerfield/audio-video.m3u8"
#EXT-X-MEDIA:TYPE=VIDEO,GROUP-ID="low",NAME="Dugout",DEFAULT=NO,URI="low/dugout/audio-video.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=1280000,VIDEO="low"
low/main/audio-video.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=65000,CODECS="mp4a.40.5"
main/audio-only.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        
        // Should have: 2 from EXT-X-STREAM-INF + 2 from EXT-X-MEDIA (non-default)
        assert_eq!(playlist.channels.len(), 4);

        // Check that alternate angles are included
        let names: Vec<&str> = playlist.channels.iter().map(|c| c.name.as_str()).collect();
        assert!(names.iter().any(|n| n.contains("Centerfield")));
        assert!(names.iter().any(|n| n.contains("Dugout")));
    }

    #[test]
    fn test_hls_master_hdr_dolby_sdr() {
        let content = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-STREAM-INF:BANDWIDTH=3971374,VIDEO-RANGE=SDR,CODECS="hvc1.2.4.L123.B0",RESOLUTION=1280x720
sdr_720/prog_index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5327059,VIDEO-RANGE=PQ,CODECS="dvh1.05.01",RESOLUTION=1280x720
dolby_720/prog_index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5280654,VIDEO-RANGE=PQ,CODECS="hvc1.2.4.L123.B0",RESOLUTION=1280x720
hdr10_720/prog_index.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 3);

        // Check SDR labeling
        assert!(playlist.channels[0].name.contains("SDR"));
        assert!(playlist.channels[0].name.contains("1280x720"));

        // Check Dolby Vision detection (PQ + dvh codec)
        assert!(playlist.channels[1].name.contains("Dolby Vision"));

        // Check HDR10 detection (PQ + non-dvh codec)
        assert!(playlist.channels[2].name.contains("HDR10"));
    }

    #[test]
    fn test_hls_master_ignore_iframe_streams() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1280000
low/audio-video.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=86000,URI="low/iframe.m3u8",PROGRAM-ID=1
#EXT-X-STREAM-INF:BANDWIDTH=2560000
mid/audio-video.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=150000,URI="mid/iframe.m3u8",PROGRAM-ID=1
#EXT-X-STREAM-INF:BANDWIDTH=7680000
hi/audio-video.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH=550000,URI="hi/iframe.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=65000,CODECS="mp4a.40.5"
audio-only.m3u8
#EXT-X-I-FRAME-STREAM-INF:BANDWIDTH="INVALIDBW",URI="hi/iframe.m3u8""#;

        let playlist = parse_m3u_playlist(content);
        
        // Should only have 4 channels (I-FRAME streams ignored)
        assert_eq!(playlist.channels.len(), 4);

        let urls: Vec<&str> = playlist.channels.iter().map(|c| c.url.as_str()).collect();
        assert!(urls.contains(&"low/audio-video.m3u8"));
        assert!(urls.contains(&"mid/audio-video.m3u8"));
        assert!(urls.contains(&"hi/audio-video.m3u8"));
        assert!(urls.contains(&"audio-only.m3u8"));
        
        // Verify no iframe URLs
        assert!(!urls.iter().any(|u| u.contains("iframe")));
    }

    #[test]
    fn test_hls_audio_tracks() {
        let content = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",DEFAULT=YES,URI="audio/en.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="Spanish",DEFAULT=NO,URI="audio/es.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="French",DEFAULT=NO,URI="audio/fr.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2560000,AUDIO="audio"
video.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        
        // Should have: 1 video + 2 non-default audio tracks
        assert_eq!(playlist.channels.len(), 3);

        // Check audio tracks are labeled correctly
        let groups: Vec<Option<&str>> = playlist.channels.iter().map(|c| c.group.as_deref()).collect();
        assert!(groups.iter().any(|g| *g == Some("HLS Audio")));
    }

    #[test]
    fn test_hls_subtitles() {
        let content = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="English",DEFAULT=YES,URI="subs/en.m3u8"
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="subs",NAME="Spanish",DEFAULT=NO,URI="subs/es.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=2560000,SUBTITLES="subs"
video.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        
        // Should have: 1 video + 1 non-default subtitle track
        assert_eq!(playlist.channels.len(), 2);

        // Check subtitle track is labeled correctly
        let groups: Vec<Option<&str>> = playlist.channels.iter().map(|c| c.group.as_deref()).collect();
        assert!(groups.iter().any(|g| *g == Some("HLS Subtitles")));
    }

    // ========== Bandwidth Formatting Tests ==========

    #[test]
    fn test_bandwidth_formatting() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=500
stream1.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5000
stream2.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=500000
stream3.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=5000000
stream4.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=50000000
stream5.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 5);

        assert_eq!(playlist.channels[0].name, "500 bps");
        assert_eq!(playlist.channels[1].name, "5 Kbps");
        assert_eq!(playlist.channels[2].name, "500 Kbps");
        assert_eq!(playlist.channels[3].name, "5.0 Mbps");
        assert_eq!(playlist.channels[4].name, "50.0 Mbps");
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_unicode_in_name() {
        let content = r#"#EXTM3U
#EXTINF:-1,日本語チャンネル
http://server.com/jp.ts
#EXTINF:-1,Канал Россия
http://server.com/ru.ts
#EXTINF:-1,القناة العربية
http://server.com/ar.ts"#;

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 3);
        assert!(channels[0].name.contains("日本語"));
        assert!(channels[1].name.contains("Россия"));
        assert!(channels[2].name.contains("العربية"));
    }

    #[test]
    fn test_empty_playlist() {
        let content = "";
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 0);
    }

    #[test]
    fn test_header_only() {
        let content = "#EXTM3U";
        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 0);
    }

    #[test]
    fn test_extinf_without_url() {
        let content = r#"#EXTM3U
#EXTINF:-1,Channel 1
#EXTINF:-1,Channel 2
http://server.com/ch2.ts"#;

        let channels = parse_m3u(content);
        // Only Channel 2 should be parsed (Channel 1 has no URL)
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Channel 2");
    }

    #[test]
    fn test_url_without_extinf() {
        let content = r#"#EXTM3U
http://server.com/ch1.ts
#EXTINF:-1,Channel 2
http://server.com/ch2.ts"#;

        let channels = parse_m3u(content);
        // URL without EXTINF should be ignored
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].name, "Channel 2");
    }

    #[test]
    fn test_whitespace_handling() {
        let content = "#EXTM3U

#EXTINF:-1,  Channel 1  
  http://server.com/ch1.ts  

#EXTINF:-1,Channel 2
http://server.com/ch2.ts";

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 2);
        assert_eq!(channels[0].name, "Channel 1");
        assert_eq!(channels[0].url, "http://server.com/ch1.ts");
    }

    #[test]
    fn test_special_characters_in_name() {
        let content = r#"#EXTM3U
#EXTINF:-1,Channel "Test" & <Special> 'Chars'
http://server.com/ch1.ts"#;

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 1);
        assert!(channels[0].name.contains("Test"));
        assert!(channels[0].name.contains("&"));
    }

    #[test]
    fn test_m3u_with_epg_url() {
        let content = r#"#EXTM3U x-tvg-url="http://epg.com/guide.xml"
#EXTINF:-1,Channel 1
http://server.com/ch1.ts"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.epg_url.as_deref(), Some("http://epg.com/guide.xml"));
        assert_eq!(playlist.channels.len(), 1);
    }

    #[test]
    fn test_m3u_with_url_tvg() {
        let content = r#"#EXTM3U url-tvg="http://epg.com/alt-guide.xml"
#EXTINF:-1,Channel 1
http://server.com/ch1.ts"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.epg_url.as_deref(), Some("http://epg.com/alt-guide.xml"));
    }

    #[test]
    fn test_m3u_catchup_attrs() {
        let content = r#"#EXTM3U
#EXTINF:-1 tvg-id="CH1" catchup="default" catchup-days="7",Channel 1
http://server.com/ch1.ts"#;

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].catchup.as_deref(), Some("default"));
        assert_eq!(channels[0].catchup_days, Some(7));
    }

    #[test]
    fn test_m3u_all_attributes() {
        let content = r#"#EXTM3U
#EXTINF:-1 tvg-id="CH1" tvg-name="Channel One" tvg-logo="http://logo.png" tvg-chno="42" group-title="News" channel-id="ch1" channel-number="42" catchup="shift" catchup-days="3",Channel 1
http://server.com/ch1.ts"#;

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].tvg_id.as_deref(), Some("CH1"));
        assert_eq!(channels[0].tvg_name.as_deref(), Some("Channel One"));
        assert_eq!(channels[0].tvg_logo.as_deref(), Some("http://logo.png"));
        assert_eq!(channels[0].tvg_chno, Some(42));
        assert_eq!(channels[0].group.as_deref(), Some("News"));
        assert_eq!(channels[0].channel_id.as_deref(), Some("ch1"));
        assert_eq!(channels[0].channel_number, Some(42));
        assert_eq!(channels[0].catchup.as_deref(), Some("shift"));
        assert_eq!(channels[0].catchup_days, Some(3));
    }

    #[test]
    fn test_m3u_duration_values() {
        let content = r#"#EXTM3U
#EXTINF:-1,Live Channel
http://server.com/live.ts
#EXTINF:0,Zero Duration
http://server.com/zero.ts
#EXTINF:3600,One Hour
http://server.com/hour.ts"#;

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 3);
        // Duration not stored, just verify parsing works
        assert_eq!(channels[0].name, "Live Channel");
        assert_eq!(channels[1].name, "Zero Duration");
        assert_eq!(channels[2].name, "One Hour");
    }

    #[test]
    fn test_m3u_various_url_schemes() {
        let content = r#"#EXTM3U
#EXTINF:-1,HTTP Stream
http://server.com/stream.ts
#EXTINF:-1,HTTPS Stream
https://secure.com/stream.ts
#EXTINF:-1,RTMP Stream
rtmp://rtmp.server.com/live/stream
#EXTINF:-1,RTSP Stream
rtsp://camera.local/feed
#EXTINF:-1,UDP Multicast
udp://@239.0.0.1:1234
#EXTINF:-1,RTP Stream
rtp://239.0.0.1:5004"#;

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 6);
        assert!(channels[0].url.starts_with("http://"));
        assert!(channels[1].url.starts_with("https://"));
        assert!(channels[2].url.starts_with("rtmp://"));
        assert!(channels[3].url.starts_with("rtsp://"));
        assert!(channels[4].url.starts_with("udp://"));
        assert!(channels[5].url.starts_with("rtp://"));
    }

    #[test]
    fn test_m3u_pipe_in_url() {
        // Some providers use pipe characters in URLs
        let content = r#"#EXTM3U
#EXTINF:-1,Piped URL
http://server.com/stream|User-Agent=VLC
#EXTINF:-1,Normal URL
http://server.com/normal.ts"#;

        let channels = parse_m3u(content);
        assert_eq!(channels.len(), 2);
        assert!(channels[0].url.contains("|"));
    }

    #[test]
    fn test_m3u_long_playlist() {
        // Test performance with many channels
        let mut content = String::from("#EXTM3U\n");
        for i in 0..1000 {
            content.push_str(&format!(
                "#EXTINF:-1 tvg-id=\"CH{}\" group-title=\"Group {}\",Channel {}\n",
                i, i % 10, i
            ));
            content.push_str(&format!("http://server.com/ch{}.ts\n", i));
        }

        let channels = parse_m3u(&content);
        assert_eq!(channels.len(), 1000);
        assert_eq!(channels[999].name, "Channel 999");
    }

    // ========== HLS Edge Cases ==========

    #[test]
    fn test_hls_empty_master_playlist() {
        let content = r#"#EXTM3U
#EXT-X-VERSION:3"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 0);
    }

    #[test]
    fn test_hls_stream_inf_without_url() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1280000
#EXT-X-STREAM-INF:BANDWIDTH=2560000
actual_stream.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        // First STREAM-INF has no URL (next line is another tag)
        // Only second one should be parsed
        assert_eq!(playlist.channels.len(), 1);
        assert_eq!(playlist.channels[0].url, "actual_stream.m3u8");
    }

    #[test]
    fn test_hls_mixed_master_and_media_tags() {
        // Unusual but valid: master playlist tags with targetduration
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1280000
stream.m3u8
#EXT-X-TARGETDURATION:10"#;

        let playlist = parse_m3u_playlist(content);
        // STREAM-INF takes precedence, not treated as media playlist
        assert_eq!(playlist.channels.len(), 1);
        assert_eq!(playlist.channels[0].url, "stream.m3u8");
    }

    #[test]
    fn test_hls_absolute_urls() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=1280000
http://cdn.example.com/stream/720p.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2560000
https://cdn.example.com/stream/1080p.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 2);
        assert!(playlist.channels[0].url.starts_with("http://"));
        assert!(playlist.channels[1].url.starts_with("https://"));
    }

    #[test]
    fn test_hls_frame_rate() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,FRAME-RATE=29.97
stream_30fps.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=6000000,RESOLUTION=1920x1080,FRAME-RATE=59.94
stream_60fps.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 2);
        // Frame rate not included in name currently, just verify parsing works
        assert!(playlist.channels[0].name.contains("1920x1080"));
    }

    #[test]
    fn test_hls_closed_captions_none() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,CLOSED-CAPTIONS=NONE
stream.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 1);
    }

    #[test]
    fn test_hls_video_range_hlg() {
        let content = r#"#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=5000000,VIDEO-RANGE=HLG,RESOLUTION=1920x1080
hlg_stream.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        assert_eq!(playlist.channels.len(), 1);
        // HLG is treated as HDR10 currently
        assert!(playlist.channels[0].name.contains("HDR10"));
    }

    #[test]
    fn test_hls_media_without_uri() {
        // EXT-X-MEDIA without URI (embedded in video stream)
        let content = r#"#EXTM3U
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio",NAME="English",DEFAULT=YES,AUTOSELECT=YES
#EXT-X-STREAM-INF:BANDWIDTH=2560000,AUDIO="audio"
video.m3u8"#;

        let playlist = parse_m3u_playlist(content);
        // Only video stream, no separate audio channel (no URI)
        assert_eq!(playlist.channels.len(), 1);
    }

    #[test]
    fn test_hls_complex_real_world() {
        // Based on Apple's HLS examples
        let content = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-INDEPENDENT-SEGMENTS

#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud1",LANGUAGE="en",NAME="English",AUTOSELECT=YES,DEFAULT=YES,CHANNELS="2",URI="a1/prog_index.m3u8"
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="aud1",LANGUAGE="es",NAME="Spanish",AUTOSELECT=YES,DEFAULT=NO,CHANNELS="2",URI="a2/prog_index.m3u8"

#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID="sub1",LANGUAGE="en",NAME="English",AUTOSELECT=YES,DEFAULT=YES,FORCED=NO,URI="s1/en/prog_index.m3u8"

#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=2168183,BANDWIDTH=2400000,VIDEO-RANGE=SDR,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=960x540,FRAME-RATE=60.000,CLOSED-CAPTIONS="cc1",AUDIO="aud1",SUBTITLES="sub1"
v5/prog_index.m3u8
#EXT-X-STREAM-INF:AVERAGE-BANDWIDTH=7968779,BANDWIDTH=8500000,VIDEO-RANGE=SDR,CODECS="avc1.640028,mp4a.40.2",RESOLUTION=1920x1080,FRAME-RATE=60.000,CLOSED-CAPTIONS="cc1",AUDIO="aud1",SUBTITLES="sub1"
v9/prog_index.m3u8

#EXT-X-I-FRAME-STREAM-INF:AVERAGE-BANDWIDTH=183689,BANDWIDTH=187492,CODECS="avc1.640028",RESOLUTION=960x540,URI="v5/iframe_index.m3u8"
#EXT-X-I-FRAME-STREAM-INF:AVERAGE-BANDWIDTH=752903,BANDWIDTH=763864,CODECS="avc1.640028",RESOLUTION=1920x1080,URI="v9/iframe_index.m3u8""#;

        let playlist = parse_m3u_playlist(content);
        
        // Should have:
        // - 2 video streams (STREAM-INF)
        // - 1 non-default audio (Spanish)
        // - 0 subtitles (default is skipped)
        // - 0 i-frame streams (ignored)
        assert!(playlist.channels.len() >= 2);
        
        // Verify video streams present
        let urls: Vec<&str> = playlist.channels.iter().map(|c| c.url.as_str()).collect();
        assert!(urls.contains(&"v5/prog_index.m3u8"));
        assert!(urls.contains(&"v9/prog_index.m3u8"));
        
        // Verify no i-frame streams
        assert!(!urls.iter().any(|u| u.contains("iframe")));
    }
}
