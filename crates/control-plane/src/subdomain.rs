use db::PgPool;

pub async fn generate(pool: &PgPool, vm_name: &str, username: &str) -> anyhow::Result<String> {
    let base_name = slugify(vm_name);
    let slug = format!("{base_name}.{username}");

    let row = sqlx::query("SELECT id FROM vms WHERE subdomain = $1")
        .bind(&slug)
        .fetch_optional(pool)
        .await?;

    if row.is_none() {
        return Ok(slug);
    }

    for n in 2u32..=99 {
        let candidate = format!("{base_name}-{n}.{username}");
        let row = sqlx::query("SELECT id FROM vms WHERE subdomain = $1")
            .bind(&candidate)
            .fetch_optional(pool)
            .await?;
        if row.is_none() {
            return Ok(candidate);
        }
    }

    anyhow::bail!("could not generate unique subdomain for {slug} after 99 attempts")
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
}
