Implement single writer or multiple readers rust pattern at proxy level

1. Detect dual write pattern

Use actor model canaries and look out for more than one outgoing write

2. Outbox pattern

Add idempotency key, handle retries, delays

3. Bucketing technique

Split readers and write traffic and latency buckets. 10ms, 1s, 100s, 10000s
