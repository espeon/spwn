use db::PgPool;
use rand::Rng;

pub async fn generate(pool: &PgPool, vm_name: &str) -> anyhow::Result<String> {
    let base = slugify(vm_name);

    if is_available(pool, &base).await? {
        return Ok(base);
    }

    for _ in 0..20 {
        let suffix = random_suffix();
        let candidate = format!("{base}-{suffix}");
        if is_available(pool, &candidate).await? {
            return Ok(candidate);
        }
    }

    anyhow::bail!("could not generate unique subdomain for '{vm_name}' after 20 attempts")
}

async fn is_available(pool: &PgPool, subdomain: &str) -> anyhow::Result<bool> {
    let row = sqlx::query("SELECT id FROM vms WHERE subdomain = $1")
        .bind(subdomain)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_none())
}

fn random_suffix() -> String {
    let mut rng = rand::thread_rng();
    (0..3).map(|_| rng.gen_range(b'a'..=b'z') as char).collect()
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
    use super::slugify;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("my-app"), "my-app");
    }

    #[test]
    fn slugify_spaces_and_specials() {
        assert_eq!(slugify("My Cool VM!"), "my-cool-vm");
    }

    #[test]
    fn slugify_leading_trailing_hyphens() {
        assert_eq!(slugify("--hello--"), "hello");
    }

    #[test]
    fn slugify_uppercase() {
        assert_eq!(slugify("WebServer"), "webserver");
    }
}
