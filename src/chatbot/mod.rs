//! Chatbot module - relays Telegram messages to Claude Code.

pub mod claude_code;
pub mod context;
pub mod database;
pub mod debounce;
pub mod docx;
pub mod engine;
pub mod reminders;
pub mod gemini;
pub mod message;
pub mod peer;
pub mod signals;
pub mod telegram;
pub mod tools;
pub mod tts;
pub mod whisper;

pub use claude_code::ClaudeCode;
pub use engine::{system_prompt, ChatbotConfig, ChatbotEngine, TrustedUser};
pub use message::{ChatMessage, ReplyTo};
pub use telegram::TelegramClient;
pub use whisper::Whisper;
