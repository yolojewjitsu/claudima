//! Telegram client using teloxide.

use std::time::Duration;

use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{ChatPermissions, FileId, InputFile, MessageId, ParseMode, ReactionType, ReplyParameters};
use tracing::{info, warn};

/// User info from Telegram.
pub struct ChatMemberInfo {
    pub user_id: i64,
    pub username: Option<String>,
    pub first_name: String,
    pub last_name: Option<String>,
    pub is_bot: bool,
    pub is_premium: bool,
    pub language_code: Option<String>,
    pub status: String,
    pub custom_title: Option<String>,
}

/// Telegram API client.
pub struct TelegramClient {
    bot: Bot,
}

impl TelegramClient {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_to_message_id: Option<i64>,
    ) -> Result<i64, String> {
        let chat_id = ChatId(chat_id);
        let mut request = self
            .bot
            .send_message(chat_id, text)
            .parse_mode(ParseMode::Html);

        if let Some(msg_id) = reply_to_message_id {
            let reply_params = ReplyParameters::new(MessageId(msg_id as i32));
            request = request.reply_parameters(reply_params);
        }

        request.await.map(|msg| msg.id.0 as i64).map_err(|e| {
            let msg = format!("Failed to send: {e}");
            warn!("{}", msg);
            msg
        })
    }

    pub async fn get_chat_member(
        &self,
        chat_id: i64,
        user_id: i64,
    ) -> Result<ChatMemberInfo, String> {
        info!("Getting chat member: chat={}, user={}", chat_id, user_id);
        let chat_id = ChatId(chat_id);
        let user_id = UserId(user_id as u64);

        let member = self
            .bot
            .get_chat_member(chat_id, user_id)
            .await
            .map_err(|e| {
                let msg = format!("Failed to get chat member: {e}");
                warn!("{}", msg);
                msg
            })?;

        // Extract status and custom title from member kind
        let (status, custom_title) = match &member.kind {
            teloxide::types::ChatMemberKind::Owner(o) => ("owner".to_string(), o.custom_title.clone()),
            teloxide::types::ChatMemberKind::Administrator(a) => ("administrator".to_string(), a.custom_title.clone()),
            teloxide::types::ChatMemberKind::Member(_) => ("member".to_string(), None),
            teloxide::types::ChatMemberKind::Restricted(_) => ("restricted".to_string(), None),
            teloxide::types::ChatMemberKind::Left => ("left".to_string(), None),
            teloxide::types::ChatMemberKind::Banned(_) => ("banned".to_string(), None),
        };

        Ok(ChatMemberInfo {
            user_id: member.user.id.0 as i64,
            username: member.user.username.clone(),
            first_name: member.user.first_name.clone(),
            last_name: member.user.last_name.clone(),
            is_bot: member.user.is_bot,
            is_premium: member.user.is_premium,
            language_code: member.user.language_code.clone(),
            status,
            custom_title,
        })
    }

    /// Get user's profile photo as bytes.
    pub async fn get_profile_photo(&self, user_id: i64) -> Result<Option<Vec<u8>>, String> {
        info!("Getting profile photo for user {}", user_id);
        let user_id = UserId(user_id as u64);

        let photos = self
            .bot
            .get_user_profile_photos(user_id)
            .limit(1)
            .await
            .map_err(|e| format!("Failed to get profile photos: {e}"))?;

        if photos.photos.is_empty() {
            return Ok(None);
        }

        // Get the smallest photo (first in the array, usually 160x160)
        let photo_sizes = &photos.photos[0];
        if photo_sizes.is_empty() {
            return Ok(None);
        }

        // Get the largest available size for better quality
        let photo = photo_sizes.last().unwrap();
        let file = self.bot.get_file(photo.file.id.clone()).await
            .map_err(|e| format!("Failed to get photo file: {e}"))?;

        let mut data = Vec::new();
        self.bot.download_file(&file.path, &mut data).await
            .map_err(|e| format!("Failed to download photo: {e}"))?;

        info!("Downloaded profile photo ({} bytes)", data.len());
        Ok(Some(data))
    }

    pub async fn set_message_reaction(
        &self,
        chat_id: i64,
        message_id: i64,
        emoji: &str,
    ) -> Result<(), String> {
        info!("Adding reaction {} to msg {} in chat {}", emoji, message_id, chat_id);

        let chat_id = ChatId(chat_id);
        let message_id = MessageId(message_id as i32);
        let reaction = ReactionType::Emoji {
            emoji: emoji.to_string(),
        };

        self.bot
            .set_message_reaction(chat_id, message_id)
            .reaction(vec![reaction])
            .await
            .map_err(|e| {
                let msg = format!("Failed to add reaction: {e}");
                warn!("{}", msg);
                msg
            })?;

        Ok(())
    }

    /// Delete a message.
    pub async fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), String> {
        info!("🗑️ Deleting message {} in chat {}", message_id, chat_id);

        self.bot
            .delete_message(ChatId(chat_id), MessageId(message_id as i32))
            .await
            .map_err(|e| {
                let msg = format!("Failed to delete message: {e}");
                warn!("{}", msg);
                msg
            })?;

        Ok(())
    }

    /// Mute a user temporarily.
    pub async fn mute_user(
        &self,
        chat_id: i64,
        user_id: i64,
        duration_minutes: i64,
    ) -> Result<(), String> {
        info!("🔇 Muting user {} in chat {} for {} minutes", user_id, chat_id, duration_minutes);

        let until = chrono::Utc::now() + Duration::from_secs((duration_minutes * 60) as u64);

        // Remove all permissions (mute)
        let permissions = ChatPermissions::empty();

        self.bot
            .restrict_chat_member(ChatId(chat_id), UserId(user_id as u64), permissions)
            .until_date(until)
            .await
            .map_err(|e| {
                let msg = format!("Failed to mute user: {e}");
                warn!("{}", msg);
                msg
            })?;

        Ok(())
    }

    /// Ban a user permanently.
    pub async fn ban_user(&self, chat_id: i64, user_id: i64) -> Result<(), String> {
        info!("🚫 Banning user {} from chat {}", user_id, chat_id);

        self.bot
            .ban_chat_member(ChatId(chat_id), UserId(user_id as u64))
            .await
            .map_err(|e| {
                let msg = format!("Failed to ban user: {e}");
                warn!("{}", msg);
                msg
            })?;

        Ok(())
    }

    /// Kick a user (ban + immediate unban so they can rejoin).
    pub async fn kick_user(&self, chat_id: i64, user_id: i64) -> Result<(), String> {
        info!("👢 Kicking user {} from chat {}", user_id, chat_id);

        // Ban first
        self.bot
            .ban_chat_member(ChatId(chat_id), UserId(user_id as u64))
            .await
            .map_err(|e| {
                let msg = format!("Failed to kick user: {e}");
                warn!("{}", msg);
                msg
            })?;

        // Immediately unban so they can rejoin
        self.bot
            .unban_chat_member(ChatId(chat_id), UserId(user_id as u64))
            .await
            .map_err(|e| {
                let msg = format!("Failed to unban after kick: {e}");
                warn!("{}", msg);
                msg
            })?;

        Ok(())
    }

    /// Get list of chat administrators.
    pub async fn get_chat_admins(&self, chat_id: i64) -> Result<String, String> {
        info!("👥 Getting admins for chat {}", chat_id);

        let admins = self
            .bot
            .get_chat_administrators(ChatId(chat_id))
            .await
            .map_err(|e| {
                let msg = format!("Failed to get chat admins: {e}");
                warn!("{}", msg);
                msg
            })?;

        let admin_list: Vec<serde_json::Value> = admins
            .iter()
            .map(|m| {
                serde_json::json!({
                    "user_id": m.user.id.0,
                    "username": m.user.username,
                    "first_name": m.user.first_name,
                    "is_owner": matches!(m.kind, teloxide::types::ChatMemberKind::Owner(_)),
                })
            })
            .collect();

        Ok(serde_json::to_string(&admin_list).unwrap_or_else(|_| "[]".to_string()))
    }

    /// Send an image from bytes.
    pub async fn send_image(
        &self,
        chat_id: i64,
        image_data: Vec<u8>,
        caption: Option<&str>,
        reply_to_message_id: Option<i64>,
    ) -> Result<i64, String> {
        info!("📷 Sending image to chat {} ({} bytes)", chat_id, image_data.len());

        let chat_id = ChatId(chat_id);
        let input_file = InputFile::memory(image_data).file_name("image.png");

        let mut request = self.bot.send_photo(chat_id, input_file);

        if let Some(cap) = caption {
            request = request.caption(cap);
        }

        if let Some(msg_id) = reply_to_message_id {
            let reply_params = ReplyParameters::new(MessageId(msg_id as i32));
            request = request.reply_parameters(reply_params);
        }

        request.await.map(|msg| msg.id.0 as i64).map_err(|e| {
            let msg = format!("Failed to send image: {e}");
            warn!("{}", msg);
            msg
        })
    }

    /// Download an image by file_id.
    /// Returns (bytes, media_type).
    pub async fn download_image(&self, file_id: &str) -> Result<(Vec<u8>, String), String> {
        // Get file info
        let file = self.bot.get_file(FileId(file_id.to_string())).await.map_err(|e| {
            format!("Failed to get file info: {e}")
        })?;

        let file_path = &file.path;

        // Download file content
        let mut data = Vec::new();
        self.bot.download_file(file_path, &mut data).await.map_err(|e| {
            format!("Failed to download file: {e}")
        })?;

        // Determine media type from extension
        let media_type = if file_path.ends_with(".jpg") || file_path.ends_with(".jpeg") {
            "image/jpeg"
        } else if file_path.ends_with(".png") {
            "image/png"
        } else if file_path.ends_with(".webp") {
            "image/webp"
        } else {
            "image/jpeg" // Default for Telegram images
        };

        info!("📥 Downloaded image ({} bytes, {})", data.len(), media_type);
        Ok((data, media_type.to_string()))
    }

    /// Send a voice message from bytes (OGG Opus format).
    pub async fn send_voice(
        &self,
        chat_id: i64,
        voice_data: Vec<u8>,
        caption: Option<&str>,
        reply_to_message_id: Option<i64>,
    ) -> Result<i64, String> {
        info!("🔊 Sending voice to chat {} ({} bytes)", chat_id, voice_data.len());

        let chat_id = ChatId(chat_id);
        let input_file = InputFile::memory(voice_data).file_name("voice.ogg");

        let mut request = self.bot.send_voice(chat_id, input_file);

        if let Some(cap) = caption {
            request = request.caption(cap);
        }

        if let Some(msg_id) = reply_to_message_id {
            let reply_params = ReplyParameters::new(MessageId(msg_id as i32));
            request = request.reply_parameters(reply_params);
        }

        request.await.map(|msg| msg.id.0 as i64).map_err(|e| {
            let msg = format!("Failed to send voice: {e}");
            warn!("{}", msg);
            msg
        })
    }

}
