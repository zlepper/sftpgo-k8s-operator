use crate::auth::{create_basic_auth_header, create_bearer_auth_header, AuthContext};
use crate::client::SftpgoClientBase;
use crate::error_response::{handle_response, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::AUTHORIZATION;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Deserialize)]
pub struct AdminAccessToken {
    pub access_token: String,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait AdminAccessTokenClient: SftpgoClientBase {
    async fn create_admin_access_token(
        &self,
        username: &str,
        password: &str,
    ) -> Result<AdminAccessToken> {
        let url = self.url_for("/api/v2/token")?;
        let auth_header_value = create_basic_auth_header(username, password);
        let res = self
            .get_client()
            .get(url)
            .header(AUTHORIZATION, auth_header_value)
            .send()
            .await?;

        handle_response(res).await
    }
}

impl<T> AdminAccessTokenClient for T where T: SftpgoClientBase {}

pub struct RefreshableAdminAuthContext<T>
where
    T: AdminAccessTokenClient + Send + Sync,
{
    username: String,
    password: String,
    client: T,
    token: Arc<RwLock<Option<StoredAccessToken>>>,
}

impl<T> RefreshableAdminAuthContext<T>
where
    T: AdminAccessTokenClient + Sync + Send,
{
    pub fn new(username: String, password: String, client: T) -> Self {
        Self {
            username,
            password,
            client,
            token: Arc::new(RwLock::new(None)),
        }
    }
}

struct StoredAccessToken {
    access_token: String,
    expires_at: DateTime<Utc>,
}

#[async_trait]
impl<T> AuthContext for RefreshableAdminAuthContext<T>
where
    T: AdminAccessTokenClient + Sync + Send,
{
    async fn get_auth_header_value(&self) -> Result<String> {
        {
            // Try to get the existing token purely as read
            let token = self.token.read().await;

            if let Some(token) = &*token {
                if token.expires_at > Utc::now() {
                    return Ok(create_bearer_auth_header(&token.access_token));
                }
            }
        }

        // So the token is expired or doesn't exist, so we need to refresh it
        {
            // Lock access to avoid a stampede
            let mut token = self.token.write().await;

            // Check if another thread already refreshed the token
            if let Some(token) = &*token {
                if token.expires_at > Utc::now() {
                    return Ok(create_bearer_auth_header(&token.access_token));
                }
            }

            // Refresh the token
            let new_token = self
                .client
                .create_admin_access_token(&self.username, &self.password)
                .await?;

            let header_value = create_bearer_auth_header(&new_token.access_token);

            *token = Some(StoredAccessToken {
                access_token: new_token.access_token,
                expires_at: new_token.expires_at - chrono::Duration::seconds(30),
            });

            return Ok(header_value);
        }
    }
}
