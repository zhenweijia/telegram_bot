use log::info;

#[cfg(feature = "lambda")]
use log::warn;
use once_cell::sync::OnceCell;
use teloxide::RequestError;
use teloxide::requests::Requester;
use teloxide::{prelude::*, utils::command::BotCommands};

#[cfg(feature = "lambda")]
use lambda_runtime::{Error as LambdaError, LambdaEvent};
#[cfg(feature = "lambda")]
use serde_json::Value;

use crate::commands::{Command, answer};

static BOT_USERNAME: OnceCell<String> = OnceCell::new();

async fn resolve_bot_username(bot: &Bot) -> Result<&str, RequestError> {
    if let Some(username) = BOT_USERNAME.get() {
        return Ok(username.as_str());
    }

    let bot_user = bot.get_me().await?;
    let username = bot_user
        .username
        .clone()
        .unwrap_or_else(|| "bot".to_string());

    let _ = BOT_USERNAME.set(username);

    Ok(BOT_USERNAME
        .get()
        .expect("bot username should be set")
        .as_str())
}

pub async fn handle_message(bot: Bot, msg: Message) -> ResponseResult<()> {
    if let Some(text) = msg.text() {
        // Get bot info to use the correct username for command parsing
        let bot_username = resolve_bot_username(&bot).await?;

        info!("📝 Processing message: '{text}' with bot username: @{bot_username}");

        // Check if bot is mentioned in the message
        let bot_mention = format!("@{bot_username}");
        let is_private_chat = msg.chat.is_private();
        let is_mentioned = text.contains(&bot_mention);

        info!(
            "💬 Chat type: {}, Bot mentioned: {}",
            if is_private_chat { "Private" } else { "Group" },
            is_mentioned
        );

        // Process message if it's a private chat OR bot is mentioned in group
        if is_private_chat || is_mentioned {
            let processed_text = if is_mentioned {
                // Remove bot mention and clean up the text
                let cleaned = text.replace(&bot_mention, "").trim().to_string();
                info!("🧽 Cleaned text after removing mention: '{cleaned}'");
                cleaned
            } else {
                text.to_string()
            };

            // Try to parse as command first
            if let Ok(cmd) = Command::parse(&processed_text, "") {
                info!("✅ Command parsed successfully: {cmd:?}");
                answer(bot, msg, cmd).await?;
            } else if processed_text.starts_with('/') {
                // If it starts with '/' but couldn't parse, it's an unknown command
                info!("❌ Unknown command: '{processed_text}'");
                let response = format!(
                    "Unknown command: {}\n\nAvailable commands:\n{}",
                    processed_text,
                    Command::descriptions()
                );
                bot.send_message(msg.chat.id, response).await?;
            } else if !processed_text.trim().is_empty() {
                // Not a command, treat as general AI chat (default behavior)
                info!(
                    "🤖 No command detected - defaulting to /general for message: '{processed_text}'"
                );
                info!("🔄 Converting to Command::General");
                answer(bot, msg, Command::General(processed_text)).await?;
            } else {
                // Empty message after mention removal
                info!("🙄 Empty message after processing mention");
                let response = if is_private_chat {
                    format!(
                        "Hello! Send me a command or message.\n\n{}",
                        Command::descriptions()
                    )
                } else {
                    format!(
                        "Hello! You mentioned me. Send a command or message after @{}.\n\n{}",
                        bot_username,
                        Command::descriptions()
                    )
                };
                bot.send_message(msg.chat.id, response).await?;
            }
        } else {
            // In group chat but bot not mentioned - ignore
            info!("😶 Group message without bot mention - ignoring");
        }
    } else {
        info!("📷 Received non-text message");
    }
    Ok(())
}

#[cfg(feature = "lambda")]
pub async fn lambda_handler(event: LambdaEvent<Value>) -> Result<Value, LambdaError> {
    info!("🔗 Lambda received event: {:?}", event.payload);

    let bot = Bot::from_env();

    // Parse the Telegram webhook update from the Lambda event body
    if let Some(body) = event.payload.get("body").and_then(|b| b.as_str()) {
        info!("📦 Extracted body from Lambda event: {body}");

        if let Ok(update) = serde_json::from_str::<teloxide::types::Update>(body) {
            info!("✅ Successfully parsed Telegram update: {:?}", update.id);

            if let teloxide::types::UpdateKind::Message(message) = update.kind {
                let _ = handle_message(bot, message).await;
            } else {
                info!("🔄 Received non-message update in Lambda");
            }
        } else {
            warn!("❌ Failed to parse Telegram update from body: {body}");
        }
    } else {
        warn!("❌ No body field found in Lambda event");
    }

    // Return success response
    Ok(serde_json::json!({
        "statusCode": 200,
        "body": "OK"
    }))
}
