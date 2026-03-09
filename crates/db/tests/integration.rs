use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;

async fn setup() -> (ContainerAsync<Postgres>, db::PgPool) {
    let container = Postgres::default().start().await.expect("start postgres");
    let port = container.get_host_port_ipv4(5432).await.expect("get port");
    let url = format!("postgres://postgres:postgres@localhost:{port}/postgres");
    let pool = db::connect(&url).await.expect("connect");
    db::migrate(&pool).await.expect("migrate");
    (container, pool)
}

fn new_account(email: &str) -> db::NewAccount {
    new_account_with_username(
        email,
        &email.split('@').next().unwrap_or("user").replace('.', "-"),
    )
}

fn new_account_with_username(email: &str, username: &str) -> db::NewAccount {
    db::NewAccount {
        id: uuid::Uuid::new_v4().to_string(),
        email: email.to_string(),
        password_hash: "hash".to_string(),
        username: username.to_string(),
        created_at: 1_000_000,
    }
}

// ── accounts ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_create_and_get_account() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("alice@example.com", "alice");
    let id = acct.id.clone();
    db::create_account(&pool, &acct).await.expect("create");

    let by_email = db::get_account_by_email(&pool, "alice@example.com")
        .await
        .expect("get by email")
        .expect("should exist");
    assert_eq!(by_email.email, "alice@example.com");
    assert_eq!(by_email.username, "alice");
    assert_eq!(by_email.vcpu_limit, 8);
    assert_eq!(by_email.mem_limit_mb, 12288);
    assert_eq!(by_email.vm_limit, 5);

    let by_id = db::get_account(&pool, &id)
        .await
        .expect("get by id")
        .expect("should exist");
    assert_eq!(by_id.id, id);

    let by_username = db::get_account_by_username(&pool, "alice")
        .await
        .expect("get by username")
        .expect("should exist");
    assert_eq!(by_username.id, id);
}

#[tokio::test]
async fn test_get_account_missing() {
    let (_c, pool) = setup().await;
    let result = db::get_account_by_email(&pool, "nobody@example.com")
        .await
        .expect("query ok");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_duplicate_email_rejected() {
    let (_c, pool) = setup().await;
    db::create_account(
        &pool,
        &new_account_with_username("dup@example.com", "dup-user"),
    )
    .await
    .expect("first insert ok");
    let err = db::create_account(
        &pool,
        &new_account_with_username("dup@example.com", "dup-user2"),
    )
    .await
    .expect_err("second insert should fail");
    assert!(
        err.to_string().contains("unique") || err.to_string().contains("duplicate"),
        "expected unique violation, got: {err}"
    );
}

#[tokio::test]
async fn test_duplicate_username_rejected() {
    let (_c, pool) = setup().await;
    db::create_account(
        &pool,
        &new_account_with_username("user1@example.com", "taken"),
    )
    .await
    .expect("first insert ok");
    let err = db::create_account(
        &pool,
        &new_account_with_username("user2@example.com", "taken"),
    )
    .await
    .expect_err("second insert with same username should fail");
    assert!(
        err.to_string().contains("unique") || err.to_string().contains("duplicate"),
        "expected unique violation, got: {err}"
    );
}

#[tokio::test]
async fn test_update_account_profile() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("profile@example.com", "profile-user");
    let id = acct.id.clone();
    db::create_account(&pool, &acct).await.expect("create");

    let update = db::UpdateAccountProfile {
        display_name: Some("Profile User".to_string()),
        avatar_bytes: Some(vec![0x89, 0x50, 0x4e, 0x47]),
    };
    db::update_account_profile(&pool, &id, &update)
        .await
        .expect("update profile");

    let fetched = db::get_account(&pool, &id)
        .await
        .expect("get account")
        .expect("should exist");
    assert_eq!(fetched.display_name.as_deref(), Some("Profile User"));
    assert_eq!(
        fetched.avatar_bytes.as_deref(),
        Some(&[0x89u8, 0x50, 0x4e, 0x47][..])
    );
}

#[tokio::test]
async fn test_update_profile_clears_avatar() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("clear@example.com", "clear-user");
    let id = acct.id.clone();
    db::create_account(&pool, &acct).await.expect("create");

    db::update_account_profile(
        &pool,
        &id,
        &db::UpdateAccountProfile {
            display_name: Some("Clear User".to_string()),
            avatar_bytes: Some(vec![1, 2, 3]),
        },
    )
    .await
    .expect("set avatar");

    db::update_account_profile(
        &pool,
        &id,
        &db::UpdateAccountProfile {
            display_name: Some("Clear User".to_string()),
            avatar_bytes: None,
        },
    )
    .await
    .expect("clear avatar");

    let fetched = db::get_account(&pool, &id)
        .await
        .expect("get")
        .expect("exists");
    assert!(fetched.avatar_bytes.is_none());
}

// ── sessions ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_session_lifecycle() {
    let (_c, pool) = setup().await;
    let acct = new_account("bob@example.com");
    db::create_account(&pool, &acct)
        .await
        .expect("create account");

    let session = db::NewSession {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: acct.id.clone(),
        created_at: 1_000_000,
        expires_at: 9_999_999_999,
    };
    let sid = session.id.clone();

    db::create_session(&pool, &session)
        .await
        .expect("create session");

    let fetched = db::get_session(&pool, &sid)
        .await
        .expect("query ok")
        .expect("should exist");
    assert_eq!(fetched.account_id, acct.id);
    assert_eq!(fetched.expires_at, 9_999_999_999);

    db::delete_session(&pool, &sid).await.expect("delete");

    let gone = db::get_session(&pool, &sid).await.expect("query ok");
    assert!(gone.is_none(), "session should be gone after delete");
}

#[tokio::test]
async fn test_delete_expired_sessions() {
    let (_c, pool) = setup().await;
    let acct = new_account("carol@example.com");
    db::create_account(&pool, &acct)
        .await
        .expect("create account");

    let expired = db::NewSession {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: acct.id.clone(),
        created_at: 1_000,
        expires_at: 2_000,
    };
    let live = db::NewSession {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: acct.id.clone(),
        created_at: 1_000,
        expires_at: 9_999_999_999,
    };
    let expired_id = expired.id.clone();
    let live_id = live.id.clone();

    db::create_session(&pool, &expired)
        .await
        .expect("create expired");
    db::create_session(&pool, &live).await.expect("create live");

    let deleted = db::delete_expired_sessions(&pool)
        .await
        .expect("delete expired");
    assert_eq!(deleted, 1);

    assert!(
        db::get_session(&pool, &expired_id)
            .await
            .expect("query")
            .is_none()
    );
    assert!(
        db::get_session(&pool, &live_id)
            .await
            .expect("query")
            .is_some()
    );
}

// ── quota ─────────────────────────────────────────────────────────────────────

async fn insert_vm_with_status(
    pool: &db::PgPool,
    account_id: &str,
    vcores: i32,
    memory_mb: i32,
    status: &str,
) -> String {
    let vm_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO vms (id, account_id, name, status, subdomain, vcores, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, created_at)
         VALUES ($1,$2,'test','running',$1,$3,$4,'/k','/r','/o','init','1.2.3.4',8080,0)",
    )
    .bind(&vm_id)
    .bind(account_id)
    .bind(vcores)
    .bind(memory_mb)
    .execute(pool)
    .await
    .expect("insert vm");

    // set actual status
    sqlx::query("UPDATE vms SET status=$1 WHERE id=$2")
        .bind(status)
        .bind(&vm_id)
        .execute(pool)
        .await
        .expect("set status");

    vm_id
}

#[tokio::test]
async fn test_quota_reserve_success() {
    let (_c, pool) = setup().await;
    let acct = new_account("quota-ok@example.com");
    db::create_account(&pool, &acct).await.expect("create");

    // create a stopped vm to reserve
    let vm_id = insert_vm_with_status(&pool, &acct.id, 2, 512, "stopped").await;

    db::check_quota_and_reserve(&pool, &acct.id, &vm_id, 2, 512)
        .await
        .expect("quota should pass");

    // vm should now be 'starting'
    let status: String = sqlx::query_scalar("SELECT status FROM vms WHERE id=$1")
        .bind(&vm_id)
        .fetch_one(&pool)
        .await
        .expect("fetch status");
    assert_eq!(status, "starting");
}

#[tokio::test]
async fn test_quota_vm_limit_exceeded() {
    let (_c, pool) = setup().await;
    let acct = new_account("quota-vmlimit@example.com");
    db::create_account(&pool, &acct).await.expect("create");

    // set vm_limit = 1, then put 1 running vm
    sqlx::query("UPDATE accounts SET vm_limit=1 WHERE id=$1")
        .bind(&acct.id)
        .execute(&pool)
        .await
        .expect("update limit");

    insert_vm_with_status(&pool, &acct.id, 1, 256, "running").await;

    let candidate = insert_vm_with_status(&pool, &acct.id, 1, 256, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &acct.id, &candidate, 1, 256)
        .await
        .expect_err("should exceed vm limit");
    assert!(
        matches!(err, db::QuotaError::Exceeded(_)),
        "expected Exceeded, got: {err}"
    );
}

#[tokio::test]
async fn test_quota_vcpu_limit_exceeded() {
    let (_c, pool) = setup().await;
    let acct = new_account("quota-vcpu@example.com");
    db::create_account(&pool, &acct).await.expect("create");

    // vcpu_limit = 4, already using 3
    sqlx::query("UPDATE accounts SET vcpu_limit=4 WHERE id=$1")
        .bind(&acct.id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 3, 256, "running").await;
    let candidate = insert_vm_with_status(&pool, &acct.id, 2, 256, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &acct.id, &candidate, 2, 256)
        .await
        .expect_err("should exceed vcpu limit");
    assert!(matches!(err, db::QuotaError::Exceeded(_)));
}

#[tokio::test]
async fn test_quota_mem_limit_exceeded() {
    let (_c, pool) = setup().await;
    let acct = new_account("quota-mem@example.com");
    db::create_account(&pool, &acct).await.expect("create");

    // mem_limit_mb = 1024, already using 768
    sqlx::query("UPDATE accounts SET mem_limit_mb=1024 WHERE id=$1")
        .bind(&acct.id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 1, 768, "running").await;
    let candidate = insert_vm_with_status(&pool, &acct.id, 1, 512, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &acct.id, &candidate, 1, 512)
        .await
        .expect_err("should exceed mem limit");
    assert!(matches!(err, db::QuotaError::Exceeded(_)));
}

#[tokio::test]
async fn test_quota_only_counts_active_vms() {
    let (_c, pool) = setup().await;
    let acct = new_account("quota-stopped@example.com");
    db::create_account(&pool, &acct).await.expect("create");

    // stopped vms don't count against quota
    sqlx::query("UPDATE accounts SET vcpu_limit=2 WHERE id=$1")
        .bind(&acct.id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 2, 512, "stopped").await;
    insert_vm_with_status(&pool, &acct.id, 2, 512, "stopped").await;

    let candidate = insert_vm_with_status(&pool, &acct.id, 2, 512, "stopped").await;

    db::check_quota_and_reserve(&pool, &acct.id, &candidate, 2, 512)
        .await
        .expect("stopped vms should not count against quota");
}
