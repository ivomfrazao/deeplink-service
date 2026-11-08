use chrono::{DateTime, Utc};
use constant_time_eq::constant_time_eq;
use uuid::Uuid;

/// Public `client_id` used in Basic Auth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClientId(pub Uuid);

impl ClientId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ClientId {
    fn default() -> Self {
        Self::new()
    }
}

/// Logical routing identifier, e.g. `onboarding-service`. Immutable once set.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Audience(String);

#[derive(Debug, PartialEq, Eq)]
pub enum ClientError {
    AlreadyDeactivated,
    EmptyAudience,
    EmptySecretHash,
    ExpiresAtInPast,
    NotActive,
}

impl Audience {
    pub fn new(value: impl Into<String>) -> Result<Self, ClientError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ClientError::EmptyAudience);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An application that interacts with the Deeplink Service.
/// See docs/data_model.md#client.
#[derive(Debug, Clone)]
pub struct Client {
    pub id: ClientId,
    pub secret_hash: String,
    pub previous_secret_hash: Option<String>,
    pub previous_secret_expires_at: Option<DateTime<Utc>>,
    pub audience: Audience,
    pub name: String,
    pub active: bool,
    pub deactivated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub created_by: String,
    pub updated_at: DateTime<Utc>,
    pub updated_by: String,
}

impl Client {
    pub fn new(
        audience: Audience,
        name: String,
        secret_hash: String,
        now: DateTime<Utc>,
        created_by: String,
    ) -> Self {
        Self {
            id: ClientId::new(),
            secret_hash,
            previous_secret_hash: None,
            previous_secret_expires_at: None,
            audience,
            name,
            active: true,
            deactivated_at: None,
            created_at: now,
            created_by: created_by.clone(),
            updated_at: now,
            updated_by: created_by,
        }
    }

    /// Clients are never hard-deleted; deactivation is the retirement mechanism.
    pub fn deactivate(&mut self, now: DateTime<Utc>, by: String) -> Result<(), ClientError> {
        if !self.active {
            return Err(ClientError::AlreadyDeactivated);
        }
        self.active = false;
        self.deactivated_at = Some(now);
        self.updated_at = now;
        self.updated_by = by;
        Ok(())
    }

    /// Rotate the secret hash for this client. The previous secret hash is retained for a grace period, after which it is no longer accepted.
    pub fn rotate_secret(
        &mut self,
        new_secret_hash: String,
        expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
        by: String,
    ) -> Result<(), ClientError> {
        if !self.active {
            return Err(ClientError::NotActive);
        }
        if new_secret_hash.trim().is_empty() {
            return Err(ClientError::EmptySecretHash);
        }
        if expires_at <= now {
            return Err(ClientError::ExpiresAtInPast);
        }

        self.previous_secret_hash = Some(self.secret_hash.clone());
        self.previous_secret_expires_at = Some(expires_at);
        self.secret_hash = new_secret_hash;
        self.updated_at = now;
        self.updated_by = by;
        Ok(())
    }

    /// Check if the provided secret hash is accepted for this client, considering the current and previous hashes and their expiration.
    pub fn accepts_secret_hash(&self, candidate: &String, now: DateTime<Utc>) -> bool {
        if !self.active {
            return false;
        }
        if constant_time_eq(self.secret_hash.as_bytes(), candidate.as_bytes()) {
            return true;
        }
        if let (Some(prev_hash), Some(expires_at)) =
            (&self.previous_secret_hash, self.previous_secret_expires_at)
            && expires_at > now
            && constant_time_eq(prev_hash.as_bytes(), candidate.as_bytes())
        {
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn test_now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).unwrap()
    }

    fn client() -> Client {
        Client::new(
            Audience::new("onboarding-service").unwrap(),
            "Onboarding".into(),
            "hash".into(),
            test_now(),
            "operator:abc".into(),
        )
    }

    #[test]
    fn audience_rejects_blank() {
        let blank = "  ";

        assert_eq!(Audience::new(blank), Err(ClientError::EmptyAudience));
    }

    #[test]
    fn new_client_is_active() {
        let c = client();

        assert!(c.active && c.deactivated_at.is_none());
    }

    #[test]
    fn deactivate_twice_fails() {
        let mut c = client();
        let now = test_now();
        let by = "operator:abc";

        c.deactivate(now, by.into()).unwrap();

        assert!(!c.active);
        assert_eq!(
            c.deactivate(now, by.into()),
            Err(ClientError::AlreadyDeactivated)
        );
    }

    #[test]
    fn rotate_secret_rejects_blank() {
        let mut c = client();
        let now = test_now();
        let expires_at = now + Duration::hours(1);
        let blank_hash = String::new();
        let by = "operator:abc";

        assert_eq!(
            c.rotate_secret(blank_hash, expires_at, now, by.into()),
            Err(ClientError::EmptySecretHash)
        );
    }

    #[test]
    fn rotate_secret_rejects_past_expiration() {
        let mut c = client();
        let now = test_now();
        let expires_at = now - Duration::hours(1);
        let new_hash = "newhash";
        let by = "operator:abc";

        assert_eq!(
            c.rotate_secret(new_hash.into(), expires_at, now, by.into()),
            Err(ClientError::ExpiresAtInPast)
        );
    }

    #[test]
    fn rotate_secret_rejects_inactive_client() {
        let mut c = client();
        let now = test_now();
        let expires_at = now + Duration::hours(1);
        let new_hash = "newhash";
        let by = "operator:abc";

        c.deactivate(now, by.into()).unwrap();

        assert_eq!(
            c.rotate_secret(new_hash.into(), expires_at, now, by.into()),
            Err(ClientError::NotActive)
        );
    }

    #[test]
    fn accepts_secret_hash_rejects_inactive_client() {
        let mut c = client();
        let now = test_now();
        let hash = c.secret_hash.clone();
        let by = "operator:abc";

        c.deactivate(now, by.into()).unwrap();

        assert!(!c.accepts_secret_hash(&hash, now));
    }

    #[test]
    fn accepts_secret_hash_checks_current_and_previous() {
        let mut c = client();
        let now = test_now();
        let expires_at = now + Duration::hours(1);
        let just_before_expiry = expires_at - Duration::seconds(1);
        let old_hash = c.secret_hash.clone();
        let new_hash = String::from("newhash");
        let by = "operator:abc";

        c.rotate_secret(new_hash.clone(), expires_at, now, by.into())
            .unwrap();

        assert!(c.accepts_secret_hash(&new_hash, now));
        assert!(c.accepts_secret_hash(&old_hash, now)); // previous hash still valid
        assert!(c.accepts_secret_hash(&old_hash, just_before_expiry)); // still valid
        assert!(!c.accepts_secret_hash(&old_hash, expires_at)); // expired
    }

    #[test]
    fn accepts_secret_hash_rotate_twice_rejects_oldest() {
        let mut c = client();
        let now = test_now();
        let first_expires_at = now + Duration::hours(1);
        let second_expires_at = now + Duration::hours(2);
        let old_hash = c.secret_hash.clone();
        let new_hash = String::from("newhash");
        let newer_hash = String::from("newerhash");
        let by = "operator:abc";

        c.rotate_secret(new_hash.clone(), first_expires_at, now, by.into())
            .unwrap();
        c.rotate_secret(newer_hash.clone(), second_expires_at, now, by.into())
            .unwrap();

        assert!(c.accepts_secret_hash(&newer_hash, now));
        assert!(c.accepts_secret_hash(&new_hash, now)); // previous hash still valid
        assert!(!c.accepts_secret_hash(&old_hash, now)); // oldest hash dropped
    }
}
