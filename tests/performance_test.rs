use reqwest::Client;
use tokio::task;
use std::sync::Arc;

#[tokio::test]
async fn performance_test() {
    let client = Arc::new(Client::new());
    let concurrent_requests = 10;
    let total_requests = 50;

    let mut handles = vec![];

    for _ in 0..concurrent_requests {
        let client = client.clone();
        let handle = task::spawn(async move {
            for _ in 0..(total_requests / concurrent_requests) {
                let res = client.post("http://localhost:8080/enqueue")
                    .json(&serde_json::json!({
                        "provider": "MTC",
                        "text": "it`s just test message"
                    }))
                    .send()
                    .await;

                match res {
                    Ok(r) => println!("Response: {}", r.status()),
                    Err(e) => eprintln!("Request failed: {}", e),
                }
            }
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.await.unwrap();
    }
}
