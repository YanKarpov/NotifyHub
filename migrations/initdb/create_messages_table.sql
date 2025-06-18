CREATE TABLE IF NOT EXISTS messages (
  id SERIAL PRIMARY KEY,
  provider TEXT NOT NULL,
  text TEXT NOT NULL,
  status TEXT NOT NULL,
  retry_count INT NOT NULL DEFAULT 0,
  created_at TIMESTAMP NOT NULL DEFAULT NOW(),
  next_retry_at TIMESTAMP
);
