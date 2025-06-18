use sqlx::PgPool;
use chrono::{Utc, Duration};
use tokio::time::{sleep, Duration as TokioDuration};
use crate::handlers::Message;
use async_trait::async_trait;
use tokio::sync::{broadcast::Sender, mpsc};
use std::sync::Arc;

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
        // Simulate failure:
        // Err("Simulated failure".into())
        Ok(())
    }
}

///Новый динамический пул воркеров + планировщик
pub async fn dynamic_worker_pool(
    db_pool: PgPool,
    provider: impl Provider + Clone + 'static,
    max_workers: usize,
    poll_interval: TokioDuration,
    tx: Sender<String>,
) {
    let (msg_tx, mut msg_rx) = mpsc::channel::<DbMessage>(100);

    let db_pool = Arc::new(db_pool);

    // Scheduler task: периодически опрашивает БД и кладёт сообщения в канал
    let db_pool_clone = db_pool.clone();
    let msg_tx_clone = msg_tx.clone();
    tokio::spawn(async move {
        loop {
            let messages = sqlx::query_as!(
                DbMessage,
                r#"
                SELECT id, provider, text, status, retry_count, next_retry_at
                FROM messages
                WHERE (status = 'pending' OR status = 'retrying')
                  AND (next_retry_at IS NULL OR next_retry_at <= $1)
                ORDER BY id
                LIMIT 20
                "#,
                Utc::now().naive_utc()
            )
            .fetch_all(&*db_pool_clone)
            .await
            .expect("Failed to fetch messages");

            for msg in messages {
                if msg_tx_clone.send(msg).await.is_err() {
                    break; // канал закрыт
                }
            }

            sleep(poll_interval).await;
        }
    });

    // Запускаем пул воркеров
    for i in 0..max_workers {
        let db_pool_clone = db_pool.clone();
        let provider_clone = provider.clone();
        let mut msg_rx_clone = msg_rx.clone();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            while let Some(msg) = msg_rx_clone.recv().await {
                println!("Worker #{i}: processing message id={}", msg.id);

                let send_result = provider_clone
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
                        .execute(&*db_pool_clone)
                        .await
                        .unwrap();

                        let _ = tx_clone.send(format!("Message {} sent successfully", msg.id));
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
                            .execute(&*db_pool_clone)
                            .await
                            .unwrap();

                            let _ = tx_clone.send(format!("Message {} failed permanently", msg.id));
                        } else {
                            let delay_secs = 2_i64.pow(new_retry_count as u32);
                            let next_retry_at = Utc::now().naive_utc() + Duration::seconds(delay_secs);

                            sqlx::query!(
                                "UPDATE messages SET status = 'retrying', retry_count = $1, next_retry_at = $2 WHERE id = $3",
                                new_retry_count,
                                next_retry_at,
                                msg.id
                            )
                            .execute(&*db_pool_clone)
                            .await
                            .unwrap();

                            let _ = tx_clone.send(format!(
                                "Message {} will retry after {} seconds (attempt {})",
                                msg.id, delay_secs, new_retry_count
                            ));
                        }
                    }
                }
            }
        });
    }
}

/// ```rust
/// pub async fn worker_loop(
///     db_pool: PgPool,
///     provider: impl Provider + Send + Sync + 'static,
///     tx: Sender<String>,
/// ) {
///     loop {
///         let messages = sqlx::query_as!(
///             DbMessage,
///             r#"
///             SELECT id, provider, text, status, retry_count, next_retry_at
///             FROM messages
///             WHERE (status = 'pending' OR status = 'retrying')
///               AND (next_retry_at IS NULL OR next_retry_at <= $1)
///             LIMIT 10
///             "#,
///             Utc::now().naive_utc()
///         )
///         .fetch_all(&db_pool)
///         .await
///         .expect("Failed to fetch messages");
///
///         for msg in messages {
///             println!("Worker: processing message id={}", msg.id);
///
///             let send_result = provider
///                 .send(&Message {
///                     provider: msg.provider.clone(),
///                     text: msg.text.clone(),
///                     scheduled_at: msg.next_retry_at, 
///                 })
///                 .await;
///
///             match send_result {
///                 Ok(_) => {
///                     sqlx::query!(
///                         "UPDATE messages SET status = 'done' WHERE id = $1",
///                         msg.id
///                     )
///                     .execute(&db_pool)
///                     .await
///                     .unwrap();
///
///                     let _ = tx.send(format!("Message {} sent successfully", msg.id));
///                 }
///                 Err(_) => {
///                     let new_retry_count = msg.retry_count + 1;
///                     let max_retries = 5;
///
///                     if new_retry_count >= max_retries {
///                         sqlx::query!(
///                             "UPDATE messages SET status = 'failed', retry_count = $1 WHERE id = $2",
///                             new_retry_count,
///                             msg.id
///                         )
///                         .execute(&db_pool)
///                         .await
///                         .unwrap();
///
///                         let _ = tx.send(format!("Message {} failed permanently", msg.id));
///                     } else {
///                         let delay_secs = 2_i64.pow(new_retry_count as u32);
///                         let next_retry_at = Utc::now().naive_utc() + Duration::seconds(delay_secs);
///
///                         sqlx::query!(
///                             "UPDATE messages SET status = 'retrying', retry_count = $1, next_retry_at = $2 WHERE id = $3",
///                             new_retry_count,
///                             next_retry_at,
///                             msg.id
///                         )
///                         .execute(&db_pool)
///                         .await
///                         .unwrap();
///
///                         let _ = tx.send(format!(
///                             "Message {} will retry after {} seconds (attempt {})",
///                             msg.id, delay_secs, new_retry_count
///                         ));
///                     }
///                 }
///             }
///         }
///
///         sleep(TokioDuration::from_secs(5)).await;
///     }
/// }
/// ```