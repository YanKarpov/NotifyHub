use sqlx::PgPool;
use chrono::{Utc, Duration};
use tokio::time::{sleep, Duration as TokioDuration};
use crate::handlers::Message;
use async_trait::async_trait;
use tokio::sync::broadcast::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{info, error};

#[derive(Debug, Clone)]
pub struct DbMessage {
    pub id: i32,
    pub provider: String,
    pub text: String,
    pub status: String,
    pub retry_count: i32,
    pub next_retry_at: Option<chrono::NaiveDateTime>,
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn send(&self, msg: &Message) -> Result<(), String>;
}

#[derive(Clone)]
pub struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn send(&self, msg: &Message) -> Result<(), String> {
        info!("Mock send: provider={} text={}", msg.provider, msg.text);
        Ok(())
    }
}

pub async fn worker_loop_with_counter(
    worker_id: usize,
    db_pool: PgPool,
    provider: impl Provider + Clone + 'static,
    tx: Sender<String>,
    counter: Arc<AtomicUsize>,
) {
    loop {
        // Получаем следующее сообщение с пометкой "pending" или "retrying"
        let maybe_msg = sqlx::query_as!(
            DbMessage,
            r#"
            SELECT id, provider, text, status, retry_count, next_retry_at
            FROM messages
            WHERE (status = 'pending' OR status = 'retrying')
              AND (next_retry_at IS NULL OR next_retry_at <= $1)
            ORDER BY id
            LIMIT 1
            FOR UPDATE SKIP LOCKED
            "#,
            Utc::now().naive_utc()
        )
        .fetch_optional(&db_pool)
        .await;

        match maybe_msg {
            Ok(Some(msg)) => {
                info!("Worker #{}: processing message id={}", worker_id, msg.id);

                let send_result = provider
                    .send(&Message {
                        provider: msg.provider.clone(),
                        text: msg.text.clone(),
                        scheduled_at: msg.next_retry_at,
                    })
                    .await;

                match send_result {
                    Ok(_) => {
                        if let Err(e) = sqlx::query!(
                            "UPDATE messages SET status = 'done' WHERE id = $1",
                            msg.id
                        )
                        .execute(&db_pool)
                        .await {
                            error!("Worker #{}: failed to update message status to done: {}", worker_id, e);
                        } else {
                            let _ = tx.send(format!("Message {} sent successfully", msg.id));
                            counter.fetch_add(1, Ordering::SeqCst);
                            let count = counter.load(Ordering::SeqCst);
                            info!("Worker #{}: processed messages count = {}", worker_id, count);
                        }
                    }
                    Err(e) => {
                        error!("Worker #{}: failed to send message id={} error: {}", worker_id, msg.id, e);
                        let new_retry_count = msg.retry_count + 1;
                        let max_retries = 5;

                        if new_retry_count >= max_retries {
                            if let Err(e) = sqlx::query!(
                                "UPDATE messages SET status = 'failed', retry_count = $1 WHERE id = $2",
                                new_retry_count,
                                msg.id
                            )
                            .execute(&db_pool)
                            .await {
                                error!("Worker #{}: failed to mark message as failed: {}", worker_id, e);
                            } else {
                                let _ = tx.send(format!("Message {} failed permanently", msg.id));
                            }
                        } else {
                            let delay_secs = 2_i64.pow(new_retry_count as u32);
                            let next_retry_at = Utc::now().naive_utc() + Duration::seconds(delay_secs);

                            if let Err(e) = sqlx::query!(
                                "UPDATE messages SET status = 'retrying', retry_count = $1, next_retry_at = $2 WHERE id = $3",
                                new_retry_count,
                                next_retry_at,
                                msg.id
                            )
                            .execute(&db_pool)
                            .await {
                                error!("Worker #{}: failed to update retry info: {}", worker_id, e);
                            } else {
                                let _ = tx.send(format!(
                                    "Message {} will retry after {} seconds (attempt {})",
                                    msg.id, delay_secs, new_retry_count
                                ));
                            }
                        }
                    }
                }
            },
            Ok(None) => {
                // Нет сообщений, ждем немного перед следующей проверкой
                sleep(TokioDuration::from_secs(2)).await;
            },
            Err(e) => {
                error!("Worker #{}: error fetching message from db: {}", worker_id, e);
                sleep(TokioDuration::from_secs(2)).await;
            }
        }
    }
}
