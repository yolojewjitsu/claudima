//! Speech-to-text transcription using whisper-rs.
//!
//! Converts voice messages (OGG Opus from Telegram) to text.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use tracing::{debug, info};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Whisper transcription engine.
pub struct Whisper {
    ctx: Arc<WhisperContext>,
}

impl Whisper {
    /// Load a Whisper model from a .bin file.
    pub fn new(model_path: &Path) -> Result<Self, String> {
        info!("Loading Whisper model from {:?}", model_path);

        if !model_path.exists() {
            return Err(format!("Model file not found: {:?}", model_path));
        }

        let ctx = WhisperContext::new_with_params(
            model_path.to_str().ok_or("Invalid model path")?,
            WhisperContextParameters::default(),
        )
        .map_err(|e| format!("Failed to load Whisper model: {e}"))?;

        info!("Whisper model loaded successfully");
        Ok(Self { ctx: Arc::new(ctx) })
    }

    /// Transcribe audio data (OGG Opus format from Telegram).
    ///
    /// Converts to 16KHz mono PCM using ffmpeg, then runs Whisper.
    pub fn transcribe(&self, ogg_data: &[u8]) -> Result<String, String> {
        debug!("Transcribing {} bytes of audio", ogg_data.len());

        // Convert OGG to 16KHz mono f32 PCM using ffmpeg
        let pcm_data = convert_ogg_to_pcm(ogg_data)?;

        // Create state for this transcription
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("Failed to create Whisper state: {e}"))?;

        // Configure parameters
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en")); // Default to English, auto-detect if needed
        params.set_translate(false);
        params.set_no_timestamps(true);
        params.set_single_segment(false);

        // Run transcription
        state
            .full(params, &pcm_data)
            .map_err(|e| format!("Whisper transcription failed: {e}"))?;

        // Collect all segments
        let mut text = String::new();
        for segment in state.as_iter() {
            if let Ok(s) = segment.to_str() {
                text.push_str(s);
                text.push(' ');
            }
        }

        let text = text.trim().to_string();
        info!("Transcribed: \"{}\"", truncate(&text, 100));
        Ok(text)
    }
}

/// Convert OGG Opus audio to 16KHz mono f32 PCM samples using ffmpeg.
fn convert_ogg_to_pcm(ogg_data: &[u8]) -> Result<Vec<f32>, String> {
    // Create temp file for input (ffmpeg needs seekable input for OGG)
    let temp_dir = std::env::temp_dir();
    let input_path = temp_dir.join(format!("whisper_input_{}.ogg", std::process::id()));

    std::fs::write(&input_path, ogg_data)
        .map_err(|e| format!("Failed to write temp input: {e}"))?;

    // Run ffmpeg to convert to raw PCM
    // Output format: 16-bit signed little-endian, 16KHz, mono
    let output = Command::new("ffmpeg")
        .args([
            "-i",
            input_path.to_str().unwrap(),
            "-ar",
            "16000", // 16KHz sample rate
            "-ac",
            "1", // Mono
            "-f",
            "s16le", // 16-bit signed little-endian PCM
            "-acodec",
            "pcm_s16le",
            "-y",  // Overwrite
            "pipe:1", // Output to stdout
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;

    // Clean up temp file
    let _ = std::fs::remove_file(&input_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg failed: {}", stderr));
    }

    // Convert i16 samples to f32
    let samples: Vec<f32> = output
        .stdout
        .chunks_exact(2)
        .map(|chunk| {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            sample as f32 / 32768.0
        })
        .collect();

    debug!("Converted to {} f32 samples", samples.len());
    Ok(samples)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello...");
    }
}
