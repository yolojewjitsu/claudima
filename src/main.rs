mod chatbot;
mod classifier;
mod claude;
mod config;
mod prefilter;
mod telegram_log;

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use teloxide::prelude::*;
use teloxide::types::ChatKind;
use tracing::{info, warn};
use tracing_subscriber::prelude::*;

use chatbot::{system_prompt, ChatMessage, ChatbotConfig, ChatbotEngine, ClaudeCode, ReplyTo, TelegramClient, TrustedUser, Whisper};
use chatbot::message::DocumentContent;
use classifier::{classify, Classification};
use claude::Client as ClaudeClient;
use config::Config;
use prefilter::{prefilter, PrefilterResult};

struct BotState {
    config: Config,
    claude: ClaudeClient,
    strikes: Mutex<HashMap<UserId, u8>>,
    chatbot: Option<ChatbotEngine>,
    dm_denied: Mutex<std::collections::HashSet<UserId>>,
    whisper: Option<Whisper>,
}

impl BotState {
    async fn new(config: Config, bot: &Bot) -> Self {
        let claude = ClaudeClient::new(config.openrouter_api_key.clone());

        // Get bot info
        let (bot_user_id, bot_username) = match bot.get_me().await {
            Ok(me) => {
                info!("Bot user ID: {}, username: @{}", me.id, me.username());
                (me.id.0 as i64, Some(me.username().to_string()))
            }
            Err(e) => {
                warn!("Failed to get bot info: {e}");
                (0, None)
            }
        };

        // Create chatbot if enabled
        let chatbot = if !config.allowed_groups.is_empty() {
            let primary_chat_id = config.primary_chat_id;
            let telegram = Arc::new(TelegramClient::new(bot.clone()));

            // Fetch owner info from Telegram
            let owner = if let Some(owner_id) = config.owner_ids.first() {
                let username = telegram.get_chat_username(owner_id.0 as i64).await.ok().flatten();
                let owner = TrustedUser::with_username(owner_id.0 as i64, username);
                info!("Owner: {}", owner.display());
                Some(owner)
            } else {
                None
            };

            // Fetch trusted DM users' usernames from Telegram and update the HashMap
            // Collect IDs first to avoid holding lock across await
            let trusted_ids: Vec<i64> = config.trusted_dm_users
                .read()
                .expect("trusted_dm_users lock poisoned")
                .keys()
                .copied()
                .collect();

            for user_id in trusted_ids {
                let username = telegram.get_chat_username(user_id).await.ok().flatten();
                // Update the HashMap with the fetched username
                {
                    let mut users = config.trusted_dm_users.write().expect("trusted_dm_users lock poisoned");
                    users.insert(user_id, username.clone());
                }
                let user_display = match &username {
                    Some(u) => format!("@{} ({})", u, user_id),
                    None => user_id.to_string(),
                };
                info!("Trusted DM user: {}", user_display);
            }

            let chatbot_config = ChatbotConfig {
                primary_chat_id,
                bot_user_id,
                bot_username: bot_username.clone(),
                owner,
                trusted_dm_users: config.trusted_dm_users.clone(),
                config_path: Some(config.config_path.clone()),
                debounce_ms: 1000,
                data_dir: Some(config.data_dir.clone()),
                gemini_api_key: if config.gemini_api_key.is_empty() { None } else { Some(config.gemini_api_key.clone()) },
                tts_endpoint: config.tts_endpoint.clone(),
                personality: config.personality.clone(),
                scan_interval_minutes: config.scan_interval_minutes,
                scan_times: config.scan_times.clone(),
                scan_timezone: config.scan_timezone,
                peer_bots: config.peer_bots.clone(),
            };

            // Fetch available TTS voices if endpoint configured
            let available_voices = if let Some(ref endpoint) = config.tts_endpoint {
                use crate::chatbot::tts::TtsClient;
                let tts = TtsClient::new(endpoint.clone());
                let voices = tts.list_voices().await;
                if !voices.is_empty() {
                    info!("TTS voices available: {}", voices.join(", "));
                }
                Some(voices)
            } else {
                None
            };

            // Start Claude Code with system prompt and session persistence
            let prompt = system_prompt(&chatbot_config, available_voices.as_deref());
            let session_file = Some(config.data_dir.join("session_id"));
            let claude_code = match ClaudeCode::start(prompt, session_file) {
                Ok(cc) => cc,
                Err(e) => {
                    panic!("Failed to start Claude Code: {}", e);
                }
            };

            let mut engine = ChatbotEngine::new(chatbot_config, telegram, claude_code);
            engine.start_debouncer();
            engine.notify_owner("hey, just restarted").await;

            info!("Chatbot enabled (primary chat: {})", primary_chat_id);
            Some(engine)
        } else {
            info!("Chatbot disabled (no allowed_groups)");
            None
        };

        // Initialize Whisper if model path is configured
        let whisper = if let Some(ref model_path) = config.whisper_model_path {
            match Whisper::new(model_path) {
                Ok(w) => {
                    info!("Whisper loaded from {:?}", model_path);
                    Some(w)
                }
                Err(e) => {
                    warn!("Failed to load Whisper model: {}", e);
                    None
                }
            }
        } else {
            info!("No Whisper model configured - voice transcription disabled");
            None
        };

        Self {
            config,
            claude,
            strikes: Mutex::new(HashMap::new()),
            chatbot,
            dm_denied: Mutex::new(std::collections::HashSet::new()),
            whisper,
        }
    }

    async fn add_strike(&self, user_id: UserId) -> u8 {
        let mut strikes = self.strikes.lock().await;
        let count = strikes.entry(user_id).or_insert(0);
        *count += 1;
        *count
    }
}

/// Parse command-line arguments.
/// Returns (config_path, system_message)
fn parse_args() -> (String, Option<String>) {
    let args: Vec<String> = std::env::args().collect();
    let mut config_path = "claudima.json".to_string();
    let mut system_message = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--message" | "-m" => {
                if i + 1 < args.len() {
                    system_message = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --message requires an argument");
                    std::process::exit(1);
                }
            }
            arg if !arg.starts_with('-') => {
                config_path = arg.to_string();
                i += 1;
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                i += 1;
            }
        }
    }

    (config_path, system_message)
}

#[tokio::main]
async fn main() {
    let (config_path, system_message) = parse_args();
    let config = Config::load(&config_path).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(1);
    });

    let bot = Bot::new(&config.telegram_bot_token);

    // Setup logging
    let log_dir = config.data_dir.join("logs");
    std::fs::create_dir_all(&log_dir).ok();
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_dir.join("claudima.log"))
        .expect("Failed to open log file");
    let (non_blocking, _guard) = tracing_appender::non_blocking(log_file);

    let registry = tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stdout)
                .with_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                ),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                ),
        );

    if let Some(log_chat_id) = config.log_chat_id {
        let tg_layer = telegram_log::TelegramLogLayer::new(bot.clone(), log_chat_id);
        registry.with(tg_layer).init();
    } else {
        registry.init();
    }

    info!("🚀 Starting claudima...");
    info!("Loaded config from {config_path}");
    info!("Owner IDs: {:?}", config.owner_ids);
    if config.dry_run {
        info!("DRY RUN mode enabled");
    }

    let state = Arc::new(BotState::new(config, &bot).await);

    // Send system message to chatbot if provided
    if let (Some(chatbot), Some(msg)) = (&state.chatbot, &system_message) {
        info!("📢 Sending system message: {}", msg);
        let system_msg = ChatMessage {
            message_id: 0,
            chat_id: 0,
            user_id: 0,
            username: "system".to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
            text: msg.clone(),
            reply_to: None,
            image: None,
            documents: vec![],
            voice_transcription: None,
        };
        chatbot.handle_message(system_msg).await;
    }

    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handle_new_message))
        .branch(Update::filter_edited_message().endpoint(handle_edited_message))
        .branch(Update::filter_channel_post().endpoint(handle_channel_post))
        .branch(Update::filter_chat_member().endpoint(handle_chat_member));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .enable_ctrlc_handler()
        .default_handler(|upd| async move {
            warn!("Unhandled update: {:?}", upd);
        })
        .error_handler(LoggingErrorHandler::with_custom_text(
            "Error in update handler",
        ))
        .build()
        .dispatch()
        .await;
}

async fn handle_new_message(bot: Bot, msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    let is_group = matches!(msg.chat.kind, ChatKind::Public(_));
    let is_private = matches!(msg.chat.kind, ChatKind::Private(_));

    let user = match msg.from {
        Some(ref u) => u,
        None => return Ok(()),
    };

    let username = user.username.as_deref().unwrap_or(&user.first_name);

    // Handle DMs
    if is_private {
        if state.config.can_dm(user.id) {
            info!("📨 DM from {} ({})", username, user.id);
            if let Some(ref chatbot) = state.chatbot {
                // Download image if present
                let image = if let Some(photos) = msg.photo() {
                    if let Some(largest) = photos.iter().max_by_key(|p| p.width * p.height) {
                        match chatbot.download_image(&largest.file.id.0).await {
                            Ok(img) => Some(img),
                            Err(e) => {
                                warn!("Failed to download image: {}", e);
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Transcribe voice if present
                let voice_transcription = transcribe_voice(&bot, &state, &msg).await;

                // Extract documents if present
                let documents = extract_documents(&bot, &msg).await;

                let chat_msg = telegram_to_chat_message_with_media(&msg, image, voice_transcription, documents);
                chatbot.handle_message(chat_msg).await;
            }
            return Ok(());
        } else {
            let mut denied = state.dm_denied.lock().await;
            if !denied.contains(&user.id) {
                denied.insert(user.id);
                info!("DM from non-trusted user {} ({}) - denial", username, user.id);
                bot.send_message(msg.chat.id, "Access denied.").await.ok();
            }
            return Ok(());
        }
    }

    if !is_group {
        return Ok(());
    }

    // Check allowed group
    if !state.config.allowed_groups.is_empty()
        && !state.config.allowed_groups.contains(&msg.chat.id)
    {
        return Ok(());
    }

    // Get text (or caption for images/voice/documents)
    let text = msg.text().or_else(|| msg.caption());
    let has_image = msg.photo().is_some();
    let has_voice = msg.voice().is_some();
    let has_document = msg.document().is_some_and(|d| {
        d.file_name.as_deref().is_some_and(|f| f.to_lowercase().ends_with(".docx"))
    });

    // Skip if no text, image, voice, or document
    if text.is_none() && !has_image && !has_voice && !has_document {
        return Ok(());
    }

    // SPAM FILTER FIRST - spam messages must NEVER reach the chatbot
    let is_spam = if let Some(text) = text {
        // Owners and trusted channels bypass spam filter
        let bypass_filter = state.config.is_owner(user.id)
            || msg.sender_chat.as_ref().is_some_and(|c| state.config.is_trusted_channel(c.id));

        if bypass_filter {
            info!("Bypass spam filter for {username} ({})", user.id);
            false
        } else {
            let prefilter_result = prefilter(text, &state.config);
            let text_preview: String = text.chars().take(100).collect();
            info!("Message from {username} ({}): \"{text_preview}\" → {:?}", user.id, prefilter_result);

            match prefilter_result {
                PrefilterResult::ObviousSpam => true,
                PrefilterResult::ObviousSafe => false,
                PrefilterResult::Ambiguous => {
                    match classify(text, &state.claude).await {
                        Ok(Classification::Spam) => {
                            info!("Haiku: spam");
                            true
                        }
                        Ok(Classification::NotSpam) => {
                            info!("Haiku: not spam");
                            false
                        }
                        Err(e) => {
                            warn!("Classification error: {e}");
                            false
                        }
                    }
                }
            }
        }
    } else {
        false // No text = not spam (image/voice only)
    };

    // Handle spam: delete, strike, ban - and DO NOT pass to chatbot
    if is_spam {
        let dry = state.config.dry_run;

        if dry {
            info!("[DRY RUN] Would delete message {}", msg.id);
        } else if let Err(e) = bot.delete_message(msg.chat.id, msg.id).await {
            warn!("Failed to delete: {e}");
        }

        let strikes = state.add_strike(user.id).await;
        info!("{username} has {strikes} strike(s)");

        if strikes >= state.config.max_strikes {
            if dry {
                info!("[DRY RUN] Would ban {username}");
            } else {
                info!("Banning {username}");
                if let Err(e) = bot.ban_chat_member(msg.chat.id, user.id).await {
                    warn!("Failed to ban: {e}");
                }
            }
        }

        // CRITICAL: Do not pass spam to chatbot
        return Ok(());
    }

    // Only non-spam messages reach the chatbot
    if let Some(ref chatbot) = state.chatbot {
        // Download image if present
        let image = if has_image {
            if let Some(photos) = msg.photo() {
                if let Some(largest) = photos.iter().max_by_key(|p| p.width * p.height) {
                    match chatbot.download_image(&largest.file.id.0).await {
                        Ok(img) => Some(img),
                        Err(e) => {
                            warn!("Failed to download image: {}", e);
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Transcribe voice if present
        let voice_transcription = transcribe_voice(&bot, &state, &msg).await;

        // Extract documents if present
        let documents = extract_documents(&bot, &msg).await;

        let chat_msg = telegram_to_chat_message_with_media(&msg, image, voice_transcription, documents);
        chatbot.handle_message(chat_msg).await;
    }

    Ok(())
}

async fn handle_channel_post(_bot: Bot, msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    // Only handle posts in allowed channels/groups
    if !state.config.allowed_groups.is_empty()
        && !state.config.allowed_groups.contains(&msg.chat.id)
    {
        return Ok(());
    }

    let text = msg.text().or_else(|| msg.caption());
    let has_image = msg.photo().is_some();

    if text.is_none() && !has_image {
        return Ok(());
    }

    // Channel posts have sender_chat instead of from
    let channel_title = msg.sender_chat.as_ref()
        .and_then(|c| c.title())
        .unwrap_or("channel");

    info!("📢 Channel post in {} ({}): {:?}",
        channel_title, msg.chat.id,
        text.map(|t| t.chars().take(100).collect::<String>()));

    if let Some(ref chatbot) = state.chatbot {
        let image = if has_image {
            if let Some(photos) = msg.photo() {
                if let Some(largest) = photos.iter().max_by_key(|p| p.width * p.height) {
                    chatbot.download_image(&largest.file.id.0).await.ok()
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let chat_msg = ChatMessage {
            message_id: msg.id.0 as i64,
            chat_id: msg.chat.id.0,
            user_id: 0,
            username: channel_title.to_string(),
            timestamp: msg.date.format("%Y-%m-%d %H:%M").to_string(),
            text: text.unwrap_or("").to_string(),
            reply_to: None,
            image,
            voice_transcription: None,
            documents: vec![],
        };
        chatbot.handle_message(chat_msg).await;
    }

    Ok(())
}

fn telegram_to_chat_message_with_media(
    msg: &Message,
    image: Option<(Vec<u8>, String)>,
    voice_transcription: Option<String>,
    documents: Vec<DocumentContent>,
) -> ChatMessage {
    let user = msg.from.as_ref();
    let user_id = user.map(|u| u.id.0 as i64).unwrap_or(0);
    let username = user
        .and_then(|u| u.username.as_deref())
        .unwrap_or_else(|| user.map(|u| u.first_name.as_str()).unwrap_or("unknown"))
        .to_string();

    let timestamp = msg.date.format("%Y-%m-%d %H:%M").to_string();
    // Use text, or caption (for images/voice), or empty
    let text = msg.text()
        .or_else(|| msg.caption())
        .unwrap_or("")
        .to_string();

    let reply_to = msg.reply_to_message().map(|reply| {
        let reply_user = reply.from.as_ref();
        let reply_username = reply_user
            .and_then(|u| u.username.as_deref())
            .unwrap_or_else(|| reply_user.map(|u| u.first_name.as_str()).unwrap_or("unknown"))
            .to_string();

        ReplyTo {
            message_id: reply.id.0 as i64,
            username: reply_username,
            text: reply.text().unwrap_or("").to_string(),
        }
    });

    ChatMessage {
        message_id: msg.id.0 as i64,
        chat_id: msg.chat.id.0,
        user_id,
        username,
        timestamp,
        text,
        reply_to,
        image,
        voice_transcription,
        documents,
    }
}

/// Download and extract text from document attachments (.docx files).
async fn extract_documents(bot: &Bot, msg: &Message) -> Vec<DocumentContent> {
    use chatbot::docx;
    use teloxide::net::Download;

    let doc = match msg.document() {
        Some(d) => d,
        None => return vec![],
    };

    // Only process .docx files
    let filename = doc.file_name.as_deref().unwrap_or("document");
    if !filename.to_lowercase().ends_with(".docx") {
        info!("📄 Skipping non-docx document: {}", filename);
        return vec![];
    }

    info!("📄 Processing document: {}", filename);

    // Download the file
    let file = match bot.get_file(doc.file.id.clone()).await {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to get document file info: {}", e);
            return vec![DocumentContent {
                filename: filename.to_string(),
                text: format!("[Document download failed: {}]", e),
            }];
        }
    };

    let mut data = Vec::new();
    if let Err(e) = bot.download_file(&file.path, &mut data).await {
        warn!("Failed to download document: {}", e);
        return vec![DocumentContent {
            filename: filename.to_string(),
            text: format!("[Document download failed: {}]", e),
        }];
    }

    info!("📥 Downloaded document ({} bytes)", data.len());

    // Extract text from docx
    match docx::extract_text(&data) {
        Ok(text) => {
            let preview = docx::preview(&text, 100);
            info!("📝 Extracted text: \"{}\"", preview);
            vec![DocumentContent {
                filename: filename.to_string(),
                text,
            }]
        }
        Err(e) => {
            warn!("Document extraction failed: {}", e);
            vec![DocumentContent {
                filename: filename.to_string(),
                text: format!("[Document extraction failed: {}]", e),
            }]
        }
    }
}

/// Download and transcribe a voice message if present.
/// Returns the transcription, or an error message if transcription failed.
async fn transcribe_voice(bot: &Bot, state: &BotState, msg: &Message) -> Option<String> {
    use teloxide::net::Download;

    let voice = msg.voice()?;

    let whisper = match state.whisper.as_ref() {
        Some(w) => w,
        None => {
            warn!("Voice message received but Whisper not configured");
            return Some("[Voice message - transcription not available (Whisper not configured)]".to_string());
        }
    };

    info!("🎤 Voice message from user {} ({} seconds)",
          msg.from.as_ref().map(|u| u.id.0).unwrap_or(0),
          voice.duration);

    // Download voice file
    let file = match bot.get_file(voice.file.id.clone()).await {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to get voice file info: {}", e);
            return Some(format!("[Voice message - download failed: {}]", e));
        }
    };

    let mut data = Vec::new();
    if let Err(e) = bot.download_file(&file.path, &mut data).await {
        warn!("Failed to download voice file: {}", e);
        return Some(format!("[Voice message - download failed: {}]", e));
    }

    info!("📥 Downloaded voice ({} bytes)", data.len());

    // Transcribe
    match whisper.transcribe(&data) {
        Ok(text) => {
            let preview: String = text.chars().take(100).collect();
            info!("📝 Transcribed: \"{}\"", preview);
            Some(text)
        }
        Err(e) => {
            warn!("Transcription failed: {}", e);
            Some(format!("[Voice message - transcription failed: {}]", e))
        }
    }
}

async fn handle_edited_message(msg: Message, state: Arc<BotState>) -> ResponseResult<()> {
    let is_group = matches!(msg.chat.kind, ChatKind::Public(_));
    if !is_group {
        return Ok(());
    }

    if !state.config.allowed_groups.is_empty()
        && !state.config.allowed_groups.contains(&msg.chat.id)
    {
        return Ok(());
    }

    let text = match msg.text() {
        Some(t) => t,
        None => return Ok(()),
    };

    if let Some(ref chatbot) = state.chatbot {
        chatbot.handle_edit(msg.id.0 as i64, text).await;
    }

    Ok(())
}

async fn handle_chat_member(update: teloxide::types::ChatMemberUpdated, state: Arc<BotState>) -> ResponseResult<()> {
    // Only track for allowed groups
    if !state.config.allowed_groups.is_empty()
        && !state.config.allowed_groups.contains(&update.chat.id)
    {
        return Ok(());
    }

    let Some(ref chatbot) = state.chatbot else {
        return Ok(());
    };

    let user = &update.new_chat_member.user;
    let user_id = user.id.0 as i64;
    let username = user.username.clone();
    let first_name = user.first_name.clone();

    use teloxide::types::ChatMemberStatus;
    match update.new_chat_member.status() {
        ChatMemberStatus::Member | ChatMemberStatus::Administrator | ChatMemberStatus::Owner => {
            // User joined or was added
            if matches!(update.old_chat_member.status(), ChatMemberStatus::Left | ChatMemberStatus::Banned) {
                info!("👋 Member joined: {} ({})", first_name, user_id);
                chatbot.handle_member_joined(user_id, username, first_name).await;
            }
        }
        ChatMemberStatus::Left => {
            info!("👋 Member left: {} ({})", first_name, user_id);
            chatbot.handle_member_left(user_id).await;
        }
        ChatMemberStatus::Banned => {
            info!("🚫 Member banned: {} ({})", first_name, user_id);
            chatbot.handle_member_banned(user_id).await;
        }
        _ => {}
    }

    Ok(())
}
