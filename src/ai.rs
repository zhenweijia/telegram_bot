use async_trait::async_trait;
use async_openai::{
    types::{ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs},
    Client,
};
use log::{info, warn};
use std::error::Error;
use crate::storage::{create_storage, get_default_model};

// Extensible AI backend trait
#[async_trait]
pub trait AiBackend: Send + Sync {
    async fn chat(&self, message: &str) -> Result<String, Box<dyn Error + Send + Sync>>;
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
}

// OpenAI ChatGPT implementation using async-openai SDK
pub struct OpenAiBackend {
    client: Client<async_openai::config::OpenAIConfig>,
    model: String,
}

impl OpenAiBackend {
    pub fn new(api_key: String, model: String) -> Self {
        let client = Client::with_config(async_openai::config::OpenAIConfig::new().with_api_key(api_key));
        Self { client, model }
    }
}

#[async_trait]
impl AiBackend for OpenAiBackend {
    async fn chat(&self, message: &str) -> Result<String, Box<dyn Error + Send + Sync>> {
        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .max_tokens(500u32)
            .messages(vec![
                ChatCompletionRequestUserMessageArgs::default()
                    .content(message)
                    .build()?
                    .into()
            ])
            .build()?;

        let response = self.client.chat().create(request).await?;

        if let Some(choice) = response.choices.first() {
            if let Some(content) = &choice.message.content {
                Ok(content.trim().to_string())
            } else {
                Err("No content in OpenAI response".into())
            }
        } else {
            Err("No response from OpenAI".into())
        }
    }

    fn name(&self) -> &'static str {
        "OpenAI ChatGPT"
    }
}

// Available OpenAI models
pub fn get_available_models() -> Vec<String> {
    vec![
        "gpt-4o".to_string(),
        "gpt-4o-mini".to_string(),
        "gpt-4".to_string(),
        "gpt-4-turbo".to_string(),
        "gpt-3.5-turbo".to_string(),
    ]
}

// Get current model for a specific chat from DynamoDB
pub async fn get_current_model(chat_id: &str) -> String {
    info!("🔍 Getting current model for chat_id: {chat_id}");
    
    match create_storage().await {
        Ok(storage) => {
            match storage.get_user_model(chat_id).await {
                Ok(Some(model)) => {
                    info!("✅ Found user model preference: {model}");
                    model
                }
                Ok(None) => {
                    let default = get_default_model();
                    info!("🎯 Using default model for new user: {default}");
                    default
                }
                Err(e) => {
                    warn!("⚠️ Failed to get user model from storage: {e}");
                    let default = get_default_model();
                    warn!("🎯 Fallback to default model: {default}");
                    default
                }
            }
        }
        Err(e) => {
            warn!("⚠️ Failed to create storage client: {e}");
            let default = get_default_model();
            warn!("🎯 Fallback to default model: {default}");
            default
        }
    }
}

// Set current model for a specific chat in DynamoDB
pub async fn set_current_model(chat_id: &str, model: String) -> Result<(), Box<dyn Error + Send + Sync>> {
    info!("💾 Setting model for chat_id {chat_id} to: {model}");
    
    let storage = create_storage().await?;
    storage.set_user_model(chat_id, &model).await?;
    
    info!("✅ Successfully saved model preference");
    Ok(())
}

// AI Backend factory with configurable model
pub fn create_ai_backend_with_model(model: &str) -> Result<Box<dyn AiBackend>, Box<dyn Error + Send + Sync>> {
    if let Ok(api_key) = std::env::var("OPENAI_API_KEY") {
        Ok(Box::new(OpenAiBackend::new(api_key, model.to_string())))
    } else {
        Err("OPENAI_API_KEY environment variable not set".into())
    }
}

