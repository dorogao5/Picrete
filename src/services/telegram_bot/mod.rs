use std::collections::HashMap;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::api::validation::validate_image_upload;
use crate::core::state::AppState;
use crate::core::time::primitive_now_utc;
use crate::db::types::SessionStatus;
use crate::repositories;
use crate::services::submission_images::SubmissionImagesService;

const BOT_OFFSET_KEY: &str = "default";
const TELEGRAM_LOGIN_RATE_LIMIT: u64 = 10;
const TELEGRAM_LOGIN_RATE_WINDOW_SECONDS: u64 = 60;

#[derive(Clone)]
pub(crate) struct TelegramBotRuntime {
    state: AppState,
    client: Client,
    token: String,
    login_states: std::sync::Arc<Mutex<HashMap<i64, LoginStep>>>,
}

#[derive(Debug, Clone)]
enum LoginStep {
    AwaitUsername,
    AwaitPassword { username: String },
}

#[derive(Debug, Deserialize)]
struct TgGetUpdatesResponse {
    ok: bool,
    result: Vec<TgUpdate>,
}

#[derive(Debug, Deserialize)]
struct TgUpdate {
    update_id: i64,
    message: Option<TgMessage>,
}

#[derive(Debug, Deserialize)]
struct TgMessage {
    message_id: i64,
    chat: TgChat,
    from: Option<TgUser>,
    text: Option<String>,
    photo: Option<Vec<TgPhotoSize>>,
    document: Option<TgDocument>,
}

#[derive(Debug, Deserialize)]
struct TgChat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
}

#[derive(Debug, Deserialize)]
struct TgUser {
    id: i64,
    username: Option<String>,
    first_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgPhotoSize {
    file_id: String,
}

#[derive(Debug, Deserialize)]
struct TgDocument {
    file_id: String,
    file_name: Option<String>,
    mime_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgGetFileResponse {
    ok: bool,
    result: TgFile,
}

#[derive(Debug, Deserialize)]
struct TgFile {
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TgOkResponse {
    ok: bool,
    description: Option<String>,
}

impl TelegramBotRuntime {
    pub(crate) fn new(state: AppState) -> Self {
        Self {
            token: state.settings().telegram().token.clone(),
            state,
            client: Client::new(),
            login_states: std::sync::Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) async fn run(&self) -> Result<()> {
        if !self.state.settings().telegram().enabled {
            tracing::info!("Telegram bot is disabled, runtime exits");
            return Ok(());
        }

        if self.token.is_empty() {
            return Err(anyhow!("TG_TOKEN is empty while TELEGRAM_BOT_ENABLED=true"));
        }

        tracing::info!("Telegram bot runtime started");

        let mut offset =
            repositories::telegram_offsets::get_update_offset(self.state.db(), BOT_OFFSET_KEY)
                .await
                .context("Failed to load persisted Telegram updates offset")?
                .unwrap_or(0);

        loop {
            let updates = match self.get_updates(offset).await {
                Ok(updates) => updates,
                Err(error) => {
                    tracing::error!(error = %error, "Failed to fetch Telegram updates");
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
            };

            for update in updates {
                offset = update.update_id + 1;
                if let Some(message) = update.message {
                    if let Err(error) = self.handle_message(message).await {
                        tracing::error!(error = %error, "Failed to handle Telegram message");
                    }
                }
                if let Err(error) = repositories::telegram_offsets::upsert_update_offset(
                    self.state.db(),
                    BOT_OFFSET_KEY,
                    offset,
                    primitive_now_utc(),
                )
                .await
                {
                    tracing::error!(
                        error = %error,
                        offset,
                        "Failed to persist Telegram updates offset"
                    );
                }
            }
        }
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<TgUpdate>> {
        let timeout = self.state.settings().telegram().poll_timeout_seconds;
        let response = self
            .client
            .get(format!("https://api.telegram.org/bot{}/getUpdates", self.token))
            .query(&[("timeout", timeout.to_string()), ("offset", offset.to_string())])
            .send()
            .await
            .context("Telegram getUpdates request failed")?;

        let parsed: TgGetUpdatesResponse =
            response.json().await.context("Failed to decode Telegram getUpdates payload")?;

        if !parsed.ok {
            return Err(anyhow!("Telegram API returned ok=false for getUpdates"));
        }

        Ok(parsed.result)
    }

    async fn handle_message(&self, message: TgMessage) -> Result<()> {
        let Some(ref from) = message.from else {
            return Ok(());
        };

        let chat_id = message.chat.id;
        let telegram_user_id = from.id;
        let is_private_chat = message.chat.chat_type == "private";

        if !is_private_chat {
            let has_pending_login = self.login_states.lock().await.contains_key(&from.id);
            let has_sensitive_payload = message.photo.is_some()
                || message.document.is_some()
                || message.text.as_deref().map(|text| text.starts_with('/')).unwrap_or(false)
                || has_pending_login;

            if has_sensitive_payload {
                self.send_message(
                    chat_id,
                    "Для безопасности бот принимает команды, логин и фото только в личном чате.",
                )
                .await?;
            }
            return Ok(());
        }

        if let Some(text) = message.text.as_deref() {
            if text.starts_with("/") {
                return self.handle_command(chat_id, &from, text).await;
            }

            if self.handle_login_step(chat_id, &from, text, message.message_id).await? {
                return Ok(());
            }
        }

        if message.photo.is_some() || message.document.is_some() {
            self.handle_image_upload(chat_id, &from, &message).await?;
            return Ok(());
        }

        if repositories::telegram_links::find_by_telegram_user_id(self.state.db(), telegram_user_id)
            .await
            .ok()
            .flatten()
            .is_none()
        {
            self.send_message(
                chat_id,
                "Вы не авторизованы. Отправьте /login и введите логин/пароль Picrete.",
            )
            .await?;
        }

        Ok(())
    }

    async fn handle_command(&self, chat_id: i64, from: &TgUser, raw_command: &str) -> Result<()> {
        let command = raw_command.trim();

        if command.starts_with("/start") {
            self.send_message(
                chat_id,
                "Бот Picrete: /login, /works, /use <номер|session_id>, /logout. После /use присылайте фото.",
            )
            .await?;
            return Ok(());
        }

        if command.starts_with("/login") {
            if !self
                .check_rate_limit(
                    &format!("rl:tg:login:init:{}", from.id),
                    TELEGRAM_LOGIN_RATE_LIMIT,
                    TELEGRAM_LOGIN_RATE_WINDOW_SECONDS,
                )
                .await
            {
                self.send_message(
                    chat_id,
                    "Слишком много попыток входа. Подождите минуту и повторите /login.",
                )
                .await?;
                return Ok(());
            }
            self.login_states.lock().await.insert(from.id, LoginStep::AwaitUsername);
            self.send_message(chat_id, "Введите username от Picrete:").await?;
            return Ok(());
        }

        if command.starts_with("/logout") {
            repositories::telegram_sessions::clear_selected(self.state.db(), from.id).await.ok();
            repositories::telegram_links::delete_by_telegram_user_id(self.state.db(), from.id)
                .await
                .ok();
            self.login_states.lock().await.remove(&from.id);
            self.send_message(chat_id, "Вы вышли из бота.").await?;
            return Ok(());
        }

        if command.starts_with("/works") {
            let Some(link) =
                repositories::telegram_links::find_by_telegram_user_id(self.state.db(), from.id)
                    .await
                    .context("Failed to load telegram link")?
            else {
                self.send_message(chat_id, "Сначала авторизуйтесь через /login.").await?;
                return Ok(());
            };

            let sessions =
                repositories::sessions::list_active_by_student(self.state.db(), &link.user_id)
                    .await
                    .context("Failed to list active sessions")?;

            if sessions.is_empty() {
                self.send_message(
                    chat_id,
                    "Активных работ нет. Сначала начните работу на сайте Picrete, затем повторите /works.",
                )
                .await?;
                return Ok(());
            }

            let mut lines = Vec::with_capacity(sessions.len() + 1);
            lines.push("Активные работы:".to_string());
            for (idx, session) in sessions.iter().enumerate() {
                lines.push(format!("{}. {}", idx + 1, escape_html(&session.exam_title)));
                lines.push(format!("<code>/use {}</code>", escape_html(&session.id)));
                if idx + 1 < sessions.len() {
                    lines.push(String::new());
                }
            }
            self.send_message_html(chat_id, &lines.join("\n")).await?;
            return Ok(());
        }

        if command.starts_with("/use") {
            let Some(link) =
                repositories::telegram_links::find_by_telegram_user_id(self.state.db(), from.id)
                    .await
                    .context("Failed to load telegram link")?
            else {
                self.send_message(chat_id, "Сначала авторизуйтесь через /login.").await?;
                return Ok(());
            };

            let parts = command.split_whitespace().collect::<Vec<_>>();
            if parts.len() < 2 {
                self.send_message(chat_id, "Использование: /use <номер|session_id>").await?;
                return Ok(());
            }

            let sessions =
                repositories::sessions::list_active_by_student(self.state.db(), &link.user_id)
                    .await
                    .context("Failed to list active sessions")?;

            if sessions.is_empty() {
                self.send_message(
                    chat_id,
                    "Нет доступных активных работ. Сначала начните работу на сайте.",
                )
                .await?;
                return Ok(());
            }

            let selected = if let Ok(index) = parts[1].parse::<usize>() {
                sessions.get(index.saturating_sub(1))
            } else {
                sessions.iter().find(|item| item.id == parts[1])
            };

            let Some(session) = selected else {
                self.send_message(
                    chat_id,
                    "Работа не найдена. Выполните /works и выберите корректный номер.",
                )
                .await?;
                return Ok(());
            };

            repositories::telegram_sessions::upsert_selected(
                self.state.db(),
                from.id,
                &session.course_id,
                &session.id,
                primitive_now_utc(),
            )
            .await
            .context("Failed to persist selected session")?;

            self.send_message(
                chat_id,
                &format!(
                    "Выбрана работа: {} (session={}). Теперь можно присылать фото.",
                    session.exam_title, session.id
                ),
            )
            .await?;
            return Ok(());
        }

        self.send_message(chat_id, "Неизвестная команда. Доступно: /login, /works, /use, /logout")
            .await?;
        Ok(())
    }

    async fn handle_login_step(
        &self,
        chat_id: i64,
        from: &TgUser,
        text: &str,
        message_id: i64,
    ) -> Result<bool> {
        let step = self.login_states.lock().await.get(&from.id).cloned();
        let Some(step) = step else {
            return Ok(false);
        };

        match step {
            LoginStep::AwaitUsername => {
                self.login_states.lock().await.insert(
                    from.id,
                    LoginStep::AwaitPassword { username: text.trim().to_string() },
                );
                self.send_message(
                    chat_id,
                    "Введите пароль от Picrete:\nсообщение с паролем будет удалено сразу после проверки.",
                )
                .await?;
                Ok(true)
            }
            LoginStep::AwaitPassword { username } => {
                self.try_delete_message(chat_id, message_id).await;

                if !self
                    .check_rate_limit(
                        &format!("rl:tg:login:attempt:{}", from.id),
                        TELEGRAM_LOGIN_RATE_LIMIT,
                        TELEGRAM_LOGIN_RATE_WINDOW_SECONDS,
                    )
                    .await
                {
                    metrics::counter!("telegram_auth_fail_total").increment(1);
                    self.login_states.lock().await.remove(&from.id);
                    self.send_message(
                        chat_id,
                        "Слишком много попыток входа. Подождите минуту и повторите /login.",
                    )
                    .await?;
                    return Ok(true);
                }

                let user = repositories::users::find_by_username(self.state.db(), &username)
                    .await
                    .context("Failed to fetch user for telegram login")?;

                let Some(user) = user else {
                    metrics::counter!("telegram_auth_fail_total").increment(1);
                    self.login_states.lock().await.remove(&from.id);
                    self.send_message(chat_id, "Неверный логин или пароль. Повторите /login.")
                        .await?;
                    return Ok(true);
                };

                let valid =
                    crate::core::security::verify_password(text.trim(), &user.hashed_password)
                        .unwrap_or(false);

                if !valid || !user.is_active {
                    metrics::counter!("telegram_auth_fail_total").increment(1);
                    self.login_states.lock().await.remove(&from.id);
                    self.send_message(chat_id, "Неверный логин или пароль. Повторите /login.")
                        .await?;
                    return Ok(true);
                }

                repositories::telegram_links::upsert_link(
                    self.state.db(),
                    from.id,
                    &user.id,
                    from.username.as_deref(),
                    from.first_name.as_deref(),
                    primitive_now_utc(),
                )
                .await
                .context("Failed to save telegram link")?;

                self.login_states.lock().await.remove(&from.id);
                self.send_message(
                    chat_id,
                    "Авторизация успешна. Выполните /works и выберите работу командой /use.",
                )
                .await?;
                Ok(true)
            }
        }
    }

    async fn handle_image_upload(
        &self,
        chat_id: i64,
        from: &TgUser,
        message: &TgMessage,
    ) -> Result<()> {
        let Some(link) =
            repositories::telegram_links::find_by_telegram_user_id(self.state.db(), from.id)
                .await
                .context("Failed to load linked user")?
        else {
            self.send_message(chat_id, "Сначала авторизуйтесь через /login.").await?;
            return Ok(());
        };

        repositories::telegram_links::touch_last_seen(
            self.state.db(),
            from.id,
            primitive_now_utc(),
        )
        .await
        .ok();

        let Some(selected) =
            repositories::telegram_sessions::get_selected(self.state.db(), from.id)
                .await
                .context("Failed to load selected session")?
        else {
            self.send_message(chat_id, "Сначала выберите работу: /works и затем /use <номер>.")
                .await?;
            return Ok(());
        };

        let Some(session) = repositories::sessions::find_by_id(
            self.state.db(),
            &selected.course_id,
            &selected.session_id,
        )
        .await
        .context("Failed to fetch selected session")?
        else {
            self.send_message(
                chat_id,
                "Выбранная работа больше недоступна. Выполните /works снова.",
            )
            .await?;
            repositories::telegram_sessions::clear_selected(self.state.db(), from.id).await.ok();
            return Ok(());
        };

        if session.student_id != link.user_id {
            self.send_message(chat_id, "Сессия не принадлежит вашему аккаунту.").await?;
            return Ok(());
        }

        let (_, status, _) =
            crate::api::submissions::helpers::enforce_deadline(&session, self.state.db())
                .await
                .map_err(|e| anyhow!("Failed to enforce session deadline: {e:?}"))?;

        if status != SessionStatus::Active {
            self.send_message(chat_id, "Сессия уже завершена, загрузка недоступна.").await?;
            return Ok(());
        }

        let (file_id, filename, mime_type) = extract_image_payload(message)?;
        validate_image_upload(
            &filename,
            &mime_type,
            &self.state.settings().storage().allowed_image_extensions,
        )
        .map_err(|e| anyhow!("Invalid image: {e:?}"))?;

        let bytes = self.download_file_bytes(&file_id).await?;
        let max_bytes = self.state.settings().storage().max_upload_size_mb * 1024 * 1024;
        if bytes.len() as u64 > max_bytes {
            self.send_message(
                chat_id,
                &format!(
                    "Файл слишком большой: лимит {}MB",
                    self.state.settings().storage().max_upload_size_mb
                ),
            )
            .await?;
            return Ok(());
        }

        let uploaded = SubmissionImagesService::upload_from_telegram(
            &self.state,
            &session,
            &filename,
            &mime_type,
            bytes,
        )
        .await;

        match uploaded {
            Ok(image) => {
                self.send_message(chat_id, &format!("Фото загружено (#{})", image.order_index + 1))
                    .await?;
            }
            Err(error) => {
                self.send_message(chat_id, &format!("Не удалось загрузить фото: {}", error))
                    .await?;
            }
        }

        Ok(())
    }

    async fn download_file_bytes(&self, file_id: &str) -> Result<Vec<u8>> {
        let file_info = self
            .client
            .get(format!("https://api.telegram.org/bot{}/getFile", self.token))
            .query(&[("file_id", file_id)])
            .send()
            .await
            .context("Telegram getFile request failed")?;

        let payload: TgGetFileResponse =
            file_info.json().await.context("Failed to decode Telegram getFile payload")?;

        if !payload.ok {
            return Err(anyhow!("Telegram getFile returned ok=false"));
        }

        let file_path = payload
            .result
            .file_path
            .ok_or_else(|| anyhow!("Telegram getFile missing file_path"))?;

        let response = self
            .client
            .get(format!("https://api.telegram.org/file/bot{}/{}", self.token, file_path))
            .send()
            .await
            .context("Failed to download Telegram file")?;

        let bytes = response.bytes().await.context("Failed to read Telegram file bytes")?;
        Ok(bytes.to_vec())
    }

    async fn check_rate_limit(&self, key: &str, limit: u64, window_seconds: u64) -> bool {
        match self.state.redis().rate_limit(key, limit, window_seconds).await {
            Ok(value) => value,
            Err(error) => {
                tracing::error!(error = %error, rate_limit_key = key, "Failed to check Telegram rate limit");
                false
            }
        }
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<()> {
        self.client
            .post(format!("https://api.telegram.org/bot{}/sendMessage", self.token))
            .json(&json!({
                "chat_id": chat_id,
                "text": text,
            }))
            .send()
            .await
            .context("Failed to send Telegram message")?;
        Ok(())
    }

    async fn send_message_html(&self, chat_id: i64, text: &str) -> Result<()> {
        self.client
            .post(format!("https://api.telegram.org/bot{}/sendMessage", self.token))
            .json(&json!({
                "chat_id": chat_id,
                "text": text,
                "parse_mode": "HTML",
                "disable_web_page_preview": true,
            }))
            .send()
            .await
            .context("Failed to send Telegram HTML message")?;
        Ok(())
    }

    async fn try_delete_message(&self, chat_id: i64, message_id: i64) {
        if let Err(error) = self.delete_message(chat_id, message_id).await {
            tracing::warn!(
                error = %error,
                chat_id,
                message_id,
                "Failed to delete Telegram message with sensitive input"
            );
        }
    }

    async fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<()> {
        let response = self
            .client
            .post(format!("https://api.telegram.org/bot{}/deleteMessage", self.token))
            .json(&json!({
                "chat_id": chat_id,
                "message_id": message_id,
            }))
            .send()
            .await
            .context("Failed to request Telegram message deletion")?;

        let payload: TgOkResponse =
            response.json().await.context("Failed to decode Telegram deleteMessage payload")?;

        if payload.ok {
            return Ok(());
        }

        let description =
            payload.description.unwrap_or_else(|| "unknown Telegram API error".to_string());
        Err(anyhow!("Telegram deleteMessage returned ok=false: {description}"))
    }
}

fn escape_html(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn extract_image_payload(message: &TgMessage) -> Result<(String, String, String)> {
    if let Some(photos) = &message.photo {
        if let Some(last) = photos.last() {
            let timestamp = time::OffsetDateTime::now_utc().unix_timestamp();
            return Ok((
                last.file_id.clone(),
                format!("telegram_{timestamp}.jpg"),
                "image/jpeg".to_string(),
            ));
        }
    }

    if let Some(document) = &message.document {
        let mime =
            document.mime_type.clone().unwrap_or_else(|| "application/octet-stream".to_string());
        if !mime.starts_with("image/") {
            return Err(anyhow!("Only image documents are supported"));
        }

        let filename = document.file_name.clone().unwrap_or_else(|| {
            let timestamp = time::OffsetDateTime::now_utc().unix_timestamp();
            format!("telegram_{timestamp}.jpg")
        });
        return Ok((document.file_id.clone(), filename, mime));
    }

    Err(anyhow!("Message does not contain an image"))
}

pub(crate) async fn run(state: AppState) -> Result<()> {
    TelegramBotRuntime::new(state).run().await
}
