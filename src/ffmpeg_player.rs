// Internal video player using ffmpeg-next
// Requires FFmpeg libraries: libavcodec, libavformat, libavutil, libswscale
//
// To install FFmpeg development libraries:
// - Ubuntu/Debian: sudo apt install libavcodec-dev libavformat-dev libavutil-dev libswscale-dev libavdevice-dev
// - Fedora: sudo dnf install ffmpeg-devel
// - macOS: brew install ffmpeg
// - Windows: Download from https://ffmpeg.org and set FFMPEG_DIR environment variable

#[cfg(feature = "internal-player")]
mod player_impl {
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    extern crate ffmpeg_next as ffmpeg;
    use ffmpeg::format::Pixel;
    use ffmpeg::media::Type;
    use ffmpeg::software::scaling::{context::Context as ScalingContext, flag::Flags};
    use ffmpeg::util::frame::video::Video as VideoFrame;

    /// Player state
    #[derive(Debug, Clone, PartialEq)]
    pub enum PlayerState {
        Stopped,
        Loading,
        Playing,
        Paused,
        Error(String),
    }

    /// Decoded video frame for rendering
    pub struct DecodedFrame {
        pub width: u32,
        pub height: u32,
        pub data: Vec<u8>, // RGB24 data
        pub pts: i64,
    }

    /// Commands to send to player thread
    enum PlayerCommand {
        Stop,
        Pause,
        Resume,
    }

    /// Messages from player thread
    pub enum PlayerMessage {
        StateChanged(PlayerState),
        Frame(DecodedFrame),
        Error(String),
        Finished,
    }

    /// Internal video player
    pub struct InternalPlayer {
        state: Arc<Mutex<PlayerState>>,
        command_sender: Option<Sender<PlayerCommand>>,
        message_receiver: Option<Receiver<PlayerMessage>>,
        current_frame: Arc<Mutex<Option<DecodedFrame>>>,
        url: String,
        channel_name: String,
        volume: f32,
        muted: bool,
    }

    impl InternalPlayer {
        pub fn new() -> Self {
            // Initialize FFmpeg
            ffmpeg::init().ok();
            
            Self {
                state: Arc::new(Mutex::new(PlayerState::Stopped)),
                command_sender: None,
                message_receiver: None,
                current_frame: Arc::new(Mutex::new(None)),
                url: String::new(),
                channel_name: String::new(),
                volume: 1.0,
                muted: false,
            }
        }

        /// Get current player state
        pub fn state(&self) -> PlayerState {
            self.state.lock().unwrap().clone()
        }

        /// Get the latest decoded frame
        pub fn take_frame(&self) -> Option<DecodedFrame> {
            self.current_frame.lock().unwrap().take()
        }

        /// Check for messages from player thread
        pub fn poll_messages(&mut self) -> Vec<PlayerMessage> {
            let mut messages = Vec::new();
            if let Some(ref receiver) = self.message_receiver {
                loop {
                    match receiver.try_recv() {
                        Ok(msg) => messages.push(msg),
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            self.message_receiver = None;
                            break;
                        }
                    }
                }
            }
            messages
        }

        /// Play a stream URL
        pub fn play(&mut self, name: &str, url: &str, _buffer_secs: u32, user_agent: &str) {
            self.stop();
            self.url = url.to_string();
            self.channel_name = name.to_string();
            
            *self.state.lock().unwrap() = PlayerState::Loading;
            
            let (cmd_tx, cmd_rx) = channel();
            let (msg_tx, msg_rx) = channel();
            
            self.command_sender = Some(cmd_tx);
            self.message_receiver = Some(msg_rx);
            
            let url = url.to_string();
            let user_agent = user_agent.to_string();
            let state = Arc::clone(&self.state);
            let current_frame = Arc::clone(&self.current_frame);
            
            thread::spawn(move || {
                Self::decode_thread(url, user_agent, state, current_frame, cmd_rx, msg_tx);
            });
        }

        fn decode_thread(
            url: String,
            user_agent: String,
            state: Arc<Mutex<PlayerState>>,
            current_frame: Arc<Mutex<Option<DecodedFrame>>>,
            cmd_rx: Receiver<PlayerCommand>,
            msg_tx: Sender<PlayerMessage>,
        ) {
            // Set options for network streams
            let mut options = ffmpeg::Dictionary::new();
            options.set("user_agent", &user_agent);
            options.set("reconnect", "1");
            options.set("reconnect_streamed", "1");
            options.set("reconnect_delay_max", "5");
            options.set("timeout", "5000000"); // 5 second timeout
            
            // Open input
            let mut ictx = match ffmpeg::format::input_with_dictionary(&url, options) {
                Ok(ctx) => ctx,
                Err(e) => {
                    *state.lock().unwrap() = PlayerState::Error(e.to_string());
                    let _ = msg_tx.send(PlayerMessage::Error(format!("Failed to open stream: {}", e)));
                    return;
                }
            };
            
            // Find video stream
            let video_stream_index = match ictx.streams().best(Type::Video) {
                Some(stream) => stream.index(),
                None => {
                    *state.lock().unwrap() = PlayerState::Error("No video stream found".to_string());
                    let _ = msg_tx.send(PlayerMessage::Error("No video stream found".to_string()));
                    return;
                }
            };
            
            let video_stream = ictx.stream(video_stream_index).unwrap();
            let context_decoder = ffmpeg::codec::context::Context::from_parameters(video_stream.parameters()).unwrap();
            
            let mut decoder = match context_decoder.decoder().video() {
                Ok(d) => d,
                Err(e) => {
                    *state.lock().unwrap() = PlayerState::Error(e.to_string());
                    let _ = msg_tx.send(PlayerMessage::Error(format!("Failed to create decoder: {}", e)));
                    return;
                }
            };
            
            // Get video dimensions
            let width = decoder.width();
            let height = decoder.height();
            
            // Scale to reasonable size if too large
            let (target_width, target_height) = if width > 1280 || height > 720 {
                let scale = f64::min(1280.0 / width as f64, 720.0 / height as f64);
                ((width as f64 * scale) as u32, (height as f64 * scale) as u32)
            } else {
                (width, height)
            };
            
            // Create scaler to convert to RGB24
            let mut scaler = match ScalingContext::get(
                decoder.format(),
                width,
                height,
                Pixel::RGB24,
                target_width,
                target_height,
                Flags::BILINEAR,
            ) {
                Ok(s) => s,
                Err(e) => {
                    *state.lock().unwrap() = PlayerState::Error(e.to_string());
                    let _ = msg_tx.send(PlayerMessage::Error(format!("Failed to create scaler: {}", e)));
                    return;
                }
            };
            
            *state.lock().unwrap() = PlayerState::Playing;
            let _ = msg_tx.send(PlayerMessage::StateChanged(PlayerState::Playing));
            
            let mut paused = false;
            let frame_duration = Duration::from_secs_f64(1.0 / 30.0); // Target 30fps display
            let mut last_frame_time = Instant::now();
            
            // Packet processing loop
            for (stream, packet) in ictx.packets() {
                // Check for commands
                match cmd_rx.try_recv() {
                    Ok(PlayerCommand::Stop) => break,
                    Ok(PlayerCommand::Pause) => {
                        paused = true;
                        *state.lock().unwrap() = PlayerState::Paused;
                        let _ = msg_tx.send(PlayerMessage::StateChanged(PlayerState::Paused));
                    }
                    Ok(PlayerCommand::Resume) => {
                        paused = false;
                        *state.lock().unwrap() = PlayerState::Playing;
                        let _ = msg_tx.send(PlayerMessage::StateChanged(PlayerState::Playing));
                    }
                    Err(_) => {}
                }
                
                // Skip if paused
                if paused {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                
                // Only process video packets
                if stream.index() != video_stream_index {
                    continue;
                }
                
                // Decode packet
                if decoder.send_packet(&packet).is_err() {
                    continue;
                }
                
                let mut decoded = VideoFrame::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    // Scale to RGB24
                    let mut rgb_frame = VideoFrame::empty();
                    if scaler.run(&decoded, &mut rgb_frame).is_ok() {
                        // Extract RGB data
                        let data = rgb_frame.data(0);
                        let stride = rgb_frame.stride(0);
                        
                        // Copy frame data (handling stride)
                        let mut frame_data = Vec::with_capacity((target_width * target_height * 3) as usize);
                        for y in 0..target_height as usize {
                            let row_start = y * stride;
                            let row_end = row_start + (target_width as usize * 3);
                            frame_data.extend_from_slice(&data[row_start..row_end]);
                        }
                        
                        let frame = DecodedFrame {
                            width: target_width,
                            height: target_height,
                            data: frame_data,
                            pts: decoded.pts().unwrap_or(0),
                        };
                        
                        // Store frame
                        *current_frame.lock().unwrap() = Some(frame);
                        
                        // Rate limiting to avoid overwhelming the UI
                        let elapsed = last_frame_time.elapsed();
                        if elapsed < frame_duration {
                            thread::sleep(frame_duration - elapsed);
                        }
                        last_frame_time = Instant::now();
                    }
                }
            }
            
            *state.lock().unwrap() = PlayerState::Stopped;
            let _ = msg_tx.send(PlayerMessage::Finished);
        }

        /// Stop playback
        pub fn stop(&mut self) {
            if let Some(ref sender) = self.command_sender {
                let _ = sender.send(PlayerCommand::Stop);
            }
            self.command_sender = None;
            self.message_receiver = None;
            *self.state.lock().unwrap() = PlayerState::Stopped;
            *self.current_frame.lock().unwrap() = None;
        }

        /// Toggle pause
        pub fn toggle_pause(&mut self) {
            if let Some(ref sender) = self.command_sender {
                let state = self.state.lock().unwrap().clone();
                match state {
                    PlayerState::Playing => {
                        let _ = sender.send(PlayerCommand::Pause);
                    }
                    PlayerState::Paused => {
                        let _ = sender.send(PlayerCommand::Resume);
                    }
                    _ => {}
                }
            }
        }

        /// Set volume (0.0 - 1.0)
        pub fn set_volume(&mut self, volume: f32) {
            self.volume = volume.clamp(0.0, 1.0);
        }

        /// Toggle mute
        pub fn toggle_mute(&mut self) {
            self.muted = !self.muted;
        }

        /// Check if muted
        pub fn is_muted(&self) -> bool {
            self.muted
        }

        /// Get current URL
        pub fn current_url(&self) -> &str {
            &self.url
        }

        /// Get channel name
        pub fn channel_name(&self) -> &str {
            &self.channel_name
        }
    }

    impl Drop for InternalPlayer {
        fn drop(&mut self) {
            self.stop();
        }
    }
}

// Stub implementation when internal-player feature is disabled
#[cfg(not(feature = "internal-player"))]
mod player_impl {
    #[derive(Debug, Clone, PartialEq)]
    pub enum PlayerState {
        Stopped,
        Loading,
        Playing,
        Paused,
        Error(String),
    }

    pub struct DecodedFrame {
        pub width: u32,
        pub height: u32,
        pub data: Vec<u8>,
        pub pts: i64,
    }

    pub enum PlayerMessage {
        StateChanged(PlayerState),
        Frame(DecodedFrame),
        Error(String),
        Finished,
    }

    pub struct InternalPlayer {
        state: PlayerState,
        channel_name: String,
    }

    impl InternalPlayer {
        pub fn new() -> Self {
            Self {
                state: PlayerState::Stopped,
                channel_name: String::new(),
            }
        }

        pub fn state(&self) -> PlayerState {
            self.state.clone()
        }

        pub fn take_frame(&self) -> Option<DecodedFrame> {
            None
        }

        pub fn poll_messages(&mut self) -> Vec<PlayerMessage> {
            Vec::new()
        }

        pub fn play(&mut self, name: &str, _url: &str, _buffer_secs: u32, _user_agent: &str) {
            self.channel_name = name.to_string();
            self.state = PlayerState::Error("Internal player not enabled. Build with --features internal-player".to_string());
        }

        pub fn stop(&mut self) {
            self.state = PlayerState::Stopped;
        }

        pub fn toggle_pause(&mut self) {}
        pub fn set_volume(&mut self, _volume: f32) {}
        pub fn toggle_mute(&mut self) {}
        pub fn is_muted(&self) -> bool { false }
        pub fn current_url(&self) -> &str { "" }
        pub fn channel_name(&self) -> &str { &self.channel_name }
    }
}

// Re-export
pub use player_impl::*;

/// Player window that can be embedded in egui
pub struct PlayerWindow {
    pub player: InternalPlayer,
    pub texture: Option<egui::TextureHandle>,
    pub show_controls: bool,
    last_error: Option<String>,
}

impl PlayerWindow {
    pub fn new() -> Self {
        Self {
            player: InternalPlayer::new(),
            texture: None,
            show_controls: true,
            last_error: None,
        }
    }

    /// Play a channel
    pub fn play(&mut self, name: &str, url: &str, buffer_secs: u32, user_agent: &str) {
        self.last_error = None;
        self.texture = None;
        self.player.play(name, url, buffer_secs, user_agent);
    }

    /// Stop playback
    pub fn stop(&mut self) {
        self.player.stop();
        self.texture = None;
    }

    /// Render the player UI
    pub fn show(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // Process messages
        for msg in self.player.poll_messages() {
            match msg {
                PlayerMessage::Error(e) => {
                    self.last_error = Some(e);
                }
                _ => {}
            }
        }

        // Check for new frames
        if let Some(frame) = self.player.take_frame() {
            let image = egui::ColorImage::from_rgb(
                [frame.width as usize, frame.height as usize],
                &frame.data,
            );
            
            self.texture = Some(ctx.load_texture(
                "video_frame",
                image,
                egui::TextureOptions::LINEAR,
            ));
        }

        ui.vertical_centered(|ui| {
            // Render video or status
            if let Some(ref texture) = self.texture {
                let available = ui.available_size();
                let tex_size = texture.size_vec2();
                let aspect = tex_size.x / tex_size.y;
                
                let (width, height) = if available.x / available.y > aspect {
                    (available.y * aspect * 0.9, available.y * 0.9)
                } else {
                    (available.x * 0.9, available.x / aspect * 0.9)
                };
                
                ui.image((texture.id(), egui::vec2(width, height)));
            } else {
                ui.add_space(50.0);
                
                match self.player.state() {
                    PlayerState::Loading => {
                        ui.spinner();
                        ui.label("Connecting to stream...");
                    }
                    PlayerState::Stopped => {
                        ui.label("Playback stopped");
                        if let Some(ref error) = self.last_error {
                            ui.add_space(10.0);
                            ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                        }
                    }
                    PlayerState::Error(ref e) => {
                        ui.colored_label(egui::Color32::RED, format!("Error: {}", e));
                    }
                    _ => {}
                }
            }
            
            // Show any errors even while playing
            if matches!(self.player.state(), PlayerState::Playing) {
                if let Some(ref error) = self.last_error {
                    ui.add_space(5.0);
                    ui.colored_label(egui::Color32::YELLOW, format!("⚠ {}", error));
                }
            }
        });

        // Controls
        if self.show_controls {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(self.player.channel_name());
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⏹ Stop").clicked() {
                        self.stop();
                    }
                    
                    let pause_text = if matches!(self.player.state(), PlayerState::Paused) {
                        "▶ Play"
                    } else {
                        "⏸ Pause"
                    };
                    if ui.button(pause_text).clicked() {
                        self.player.toggle_pause();
                    }
                });
            });
        }

        // Request continuous repaint while playing
        if matches!(self.player.state(), PlayerState::Playing | PlayerState::Loading) {
            ctx.request_repaint();
        }
    }

    /// Check if currently playing
    pub fn is_playing(&self) -> bool {
        matches!(self.player.state(), PlayerState::Playing | PlayerState::Loading)
    }
}
