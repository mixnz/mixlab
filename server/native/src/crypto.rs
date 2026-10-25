//! What the server needs a cipher for, which is not much: it holds a verifier and hands out
//! tokens. **Nothing here touches a record.** The keys that make a record readable are derived on
//! the machine that wrote it and never arrive.

use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Crockford base32: the alphabet without `I`, `L`, `O` and `U`, so nothing read off a screen is
/// ambiguous. The same one the recovery key uses (D2), so a person learns one way of typing a
/// code from this product rather than two.
const BASE32: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CODE_CHARS: usize = 8;

/// A code a person types, shown as `XXXX-XXXX`. Forty bits, which is why guessing is rate limited.
pub fn random_code() -> String {
    let mut bytes = [0u8; CODE_CHARS];
    rand::rng().fill_bytes(&mut bytes);
    let code: String = bytes
        .iter()
        .map(|byte| BASE32[usize::from(*byte) % 32] as char)
        .collect();
    format!("{}-{}", &code[..4], &code[4..])
}

/// Forgiving about case and separators, strict about the alphabet — the rule
/// `parse_recovery_key` already applies on the client. A character outside the alphabet means the
/// person has the wrong thing in front of them and should be told so.
pub fn normalise_code(presented: &str) -> Option<String> {
    let stripped: String = presented
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-')
        .map(|character| character.to_ascii_uppercase())
        .collect();
    if stripped.len() != CODE_CHARS {
        return None;
    }
    stripped
        .bytes()
        .all(|byte| BASE32.contains(&byte))
        .then_some(stripped)
}

/// 32 random bytes as hex: a session token, a device id. Never something a person types.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// 16 random bytes as hex: an account's id on the wire (`accountId`), made once at registration
/// and never reused.
pub fn random_public_id() -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// How many bytes a salt is, fixed so that an invented one cannot be told apart by length.
pub const SALT_BYTES: usize = 16;

/// HKDF-SHA256 output (D2).
pub const VERIFIER_BYTES: usize = 32;

/// XChaCha20-Poly1305 over a 32-byte key: 24 nonce, 32 sealed, 16 tag (D2).
pub const WRAPPED_KEY_BYTES: usize = 72;

/// The salt handed back for an address that has no account (D4a).
///
/// Stable, so asking twice gives the same answer; unguessable, because the pepper never leaves this
/// deployment; and the same shape as a real one. Without all three, `/v1/auth/params` is the
/// cheapest account-enumeration oracle in the protocol.
pub fn invented_salt(pepper: &str, account_key: &str) -> String {
    use base64::Engine as _;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(pepper.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(b"salt/v1");
    mac.update(&[0u8]);
    mac.update(account_key.as_bytes());
    base64::engine::general_purpose::STANDARD.encode(&mac.finalize().into_bytes()[..SALT_BYTES])
}

pub fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// The stored password verifier: `HMAC(pepper, A)`. `A` already is a verifier — the client derived
/// it and the password is not recoverable from it — so this is not about hashing a password. It is
/// about the pepper living outside the database, so that a database taken on its own is not a list
/// of values that can be replayed against this server.
pub fn peppered(pepper: &str, a: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(pepper.as_bytes())
        .expect("HMAC accepts a key of any length");
    mac.update(a.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Comparison that does not leak where two values first differ. Both arguments are hashes of the
/// same fixed length, so an early return costs nothing that matters — but a verifier comparison is
/// exactly the place where "it was fine in practice" stops being an argument.
pub fn same_secret(left: &str, right: &str) -> bool {
    left.as_bytes().ct_eq(right.as_bytes()).into()
}

/// The only name an account has (D4a):
///
/// ```text
/// account_key = SHA-256("mixlab-sync/account/v1" || 0x00 || lowercase(trim(email)))
/// ```
///
/// **Frozen, and deployment-independent on purpose.** It is the unique key of the row here and the
/// name of the Durable Object in `../worker/`, so the same address is the same account on either,
/// and a row means the same thing wherever it is carried. It carries no pepper: a peppered value
/// could not mean the same thing on two servers, which is the whole point of it.
///
/// The label is frozen the way D2's five HKDF labels are. Changing it corrupts nothing; it makes
/// every existing account unfindable.
pub fn account_key(email: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ACCOUNT_LABEL);
    hasher.update([0u8]);
    hasher.update(email.trim().to_lowercase().as_bytes());
    hex::encode(hasher.finalize())
}

const ACCOUNT_LABEL: &[u8] = b"mixlab-sync/account/v1";

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The vector both implementations assert**, from D4a. If this and the one in
    /// `../worker/src/crypto.test.ts` ever disagree, an account means two different things on the
    /// two servers and a row cannot cross between them — which is the whole reason it is frozen.
    #[test]
    fn the_account_key_matches_the_specification() {
        assert_eq!(
            account_key("alice@example.com"),
            "176d00c0673f7e1e711ea55a7d9345f43949376bd9777c4854be01448b5b74a4"
        );
    }

    #[test]
    fn the_account_key_ignores_case_and_surrounding_space() {
        let wanted = account_key("alice@example.com");
        assert_eq!(account_key("Alice@Example.com"), wanted);
        assert_eq!(account_key("  alice@example.com  "), wanted);
        assert_ne!(account_key("bob@example.com"), wanted);
    }

    #[test]
    fn a_code_is_eight_characters_in_two_groups() {
        for _ in 0..50 {
            let code = random_code();
            assert_eq!(code.len(), 9, "{code}");
            assert_eq!(&code[4..5], "-", "{code}");
            assert!(normalise_code(&code).is_some(), "{code}");
        }
    }

    #[test]
    fn a_code_is_read_back_however_it_was_typed() {
        let code = random_code();
        let bare = code.replace('-', "");
        assert_eq!(normalise_code(&code).as_deref(), Some(bare.as_str()));
        assert_eq!(
            normalise_code(&code.to_lowercase()).as_deref(),
            Some(bare.as_str())
        );
        assert_eq!(
            normalise_code(&code.replace('-', " ")).as_deref(),
            Some(bare.as_str())
        );
    }

    #[test]
    fn a_code_refuses_the_letters_the_alphabet_leaves_out() {
        // `I`, `L`, `O` and `U` are not in it, so a code carrying one means the person has the
        // wrong thing in front of them and should be told so.
        for presented in [
            "IIII-IIII",
            "LLLL-LLLL",
            "OOOO-OOOO",
            "UUUU-UUUU",
            "ABC-ABC",
            "",
        ] {
            assert!(normalise_code(presented).is_none(), "{presented}");
        }
    }
}
