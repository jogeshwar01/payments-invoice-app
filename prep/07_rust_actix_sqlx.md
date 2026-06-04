# Rust, Actix, Sqlx, And Async Questions

## Why Rust for this assignment

Good answer:

> "Rust gives strong compile-time guarantees around data ownership and error handling, and it is a good fit for high-concurrency backend services. The main value here is not raw speed; it is making invalid states and unchecked errors harder to hide."

## Ownership and borrowing examples in this code

### `AppState` is cloned cheaply

`AppState` contains:

- `PgPool`
- `Arc<PspClient>`
- `Arc<Config>`

Cloning it does not clone a database connection or HTTP client. It clones handles.

Good answer:

> "`PgPool` and `Arc` are cheap clone handles. That lets Actix give each worker access to shared state without global mutable state."

### Why use `Arc`

`PspClient` and `Config` are shared across request handlers and background tasks.

`Arc` provides thread-safe reference counting.

## Actix extractors

`BusinessCtx` implements `FromRequest`.

Purpose:

- Pulls bearer token from headers.
- Hashes and looks up API key.
- Makes authenticated routes explicit in function signatures.

`IdempotencyKey` also implements `FromRequest`.

Purpose:

- Validates required header.
- Keeps `/pay` handler focused on domain logic.

If asked:

> "Extractors are a good fit for cross-cutting request concerns that should be validated before handler logic."

## Async Rust

### What does `async` do

An `async fn` returns a future. The Tokio runtime polls futures and runs other tasks while one task waits on I/O.

In this service:

- Database calls are async.
- HTTP PSP calls are async.
- Background workers sleep and poll periodically.

### Why not block

Blocking inside async workers can starve runtime threads.

Examples:

- Do not use long CPU loops in request handlers.
- Do not use blocking network clients.
- Keep `std::sync::Mutex` locks short.

## Mutex in mock PSP

Mock PSP uses `std::sync::Mutex<HashMap<...>>`.

Why acceptable:

- It is a tiny test/mock service.
- Locks are held only for quick HashMap access.
- The 30-second sleep happens outside the lock.

Production:

- External PSP would be durable storage/service.
- If this mock had heavy traffic, use a database or async lock with careful design.

Important nuance:

> "Using `std::sync::Mutex` in async code is not automatically wrong; holding it across `.await` is the real danger. This code does not hold the guard across sleeps."

## Error handling

`ApiError` implements Actix `ResponseError`.

Benefits:

- Central status-code mapping.
- Consistent JSON error format.
- Internal errors do not leak details to clients.

Common mappings:

- `400`: bad input.
- `401`: missing/invalid API key.
- `404`: missing resource.
- `409`: idempotency conflict or payment in progress.
- `422`: invalid domain state.
- `500`: unexpected internal failure.

If asked 409 vs 422:

- `409`: request conflicts with another request/resource version, e.g. idempotency key reuse or payment in flight.
- `422`: syntactically valid request violates domain state, e.g. paying a paid invoice.

## Serde

Used for:

- request JSON deserialization;
- response JSON serialization;
- enum tagging for PSP outcomes;
- timestamp/UUID serialization.

State enum:

- `#[serde(rename_all = "lowercase")]` makes JSON state values match DB strings.

## Sqlx

Why sqlx:

- Async Postgres support.
- Migrations.
- Strong Rust type mapping for common DB types.

Runtime query tradeoff:

- Build does not need DB.
- SQL errors caught at runtime/integration test time.

Compile-time checked alternative:

- `query!` macros with online DB or offline metadata.
- Better validation, more setup.

## Reqwest timeout design

PSP client:

- total timeout around 5 seconds;
- connect timeout around 2 seconds;
- no foreground retry.

Why:

- `/pay` should not hang 30 seconds.
- Retry after ambiguous timeout can double charge.
- Reconciler is the safe retry mechanism.

## Tokio tasks

`tokio::spawn` runs:

- webhook dispatcher loop;
- payment reconciler loop;
- test HTTP server tasks.

Production concern:

- Spawned loops need lifecycle management, graceful shutdown, and observability.
- In the MVP, they are simple infinite loops.

## Common Rust questions

### What is `Result<T, E>`?

Explicit success/error return type. The `?` operator propagates errors after converting with `From`.

### What is `Option<T>`?

Represents value-or-none without null.

### Why newtype `Cents`

Avoids mixing raw integers accidentally and centralizes checked money operations.

### What is `Send` / `Sync`

- `Send`: value can be moved to another thread.
- `Sync`: references can be shared across threads.

Actix/Tokio worker state generally needs thread-safe types because handlers run concurrently.

### What is `Arc<T>`

Atomic reference-counted pointer for shared ownership across threads/tasks.

### Why not global mutable state

State should be injected through `AppState` so tests can spawn isolated app instances and dependencies are explicit.

