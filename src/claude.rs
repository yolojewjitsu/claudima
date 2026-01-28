use serde::{Deserialize, Serialize};

pub struct Client {
    api_key: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone, Copy)]
pub enum Role {
    User,
}

#[derive(Debug, Clone, Copy)]
pub enum Model {
    Haiku,
}

impl Model {
    fn as_str(&self) -> &'static str {
        match self {
            // OpenRouter model IDs
            Model::Haiku => "anthropic/claude-3-haiku",
        }
    }
}

// OpenRouter uses OpenAI-compatible format
#[derive(Serialize)]
struct ApiRequest {
    model: &'static str,
    max_tokens: u32,
    messages: Vec<ApiMessage>,
}

#[derive(Serialize)]
struct ApiMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Deserialize)]
struct ResponseMessage {
    content: String,
}

impl Client {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            http: reqwest::Client::new(),
        }
    }

    pub async fn message(
        &self,
        model: Model,
        messages: &[Message],
        max_tokens: u32,
    ) -> Result<String, Error> {
        let api_messages: Vec<ApiMessage> = messages
            .iter()
            .map(|m| ApiMessage {
                role: match m.role {
                    Role::User => "user",
                },
                content: m.content.clone(),
            })
            .collect();

        let request = ApiRequest {
            model: model.as_str(),
            max_tokens,
            messages: api_messages,
        };

        let response = self
            .http
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Api(format!("{status}: {body}")));
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .map_err(|e| Error::Parse(e.to_string()))?;

        api_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or(Error::Empty)
    }
}

#[derive(Debug)]
pub enum Error {
    Http(String),
    Api(String),
    Parse(String),
    Empty,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Http(e) => write!(f, "HTTP error: {e}"),
            Error::Api(e) => write!(f, "API error: {e}"),
            Error::Parse(e) => write!(f, "Parse error: {e}"),
            Error::Empty => write!(f, "Empty response"),
        }
    }
}

impl std::error::Error for Error {}
