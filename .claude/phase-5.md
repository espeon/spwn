# phase 5: auth + accounts

**goal:** signup/login with session cookies, all VM API routes require auth, quota enforced with a serializable transaction so concurrent starts can't race.

**done when:**

- `POST /auth/signup` works with a valid invite code (from env `INVITE_CODE`)
- `POST /auth/login` + `POST /auth/logout` work
- all `/api/vms` routes require a valid session, return 401 otherwise
- `account_id` is real (no more hardcoded `"dev"`)
- VM start is rejected if account is over quota
- quota limits live in the `accounts` table (per-account, adjustable)

---

## db changes

### migration `0006_accounts.sql`

```sql
CREATE TABLE IF NOT EXISTS accounts (
    id           TEXT PRIMARY KEY,
    email        TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    vcpu_limit   INTEGER NOT NULL DEFAULT 8,
    mem_limit_mb INTEGER NOT NULL DEFAULT 12288,
    vm_limit     INTEGER NOT NULL DEFAULT 5,
    created_at   BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL
);

-- vms already have account_id TEXT; it just pointed to "dev" before
```

### new db functions

```rust
pub async fn create_account(pool, account: &NewAccount) -> Result<AccountRow>
pub async fn get_account_by_email(pool, email: &str) -> Result<Option<AccountRow>>
pub async fn get_account(pool, id: &str) -> Result<Option<AccountRow>>

pub async fn create_session(pool, session: &NewSession) -> Result<SessionRow>
pub async fn get_session(pool, id: &str) -> Result<Option<SessionRow>>
pub async fn delete_session(pool, id: &str) -> Result<()>
pub async fn delete_expired_sessions(pool) -> Result<u64>

// quota: serializable tx — check running totals then set status='starting'
pub async fn check_quota_and_reserve(pool, account_id: &str, vm_id: &str, vcores: i32, mem_mb: i32) -> Result<()>
```

`check_quota_and_reserve` runs in a `SERIALIZABLE` transaction:
1. `SELECT SUM(vcores), SUM(memory_mb) FROM vms WHERE account_id=$1 AND status IN ('running','starting')`
2. compare against `accounts.vcpu_limit` / `accounts.mem_limit_mb`
3. if within limits: `UPDATE vms SET status='starting' WHERE id=$2`
4. commit — serialization failure retried once by caller

---

## auth crate (new)

`crates/auth` — shared between control-plane only for now:

```
crates/auth/
  Cargo.toml
  src/
    lib.rs        re-exports
    password.rs   argon2 hash/verify
    session.rs    session middleware (axum extractor)
    routes.rs     signup / login / logout handlers
```

### password.rs

```rust
pub fn hash_password(password: &str) -> Result<String>   // argon2id
pub fn verify_password(password: &str, hash: &str) -> Result<bool>
```

### session middleware

axum extractor that reads `session_id` cookie, looks up in postgres, returns `AccountId(String)`:

```rust
pub struct AccountId(pub String);

#[async_trait]
impl<S> FromRequestParts<S> for AccountId { ... }
```

returns `401 Unauthorized` if cookie missing or session expired/invalid.

### routes.rs

```
POST /auth/signup   { email, password, invite_code } → 201 | 400 | 403
POST /auth/login    { email, password }              → 200 (sets cookie) | 401
POST /auth/logout                                   → 204 (clears cookie)
GET  /auth/me                                       → { id, email }
```

signup checks `invite_code == env::var("INVITE_CODE")`.

session cookie: `HttpOnly; SameSite=Lax; Path=/` — no `Secure` in dev, add in prod.

---

## control-plane changes

### router update

```rust
// public
.route("/auth/signup", post(signup))
.route("/auth/login",  post(login))
.route("/auth/logout", post(logout))
.route("/auth/me",     get(me))

// protected — require valid session
.route("/api/vms", ...)
.route("/api/vms/:id", ...)
// etc.
.layer(axum::middleware::from_extractor::<AccountId>())  // only on /api/* routes
```

### VmOps trait

add `account_id: &str` param to `create_vm` and `list_vms` (they already pass it, just hardcoded to `"dev"`). update `ControlPlaneOps` to use the real account id from the session extractor.

### quota enforcement

in `ControlPlaneOps::start_vm`:

```rust
// replace db::set_vm_status("starting") with:
db::check_quota_and_reserve(&self.pool, &vm.account_id, vm_id, vm.vcores, vm.memory_mb).await
    .map_err(|e| anyhow!("quota exceeded: {e}"))?;
```

---

## env vars (control-plane)

| var | description |
|-----|-------------|
| `INVITE_CODE` | required — signup rejected without this code |
| `SESSION_TTL_SECS` | default 604800 (7 days) |

---

## what's NOT in scope

- email verification
- password reset
- rate limiting signup
- TLS on session cookie (prod concern)
- multi-invite / per-invite quotas
