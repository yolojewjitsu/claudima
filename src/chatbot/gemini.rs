//! Gemini API client for image generation (Nano Banana).

use base64::Engine;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

const GEMINI_API_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash-image:generateContent";

pub struct GeminiClient {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct GenerateRequest {
    contents: Vec<Content>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "responseModalities")]
    response_modalities: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct GenerateResponse {
    candidates: Option<Vec<Candidate>>,
    error: Option<ApiError>,
}

#[derive(Deserialize, Debug)]
struct ApiError {
    message: String,
}

#[derive(Deserialize, Debug)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize, Debug)]
struct CandidateContent {
    parts: Vec<ResponsePart>,
}

#[derive(Deserialize, Debug)]
struct ResponsePart {
    #[serde(rename = "inlineData")]
    inline_data: Option<InlineData>,
}

#[derive(Deserialize, Debug)]
struct InlineData {
    data: String,
}

pub struct GeneratedImage {
    pub data: Vec<u8>,
}

impl GeminiClient {
    pub fn new(api_key: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("Failed to build HTTP client");

        Self { api_key, client }
    }

    /// Generate an image from a text prompt.
    pub async fn generate_image(&self, prompt: &str) -> Result<GeneratedImage, String> {
        info!("🎨 Generating image: {}", prompt);

        let request = GenerateRequest {
            contents: vec![Content {
                parts: vec![Part {
                    text: prompt.to_string(),
                }],
            }],
            generation_config: GenerationConfig {
                response_modalities: vec!["TEXT".to_string(), "IMAGE".to_string()],
            },
        };

        let url = format!("{}?key={}", GEMINI_API_URL, self.api_key);

        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {e}"))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {e}"))?;

        debug!("Gemini response status: {status}");

        if !status.is_success() {
            return Err(format!("API error {status}: {body}"));
        }

        let parsed: GenerateResponse =
            serde_json::from_str(&body).map_err(|e| format!("Failed to parse response: {e}"))?;

        if let Some(error) = parsed.error {
            return Err(format!("Gemini error: {}", error.message));
        }

        let candidates = parsed.candidates.ok_or("No candidates in response")?;
        let candidate = candidates.first().ok_or("Empty candidates array")?;
        let content = candidate
            .content
            .as_ref()
            .ok_or("No content in candidate")?;

        // Find the image part
        for part in &content.parts {
            if let Some(ref inline_data) = part.inline_data {
                let data = base64::engine::general_purpose::STANDARD
                    .decode(&inline_data.data)
                    .map_err(|e| format!("Failed to decode base64: {e}"))?;

                info!("🎨 Image generated: {} bytes", data.len());

                return Ok(GeneratedImage { data });
            }
        }

        Err("No image in response".to_string())
    }
}
