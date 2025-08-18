use anyhow::Result;
use rodio::{OutputStream, Sink};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, error, info};

/// Audio feedback for save events
pub struct AudioFeedback {
    sink: Arc<Mutex<Option<Sink>>>,
    enabled: Arc<Mutex<bool>>,
}

impl AudioFeedback {
    pub fn new() -> Result<Self> {
        // We'll create the audio stream on-demand in a separate thread
        info!("Audio feedback initialized");
        
        Ok(Self {
            sink: Arc::new(Mutex::new(None)),
            enabled: Arc::new(Mutex::new(true)),
        })
    }
    
    pub fn set_enabled(&self, enabled: bool) {
        *self.enabled.lock().unwrap() = enabled;
        debug!("Audio feedback {}", if enabled { "enabled" } else { "disabled" });
    }
    
    /// Play a success sound (pleasant chime)
    pub fn play_success(&self) {
        if !*self.enabled.lock().unwrap() {
            return;
        }
        
        debug!("Playing success sound");
        self.play_tone(800.0, 0.15); // Higher pitch, short duration
    }
    
    /// Play an info sound (subtle click)
    pub fn play_info(&self) {
        if !*self.enabled.lock().unwrap() {
            return;
        }
        
        debug!("Playing info sound");
        self.play_tone(400.0, 0.1); // Medium pitch, very short
    }
    
    /// Play an error sound (low buzz)
    pub fn play_error(&self) {
        if !*self.enabled.lock().unwrap() {
            return;
        }
        
        debug!("Playing error sound");
        self.play_tone(200.0, 0.3); // Low pitch, longer duration
    }
    
    /// Play a simple tone at the given frequency for the given duration
    fn play_tone(&self, frequency: f32, duration_secs: f32) {
        // Generate a simple sine wave tone
        let sample_rate = 44100;
        let samples_count = (sample_rate as f32 * duration_secs) as usize;
        
        let mut samples = Vec::with_capacity(samples_count);
        for i in 0..samples_count {
            let t = i as f32 / sample_rate as f32;
            let sample = (t * frequency * 2.0 * std::f32::consts::PI).sin();
            // Apply envelope to avoid clicks (fade in and out)
            let envelope = if i < 100 {
                i as f32 / 100.0
            } else if i > samples_count - 100 {
                (samples_count - i) as f32 / 100.0
            } else {
                1.0
            };
            samples.push(sample * envelope * 0.2); // Keep volume low
        }
        
        // Play the sound in a separate thread to avoid Send/Sync issues
        std::thread::spawn(move || {
            if let Ok((_stream, stream_handle)) = OutputStream::try_default() {
                if let Ok(sink) = Sink::try_new(&stream_handle) {
                    // Create a source from the samples
                    let source = rodio::buffer::SamplesBuffer::new(1, sample_rate, samples);
                    
                    // Play the sound
                    sink.append(source);
                    
                    // Wait for the sound to finish
                    std::thread::sleep(Duration::from_secs_f32(duration_secs));
                }
            }
        });
    }
    
    /// Play feedback based on save result
    pub fn play_save_result(&self, result: &crate::monitor::SaveResult) {
        match result {
            crate::monitor::SaveResult::Success { .. } => {
                self.play_success();
            }
            crate::monitor::SaveResult::NoChanges => {
                self.play_info();
            }
            crate::monitor::SaveResult::Failed(_) => {
                self.play_error();
            }
        }
    }
}

impl Default for AudioFeedback {
    fn default() -> Self {
        match Self::new() {
            Ok(audio) => audio,
            Err(e) => {
                error!("Failed to initialize audio feedback: {}. Audio will be disabled.", e);
                // Return a dummy instance with audio disabled
                Self {
                    sink: Arc::new(Mutex::new(None)),
                    enabled: Arc::new(Mutex::new(false)),
                }
            }
        }
    }
}