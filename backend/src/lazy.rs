//! Writes that don't need to wait for the disk flush: audit rows (activity
//! log, file origin, connector calls) and "last seen" touches. They commit
//! with `synchronous_commit = off`, so a burst of uploads doesn't queue on WAL
//! flushes (on network storage each flush is slow). The file row itself still
//! commits durably. Worst case after a database crash: the last fraction of a
//! second of these rows is lost; nothing is ever corrupted.

use sqlx::{PgPool, Postgres};

pub async fn run<'q>(db: &PgPool, q: sqlx::query::Query<'q, Postgres, sqlx::postgres::PgArguments>) -> Result<u64, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("SET LOCAL synchronous_commit = off").execute(&mut *tx).await?;
    let n = q.execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    Ok(n)
}
