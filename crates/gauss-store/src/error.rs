#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0} not found")]
    NotFound(&'static str),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("invalid reference: {0}")]
    InvalidReference(&'static str),

    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

impl StoreError {
    /// Map unique-violation/foreign-key errors onto domain errors; everything
    /// else stays a database error.
    pub(crate) fn from_db(err: sqlx::Error, entity: &'static str) -> Self {
        if let sqlx::Error::Database(db) = &err {
            if db.is_unique_violation() {
                return Self::Conflict(format!("{entity} already exists"));
            }
            if db.is_foreign_key_violation() {
                return Self::InvalidReference(entity);
            }
        }
        Self::Sqlx(err)
    }
}
