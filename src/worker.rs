use sqlx::PgPool;
use chrono::{Utc, Duration};
use tokio::time::{sleep, Duration as TokioDuration};
use crate::handlers::Message;
use async_trait::async_trait;
use tokio::sync::broadcast::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

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
        println!("Mock send: provider={} text={}", msg.provider, msg.text);
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
        .await
        .expect("Failed to fetch message");

        if let Some(msg) = maybe_msg {
            println!("Worker #{worker_id}: processing message id={}", msg.id);

            let send_result = provider
                .send(&Message {
                    provider: msg.provider.clone(),
                    text: msg.text.clone(),
                    scheduled_at: msg.next_retry_at,
                })
                .await;

            match send_result {
                Ok(_) => {
                    sqlx::query!(
                        "UPDATE messages SET status = 'done' WHERE id = $1",
                        msg.id
                    )
                    .execute(&db_pool)
                    .await
                    .unwrap();

                    let _ = tx.send(format!("Message {} sent successfully", msg.id));
                    counter.fetch_add(1, Ordering::SeqCst);

                    let count = counter.load(Ordering::SeqCst);
                    println!("Worker #{worker_id}: processed messages count = {}", count);
                }
                Err(_) => {
                    let new_retry_count = msg.retry_count + 1;
                    let max_retries = 5;

                    if new_retry_count >= max_retries {
                        sqlx::query!(
                            "UPDATE messages SET status = 'failed', retry_count = $1 WHERE id = $2",
                            new_retry_count,
                            msg.id
                        )
                        .execute(&db_pool)
                        .await
                        .unwrap();

                        let _ = tx.send(format!("Message {} failed permanently", msg.id));
                    } else {
                        let delay_secs = 2_i64.pow(new_retry_count as u32);
                        let next_retry_at = Utc::now().naive_utc() + Duration::seconds(delay_secs);

                        sqlx::query!(
                            "UPDATE messages SET status = 'retrying', retry_count = $1, next_retry_at = $2 WHERE id = $3",
                            new_retry_count,
                            next_retry_at,
                            msg.id
                        )
                        .execute(&db_pool)
                        .await
                        .unwrap();

                        let _ = tx.send(format!(
                            "Message {} will retry after {} seconds (attempt {})",
                            msg.id, delay_secs, new_retry_count
                        ));
                    }
                }
            }
        } else {
            // Нет сообщений, ждем немного перед следующей проверкой
            sleep(TokioDuration::from_secs(2)).await;
        }
    }
}
