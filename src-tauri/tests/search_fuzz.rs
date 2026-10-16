//! Property-style fuzz harness for `search_notes_query`.
//!
//! `cargo test --test search_fuzz` (no model downloads, no network) runs
//! hundreds of randomized queries through the same LIKE path the UI uses and
//! asserts no input can panic, raise a SQL error, or return malformed rows.
//!
//! The fuzzer is deterministic via a fixed seed so failures reproduce; bump
//! the seed and re-run to extend coverage. Each input class is checked
//! separately so a regression points at the right edge.
//!
//! What's NOT covered here: accuracy of the matches (the LIKE path is a
//! substring scan, so correctness is mechanical), and FTS5 (the project
//! uses LIKE for the notes search today; a future FTS5 migration should
//! add a parallel fuzz).

use arya_lib::db::test_pool;
use arya_lib::notes::search_notes_query;

const FUZZ_ITERATIONS: usize = 256;
const DETERMINISTIC_SEED: u64 = 0xA17A_C0DE;

async fn seed_corpus(pool: &sqlx::SqlitePool) {
    let corpus = [
        ("c1", "alpha bravo charlie"),
        ("c2", "100% complete _under_score"),
        ("c3", "with 'apos' and \"quote\""),
        ("c4", "unicode: 漢字 ñ ümlaut 🦀 emoji"),
        ("c5", "sql: SELECT * FROM notes WHERE x = 'y'"),
        ("c6", ""), // empty title is allowed
    ];
    for (id, title) in corpus {
        sqlx::query(
            "INSERT INTO notes (id, title, created_at) VALUES (?1, ?2, '2024-01-01T00:00:00Z')",
        )
        .bind(id)
        .bind(title)
        .execute(pool)
        .await
        .expect("seed");
    }
}

/// Tiny xorshift PRNG so the fuzzer is reproducible without a `rand` dep.
struct Xs(u64);
impl Xs {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn range(&mut self, n: usize) -> usize {
        (self.next() as usize) % n
    }
}

/// Build a string the fuzzer would never produce by accident: every byte
/// class (control, NUL, %, _, quote, backslash, surrogate-ish) at least
/// once, repeated. Owns its strings so the returned `Vec` doesn't borrow
/// temporaries.
fn adversarial_pool() -> Vec<String> {
    vec![
        "".into(),
        " ".into(),
        "%".into(),
        "_".into(),
        "\\".into(),
        "\"".into(),
        "'".into(),
        "''".into(),
        "'; DROP TABLE notes; --".into(),
        "%' OR 1=1 --".into(),
        "_%_%".into(),
        "\0".into(),
        "\n\r\t".into(),
        "🚀🦀🔥".into(), // 4-byte UTF-8 (above the BMP)
        "漢漢漢".into(),
        "ascii \0\u{1}\u{7f}".into(),
        "x".repeat(64), // long-ish to exercise buffer growth paths
        "alpha%".into(),
        "%alpha".into(),
        "al%ha".into(),
    ]
}

async fn assert_safe(pool: &sqlx::SqlitePool, input: &str) {
    // Property 1: never error.
    let hits = search_notes_query(pool, input)
        .await
        .unwrap_or_else(|e| panic!("search panicked on input {input:?}: {e}"));
    // Property 2: every returned id resolves to an existing row.
    for hit in &hits {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM notes WHERE id = ?1")
            .bind(&hit.id)
            .fetch_one(pool)
            .await
            .unwrap();
        assert!(exists > 0, "search returned dangling id {}", hit.id);
    }
}

#[tokio::test]
async fn adversarial_inputs_never_panic_or_error() {
    let pool = test_pool().await;
    seed_corpus(&pool).await;
    for s in adversarial_pool() {
        assert_safe(&pool, &s).await;
    }
}

#[tokio::test]
async fn randomized_inputs_never_panic_or_error() {
    let pool = test_pool().await;
    seed_corpus(&pool).await;

    let mut rng = Xs(DETERMINISTIC_SEED);
    let alphabet: Vec<char> = (0..64)
        .map(|_| char::from_u32(0x20 + rng.range(96) as u32).unwrap_or('?'))
        .chain(
            ['%', '_', '\'', '"', '\\', '\0', '\n', '\r', '\u{0}']
                .iter()
                .copied(),
        )
        .collect();
    let special = ['%', '_', '\'', '"', '\\', '\0'];

    for _ in 0..FUZZ_ITERATIONS {
        let len = 1 + rng.range(40);
        let mut s = String::with_capacity(len);
        for _ in 0..len {
            if rng.range(8) == 0 {
                s.push(special[rng.range(special.len())]);
            } else {
                s.push(alphabet[rng.range(alphabet.len())]);
            }
        }
        assert_safe(&pool, &s).await;
    }
}

#[tokio::test]
async fn empty_input_returns_all_rows() {
    let pool = test_pool().await;
    seed_corpus(&pool).await;
    let hits = search_notes_query(&pool, "").await.unwrap();
    // Empty LIKE '%'  matches every row.
    assert_eq!(hits.len(), 6, "empty query should match every seeded row");
}

#[tokio::test]
async fn wildcards_only_match_nothing_if_no_literal_row_carries_them() {
    let pool = test_pool().await;
    // A pool with NO '%' anywhere — the LIKE wildcard must not falsely hit.
    sqlx::query("INSERT INTO notes (id, title, created_at) VALUES ('z1', 'plain title', '2024-01-01T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO notes (id, title, created_at) VALUES ('z2', 'also plain', '2024-01-01T00:00:00Z')")
        .execute(&pool)
        .await
        .unwrap();

    let hits = search_notes_query(&pool, "%").await.unwrap();
    assert!(
        hits.is_empty(),
        "a literal % query should not match plain rows; got {hits:?}"
    );

    let hits = search_notes_query(&pool, "_").await.unwrap();
    assert!(
        hits.is_empty(),
        "a literal _ query should not match plain rows; got {hits:?}"
    );
}
