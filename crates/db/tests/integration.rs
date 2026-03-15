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
        dotfiles_repo: None,
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
            dotfiles_repo: None,
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
            dotfiles_repo: None,
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

async fn create_test_namespace(pool: &db::PgPool, account_id: &str) {
    let ns_id = format!("ns_{account_id}");
    sqlx::query(
        "INSERT INTO namespaces (id, slug, kind, owner_id, vcpu_limit, mem_limit_mb, vm_limit, created_at)
         VALUES ($1,$2,'personal',$2,8000,12288,5,0)
         ON CONFLICT DO NOTHING",
    )
    .bind(&ns_id)
    .bind(account_id)
    .execute(pool)
    .await
    .expect("create test namespace");
}

async fn insert_vm_with_status(
    pool: &db::PgPool,
    account_id: &str,
    vcpus: i64,
    memory_mb: i32,
    status: &str,
) -> String {
    let vm_id = uuid::Uuid::new_v4().to_string();
    let ns_id = format!("ns_{account_id}");
    sqlx::query(
        "INSERT INTO vms (id, account_id, namespace_id, name, status, subdomain, vcpus, memory_mb,
         kernel_path, rootfs_path, overlay_path, real_init, ip_address, exposed_port, created_at)
         VALUES ($1,$2,$3,'test','running',$1,$4,$5,'/k','/r','/o','init','1.2.3.4',8080,0)",
    )
    .bind(&vm_id)
    .bind(account_id)
    .bind(&ns_id)
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
    create_test_namespace(&pool, &acct.id).await;
    let ns_id = format!("ns_{}", acct.id);

    // create a stopped vm to reserve
    let vm_id = insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;

    db::check_quota_and_reserve(&pool, &ns_id, &vm_id, 2000, 512)
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
    create_test_namespace(&pool, &acct.id).await;
    let ns_id = format!("ns_{}", acct.id);

    // set vm_limit = 1, then put 1 running vm
    sqlx::query("UPDATE namespaces SET vm_limit=1 WHERE id=$1")
        .bind(&ns_id)
        .execute(&pool)
        .await
        .expect("update limit");

    insert_vm_with_status(&pool, &acct.id, 1000, 256, "running").await;

    let candidate = insert_vm_with_status(&pool, &acct.id, 1000, 256, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &ns_id, &candidate, 1000, 256)
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
    create_test_namespace(&pool, &acct.id).await;
    let ns_id = format!("ns_{}", acct.id);

    // vcpu_limit = 4000m, already using 3000m
    sqlx::query("UPDATE namespaces SET vcpu_limit=4000 WHERE id=$1")
        .bind(&ns_id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 3000, 256, "running").await;
    let candidate = insert_vm_with_status(&pool, &acct.id, 2000, 256, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &ns_id, &candidate, 2000, 256)
        .await
        .expect_err("should exceed vcpu limit");
    assert!(matches!(err, db::QuotaError::Exceeded(_)));
}

#[tokio::test]
async fn test_quota_mem_limit_exceeded() {
    let (_c, pool) = setup().await;
    let acct = new_account("quota-mem@example.com");
    db::create_account(&pool, &acct).await.expect("create");
    create_test_namespace(&pool, &acct.id).await;
    let ns_id = format!("ns_{}", acct.id);

    // mem_limit_mb = 1024, already using 768
    sqlx::query("UPDATE namespaces SET mem_limit_mb=1024 WHERE id=$1")
        .bind(&ns_id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 1000, 768, "running").await;
    let candidate = insert_vm_with_status(&pool, &acct.id, 1000, 512, "stopped").await;

    let err = db::check_quota_and_reserve(&pool, &ns_id, &candidate, 1000, 512)
        .await
        .expect_err("should exceed mem limit");
    assert!(matches!(err, db::QuotaError::Exceeded(_)));
}

#[tokio::test]
async fn test_quota_only_counts_active_vms() {
    let (_c, pool) = setup().await;
    let acct = new_account("quota-stopped@example.com");
    db::create_account(&pool, &acct).await.expect("create");
    create_test_namespace(&pool, &acct.id).await;
    let ns_id = format!("ns_{}", acct.id);

    // stopped vms don't count against quota
    sqlx::query("UPDATE namespaces SET vcpu_limit=2000 WHERE id=$1")
        .bind(&ns_id)
        .execute(&pool)
        .await
        .expect("update");

    insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;
    insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;

    let candidate = insert_vm_with_status(&pool, &acct.id, 2000, 512, "stopped").await;

    db::check_quota_and_reserve(&pool, &ns_id, &candidate, 2000, 512)
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
        namespace_id: format!("ns_{account_id}"),
    }
}

#[tokio::test]
async fn test_create_and_get_vm() {
    let (_c, pool) = setup().await;
    let acct = new_account("vm-create@example.com");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

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
    create_test_namespace(&pool, &acct_a.id).await;
    create_test_namespace(&pool, &acct_b.id).await;

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
    create_test_namespace(&pool, &acct.id).await;

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

// ── vm lifecycle ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_vm_status_transitions() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("status@example.com", "status-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::set_vm_status(&pool, &vm_id, "starting")
        .await
        .expect("set starting");
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.status, "starting");

    db::set_vm_running(&pool, &vm_id, 1234, "fc-tap-1", "/run/fc.sock")
        .await
        .expect("set running");
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.status, "running");
    assert_eq!(fetched.pid, Some(1234));
    assert_eq!(fetched.tap_device.as_deref(), Some("fc-tap-1"));
    assert_eq!(fetched.socket_path.as_deref(), Some("/run/fc.sock"));
    assert!(fetched.last_started_at.is_some());

    db::set_vm_stopped(&pool, &vm_id).await.expect("set stopped");
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.status, "stopped");
    assert!(fetched.pid.is_none());
    assert!(fetched.tap_device.is_none());
    assert!(fetched.socket_path.is_none());
}

#[tokio::test]
async fn test_set_vm_pid() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("pid@example.com", "pid-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::set_vm_pid(&pool, &vm_id, 9999).await.expect("set pid");
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.pid, Some(9999));
}

#[tokio::test]
async fn test_set_vm_host_and_get_vms_by_host() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("host@example.com", "host-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;
    db::upsert_host(&pool, &new_host("host-abc")).await.expect("upsert host");

    let mut vm = new_vm(&acct.id);
    vm.subdomain = "host-vm".into();
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::set_vm_host(&pool, &vm_id, "host-abc").await.expect("set host");
    let vms = db::get_vms_by_host(&pool, "host-abc").await.expect("get by host");
    assert_eq!(vms.len(), 1);
    assert_eq!(vms[0].id, vm_id);
    assert_eq!(vms[0].host_id.as_deref(), Some("host-abc"));
}

#[tokio::test]
async fn test_delete_vm() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("delete@example.com", "delete-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::delete_vm(&pool, &vm_id).await.expect("delete vm");
    let result = db::get_vm(&pool, &vm_id).await.expect("query");
    assert!(result.is_none());
}

// ── vm queries ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_get_vm_by_subdomain() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("sub@example.com", "sub-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let mut vm = new_vm(&acct.id);
    vm.subdomain = "my-vm.sub-user".into();
    db::create_vm(&pool, &vm).await.expect("create vm");

    let found = db::get_vm_by_subdomain(&pool, "my-vm.sub-user")
        .await
        .expect("query")
        .expect("should exist");
    assert_eq!(found.subdomain, "my-vm.sub-user");

    let not_found = db::get_vm_by_subdomain(&pool, "nope.sub-user")
        .await
        .expect("query");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_get_vm_by_name() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("name@example.com", "name-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let mut vm = new_vm(&acct.id);
    vm.name = "my-named-vm".into();
    vm.subdomain = "named".into();
    db::create_vm(&pool, &vm).await.expect("create vm");

    let found = db::get_vm_by_name(&pool, &acct.id, "my-named-vm")
        .await
        .expect("query")
        .expect("should exist");
    assert_eq!(found.name, "my-named-vm");

    let wrong_acct = db::get_vm_by_name(&pool, "other-id", "my-named-vm")
        .await
        .expect("query");
    assert!(wrong_acct.is_none());
}

#[tokio::test]
async fn test_get_vms_by_status_and_get_all() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("allvms@example.com", "allvms-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let mut vm_a = new_vm(&acct.id);
    vm_a.subdomain = "vm-a".into();
    let mut vm_b = new_vm(&acct.id);
    vm_b.subdomain = "vm-b".into();
    db::create_vm(&pool, &vm_a).await.expect("create a");
    db::create_vm(&pool, &vm_b).await.expect("create b");
    db::set_vm_status(&pool, &vm_b.id, "running").await.expect("set running");

    let stopped = db::get_vms_by_status(&pool, "stopped").await.expect("by status");
    assert!(stopped.iter().any(|v| v.id == vm_a.id));

    let running = db::get_vms_by_status(&pool, "running").await.expect("by status");
    assert!(running.iter().any(|v| v.id == vm_b.id));

    let all = db::get_all_vms(&pool).await.expect("get all");
    assert!(all.len() >= 2);
}

#[tokio::test]
async fn test_get_used_ips_for_host() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("ips@example.com", "ips-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;
    db::upsert_host(&pool, &new_host("host-ips")).await.expect("upsert host");

    let mut vm = new_vm(&acct.id);
    vm.subdomain = "ip-vm".into();
    vm.ip_address = "172.16.1.2".into();
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");
    db::set_vm_host(&pool, &vm_id, "host-ips").await.expect("set host");

    let ips = db::get_used_ips_for_host(&pool, "host-ips").await.expect("get ips");
    assert!(ips.contains(&"172.16.1.2".to_string()));
}

// ── vm mutations ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_rename_vm() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("rename@example.com", "rename-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let mut vm = new_vm(&acct.id);
    vm.name = "old-name".into();
    vm.subdomain = "old-name.rename-user".into();
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::rename_vm(&pool, &vm_id, "new-name", "new-name.rename-user")
        .await
        .expect("rename");

    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.name, "new-name");
    assert_eq!(fetched.subdomain, "new-name.rename-user");
}

#[tokio::test]
async fn test_update_vm_port_and_resources() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("resources@example.com", "resources-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::update_vm_port(&pool, &vm_id, 9090).await.expect("update port");
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.exposed_port, 9090);

    db::update_vm_resources(&pool, &vm_id, 2000, 1024, 200)
        .await
        .expect("update resources");
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.vcpus, 2000);
    assert_eq!(fetched.memory_mb, 1024);
    assert_eq!(fetched.bandwidth_mbps, 200);
}

#[tokio::test]
async fn test_update_disk_usage() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("disk@example.com", "disk-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::update_disk_usage_mb(&pool, &vm_id, 1337).await.expect("update disk");
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.disk_usage_mb, 1337);
}

// ── vm events ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_log_and_list_vm_events() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("events@example.com", "events-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::log_event(&pool, &vm_id, "started", None).await.expect("log started");
    db::log_event(&pool, &vm_id, "stopped", Some("graceful")).await.expect("log stopped");

    let events = db::list_vm_events(&pool, &vm_id, 10, None).await.expect("list events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "stopped");
    assert_eq!(events[0].metadata.as_deref(), Some("graceful"));
    assert_eq!(events[1].event, "started");
    assert!(events[1].metadata.is_none());

    let cursor_id = events[0].id;
    let paged = db::list_vm_events(&pool, &vm_id, 10, Some(cursor_id))
        .await
        .expect("list with cursor");
    assert_eq!(paged.len(), 1);
    assert_eq!(paged[0].event, "started");
}

// ── snapshots ─────────────────────────────────────────────────────────────────

fn new_snapshot(vm_id: &str) -> db::NewSnapshot {
    db::NewSnapshot {
        id: Uuid::new_v4().to_string(),
        vm_id: vm_id.to_string(),
        label: Some("snap-1".into()),
        snapshot_path: "/snap/vm.snap".into(),
        mem_path: "/snap/vm.mem".into(),
        size_bytes: 104857600,
    }
}

#[tokio::test]
async fn test_snapshot_crud() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("snap@example.com", "snap-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;
    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    let snap = new_snapshot(&vm_id);
    let snap_id = snap.id.clone();
    db::create_snapshot(&pool, &snap).await.expect("create snapshot");

    let fetched = db::get_snapshot(&pool, &snap_id)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(fetched.vm_id, vm_id);
    assert_eq!(fetched.label.as_deref(), Some("snap-1"));
    assert_eq!(fetched.size_bytes, 104857600);

    let list = db::list_snapshots(&pool, &vm_id).await.expect("list");
    assert_eq!(list.len(), 1);

    let count = db::count_snapshots(&pool, &vm_id).await.expect("count");
    assert_eq!(count, 1);

    db::delete_snapshot(&pool, &snap_id).await.expect("delete");
    let gone = db::get_snapshot(&pool, &snap_id).await.expect("query");
    assert!(gone.is_none());

    let count = db::count_snapshots(&pool, &vm_id).await.expect("count after delete");
    assert_eq!(count, 0);
}

// ── hosts ─────────────────────────────────────────────────────────────────────

fn new_host(id: &str) -> db::NewHost {
    db::NewHost {
        id: id.to_string(),
        name: format!("{id}-name"),
        address: "http://localhost:4000".into(),
        vcpu_total: 8000,
        mem_total_mb: 16384,
        images_dir: "/img".into(),
        overlay_dir: "/ovl".into(),
        snapshot_dir: "/snap".into(),
        kernel_path: "/vmlinux".into(),
        snapshot_addr: "http://localhost:8080".into(),
    }
}

#[tokio::test]
async fn test_upsert_and_get_host() {
    let (_c, pool) = setup().await;

    let host = new_host("host-001");
    db::upsert_host(&pool, &host).await.expect("upsert");

    let fetched = db::get_host(&pool, "host-001")
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(fetched.id, "host-001");
    assert_eq!(fetched.vcpu_total, 8000);
    assert_eq!(fetched.status, "active");

    let updated = db::NewHost { vcpu_total: 16000, ..new_host("host-001") };
    db::upsert_host(&pool, &updated).await.expect("upsert update");
    let fetched = db::get_host(&pool, "host-001").await.expect("get").expect("exists");
    assert_eq!(fetched.vcpu_total, 16000);

    let not_found = db::get_host(&pool, "nonexistent").await.expect("query");
    assert!(not_found.is_none());
}

#[tokio::test]
async fn test_list_hosts_and_active_hosts() {
    let (_c, pool) = setup().await;

    db::upsert_host(&pool, &new_host("host-a")).await.expect("upsert a");
    db::upsert_host(&pool, &new_host("host-b")).await.expect("upsert b");
    db::set_host_status(&pool, "host-b", "offline").await.expect("set offline");

    let all = db::list_hosts(&pool).await.expect("list all");
    assert!(all.len() >= 2);

    let active = db::list_active_hosts(&pool).await.expect("list active");
    assert!(active.iter().any(|h| h.id == "host-a"));
    assert!(!active.iter().any(|h| h.id == "host-b"));
}

#[tokio::test]
async fn test_update_host_heartbeat() {
    let (_c, pool) = setup().await;

    db::upsert_host(&pool, &new_host("host-hb")).await.expect("upsert");
    db::update_host_heartbeat(&pool, "host-hb", 2000, 4096)
        .await
        .expect("heartbeat");

    let fetched = db::get_host(&pool, "host-hb").await.expect("get").expect("exists");
    assert_eq!(fetched.vcpu_used, 2000);
    assert_eq!(fetched.mem_used_mb, 4096);
}

// ── cli auth codes ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_cli_auth_code_flow() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("cli@example.com", "cli-user");
    db::create_account(&pool, &acct).await.expect("create account");

    let code = "test-cli-code-abc";
    db::create_cli_auth_code(&pool, code, 9_999_999_999)
        .await
        .expect("create code");

    let fetched = db::get_cli_auth_code(&pool, code)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(fetched.code, code);
    assert_eq!(fetched.status, "pending");
    assert!(fetched.account_id.is_none());

    db::authorize_cli_auth_code(&pool, code, &acct.id)
        .await
        .expect("authorize");

    let authorized = db::get_cli_auth_code(&pool, code)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(authorized.status, "authorized");
    assert_eq!(authorized.account_id.as_deref(), Some(acct.id.as_str()));

    let missing = db::get_cli_auth_code(&pool, "no-such-code").await.expect("query");
    assert!(missing.is_none());
}

// ── account updates ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_update_theme() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("theme@example.com", "theme-user");
    let id = acct.id.clone();
    db::create_account(&pool, &acct).await.expect("create account");

    db::update_theme(&pool, &id, &db::UpdateTheme { theme: "dracula".into() })
        .await
        .expect("update theme");

    let fetched = db::get_account(&pool, &id).await.expect("get").expect("exists");
    assert_eq!(fetched.theme, "dracula");
}

#[tokio::test]
async fn test_update_username() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("uname@example.com", "old-uname");
    let id = acct.id.clone();
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &id).await;

    let mut vm = new_vm(&id);
    vm.name = "myvm".into();
    vm.subdomain = "myvm".into();
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    db::update_username(
        &pool,
        &id,
        &db::UsernameUpdate {
            new_username: "new-uname".into(),
        },
    )
    .await
    .expect("update username");

    // subdomain is unchanged — flat subdomains are decoupled from username
    let fetched = db::get_vm(&pool, &vm_id).await.expect("get").expect("exists");
    assert_eq!(fetched.subdomain, "myvm");

    let acct = db::get_account(&pool, &id).await.expect("get").expect("exists");
    assert_eq!(acct.username, "new-uname");
}

#[tokio::test]
async fn test_list_all_vms_admin() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("admin@example.com", "admin-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    db::create_vm(&pool, &vm).await.expect("create vm");

    let all = db::list_all_vms_admin(&pool).await.expect("list admin");
    assert!(all.iter().any(|v| v.account_id == acct.id));
    let record = all.iter().find(|v| v.account_id == acct.id).unwrap();
    assert_eq!(record.username, "admin-user");
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

// ── namespaces ────────────────────────────────────────────────────────────────

fn new_namespace(owner_id: &str, slug: &str) -> db::NewNamespace {
    db::NewNamespace {
        id: format!("ns_{}", Uuid::new_v4()),
        slug: slug.into(),
        kind: "personal".into(),
        display_name: Some(slug.into()),
        owner_id: owner_id.into(),
        vcpu_limit: 8000,
        mem_limit_mb: 12288,
        vm_limit: 5,
        created_at: 0,
    }
}

#[tokio::test]
async fn test_create_and_get_namespace() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("ns-create@example.com", "ns-user");
    db::create_account(&pool, &acct).await.expect("create account");

    let ns = new_namespace(&acct.id, "ns-user");
    let ns_id = ns.id.clone();
    db::create_namespace(&pool, &ns).await.expect("create namespace");

    let fetched = db::get_namespace(&pool, &ns_id)
        .await
        .expect("get namespace")
        .expect("should exist");
    assert_eq!(fetched.id, ns_id);
    assert_eq!(fetched.slug, "ns-user");
    assert_eq!(fetched.kind, "personal");
    assert_eq!(fetched.owner_id, acct.id);
    assert_eq!(fetched.vcpu_limit, 8000);
    assert_eq!(fetched.mem_limit_mb, 12288);
    assert_eq!(fetched.vm_limit, 5);
}

#[tokio::test]
async fn test_get_namespace_missing_returns_none() {
    let (_c, pool) = setup().await;
    let result = db::get_namespace(&pool, "ns_nonexistent").await.expect("query");
    assert!(result.is_none());
}

#[tokio::test]
async fn test_namespace_slug_must_be_unique() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("ns-slug@example.com", "slug-user");
    db::create_account(&pool, &acct).await.expect("create account");

    let ns1 = new_namespace(&acct.id, "same-slug");
    db::create_namespace(&pool, &ns1).await.expect("create first");

    let mut ns2 = new_namespace(&acct.id, "same-slug");
    ns2.id = format!("ns_{}", Uuid::new_v4());
    let err = db::create_namespace(&pool, &ns2).await.expect_err("duplicate slug should fail");
    assert!(err.to_string().contains("unique") || err.to_string().contains("duplicate"));
}

#[tokio::test]
async fn test_get_personal_namespace() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("ns-personal@example.com", "personal-user");
    db::create_account(&pool, &acct).await.expect("create account");

    let ns = db::NewNamespace {
        id: format!("ns_{}", &acct.id),
        slug: acct.username.clone(),
        kind: "personal".into(),
        display_name: Some(acct.username.clone()),
        owner_id: acct.id.clone(),
        vcpu_limit: 8000,
        mem_limit_mb: 12288,
        vm_limit: 5,
        created_at: 0,
    };
    db::create_namespace(&pool, &ns).await.expect("create namespace");

    let found = db::get_personal_namespace(&pool, &acct.id)
        .await
        .expect("query")
        .expect("should exist");
    assert_eq!(found.kind, "personal");
    assert_eq!(found.owner_id, acct.id);

    // returns None for unknown account
    let missing = db::get_personal_namespace(&pool, "unknown-id").await.expect("query");
    assert!(missing.is_none());
}

#[tokio::test]
async fn test_add_and_get_namespace_member() {
    let (_c, pool) = setup().await;
    let owner = new_account_with_username("ns-owner@example.com", "owner-user");
    let member = new_account_with_username("ns-member@example.com", "member-user");
    db::create_account(&pool, &owner).await.expect("create owner");
    db::create_account(&pool, &member).await.expect("create member");

    let ns = new_namespace(&owner.id, "owner-org");
    let ns_id = ns.id.clone();
    db::create_namespace(&pool, &ns).await.expect("create namespace");

    db::add_namespace_member(&pool, &ns_id, &owner.id, "owner")
        .await
        .expect("add owner");
    db::add_namespace_member(&pool, &ns_id, &member.id, "member")
        .await
        .expect("add member");

    let owner_row = db::get_namespace_member(&pool, &ns_id, &owner.id)
        .await
        .expect("query")
        .expect("should exist");
    assert_eq!(owner_row.role, "owner");

    let member_row = db::get_namespace_member(&pool, &ns_id, &member.id)
        .await
        .expect("query")
        .expect("should exist");
    assert_eq!(member_row.role, "member");

    let not_member = db::get_namespace_member(&pool, &ns_id, "random-id")
        .await
        .expect("query");
    assert!(not_member.is_none());
}

#[tokio::test]
async fn test_list_namespaces_for_account() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("ns-list@example.com", "list-user");
    let other = new_account_with_username("ns-other@example.com", "other-user");
    db::create_account(&pool, &acct).await.expect("create account");
    db::create_account(&pool, &other).await.expect("create other");

    let personal = new_namespace(&acct.id, "list-user");
    let org = db::NewNamespace {
        id: format!("ns_{}", Uuid::new_v4()),
        slug: "list-user-org".into(),
        kind: "org".into(),
        display_name: Some("List User Org".into()),
        owner_id: acct.id.clone(),
        vcpu_limit: 16000,
        mem_limit_mb: 32768,
        vm_limit: 20,
        created_at: 0,
    };
    let unrelated = new_namespace(&other.id, "other-user");

    db::create_namespace(&pool, &personal).await.expect("create personal");
    db::create_namespace(&pool, &org).await.expect("create org");
    db::create_namespace(&pool, &unrelated).await.expect("create unrelated");

    db::add_namespace_member(&pool, &personal.id, &acct.id, "owner").await.expect("add personal owner");
    db::add_namespace_member(&pool, &org.id, &acct.id, "owner").await.expect("add org owner");
    db::add_namespace_member(&pool, &unrelated.id, &other.id, "owner").await.expect("add unrelated owner");

    let namespaces = db::list_namespaces_for_account(&pool, &acct.id)
        .await
        .expect("list namespaces");

    assert_eq!(namespaces.len(), 2);
    let slugs: std::collections::HashSet<_> = namespaces.iter().map(|n| n.slug.as_str()).collect();
    assert!(slugs.contains("list-user"));
    assert!(slugs.contains("list-user-org"));
    assert!(!slugs.contains("other-user"));
}

#[tokio::test]
async fn test_vm_belongs_to_namespace() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("ns-vm@example.com", "ns-vm-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;

    let vm = new_vm(&acct.id);
    let vm_id = vm.id.clone();
    db::create_vm(&pool, &vm).await.expect("create vm");

    let fetched = db::get_vm(&pool, &vm_id).await.expect("get vm").expect("exists");
    assert_eq!(fetched.namespace_id, format!("ns_{}", acct.id));
}

#[tokio::test]
async fn test_namespace_quota_applies_to_namespace_vms() {
    let (_c, pool) = setup().await;
    let acct = new_account_with_username("ns-quota@example.com", "ns-quota-user");
    db::create_account(&pool, &acct).await.expect("create account");
    create_test_namespace(&pool, &acct.id).await;
    let ns_id = format!("ns_{}", acct.id);

    // tighten the vm_limit to 1
    sqlx::query("UPDATE namespaces SET vm_limit=1 WHERE id=$1")
        .bind(&ns_id)
        .execute(&pool)
        .await
        .expect("update limit");

    // one running VM already in the namespace
    insert_vm_with_status(&pool, &acct.id, 1000, 256, "running").await;

    // candidate to start — should be blocked by vm_limit
    let candidate = insert_vm_with_status(&pool, &acct.id, 500, 128, "stopped").await;
    let err = db::check_quota_and_reserve(&pool, &ns_id, &candidate, 500, 128)
        .await
        .expect_err("vm_limit exhausted");
    assert!(matches!(err, db::QuotaError::Exceeded(_)));
}
