use rand::Rng;

const ADJECTIVES: &[&str] = &[
    "autumn", "bold", "calm", "dark", "eager", "fast", "gentle", "happy",
    "icy", "jolly", "keen", "lively", "merry", "neat", "odd", "proud",
    "quiet", "rapid", "sharp", "tidy", "urban", "vivid", "warm", "young",
];

const NOUNS: &[&str] = &[
    "brook", "cloud", "dawn", "echo", "flame", "grove", "hill", "iris",
    "jade", "kite", "lake", "moon", "nova", "oak", "pine", "reef",
    "snow", "tide", "vale", "wind",
];

pub async fn generate(pool: &db::PgPool) -> anyhow::Result<String> {
    let candidates: Vec<String> = {
        let mut rng = rand::thread_rng();
        (0..20)
            .map(|_| {
                let adj = ADJECTIVES[rng.gen_range(0..ADJECTIVES.len())];
                let noun = NOUNS[rng.gen_range(0..NOUNS.len())];
                let suffix = format!("{:04x}", rng.gen_range(0u32..=0xFFFF));
                format!("{adj}-{noun}-{suffix}")
            })
            .collect()
    };

    for subdomain in candidates {
        let row = sqlx::query("SELECT id FROM vms WHERE subdomain = $1")
            .bind(&subdomain)
            .fetch_optional(pool)
            .await?;
        if row.is_none() {
            return Ok(subdomain);
        }
    }
    anyhow::bail!("could not generate unique subdomain after 20 attempts")
}
