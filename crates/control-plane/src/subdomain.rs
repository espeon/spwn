use db::PgPool;

pub async fn generate(pool: &PgPool, vm_name: &str) -> anyhow::Result<String> {
    let base = slugify(vm_name);

    let row = sqlx::query("SELECT id FROM vms WHERE subdomain = $1")
        .bind(&base)
        .fetch_optional(pool)
        .await?;

    if row.is_none() {
        return Ok(base);
    }

    for n in 2u32..=999 {
        let candidate = format!("{base}-{n}");
        let row = sqlx::query("SELECT id FROM vms WHERE subdomain = $1")
            .bind(&candidate)
            .fetch_optional(pool)
            .await?;
        if row.is_none() {
            return Ok(candidate);
        }
    }

    anyhow::bail!("could not generate unique subdomain for '{base}' after 999 attempts")
}

fn slugify(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_basic() {
        assert_eq!(slugify("my-app"), "my-app");
    }

    #[test]
    fn test_slugify_spaces_and_specials() {
        assert_eq!(slugify("My Cool VM!"), "my-cool-vm");
    }

    #[test]
    fn test_slugify_leading_trailing_hyphens() {
        assert_eq!(slugify("--hello--"), "hello");
    }

    #[test]
    fn test_slugify_uppercase() {
        assert_eq!(slugify("WebServer"), "webserver");
    }

    // ── integration tests (require DB via testcontainers) ─────────────────────

    async fn setup_db() -> (
        testcontainers::ContainerAsync<testcontainers_modules::postgres::Postgres>,
        db::PgPool,
    ) {
        use testcontainers::runners::AsyncRunner;
        let container = testcontainers_modules::postgres::Postgres::default()
            .start()
            .await
            .expect("start postgres");
        let port = container.get_host_port_ipv4(5432).await.expect("get port");
        let url = format!("postgres://postgres:postgres@localhost:{port}/postgres");
        let pool = db::connect(&url).await.expect("connect");
        db::migrate(&pool).await.expect("migrate");
        (container, pool)
    }

    async fn insert_account(pool: &db::PgPool, username: &str) -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let id = uuid::Uuid::new_v4().to_string();
        db::create_account(
            pool,
            &db::NewAccount {
                id: id.clone(),
                email: format!("{username}@test.com"),
                password_hash: "x".into(),
                username: username.into(),
                created_at: now,
            },
        )
        .await
        .expect("create account");
        id
    }

    async fn insert_vm_with_subdomain(pool: &db::PgPool, account_id: &str, subdomain: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let ns_id = format!("ns_{account_id}");
        sqlx::query(
            "INSERT INTO namespaces (id, slug, kind, owner_id, vcpu_limit, mem_limit_mb, vm_limit, created_at)
             VALUES ($1,$2,'personal',$2,8000,12288,5,0) ON CONFLICT DO NOTHING",
        )
        .bind(&ns_id)
        .bind(account_id)
        .execute(pool)
        .await
        .expect("ensure namespace");
        sqlx::query(
            "INSERT INTO vms (id, account_id, namespace_id, name, status, subdomain, vcpus, memory_mb, disk_mb,
             bandwidth_mbps, kernel_path, rootfs_path, real_init, ip_address, exposed_port,
             base_image, disk_usage_mb, created_at, placement_strategy)
             VALUES ($1,$2,$3,$4,'stopped',$5,1000,512,5120,100,'/vmlinux','/images/ubuntu.sqfs',
             '/sbin/init','172.16.1.2',8080,'ubuntu',0,$6,'best_fit')",
        )
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(account_id)
        .bind(&ns_id)
        .bind(subdomain)
        .bind(subdomain)
        .bind(now)
        .execute(pool)
        .await
        .expect("insert vm");
    }

    #[tokio::test]
    async fn generate_unique_no_conflict() {
        let (_c, pool) = setup_db().await;
        insert_account(&pool, "alice").await;

        let sub = generate(&pool, "my-app").await.unwrap();
        assert_eq!(sub, "my-app");
    }

    #[tokio::test]
    async fn generate_appends_counter_on_conflict() {
        let (_c, pool) = setup_db().await;
        let account_id = insert_account(&pool, "bob").await;
        insert_vm_with_subdomain(&pool, &account_id, "my-app").await;

        let sub = generate(&pool, "my-app").await.unwrap();
        assert_eq!(sub, "my-app-2");
    }

    #[tokio::test]
    async fn generate_increments_past_multiple_conflicts() {
        let (_c, pool) = setup_db().await;
        let account_id = insert_account(&pool, "carol").await;
        insert_vm_with_subdomain(&pool, &account_id, "api").await;
        insert_vm_with_subdomain(&pool, &account_id, "api-2").await;
        insert_vm_with_subdomain(&pool, &account_id, "api-3").await;

        let sub = generate(&pool, "api").await.unwrap();
        assert_eq!(sub, "api-4");
    }

    #[tokio::test]
    async fn generate_slugifies_vm_name() {
        let (_c, pool) = setup_db().await;
        insert_account(&pool, "dave").await;

        let sub = generate(&pool, "My Cool VM!").await.unwrap();
        assert_eq!(sub, "my-cool-vm");
    }

    #[tokio::test]
    async fn generate_conflict_across_users() {
        let (_c, pool) = setup_db().await;
        let alice_id = insert_account(&pool, "alice").await;
        let _bob_id = insert_account(&pool, "bob").await;
        insert_vm_with_subdomain(&pool, &alice_id, "web").await;

        // bob also wants "web" but alice already has it globally
        let sub = generate(&pool, "web").await.unwrap();
        assert_eq!(sub, "web-2");
    }
}
