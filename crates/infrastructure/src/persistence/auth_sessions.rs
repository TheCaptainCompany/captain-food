//! Postgres adapter for auth-session parking (`auth_sessions`, #112, PROP-20260724-150500).
//!
//! The provider session (access/refresh JWTs) is encrypted at rest with **AES-256-GCM** under the
//! `AUTH_SESSION_KEY` env secret (32 bytes, hex or base64) — never plaintext in the DB, same
//! credentials-at-rest stance as every token store in this repo. `park` inserts the ciphertext
//! keyed by the acceptance messageId; `claim` is a single-read DELETE…RETURNING scoped by the
//! owning `session_id` and `expires_at > now()` — a mismatch or expiry returns `None`
//! indistinguishably from absence (no oracle).

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{AeadCore, Aes256Gcm, Key, Nonce};
use application::auth_sessions::{AuthSessionStore, ParkedAuthSession};
use async_trait::async_trait;
use chrono::{Duration, Utc};
use domain::shared::errors::DomainError;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::db_err;

/// How long a parked session waits for cookie pickup before it is swept — minutes, not hours: the
/// client calls `/auth/session` immediately after the verify SUCCEEDs.
const PICKUP_TTL_MINUTES: i64 = 10;

/// The plaintext trio, serialized then encrypted as the row `ciphertext`.
#[derive(Serialize, Deserialize)]
struct SessionPlain {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

pub struct PgAuthSessionStore {
    pool: PgPool,
    cipher: Aes256Gcm,
}

impl PgAuthSessionStore {
    /// Build from the pool + the `AUTH_SESSION_KEY` env secret (32 bytes as 64 hex chars or base64).
    /// `None` when the key is unset/malformed — the composition root then falls back to refusing to
    /// park (fail-closed: no key ⇒ no session cookies, rather than plaintext at rest).
    pub fn from_env(pool: PgPool) -> Option<Self> {
        let raw = std::env::var("AUTH_SESSION_KEY").ok().filter(|s| !s.is_empty())?;
        let key_bytes = decode_key(&raw)?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key_bytes));
        Some(Self { pool, cipher })
    }
}

/// 32-byte key from hex (64 chars) or standard base64.
fn decode_key(raw: &str) -> Option<Vec<u8>> {
    let raw = raw.trim();
    if let Ok(bytes) = hex_decode(raw) {
        if bytes.len() == 32 {
            return Some(bytes);
        }
    }
    // base64 (std alphabet, padded) — minimal decoder to avoid a new dep.
    base64_decode(raw).filter(|b| b.len() == 32)
}

#[async_trait]
impl AuthSessionStore for PgAuthSessionStore {
    async fn park(&self, session: ParkedAuthSession) -> Result<(), DomainError> {
        let plain = SessionPlain {
            access_token: session.access_token,
            refresh_token: session.refresh_token,
            expires_in: session.expires_in,
        };
        let plaintext = serde_json::to_vec(&plain).map_err(|e| DomainError::Repository(e.to_string()))?;
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|_| DomainError::Repository("auth session encryption failed".into()))?;
        let expires_at = Utc::now() + Duration::minutes(PICKUP_TTL_MINUTES);

        sqlx::query(
            "INSERT INTO auth_sessions (message_id, session_id, ciphertext, nonce, expires_at, created_at) \
             VALUES ($1, $2, $3, $4, $5, now()) \
             ON CONFLICT (message_id) DO UPDATE SET \
               session_id = EXCLUDED.session_id, ciphertext = EXCLUDED.ciphertext, \
               nonce = EXCLUDED.nonce, expires_at = EXCLUDED.expires_at, created_at = now()",
        )
        .bind(session.message_id)
        .bind(session.session_id)
        .bind(&ciphertext)
        .bind(nonce.as_slice())
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(db_err)?;
        Ok(())
    }

    async fn claim(
        &self,
        message_id: Uuid,
        session_id: Option<Uuid>,
    ) -> Result<Option<ParkedAuthSession>, DomainError> {
        // Single-read: delete-and-return the row iff it exists, is unexpired, and its owner matches
        // (NULL-safe equality — both-None is a valid channel). Guessing a messageId yields nothing
        // without the minting session.
        let row = sqlx::query(
            "DELETE FROM auth_sessions \
             WHERE message_id = $1 AND expires_at > now() AND session_id IS NOT DISTINCT FROM $2 \
             RETURNING ciphertext, nonce, session_id",
        )
        .bind(message_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(db_err)?;

        let Some(row) = row else { return Ok(None) };
        let ciphertext: Vec<u8> = row.try_get("ciphertext").map_err(db_err)?;
        let nonce_bytes: Vec<u8> = row.try_get("nonce").map_err(db_err)?;
        let owner: Option<Uuid> = row.try_get("session_id").map_err(db_err)?;
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(&nonce_bytes), ciphertext.as_ref())
            .map_err(|_| DomainError::Repository("auth session decryption failed".into()))?;
        let plain: SessionPlain =
            serde_json::from_slice(&plaintext).map_err(|e| DomainError::Repository(e.to_string()))?;
        Ok(Some(ParkedAuthSession {
            message_id,
            session_id: owner,
            access_token: plain.access_token,
            refresh_token: plain.refresh_token,
            expires_in: plain.expires_in,
        }))
    }
}

// ─── tiny hex/base64 (avoid pulling a base64 dep for a one-liner) ──────────────────────────────

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if s.len() % 2 != 0 {
        return Err(());
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ())).collect()
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let val = |c: u8| A.iter().position(|&x| x == c).map(|p| p as u32);
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0;
    for &c in s.trim_end_matches('=').as_bytes() {
        let v = val(c)?;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_decodes_from_hex_and_base64_at_32_bytes() {
        let hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        assert_eq!(decode_key(hex).map(|b| b.len()), Some(32));
        // 32 zero bytes, base64 = 44 chars ending in '='
        let b64 = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
        assert_eq!(decode_key(b64).map(|b| b.len()), Some(32));
        // Wrong length rejected.
        assert_eq!(decode_key("abcd"), None);
    }
}
