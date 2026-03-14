use testcontainers::{ContainerAsync, runners::AsyncRunner};
use testcontainers_modules::postgres::Postgres;
use uuid::Uuid;

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
    assert_eq!(by_email.vcpu_limit, 8000);
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
    vcpus: i64,
    memory_mb: i32,
    status: &str,
) -> String {
    let vm_id = uuid::Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO vms (id, account_id, name, status, subdomain, vcpus, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, created_at)
         VALUES ($1,$2,'test','running',$1,$3,$4,'/k','/r','/o','init','1.2.3.4',8080,0)",
    )
    .bind(&vm_id)
    .bind(account_id)
    .bind(vcpus)
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
    let vm_id = insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;

    db::check_quota_and_reserve(&pool, &acct.id, &vm_id, 2000, 512)
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

    insert_vm_with_status(&pool, &acct.id, 1000, 256, "running").await;

    let candidate = insert_vm_with_status(&pool, &acct.id, 1000, 256, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &acct.id, &candidate, 1000, 256)
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

    // vcpu_limit = 4000m, already using 3000m
    sqlx::query("UPDATE accounts SET vcpu_limit=4000 WHERE id=$1")
        .bind(&acct.id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 3000, 256, "running").await;
    let candidate = insert_vm_with_status(&pool, &acct.id, 2000, 256, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &acct.id, &candidate, 2000, 256)
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

    insert_vm_with_status(&pool, &acct.id, 1000, 768, "running").await;
    let candidate = insert_vm_with_status(&pool, &acct.id, 1000, 512, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &acct.id, &candidate, 1000, 512)
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
    sqlx::query("UPDATE accounts SET vcpu_limit=2000 WHERE id=$1")
        .bind(&acct.id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;
    insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;

    let candidate = insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;

    db::check_quota_and_reserve(&pool, &acct.id, &candidate, 2000, 512)
        .await
        .expect("stopped vms should not count against quota");
}

// ── vms ───────────────────────────────────────────────────────────────────────

fn new_vm(account_id: &str) -> db::NewVm {
    db::NewVm {
        id: Uuid::new_v4().to_string(),
        account_id: account_id.to_string(),
        name: "test-vm".into(),
        subdomain: Uuid::new_v4().to_string(),
        vcpus: 1000,
        memory_mb: 512,
        disk_mb: 5120,
        bandwidth_mbps: 100,
        kernel_path: "/vmlinux".into(),
        rootfs_path: "/images/ubuntu.sqfs".into(),
        overlay_path: "/overlay/vm".into(),
        real_init: "/sbin/init".into(),
        ip_address: "172.16.1.2".into(),
        exposed_port: 8080,
        base_image: "ubuntu".into(),
        cloned_from: None,
        placement_strategy: "best_fit".into(),
        required_labels: None,
        region: None,
    }
}

#[tokio::test]
async fn test_create_and_get_vm() {
    let (_c, pool) = setup().await;
    let acct = new_account("vm-create@example.com");
    db::create_account(&pool, &acct).await.expect("create account");

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    let fetched = db::get_vm(&pool, &vm_id)
        .await
        .expect("get vm")
        .expect("should exist");
    assert_eq!(fetched.id, vm_id);
    assert_eq!(fetched.account_id, acct.id);
    assert_eq!(fetched.status, "stopped");
    assert_eq!(fetched.vcpus, 1000);
    assert!(fetched.region.is_none());
}

#[tokio::test]
async fn test_get_vm_missing_returns_none() {
    let (_c, pool) = setup().await;
    let result = db::get_vm(&pool, "nonexistent-id")
        .await
        .expect("query ok");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_list_vms_by_account() {
    let (_c, pool) = setup().await;
    let acct_a = new_account_with_username("a@example.com", "a");
    let acct_b = new_account_with_username("b@example.com", "b");
    db::create_account(&pool, &acct_a).await.expect("create a");
    db::create_account(&pool, &acct_b).await.expect("create b");

    let mut vm_a = new_vm(&acct_a.id);
    vm_a.subdomain = "a-vm".into();
    let mut vm_b = new_vm(&acct_b.id);
    vm_b.subdomain = "b-vm".into();

    db::create_vm(&pool, &vm_a).await.expect("create vm_a");
    db::create_vm(&pool, &vm_b).await.expect("create vm_b");

    let vms = db::list_vms(&pool, &acct_a.id).await.expect("list vms");
    assert_eq!(vms.len(), 1);
    assert_eq!(vms[0].account_id, acct_a.id);
}

#[tokio::test]
async fn test_set_and_read_vm_region() {
    let (_c, pool) = setup().await;
    let acct = new_account("region@example.com");
    db::create_account(&pool, &acct).await.expect("create account");

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::set_vm_region(&pool, &vm_id, "us-east")
        .await
        .expect("set region");

    let fetched = db::get_vm(&pool, &vm_id)
        .await
        .expect("get vm")
        .expect("should exist");
    assert_eq!(fetched.region.as_deref(), Some("us-east"));
}

// ── regions ───────────────────────────────────────────────────────────────────

async fn insert_host_with_region(pool: &db::PgPool, id: &str, region: &str, status: &str) {
    sqlx::query(
        "INSERT INTO hosts (id, name, address, vcpu_total, mem_total_mb, images_dir, overlay_dir,
         snapshot_dir, kernel_path, snapshot_addr, last_seen_at, labels)
         VALUES ($1,$1,'http://h:4000',4000,4096,'/img','/ovl','/snap','/vmlinux','http://h:8080',0,$2)",
    )
    .bind(id)
    .bind(serde_json::json!({"region": region}))
    .execute(pool)
    .await
    .expect("insert host");

    sqlx::query("UPDATE hosts SET status=$1 WHERE id=$2")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await
        .expect("set status");
}

#[tokio::test]
async fn test_list_regions_returns_host_regions() {
    let (_c, pool) = setup().await;

    insert_host_with_region(&pool, "h-us", "us-east", "active").await;
    insert_host_with_region(&pool, "h-eu", "eu-west", "active").await;

    let regions = db::list_regions(&pool).await.expect("list regions");
    assert_eq!(regions.len(), 2);

    let names: std::collections::HashSet<_> = regions.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains("us-east"));
    assert!(names.contains("eu-west"));
    assert!(regions.iter().all(|r| r.active));
}

#[tokio::test]
async fn test_list_regions_active_flag() {
    let (_c, pool) = setup().await;

    insert_host_with_region(&pool, "h-active", "us-east", "active").await;
    insert_host_with_region(&pool, "h-offline", "eu-west", "offline").await;

    let regions = db::list_regions(&pool).await.expect("list regions");
    let us = regions.iter().find(|r| r.name == "us-east").expect("us-east");
    let eu = regions.iter().find(|r| r.name == "eu-west").expect("eu-west");
    assert!(us.active);
    assert!(!eu.active);
}
