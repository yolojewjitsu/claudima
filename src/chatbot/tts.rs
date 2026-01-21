//! Text-to-speech using Fish Speech.
//!
//! Generates voice audio from text using the Fish Speech model.
//! Requires a running Fish Speech API server.

use std::process::Command;

use serde::Deserialize;
use tracing::{debug, info, warn};

/// Response from /v1/references/list endpoint.
#[derive(Debug, Deserialize)]
struct ListReferencesResponse {
    success: bool,
    reference_ids: Vec<String>,
}

/// TTS client for Fish Speech API.
pub struct TtsClient {
    endpoint: String,
    client: reqwest::Client,
}

impl TtsClient {
    /// Create a new TTS client.
    ///
    /// `endpoint` should be the base URL of the Fish Speech server,
    /// e.g., "http://localhost:8880"
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: reqwest::Client::new(),
        }
    }

    /// Get list of available voice reference IDs from Fish Speech.
    pub async fn list_voices(&self) -> Vec<String> {
        match self.client
            .get(format!("{}/v1/references/list", self.endpoint))
            .header("Accept", "application/json")
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    if let Ok(resp) = response.json::<ListReferencesResponse>().await {
                        if resp.success {
                            return resp.reference_ids;
                        }
                        warn!("Voice list API returned success=false");
                    }
                }
                warn!("Failed to parse voice list response");
                vec![]
            }
            Err(e) => {
                warn!("Failed to fetch voice list: {}", e);
                vec![]
            }
        }
    }

    /// Generate speech from text.
    ///
    /// Returns OGG Opus audio data suitable for Telegram voice messages.
    /// The `voice` parameter specifies the reference voice ID (default: "p231").
    pub async fn synthesize(&self, text: &str, voice: Option<&str>) -> Result<Vec<u8>, String> {
        let preview: String = text.chars().take(50).collect();
        info!("TTS: \"{}\"", preview);

        // Default to xtts_female voice (natural sounding female)
        let reference_id = voice.unwrap_or("xtts_female");

        // Call Fish Speech TTS endpoint
        let response = self
            .client
            .post(format!("{}/v1/tts", self.endpoint))
            .json(&serde_json::json!({
                "text": text,
                "format": "wav",
                "reference_id": reference_id
            }))
            .send()
            .await
            .map_err(|e| format!("TTS request failed: {e}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("TTS error {}: {}", status, body));
        }

        let wav_data = response
            .bytes()
            .await
            .map_err(|e| format!("Failed to read TTS response: {e}"))?;

        debug!("Got {} bytes of WAV audio", wav_data.len());

        // Convert WAV to OGG Opus for Telegram
        let ogg_data = convert_wav_to_ogg(&wav_data)?;

        info!("Generated {} bytes of voice audio", ogg_data.len());
        Ok(ogg_data)
    }
}

/// Convert WAV audio to OGG Opus format for Telegram voice messages.
fn convert_wav_to_ogg(wav_data: &[u8]) -> Result<Vec<u8>, String> {
    // Write WAV to temp file
    let temp_dir = std::env::temp_dir();
    let input_path = temp_dir.join(format!("tts_input_{}.wav", std::process::id()));
    let output_path = temp_dir.join(format!("tts_output_{}.ogg", std::process::id()));

    std::fs::write(&input_path, wav_data)
        .map_err(|e| format!("Failed to write temp WAV: {e}"))?;

    // Convert using ffmpeg with 300ms silence padding at start
    // (Telegram cuts off the first ~200ms when playing voice messages)
    let output = Command::new("ffmpeg")
        .args([
            "-y",
            "-f", "lavfi",
            "-i", "anullsrc=r=44100:cl=mono",
            "-i",
            input_path.to_str().unwrap(),
            "-filter_complex", "[0]atrim=0:0.3[silence];[silence][1:a]concat=n=2:v=0:a=1",
            "-c:a",
            "libopus",
            "-b:a",
            "64k",
            output_path.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;

    // Clean up input
    let _ = std::fs::remove_file(&input_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_file(&output_path);
        return Err(format!("ffmpeg conversion failed: {}", stderr));
    }

    // Read output
    let ogg_data = std::fs::read(&output_path)
        .map_err(|e| format!("Failed to read OGG output: {e}"))?;

    // Clean up output
    let _ = std::fs::remove_file(&output_path);

    debug!("Converted WAV ({} bytes) to OGG ({} bytes)", wav_data.len(), ogg_data.len());
    Ok(ogg_data)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_tts_client_creation() {
        use super::TtsClient;
        let client = TtsClient::new("http://localhost:8880".to_string());
        assert_eq!(client.endpoint, "http://localhost:8880");
    }
}
