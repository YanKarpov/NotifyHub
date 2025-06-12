use sqlx::PgPool;
use chrono::{Utc, Duration};
use tokio::time::{sleep, Duration as TokioDuration};
use crate::handlers::Message; 
use async_trait::async_trait;

#[derive(Debug)]
pub struct DbMessage {
    pub id: i32,
    pub provider: String,
    pub text: String,
    pub status: String,
    pub retry_count: i32,
    pub next_retry_at: Option<chrono::NaiveDateTime>,
}

#[async_trait]
pub trait Provider {
    async fn send(&self, msg: &Message) -> Result<(), String>;
}

pub struct MockProvider;

#[async_trait]
impl Provider for MockProvider {
    async fn send(&self, msg: &Message) -> Result<(), String> {
        println!("Mock send: provider={} text={}", msg.provider, msg.text);
        Ok(())
    }
}

pub async fn worker_loop(db_pool: PgPool, provider: impl Provider + Send + Sync + 'static) {
    loop {
        // Берём до 10 сообщений со статусом pending или retrying, которым пора пытаться отправлять
        let messages = sqlx::query_as!(
            DbMessage,
            r#"
            SELECT id, provider, text, status, retry_count, next_retry_at
            FROM messages
            WHERE (status = 'pending' OR status = 'retrying')
              AND (next_retry_at IS NULL OR next_retry_at <= $1)
            LIMIT 10
            "#,
            Utc::now().naive_utc()
        )
        .fetch_all(&db_pool)
        .await
        .expect("Failed to fetch messages");

        for msg in messages {
            println!("Worker: processing message id={}", msg.id);
            let send_result = provider.send(&Message {
                provider: msg.provider.clone(),
                text: msg.text.clone(),
            }).await;

            match send_result {
                Ok(_) => {
                    // Обновляем статус в базе на done
                    sqlx::query!(
                        "UPDATE messages SET status = 'done' WHERE id = $1",
                        msg.id
                    )
                    .execute(&db_pool)
                    .await
                    .unwrap();
                }
                Err(_) => {
                    let new_retry_count = msg.retry_count + 1;
                    let max_retries = 5;
                    if new_retry_count >= max_retries {
                        // Если попыток слишком много — ставим failed
                        sqlx::query!(
                            "UPDATE messages SET status = 'failed', retry_count = $1 WHERE id = $2",
                            new_retry_count,
                            msg.id
                        )
                        .execute(&db_pool)
                        .await
                        .unwrap();
                    } else {
                        // Экспоненциальная задержка: 2^retry_count секунд
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
                    }
                }
            }
        }

        sleep(TokioDuration::from_secs(5)).await;
    }
}
