use axum::http::StatusCode;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

type Error = (StatusCode, String);
struct Entry<T> {
    owner: String,
    expires: Instant,
    value: T,
}
pub struct Reviews<T>(Mutex<HashMap<String, Entry<T>>>);
impl<T> Default for Reviews<T> {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}
impl<T> Reviews<T> {
    pub fn insert(&self, owner: String, value: T, lifetime: Duration) -> Result<String, Error> {
        let mut entries = self.0.lock().map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Review storage unavailable".into(),
            )
        })?;
        entries.retain(|_, entry| entry.expires > Instant::now());
        if entries.len() >= 32 {
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                "Review limit reached. Wait for existing reviews to expire.".into(),
            ));
        }
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Review randomness unavailable".into(),
            )
        })?;
        let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        entries.insert(
            token.clone(),
            Entry {
                owner,
                expires: Instant::now() + lifetime,
                value,
            },
        );
        Ok(token)
    }
    pub fn take(
        &self,
        token: &str,
        owner: &str,
        confirmed: impl FnOnce(&T) -> bool,
    ) -> Result<T, Error> {
        let rejected = || {
            (StatusCode::CONFLICT, "Review expired, belongs to another session, or confirmation did not match. Review again.".into())
        };
        let mut entries = self.0.lock().map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Review storage unavailable".into(),
            )
        })?;
        entries.retain(|_, entry| entry.expires > Instant::now());
        let entry = entries.get(token).ok_or_else(rejected)?;
        if entry.owner != owner || !confirmed(&entry.value) {
            return Err(rejected());
        }
        // Consume approval before external I/O. Uncertain outcomes require a new review.
        Ok(entries.remove(token).ok_or_else(rejected)?.value)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_sessions_retain_one_use_reviews_and_confirmations() {
        let reviews = Reviews::default();
        let first = reviews
            .insert("alice".into(), 1, Duration::from_secs(60))
            .unwrap();
        let second = reviews
            .insert("bob".into(), 2, Duration::from_secs(60))
            .unwrap();
        assert!(reviews.take(&first, "bob", |_| true).is_err());
        assert!(reviews.take(&first, "alice", |_| false).is_err());
        assert_eq!(reviews.take(&first, "alice", |_| true).unwrap(), 1);
        assert!(reviews.take(&first, "alice", |_| true).is_err());
        assert_eq!(reviews.take(&second, "bob", |_| true).unwrap(), 2);
    }
    #[test]
    fn expired_reviews_are_removed_and_storage_is_bounded() {
        let reviews = Reviews::default();
        let expired = reviews.insert("a".into(), (), Duration::ZERO).unwrap();
        assert!(reviews.take(&expired, "a", |_| true).is_err());
        for _ in 0..32 {
            reviews
                .insert("a".into(), (), Duration::from_secs(60))
                .unwrap();
        }
        assert_eq!(
            reviews
                .insert("b".into(), (), Duration::from_secs(60))
                .unwrap_err()
                .0,
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}
