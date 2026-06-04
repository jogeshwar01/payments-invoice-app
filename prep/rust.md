Below is a **Rust interview 101 in Q&A format**, tailored for a **Backend Engineer, Rust** role in a fintech/payments company like Dodo Payments.

---

# Rust Backend Interview 101

## 1. Rust Basics

### Q1. What is Rust, and why is it used for backend systems?

Rust is a systems programming language focused on **performance, memory safety, and concurrency safety**.

For backend systems, Rust is attractive because it gives:

* Performance close to C/C++
* Memory safety without garbage collection
* Strong compile-time guarantees
* Safe concurrency
* Predictable latency
* Excellent support for networking, async programming, and serialization

For a payments company, Rust is useful because backend services need to be **fast, reliable, secure, and resistant to runtime crashes**.

---

### Q2. What are Rust’s main selling points?

The main selling points are:

1. **Ownership and borrowing**
2. **Memory safety without GC**
3. **Fearless concurrency**
4. **Zero-cost abstractions**
5. **Strong type system**
6. **Pattern matching**
7. **Good async ecosystem**
8. **Cargo package manager**
9. **Excellent error handling with `Result` and `Option`**

---

### Q3. What does “memory safety without garbage collection” mean?

Rust does not use a garbage collector like Java, Go, or Python. Instead, Rust uses its **ownership system** to determine when memory should be freed.

When a value goes out of scope, Rust automatically drops it.

Example:

```rust
fn main() {
    let name = String::from("Dodo");
    println!("{}", name);
} // name is dropped here
```

There is no manual `free()` and no GC pause.

This is valuable for payment systems because predictable latency matters.

---

# 2. Ownership, Borrowing, and Lifetimes

## Q4. What is ownership in Rust?

Ownership is Rust’s core memory management model.

Rules:

1. Every value has one owner.
2. There can only be one owner at a time.
3. When the owner goes out of scope, the value is dropped.

Example:

```rust
fn main() {
    let a = String::from("payment");
    let b = a;

    // println!("{}", a); // error
    println!("{}", b);
}
```

Here, ownership of the `String` moves from `a` to `b`.

---

## Q5. What is move semantics?

Move semantics means ownership of a value is transferred from one variable to another.

```rust
let s1 = String::from("hello");
let s2 = s1;
```

After this, `s1` is no longer valid because the heap allocation is now owned by `s2`.

This prevents double-free bugs.

---

## Q6. Why does this compile?

```rust
let x = 10;
let y = x;

println!("{}", x);
```

Because `i32` implements the `Copy` trait.

Types like integers, booleans, chars, and floats are copied instead of moved.

```rust
let x = 10;
let y = x; // copy
```

Both `x` and `y` are valid.

---

## Q7. What is the difference between `Copy` and `Clone`?

`Copy` is implicit and cheap.
`Clone` is explicit and may be expensive.

```rust
let a = 10;
let b = a; // Copy

let s1 = String::from("hello");
let s2 = s1.clone(); // Clone
```

`String` does not implement `Copy` because it owns heap memory. But it implements `Clone`, which performs a deep copy.

---

## Q8. What is borrowing?

Borrowing allows a function or variable to use a value without taking ownership.

```rust
fn print_name(name: &String) {
    println!("{}", name);
}

fn main() {
    let name = String::from("Dodo");
    print_name(&name);
    println!("{}", name);
}
```

`print_name` borrows `name`, so ownership stays with `main`.

---

## Q9. What is a mutable borrow?

A mutable borrow allows modifying a value without taking ownership.

```rust
fn add_suffix(name: &mut String) {
    name.push_str(" Payments");
}

fn main() {
    let mut company = String::from("Dodo");
    add_suffix(&mut company);
    println!("{}", company);
}
```

---

## Q10. What are Rust’s borrowing rules?

At any given time, you can have:

* Any number of immutable references, or
* Exactly one mutable reference

But not both.

This prevents data races at compile time.

Invalid example:

```rust
let mut value = String::from("payment");

let r1 = &value;
let r2 = &mut value; // error

println!("{}", r1);
```

---

## Q11. Why is this borrowing rule useful in backend systems?

It prevents bugs such as:

* Race conditions
* Use-after-free
* Iterator invalidation
* Accidental shared mutation
* Data corruption under concurrency

In financial systems, shared mutable state must be handled very carefully. Rust forces you to make ownership and mutation explicit.

---

## Q12. What are lifetimes?

Lifetimes describe how long references are valid.

They prevent dangling references.

Example:

```rust
fn longest<'a>(x: &'a str, y: &'a str) -> &'a str {
    if x.len() > y.len() {
        x
    } else {
        y
    }
}
```

Here, `'a` tells the compiler that the returned reference is valid as long as both inputs are valid.

---

## Q13. Do you always need to write lifetimes manually?

No.

Rust has lifetime elision rules. In many common cases, the compiler infers lifetimes.

Example:

```rust
fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or("")
}
```

No explicit lifetime is needed here.

---

# 3. Strings, Slices, and Collections

## Q14. What is the difference between `String` and `&str`?

`String` is an owned, heap-allocated, growable string.

`&str` is a borrowed string slice.

```rust
let owned: String = String::from("hello");
let borrowed: &str = "hello";
```

Use `String` when you need ownership or mutation.
Use `&str` when you only need to read string data.

---

## Q15. Why do Rust APIs often prefer `&str` over `String`?

Because `&str` is more flexible.

A function accepting `&str` can receive:

* String literals
* Borrowed `String`
* Slices of strings

```rust
fn process_currency(currency: &str) {
    println!("{}", currency);
}

let c = String::from("INR");
process_currency(&c);
process_currency("USD");
```

---

## Q16. What is a slice?

A slice is a view into a sequence.

```rust
let arr = [1, 2, 3, 4];
let slice = &arr[1..3];

println!("{:?}", slice); // [2, 3]
```

For strings:

```rust
let s = String::from("payment");
let part = &s[0..3];
```

Be careful with string slicing because Rust strings are UTF-8 encoded.

---

## Q17. Common Rust collections?

The most common collections are:

```rust
Vec<T>
HashMap<K, V>
HashSet<T>
BTreeMap<K, V>
VecDeque<T>
```

Examples:

```rust
let mut amounts = Vec::new();
amounts.push(100);
amounts.push(200);
```

```rust
use std::collections::HashMap;

let mut balances = HashMap::new();
balances.insert("merchant_1", 5000);
```

---

## Q18. When would you use `BTreeMap` instead of `HashMap`?

Use `BTreeMap` when you need sorted keys or deterministic ordering.

Use `HashMap` when you want average O(1) lookup.

In payments/accounting systems, deterministic ordering can be useful for:

* Reports
* Reconciliation
* Audit logs
* Stable test outputs

---

# 4. Structs, Enums, and Pattern Matching

## Q19. What is a struct?

A struct groups related data.

```rust
struct Payment {
    id: String,
    amount: i64,
    currency: String,
}
```

Example usage:

```rust
let payment = Payment {
    id: String::from("pay_123"),
    amount: 5000,
    currency: String::from("INR"),
};
```

---

## Q20. What is an enum?

An enum represents one of several possible variants.

```rust
enum PaymentStatus {
    Pending,
    Succeeded,
    Failed,
    Refunded,
}
```

Enums are powerful because variants can hold data:

```rust
enum PaymentEvent {
    Created { id: String, amount: i64 },
    Failed { id: String, reason: String },
    Refunded { id: String, amount: i64 },
}
```

---

## Q21. Why are enums useful in payments?

Payments involve state machines.

Example:

```rust
enum TransactionState {
    Initiated,
    Authorized,
    Captured,
    Settled,
    Failed(String),
    Refunded,
}
```

This is safer than representing states with plain strings.

Bad:

```rust
let status = "suceeded"; // typo
```

Good:

```rust
TransactionState::Settled
```

The compiler helps prevent invalid states.

---

## Q22. What is pattern matching?

Pattern matching allows handling different enum variants safely.

```rust
match status {
    PaymentStatus::Pending => println!("waiting"),
    PaymentStatus::Succeeded => println!("done"),
    PaymentStatus::Failed => println!("failed"),
    PaymentStatus::Refunded => println!("refunded"),
}
```

Rust requires exhaustive handling unless `_` is used.

---

## Q23. Why is exhaustive matching useful?

It prevents missed cases.

If you add a new enum variant:

```rust
enum PaymentStatus {
    Pending,
    Succeeded,
    Failed,
    Refunded,
    Chargeback,
}
```

Rust will force you to update all relevant `match` statements.

This is extremely useful in payment systems where missing a state can create business or accounting errors.

---

# 5. Error Handling

## Q24. How does Rust handle errors?

Rust mainly uses:

```rust
Option<T>
Result<T, E>
```

`Option<T>` is used when a value may or may not exist.

```rust
let maybe_user: Option<String> = Some(String::from("merchant"));
```

`Result<T, E>` is used for operations that may fail.

```rust
let result: Result<i32, String> = Ok(100);
```

---

## Q25. What is `Option<T>`?

`Option<T>` represents either:

```rust
Some(value)
None
```

Example:

```rust
fn find_discount(code: &str) -> Option<i32> {
    if code == "SAVE10" {
        Some(10)
    } else {
        None
    }
}
```

---

## Q26. What is `Result<T, E>`?

`Result<T, E>` represents success or failure.

```rust
enum Result<T, E> {
    Ok(T),
    Err(E),
}
```

Example:

```rust
fn parse_amount(input: &str) -> Result<i64, std::num::ParseIntError> {
    input.parse::<i64>()
}
```

---

## Q27. What does the `?` operator do?

The `?` operator propagates errors.

```rust
fn parse_amount(input: &str) -> Result<i64, std::num::ParseIntError> {
    let amount = input.parse::<i64>()?;
    Ok(amount)
}
```

If parsing fails, the function returns the error immediately.

Without `?`, you would write:

```rust
match input.parse::<i64>() {
    Ok(amount) => Ok(amount),
    Err(err) => Err(err),
}
```

---

## Q28. How would you model custom errors?

Commonly using enums.

```rust
#[derive(Debug)]
enum PaymentError {
    InvalidAmount,
    UnsupportedCurrency,
    GatewayTimeout,
    FraudDetected,
}
```

With `thiserror`:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
enum PaymentError {
    #[error("invalid amount")]
    InvalidAmount,

    #[error("unsupported currency: {0}")]
    UnsupportedCurrency(String),

    #[error("gateway timeout")]
    GatewayTimeout,
}
```

---

## Q29. `panic!` vs `Result`?

Use `Result` for expected failures.

Use `panic!` for unrecoverable programmer errors.

Expected failure:

```rust
fn charge_card() -> Result<(), PaymentError> {
    Err(PaymentError::GatewayTimeout)
}
```

Programmer bug:

```rust
panic!("invalid internal state");
```

In payment backends, avoid panics in request paths. Return structured errors instead.

---

## Q30. What are `unwrap()` and `expect()`?

`unwrap()` extracts a value or panics.

```rust
let amount = "100".parse::<i64>().unwrap();
```

`expect()` does the same but with a custom panic message.

```rust
let amount = "100".parse::<i64>().expect("amount should be valid");
```

In production backend code, avoid `unwrap()` on external input, database results, network calls, or payment gateway responses.

---

# 6. Traits and Generics

## Q31. What is a trait?

A trait defines shared behavior.

```rust
trait PaymentProcessor {
    fn charge(&self, amount: i64) -> Result<(), String>;
}
```

Implementing a trait:

```rust
struct StripeProcessor;

impl PaymentProcessor for StripeProcessor {
    fn charge(&self, amount: i64) -> Result<(), String> {
        println!("Charging {}", amount);
        Ok(())
    }
}
```

---

## Q32. What are traits similar to in other languages?

Traits are similar to:

* Interfaces in Java/Go
* Typeclasses in Haskell
* Protocols in Swift

But Rust traits are more powerful because they support generics, associated types, default methods, and trait bounds.

---

## Q33. What are generics?

Generics allow writing reusable code for different types.

```rust
fn identity<T>(value: T) -> T {
    value
}
```

Example with trait bounds:

```rust
fn print_value<T: std::fmt::Debug>(value: T) {
    println!("{:?}", value);
}
```

---

## Q34. What is a trait bound?

A trait bound restricts generic types to those that implement certain behavior.

```rust
fn process<T: PaymentProcessor>(processor: T) {
    let _ = processor.charge(1000);
}
```

This means `T` must implement `PaymentProcessor`.

---

## Q35. What is `impl Trait`?

`impl Trait` can simplify function signatures.

```rust
fn process(processor: impl PaymentProcessor) {
    let _ = processor.charge(1000);
}
```

This is equivalent to a generic parameter in many cases.

---

## Q36. Static dispatch vs dynamic dispatch?

Static dispatch uses generics and monomorphization.

```rust
fn process<T: PaymentProcessor>(processor: T) {}
```

The compiler generates specialized code for each type. This is fast.

Dynamic dispatch uses trait objects.

```rust
fn process(processor: Box<dyn PaymentProcessor>) {}
```

The actual method is chosen at runtime through a vtable.

Static dispatch is faster. Dynamic dispatch is useful when you need heterogeneous types or runtime selection.

---

## Q37. What is a trait object?

A trait object allows using different concrete types through the same interface.

```rust
trait Gateway {
    fn charge(&self, amount: i64);
}

struct Stripe;
struct Razorpay;

impl Gateway for Stripe {
    fn charge(&self, amount: i64) {}
}

impl Gateway for Razorpay {
    fn charge(&self, amount: i64) {}
}

let gateways: Vec<Box<dyn Gateway>> = vec![
    Box::new(Stripe),
    Box::new(Razorpay),
];
```

Useful for payment integrations where different providers implement the same behavior.

---

# 7. Async Rust and Backend Development

## Q38. What is async Rust?

Async Rust allows writing non-blocking code for I/O-heavy workloads.

Backend services spend a lot of time waiting for:

* Database queries
* HTTP calls
* Message queues
* Redis
* Payment gateways
* Webhooks

Async lets one thread handle many concurrent tasks efficiently.

---

## Q39. What is `async fn`?

An `async fn` returns a future.

```rust
async fn fetch_payment() -> Result<String, String> {
    Ok(String::from("payment_123"))
}
```

It does not execute fully until awaited.

```rust
let result = fetch_payment().await;
```

---

## Q40. What is a Future?

A Future represents a value that will be available later.

Conceptually:

```rust
Future<Output = T>
```

In Rust, futures are lazy. They do nothing until polled by an executor such as Tokio.

---

## Q41. What is Tokio?

Tokio is the most widely used async runtime in Rust.

It provides:

* Async task scheduling
* TCP/UDP networking
* Timers
* Async file I/O
* Channels
* Runtime executor

Example:

```rust
#[tokio::main]
async fn main() {
    println!("async runtime started");
}
```

---

## Q42. What is `.await`?

`.await` waits for a future to complete without blocking the entire thread.

```rust
let response = client.get(url).send().await?;
```

This lets the runtime run other tasks while waiting.

---

## Q43. Difference between blocking and non-blocking code?

Blocking code occupies the thread until work finishes.

```rust
std::thread::sleep(std::time::Duration::from_secs(5));
```

Non-blocking async code yields control to the runtime.

```rust
tokio::time::sleep(std::time::Duration::from_secs(5)).await;
```

In async backend services, accidentally using blocking calls can hurt throughput.

---

## Q44. What is `tokio::spawn`?

It starts a new async task.

```rust
tokio::spawn(async {
    println!("processing webhook");
});
```

It is useful for concurrent work, but you must handle errors and lifecycle carefully.

---

## Q45. What is the difference between concurrency and parallelism?

Concurrency means handling many tasks at once logically.

Parallelism means executing multiple tasks at the exact same time on multiple CPU cores.

Async Rust is primarily about concurrency. For CPU-heavy parallelism, use threads or libraries like Rayon.

---

## Q46. What are common Rust backend frameworks?

Common frameworks include:

* `axum`
* `actix-web`
* `warp`
* `rocket`

For modern Rust backend interviews, `axum` is especially common because it integrates well with Tokio, Tower, and Hyper.

---

## Q47. Simple Axum handler example?

```rust
use axum::{routing::get, Router};

async fn health() -> &'static str {
    "ok"
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(health));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();

    axum::serve(listener, app).await.unwrap();
}
```

In production, avoid `unwrap()` and use structured error handling.

---

## Q48. How would you structure a Rust backend service?

A clean structure might be:

```text
src/
  main.rs
  config.rs
  routes/
    mod.rs
    payments.rs
    webhooks.rs
  handlers/
    mod.rs
  services/
    payment_service.rs
    payout_service.rs
  repositories/
    payment_repo.rs
  models/
    payment.rs
    merchant.rs
  errors.rs
  telemetry.rs
```

Typical layering:

```text
HTTP Handler -> Service Layer -> Repository/Gateway Layer -> Database/External API
```

For payment systems, keep domain logic out of HTTP handlers.

---

# 8. Concurrency and Thread Safety

## Q49. What are `Send` and `Sync`?

`Send` means a type can be transferred across threads.

`Sync` means a type can be safely shared between threads.

Simplified:

```text
T: Send  => ownership of T can move to another thread
T: Sync  => &T can be shared across threads
```

These traits are automatically implemented when safe.

---

## Q50. Why are `Send` and `Sync` important in backend services?

Async runtimes may move tasks between worker threads.

If a future is spawned onto a multi-threaded runtime, captured values often need to be `Send`.

Example:

```rust
tokio::spawn(async move {
    // captured values usually need Send
});
```

This matters when using database clients, shared state, or non-thread-safe types.

---

## Q51. What is `Arc`?

`Arc<T>` is an atomically reference-counted pointer.

It allows shared ownership across threads.

```rust
use std::sync::Arc;

let config = Arc::new(AppConfig {});
let cloned = Arc::clone(&config);
```

In web servers, shared app state is often wrapped in `Arc`.

---

## Q52. What is `Mutex`?

A `Mutex<T>` protects shared mutable data.

```rust
use std::sync::Mutex;

let counter = Mutex::new(0);

{
    let mut value = counter.lock().unwrap();
    *value += 1;
}
```

In async code, prefer `tokio::sync::Mutex` when the lock may be held across `.await`.

---

## Q53. `std::sync::Mutex` vs `tokio::sync::Mutex`?

`std::sync::Mutex` blocks the thread while waiting.

`tokio::sync::Mutex` asynchronously waits without blocking the executor.

Use `std::sync::Mutex` for short, CPU-only critical sections where you do not `.await`.

Use `tokio::sync::Mutex` if locking happens in async workflows and may cross `.await`.

---

## Q54. What is `RwLock`?

`RwLock` allows multiple readers or one writer.

Useful when reads are frequent and writes are rare.

```rust
use tokio::sync::RwLock;
use std::sync::Arc;

let cache = Arc::new(RwLock::new(Vec::<String>::new()));
```

Example use cases:

* Config cache
* Feature flags
* Exchange rate cache
* Tax rules cache

---

## Q55. What are channels in Rust?

Channels allow tasks or threads to communicate safely.

Tokio example:

```rust
let (tx, mut rx) = tokio::sync::mpsc::channel(100);

tokio::spawn(async move {
    tx.send("payment_created").await.unwrap();
});

while let Some(event) = rx.recv().await {
    println!("{}", event);
}
```

Useful for background processing, queues, and event-driven architecture.

---

# 9. Serialization, APIs, and Data Modeling

## Q56. What is Serde?

Serde is Rust’s most popular serialization/deserialization framework.

Used for:

* JSON
* YAML
* TOML
* MessagePack
* Database-related serialization

Example:

```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
struct PaymentRequest {
    amount: i64,
    currency: String,
    merchant_id: String,
}
```

---

## Q57. How do you deserialize JSON in Rust?

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct PaymentRequest {
    amount: i64,
    currency: String,
}

let body = r#"{"amount":1000,"currency":"INR"}"#;
let req: PaymentRequest = serde_json::from_str(body)?;
```

---

## Q58. How should money be represented?

Avoid floating-point types like `f32` or `f64`.

Bad:

```rust
let amount = 10.99_f64;
```

Good:

```rust
let amount_minor_units: i64 = 1099;
let currency = "USD";
```

Represent money in **minor units**:

```text
USD 10.99 -> 1099 cents
INR 100.00 -> 10000 paise
JPY 500 -> 500 because JPY has no minor unit
```

For high-precision calculations, use decimal libraries such as `rust_decimal`.

---

## Q59. Why are floats bad for money?

Floating-point numbers cannot precisely represent many decimal values.

Example issue:

```text
0.1 + 0.2 != exactly 0.3
```

In payments, tiny rounding errors can cause reconciliation failures.

Use integer minor units or decimal types.

---

## Q60. How would you model a payment request?

```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
struct CreatePaymentRequest {
    merchant_id: String,
    amount: i64,
    currency: Currency,
    idempotency_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum Currency {
    INR,
    USD,
    EUR,
    GBP,
}
```

In a real system, currency may need ISO 4217 validation and database-backed metadata.

---

# 10. Databases and Persistence

## Q61. Common database libraries in Rust?

Common choices:

* `sqlx`
* `diesel`
* `sea-orm`
* `tokio-postgres`
* `redis`
* `mongodb`

For backend interviews, `sqlx` is commonly discussed because it supports async and compile-time query checking.

---

## Q62. What is a repository pattern?

Repository pattern isolates database access from business logic.

```rust
struct PaymentRepository {
    pool: PgPool,
}

impl PaymentRepository {
    async fn find_by_id(&self, id: &str) -> Result<Payment, RepositoryError> {
        // database query here
        todo!()
    }
}
```

Benefits:

* Easier testing
* Cleaner service logic
* Better separation of concerns
* Less duplication

---

## Q63. How would you handle transactions?

Use database transactions for operations that must succeed or fail together.

Example payment flow:

```text
1. Insert payment record
2. Insert ledger entry
3. Update merchant balance
4. Commit transaction
```

If any step fails, rollback.

With SQL-style pseudocode:

```rust
let mut tx = pool.begin().await?;

insert_payment(&mut tx).await?;
insert_ledger_entry(&mut tx).await?;
update_balance(&mut tx).await?;

tx.commit().await?;
```

---

## Q64. What is idempotency?

Idempotency means the same request can be safely retried without causing duplicate side effects.

In payments, idempotency is critical.

Example:

```text
POST /payments
Idempotency-Key: abc123
```

If the client retries because of a timeout, the backend should not create two payments.

---

## Q65. How would you implement idempotency?

Store the idempotency key with request hash and response.

Possible table:

```text
idempotency_keys
- key
- merchant_id
- request_hash
- status
- response_body
- created_at
```

Flow:

```text
1. Receive request with idempotency key
2. Check if key exists
3. If exists with same request hash, return stored response
4. If exists with different request hash, return conflict
5. If not exists, reserve key
6. Process payment
7. Store final response
```

Use a unique constraint on:

```text
merchant_id + idempotency_key
```

This prevents race-condition duplicates.

---

## Q66. Why are database constraints important?

Application logic alone is not enough under concurrency.

Use constraints like:

```sql
UNIQUE (merchant_id, idempotency_key)
```

or:

```sql
UNIQUE (provider_transaction_id)
```

The database becomes the final guard against duplicate records.

---

# 11. Security and Cryptography

## Q67. What security concerns matter in payment backends?

Important areas:

* Authentication
* Authorization
* Encryption in transit
* Encryption at rest
* Secure secret management
* Request signing
* Webhook verification
* Idempotency
* Audit logging
* Least privilege
* Input validation
* Rate limiting
* Replay protection
* Dependency scanning
* PCI-DSS awareness

---

## Q68. What is hashing?

Hashing converts input data into a fixed-size digest.

Properties:

* Deterministic
* One-way
* Small input change causes large output change

Example algorithms:

* SHA-256
* SHA-512
* BLAKE3

Use cases:

* Integrity checking
* Request signatures
* Storing request hashes for idempotency

Do not use plain hashes for passwords. Use password hashing algorithms like Argon2, bcrypt, or scrypt.

---

## Q69. What is HMAC?

HMAC is a keyed hash used to verify authenticity and integrity.

Example use case: webhook signature verification.

Conceptually:

```text
signature = HMAC_SHA256(secret, request_body)
```

The receiver recomputes the signature and compares it with the received signature.

---

## Q70. How should signatures be compared?

Use constant-time comparison.

Bad:

```rust
if expected == received {
    // vulnerable to timing attacks in sensitive contexts
}
```

Better:

```rust
use subtle::ConstantTimeEq;

if expected.ct_eq(&received).into() {
    // valid
}
```

Constant-time comparison reduces timing side-channel leakage.

---

## Q71. What is encryption vs hashing?

Hashing is one-way.

Encryption is reversible with a key.

Use hashing for:

* Integrity checks
* Password verification, with password-hashing algorithms
* Request fingerprints

Use encryption for:

* Protecting sensitive data that must be read later
* Tokens
* Stored secrets
* Personally identifiable information, depending on requirements

---

## Q72. What is TLS?

TLS secures communication over the network.

It provides:

* Encryption
* Server authentication
* Integrity protection

Payment APIs should always use HTTPS/TLS.

---

## Q73. What is replay protection?

Replay protection prevents an attacker from reusing a valid old request.

Common methods:

* Timestamp in signed payload
* Nonce
* Idempotency key
* Short validity window
* Signature verification

Webhook example:

```text
signed_payload = timestamp + "." + raw_body
```

Reject requests where timestamp is too old.

---

# 12. Payments-Specific Backend Questions

## Q74. How would you design a payment creation API?

High-level flow:

```text
Client -> API Gateway -> Payment Service -> DB
                                  |
                                  -> Payment Provider
                                  |
                                  -> Ledger Service
```

Steps:

```text
1. Authenticate merchant
2. Validate amount and currency
3. Check idempotency key
4. Create payment record as Pending
5. Call payment provider
6. Update status based on provider response
7. Write ledger entries
8. Return payment response
```

Important concerns:

* Idempotency
* Retries
* Timeouts
* Webhook reconciliation
* Ledger correctness
* Audit logs
* Secure provider credentials
* Rate limiting

---

## Q75. How would you handle payment provider timeouts?

A timeout does not always mean failure.

The provider may have processed the payment but failed to respond.

Handle it by:

```text
1. Mark payment as Processing or Unknown
2. Do not immediately mark as Failed
3. Retry status check using provider transaction ID
4. Wait for webhook
5. Reconcile asynchronously
```

Never blindly retry a charge without idempotency.

---

## Q76. What is a ledger?

A ledger is an immutable record of financial movements.

Instead of simply updating balances directly, you record entries.

Example:

```text
Merchant receives payment of INR 100

Debit: Customer/Processor receivable
Credit: Merchant payable
```

Ledger systems are usually append-only.

Balances are derived from ledger entries.

---

## Q77. Why is an append-only ledger useful?

Because it provides:

* Auditability
* Reconciliation
* Historical correctness
* Easier debugging
* Compliance support
* Recovery from bugs

In payments, you should know not just the balance, but exactly how the balance was produced.

---

## Q78. How would you avoid double payouts?

Use multiple layers:

```text
1. Unique payout batch ID
2. Idempotency key with payout provider
3. Database transaction
4. Payout state machine
5. Distributed lock or queue partitioning
6. Ledger-based balance checks
7. Reconciliation job
```

Database constraints are essential.

Example:

```sql
UNIQUE (merchant_id, payout_period)
```

---

## Q79. How would you model payment states?

Example:

```rust
enum PaymentStatus {
    Created,
    Pending,
    Authorized,
    Captured,
    Failed,
    Refunded,
    PartiallyRefunded,
    Disputed,
}
```

Better with transition validation:

```rust
impl PaymentStatus {
    fn can_transition_to(&self, next: &PaymentStatus) -> bool {
        matches!(
            (self, next),
            (PaymentStatus::Created, PaymentStatus::Pending)
                | (PaymentStatus::Pending, PaymentStatus::Authorized)
                | (PaymentStatus::Authorized, PaymentStatus::Captured)
                | (PaymentStatus::Captured, PaymentStatus::Refunded)
                | (PaymentStatus::Pending, PaymentStatus::Failed)
        )
    }
}
```

---

## Q80. What is reconciliation?

Reconciliation compares internal records with external records.

Examples:

```text
Internal payment status vs payment provider status
Internal payout amount vs bank settlement
Internal tax calculation vs tax provider report
Ledger balance vs database balance
```

Reconciliation jobs catch inconsistencies and repair or flag them.

---

# 13. Reliability, Scalability, and Observability

## Q81. How do you make a Rust backend reliable?

Key practices:

* Use `Result`, not panics
* Add timeouts to external calls
* Use retries with backoff
* Use idempotency
* Use circuit breakers
* Validate inputs
* Use database constraints
* Use structured logging
* Add metrics and tracing
* Use health checks
* Gracefully handle shutdown
* Test critical flows thoroughly

---

## Q82. How do you handle retries safely?

Use retries only for retryable errors.

Retryable:

```text
network timeout
HTTP 502/503/504
temporary database failure
rate limit with retry-after
```

Not safely retryable without idempotency:

```text
charge customer
create payout
issue refund
```

Use exponential backoff and jitter.

---

## Q83. What is backoff with jitter?

Backoff waits longer after each failure.

Jitter adds randomness.

Example:

```text
Attempt 1: wait 100ms
Attempt 2: wait 200ms
Attempt 3: wait 400ms
Add random jitter to avoid many clients retrying at the same time.
```

This prevents thundering herd problems.

---

## Q84. What is a circuit breaker?

A circuit breaker stops calling a failing dependency temporarily.

States:

```text
Closed -> normal
Open -> fail fast
Half-open -> test if dependency recovered
```

Useful for payment gateways, tax providers, FX providers, and banks.

---

## Q85. What is observability?

Observability means understanding system behavior from outputs.

Key pillars:

* Logs
* Metrics
* Traces

In Rust, common crates:

* `tracing`
* `tracing-subscriber`
* `opentelemetry`
* `metrics`

---

## Q86. What would you log in a payment system?

Log important events, but avoid sensitive data.

Good logs:

```text
payment_id
merchant_id
status transition
provider
latency
error code
request_id
idempotency_key hash
```

Avoid logging:

```text
card numbers
CVV
full tokens
secrets
raw authorization headers
PII unless necessary and protected
```

---

## Q87. What metrics would you track?

Useful metrics:

```text
payment_success_rate
payment_failure_rate
payment_latency_p95
payment_latency_p99
provider_error_rate
webhook_processing_lag
payout_failure_count
database_query_latency
queue_depth
retry_count
idempotency_conflict_count
```

For payments, alerting should focus on business impact, not just CPU.

---

# 14. Testing in Rust

## Q88. How do you write a unit test in Rust?

```rust
fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 3), 5);
    }
}
```

---

## Q89. What should you test in payment systems?

Test:

* Amount validation
* Currency validation
* State transitions
* Idempotency behavior
* Retry behavior
* Webhook signature verification
* Ledger entry correctness
* Error mapping
* Serialization/deserialization
* Database transaction rollback

---

## Q90. What is property-based testing?

Property-based testing checks that a property holds for many generated inputs.

Example properties:

```text
Refund amount cannot exceed captured amount.
Ledger debits must equal credits.
Currency minor units must be respected.
Payment state transitions must be valid.
```

Rust crate:

```text
proptest
```

---

## Q91. What is integration testing?

Integration tests verify multiple components together.

Example:

```text
HTTP handler + service + database
```

Rust stores integration tests in:

```text
tests/
```

Example:

```text
tests/payment_api_test.rs
```

---

# 15. Performance

## Q92. How do you optimize Rust backend performance?

First measure. Then optimize.

Useful techniques:

* Avoid unnecessary cloning
* Use references where possible
* Use connection pooling
* Add indexes to database queries
* Avoid blocking in async code
* Batch operations
* Use efficient serialization
* Use caching carefully
* Reduce lock contention
* Profile before optimizing

---

## Q93. How do you avoid unnecessary cloning?

Bad:

```rust
fn process(payment_id: String) {
    println!("{}", payment_id);
}
```

If ownership is not needed:

```rust
fn process(payment_id: &str) {
    println!("{}", payment_id);
}
```

Call:

```rust
let id = String::from("pay_123");
process(&id);
```

---

## Q94. What is zero-cost abstraction?

It means high-level abstractions compile down to efficient machine code without runtime overhead.

Examples:

* Iterators
* Generics
* Pattern matching
* Traits with static dispatch

Rust tries to give expressive code without sacrificing performance.

---

## Q95. Are iterators slower than loops?

Usually no. Rust iterators are often optimized very well.

```rust
let total: i64 = amounts.iter().sum();
```

This can be as fast as a manual loop after compiler optimization.

---

# 16. Common Interview Coding Tasks

## Q96. Write a function to validate payment amount.

```rust
#[derive(Debug)]
enum ValidationError {
    InvalidAmount,
    UnsupportedCurrency,
}

fn validate_payment(amount: i64, currency: &str) -> Result<(), ValidationError> {
    if amount <= 0 {
        return Err(ValidationError::InvalidAmount);
    }

    match currency {
        "INR" | "USD" | "EUR" | "GBP" => Ok(()),
        _ => Err(ValidationError::UnsupportedCurrency),
    }
}
```

---

## Q97. Parse a currency string into an enum.

```rust
#[derive(Debug, PartialEq)]
enum Currency {
    INR,
    USD,
    EUR,
}

impl TryFrom<&str> for Currency {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "INR" => Ok(Currency::INR),
            "USD" => Ok(Currency::USD),
            "EUR" => Ok(Currency::EUR),
            _ => Err(format!("unsupported currency: {}", value)),
        }
    }
}
```

---

## Q98. Model a payment state transition.

```rust
#[derive(Debug, PartialEq)]
enum PaymentStatus {
    Created,
    Pending,
    Captured,
    Failed,
    Refunded,
}

fn transition(
    current: PaymentStatus,
    next: PaymentStatus,
) -> Result<PaymentStatus, String> {
    let valid = matches!(
        (&current, &next),
        (PaymentStatus::Created, PaymentStatus::Pending)
            | (PaymentStatus::Pending, PaymentStatus::Captured)
            | (PaymentStatus::Pending, PaymentStatus::Failed)
            | (PaymentStatus::Captured, PaymentStatus::Refunded)
    );

    if valid {
        Ok(next)
    } else {
        Err(format!("invalid transition: {:?} -> {:?}", current, next))
    }
}
```

---

## Q99. Implement idempotency lookup logic.

Pseudo-Rust:

```rust
async fn create_payment(
    merchant_id: &str,
    idempotency_key: &str,
    request_hash: &str,
) -> Result<PaymentResponse, PaymentError> {
    if let Some(record) = find_idempotency_record(merchant_id, idempotency_key).await? {
        if record.request_hash != request_hash {
            return Err(PaymentError::IdempotencyConflict);
        }

        return Ok(record.saved_response);
    }

    reserve_idempotency_key(merchant_id, idempotency_key, request_hash).await?;

    let response = process_payment().await?;

    save_idempotency_response(merchant_id, idempotency_key, &response).await?;

    Ok(response)
}
```

Important: use database unique constraints to avoid race conditions.

---

## Q100. Verify a webhook signature conceptually.

```rust
fn verify_signature(
    raw_body: &[u8],
    received_signature: &[u8],
    secret: &[u8],
) -> bool {
    let expected_signature = compute_hmac_sha256(secret, raw_body);

    constant_time_equals(&expected_signature, received_signature)
}
```

Important points:

* Use the raw request body, not parsed JSON.
* Include timestamp if provider uses it.
* Reject old timestamps.
* Use constant-time comparison.
* Do not log secrets or full signatures.

---

# 17. Advanced Rust Questions

## Q101. What is `Box<T>`?

`Box<T>` stores data on the heap.

```rust
let value = Box::new(10);
```

Useful for:

* Recursive types
* Trait objects
* Large values
* Heap allocation

Example trait object:

```rust
Box<dyn PaymentProcessor>
```

---

## Q102. What is `Rc<T>`?

`Rc<T>` is reference-counted shared ownership for single-threaded code.

```rust
use std::rc::Rc;
```

It is not thread-safe.

For multi-threaded code, use `Arc<T>`.

---

## Q103. What is interior mutability?

Interior mutability allows mutation through an immutable reference using types like:

```rust
Cell<T>
RefCell<T>
Mutex<T>
RwLock<T>
```

Example:

```rust
use std::cell::RefCell;

let value = RefCell::new(10);
*value.borrow_mut() += 1;
```

`RefCell` checks borrow rules at runtime, not compile time.

---

## Q104. What is `Cow`?

`Cow` means Clone-On-Write.

It can hold either borrowed or owned data.

```rust
use std::borrow::Cow;

fn normalize(input: &str) -> Cow<'_, str> {
    if input.contains(" ") {
        Cow::Owned(input.replace(" ", "_"))
    } else {
        Cow::Borrowed(input)
    }
}
```

Useful when you want to avoid allocation unless modification is needed.

---

## Q105. What are macros in Rust?

Macros generate code at compile time.

Examples:

```rust
println!()
vec![]
format!()
matches!()
```

Derive macros:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Payment {
    id: String,
}
```

Macros reduce boilerplate but should be used carefully.

---

# 18. Interview Answers for This Specific Role

## Q106. Why Rust for a payments backend?

Rust is a strong fit because payments systems need:

* Low latency
* High throughput
* Memory safety
* Predictable performance
* Safe concurrency
* Strong correctness guarantees
* Good control over resource usage

The ownership model reduces runtime memory bugs. The type system helps model payment states, currencies, errors, and domain invariants safely.

---

## Q107. How would you design a scalable payment processing service in Rust?

I would design it around clear boundaries:

```text
API Layer
- Auth
- Validation
- Request parsing
- Idempotency key extraction

Service Layer
- Payment orchestration
- State transitions
- Provider routing
- Retry logic

Repository Layer
- Payment records
- Idempotency records
- Ledger entries
- Transaction handling

Integration Layer
- Payment gateways
- FX providers
- Tax providers
- Payout providers

Async Workers
- Webhooks
- Reconciliation
- Payout processing
- Retry queues
```

Critical design points:

* Store all payment state transitions
* Use idempotency keys
* Use database transactions
* Use append-only ledger entries
* Make external calls timeout-bound
* Reconcile async with providers
* Emit structured logs, metrics, and traces

---

## Q108. How would you prevent duplicate charges?

I would use:

```text
1. Client-provided idempotency key
2. Unique DB constraint on merchant_id + idempotency_key
3. Provider-side idempotency key if supported
4. Payment state machine
5. Safe retry policies
6. Webhook reconciliation
```

The key principle is: never retry a money-moving operation unless it is protected by idempotency.

---

## Q109. How would you handle high transaction volume?

Use:

* Async Rust with Tokio
* Connection pooling
* Queue-based workers
* Horizontal scaling
* Stateless API services
* Database indexing and partitioning
* Read/write separation where needed
* Batch processing for settlements
* Backpressure
* Rate limits
* Metrics-driven autoscaling

For critical financial state, prioritize correctness over raw throughput.

---

## Q110. How would you secure payment APIs?

I would apply:

* TLS everywhere
* Strong authentication
* Fine-grained authorization
* Request signing where needed
* Webhook signature verification
* Timestamp-based replay protection
* Input validation
* Rate limiting
* Audit logging
* Secret management through vault/KMS
* No sensitive data in logs
* Dependency vulnerability scanning
* Least-privilege database and service permissions

---

# 19. Rapid-Fire Rust Interview Questions

## Q111. `Vec<T>` vs array?

Array has fixed size.

```rust
let arr = [1, 2, 3];
```

`Vec<T>` is growable and heap-allocated.

```rust
let mut v = Vec::new();
v.push(1);
```

---

## Q112. `&String` vs `&str`?

Prefer `&str` for function parameters because it accepts both `String` and string literals.

---

## Q113. `String` vs `str`?

`String` is owned and growable.
`str` is an unsized string slice, usually used as `&str`.

---

## Q114. `map` vs `and_then`?

`map` transforms the success value.

```rust
Some(2).map(|x| x * 2); // Some(4)
```

`and_then` chains operations that return `Option` or `Result`.

```rust
Some("10").and_then(|s| s.parse::<i32>().ok());
```

---

## Q115. `Result` vs `Option`?

Use `Option` when absence is expected and no error details are needed.

Use `Result` when failure details matter.

---

## Q116. `iter()` vs `into_iter()`?

`iter()` borrows items.

```rust
for x in values.iter() {}
```

`into_iter()` consumes the collection.

```rust
for x in values.into_iter() {}
```

---

## Q117. `match` vs `if let`?

Use `match` when handling multiple cases.

Use `if let` for one specific case.

```rust
if let Some(value) = maybe_value {
    println!("{}", value);
}
```

---

## Q118. What is shadowing?

Shadowing allows reusing a variable name.

```rust
let amount = "100";
let amount: i64 = amount.parse().unwrap();
```

This is different from mutability.

---

## Q119. What is `derive`?

`derive` automatically implements traits.

```rust
#[derive(Debug, Clone, PartialEq)]
struct Payment {
    id: String,
}
```

---

## Q120. What is `Default`?

`Default` provides a default value.

```rust
#[derive(Default)]
struct Config {
    retries: u32,
}
```

---

# 20. Best Interview Talking Points

Use these phrases naturally:

### On Rust

> Rust helps me encode correctness into the type system instead of relying only on runtime checks.

### On payments

> For money movement, idempotency, reconciliation, and auditability are as important as latency.

### On reliability

> I would avoid treating provider timeouts as failures because the external side effect may have already happened.

### On modeling

> I prefer enums for payment states because they prevent invalid string states and force exhaustive handling.

### On security

> For webhooks, I would verify signatures against the raw body, use timestamp-based replay protection, and compare signatures in constant time.

### On scalability

> I would keep API services stateless, push long-running work to queues, use async I/O, and protect the database with proper indexing and connection pooling.

### On correctness

> In financial systems, the database should enforce invariants with constraints, not just application logic.

---

# 21. Mock Interview Set

## Rust Language

1. Explain ownership, borrowing, and lifetimes.
2. Why does Rust not need a garbage collector?
3. What is the difference between `String` and `&str`?
4. What is the difference between `Copy` and `Clone`?
5. What are `Result` and `Option`?
6. When should you use `unwrap()`?
7. What is a trait?
8. What is static dispatch?
9. What is dynamic dispatch?
10. What are `Send` and `Sync`?

## Backend Rust

1. What is Tokio?
2. What is an async runtime?
3. What happens when you call `.await`?
4. Why should you avoid blocking calls in async code?
5. How would you structure a Rust web service?
6. How would you share application state in Axum?
7. How would you handle database transactions?
8. How would you design error handling?
9. How would you test async code?
10. How would you add observability?

## Payments

1. What is idempotency?
2. How do you prevent duplicate charges?
3. How do you handle provider timeouts?
4. What is webhook verification?
5. What is reconciliation?
6. How would you model payment states?
7. Why should money not be stored as floats?
8. How would you handle refunds?
9. How would you design payout processing?
10. How would you design a ledger?

---

# 22. Mini Cheat Sheet

```text
Use String when you own text.
Use &str when you borrow text.

Use Result when something can fail.
Use Option when something may be absent.

Use enum for states.
Use struct for data.
Use trait for behavior.

Use Arc for shared ownership across threads.
Use Mutex/RwLock for shared mutation.
Use Tokio for async backend work.

Avoid unwrap in production request paths.
Avoid floats for money.
Avoid blocking calls inside async handlers.
Avoid duplicate money movement without idempotency.
```

---

# 23. What You Should Be Ready to Explain in This Interview

For this role, be especially strong on:

1. **Ownership and borrowing**
2. **Error handling**
3. **Async Rust with Tokio**
4. **Axum/Actix-style backend architecture**
5. **PostgreSQL transactions**
6. **Idempotency**
7. **Payment state machines**
8. **Webhook signature verification**
9. **Money representation**
10. **Retries, timeouts, and reconciliation**
11. **Security basics**
12. **Observability and reliability**

A strong final positioning would be:

> I use Rust not just for speed, but for correctness. In payment systems, correctness is the product. Rust’s type system, ownership model, and explicit error handling help build services where invalid states, unsafe sharing, and unhandled failures are caught early. For money movement, I would combine Rust’s safety with database constraints, idempotency, append-only ledgers, retries with backoff, and reconciliation workflows.
