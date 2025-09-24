use aws_config::BehaviorVersion;
use aws_sdk_dynamodb::{Client as DynamoDbClient, Error as DynamoDbError};
use log::{info, warn};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::fmt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserPreferences {
    pub chat_id: String,
    pub ai_model: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>, // TTL field (Unix timestamp)
}

impl UserPreferences {
    pub fn new(chat_id: String, ai_model: String) -> Self {
        let now = chrono::Utc::now();
        let expires_at = now.timestamp() + (365 * 24 * 60 * 60); // 1 year from now

        Self {
            chat_id,
            ai_model,
            updated_at: now.to_rfc3339(),
            expires_at: Some(expires_at),
        }
    }
}

#[derive(Debug)]
pub enum StorageError {
    DynamoDb(DynamoDbError),
    Configuration(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::DynamoDb(e) => write!(f, "DynamoDB error: {e}"),
            StorageError::Configuration(e) => write!(f, "Configuration error: {e}"),
        }
    }
}

impl Error for StorageError {}

impl From<DynamoDbError> for StorageError {
    fn from(error: DynamoDbError) -> Self {
        StorageError::DynamoDb(error)
    }
}

pub struct DynamoDbStorage {
    client: DynamoDbClient,
    table_name: String,
}

impl DynamoDbStorage {
    pub async fn new() -> Result<Self, StorageError> {
        let table_name = std::env::var("DYNAMODB_TABLE_NAME").map_err(|_| {
            StorageError::Configuration(
                "DYNAMODB_TABLE_NAME environment variable not set".to_string(),
            )
        })?;

        let config = aws_config::defaults(BehaviorVersion::v2025_01_17())
            .load()
            .await;

        let client = DynamoDbClient::new(&config);

        info!("🗃️ DynamoDB client initialized for table: {table_name}");

        Ok(Self { client, table_name })
    }

    pub async fn get_user_model(&self, chat_id: &str) -> Result<Option<String>, StorageError> {
        info!("📖 Getting model preference for chat_id: {chat_id}");

        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key(
                "chat_id",
                aws_sdk_dynamodb::types::AttributeValue::S(chat_id.to_string()),
            )
            .send()
            .await
            .map_err(|e| StorageError::DynamoDb(DynamoDbError::from(e)))?;

        match result.item {
            Some(item) => {
                if let Some(model_attr) = item.get("ai_model") {
                    if let Some(model) = model_attr.as_s().ok() {
                        info!("✅ Found model preference for {chat_id}: {model}");
                        return Ok(Some(model.clone()));
                    }
                }
                warn!("⚠️ Invalid model data format for chat_id: {chat_id}");
                Ok(None)
            }
            None => {
                info!("🔍 No model preference found for chat_id: {chat_id}");
                Ok(None)
            }
        }
    }

    pub async fn set_user_model(&self, chat_id: &str, model: &str) -> Result<(), StorageError> {
        info!("💾 Setting model preference for chat_id {chat_id} to: {model}");

        let preferences = UserPreferences::new(chat_id.to_string(), model.to_string());

        let mut item = HashMap::new();
        item.insert(
            "chat_id".to_string(),
            aws_sdk_dynamodb::types::AttributeValue::S(preferences.chat_id),
        );
        item.insert(
            "ai_model".to_string(),
            aws_sdk_dynamodb::types::AttributeValue::S(preferences.ai_model),
        );
        item.insert(
            "updated_at".to_string(),
            aws_sdk_dynamodb::types::AttributeValue::S(preferences.updated_at),
        );

        if let Some(expires_at) = preferences.expires_at {
            item.insert(
                "expires_at".to_string(),
                aws_sdk_dynamodb::types::AttributeValue::N(expires_at.to_string()),
            );
        }

        self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| StorageError::DynamoDb(DynamoDbError::from(e)))?;

        info!("✅ Successfully saved model preference for chat_id: {chat_id}");
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn list_all_preferences(&self) -> Result<Vec<UserPreferences>, StorageError> {
        info!("📋 Listing all user preferences");

        let result = self
            .client
            .scan()
            .table_name(&self.table_name)
            .send()
            .await
            .map_err(|e| StorageError::DynamoDb(DynamoDbError::from(e)))?;

        let mut preferences = Vec::new();

        if let Some(items) = result.items {
            for item in items {
                if let (Some(chat_id), Some(model), Some(updated_at)) = (
                    item.get("chat_id").and_then(|v| v.as_s().ok()),
                    item.get("ai_model").and_then(|v| v.as_s().ok()),
                    item.get("updated_at").and_then(|v| v.as_s().ok()),
                ) {
                    let expires_at = item
                        .get("expires_at")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|s| s.parse::<i64>().ok());

                    preferences.push(UserPreferences {
                        chat_id: chat_id.clone(),
                        ai_model: model.clone(),
                        updated_at: updated_at.clone(),
                        expires_at,
                    });
                }
            }
        }

        info!("📊 Found {} user preferences", preferences.len());
        Ok(preferences)
    }
}

// Factory function to create storage client
static STORAGE: OnceCell<DynamoDbStorage> = OnceCell::new();

pub async fn create_storage() -> Result<&'static DynamoDbStorage, StorageError> {
    if let Some(storage) = STORAGE.get() {
        return Ok(storage);
    }

    let storage = DynamoDbStorage::new().await?;
    let _ = STORAGE.set(storage);

    STORAGE.get().ok_or_else(|| {
        StorageError::Configuration("DynamoDB storage failed to initialize".to_string())
    })
}

// Helper function to get default model
pub fn get_default_model() -> String {
    std::env::var("AI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string())
}
