//! Portable, passphrase-protected notes. Public metadata is authenticated with the body.

use std::{
    fmt,
    io::{self, Read, Write},
};

pub use age::secrecy::{ExposeSecret, SecretString};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use crate::Entry;

pub const PROTECTION_VERSION: u32 = 1;
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CIPHERTEXT_BYTES: usize = 32 * 1024 * 1024;
const SCRYPT_WORK_FACTOR: u8 = 18;
const PAYLOAD_FORMAT: &str = "tui-journal/protected-note";
const ARMOR_BEGIN: &str = "-----BEGIN AGE ENCRYPTED FILE-----";
const ARMOR_END: &str = "-----END AGE ENCRYPTED FILE-----";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedNote {
    pub version: u32,
    /// A random public identifier, independent of backend-assigned entry IDs.
    pub identity: String,
    pub ciphertext: String,
}

impl fmt::Debug for EncryptedNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncryptedNote")
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

impl EncryptedNote {
    pub fn validate(&self) -> Result<(), ProtectionError> {
        if self.version != PROTECTION_VERSION {
            return Err(ProtectionError::UnsupportedVersion);
        }
        if self.ciphertext.len() > MAX_CIPHERTEXT_BYTES {
            return Err(ProtectionError::TooLarge);
        }
        if self.identity.len() != 62
            || self.identity.parse::<age::x25519::Recipient>().is_err()
            || !self.ciphertext.starts_with(ARMOR_BEGIN)
            || !self
                .ciphertext
                .trim_end_matches(['\r', '\n'])
                .ends_with(ARMOR_END)
        {
            return Err(ProtectionError::InvalidEnvelope);
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnlockedNote {
    pub content: String,
    pub tags: Vec<String>,
    pub priority: Option<u32>,
    pub native_metadata: Option<String>,
}

impl fmt::Debug for UnlockedNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UnlockedNote { [REDACTED] }")
    }
}

impl Drop for UnlockedNote {
    fn drop(&mut self) {
        self.content.zeroize();
        self.tags.zeroize();
        self.priority.zeroize();
        self.native_metadata.zeroize();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProtectionError {
    #[error("Enter a nonempty password of at most 4096 bytes")]
    InvalidPassword,
    #[error("This note uses an unsupported protection version")]
    UnsupportedVersion,
    #[error("The protected note is malformed")]
    InvalidEnvelope,
    #[error("The protected note contains unencrypted private fields")]
    PlaintextFields,
    #[error("The protected note exceeds the supported size limit")]
    TooLarge,
    #[error("The password is incorrect or the protected note is damaged")]
    UnlockFailed,
    #[error("The protected note does not match its public title, date, or identity")]
    MetadataMismatch,
    #[error("This note is not protected")]
    NotProtected,
    #[error("This note is already protected")]
    AlreadyProtected,
    #[error("Could not encrypt the note")]
    EncryptionFailed,
}

#[derive(Serialize)]
struct PayloadRef<'a> {
    format: &'a str,
    version: u32,
    identity: &'a str,
    title: &'a str,
    date: DateTime<Utc>,
    note: NoteRef<'a>,
}

#[derive(Serialize)]
struct NoteRef<'a> {
    content: &'a str,
    tags: &'a [String],
    priority: Option<u32>,
    native_metadata: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    format: String,
    version: u32,
    identity: String,
    title: String,
    date: DateTime<Utc>,
    note: UnlockedNote,
}

struct PlaintextBuffer(Zeroizing<Vec<u8>>);

impl Write for PlaintextBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_PAYLOAD_BYTES {
            return Err(io::Error::other("Protected note size limit exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn validate_protected_entry(entry: &Entry) -> Result<(), ProtectionError> {
    if let Some(protection) = &entry.protection {
        protection.validate()?;
        if !entry.content.is_empty() || !entry.tags.is_empty() || entry.priority.is_some() {
            return Err(ProtectionError::PlaintextFields);
        }
    }
    Ok(())
}

/// Encrypt a plaintext entry. Callers persist the returned envelope with empty private fields.
pub fn protect(
    entry: &Entry,
    password: &SecretString,
    native_metadata: Option<String>,
) -> Result<EncryptedNote, ProtectionError> {
    let native_metadata = native_metadata.map(Zeroizing::new);
    if entry.protection.is_some() {
        return Err(ProtectionError::AlreadyProtected);
    }
    let identity = age::x25519::Identity::generate().to_public().to_string();
    encrypt_note(entry, password, native_metadata, identity)
}

/// Encrypt edited plaintext using the original note identity and a fresh age file key and salt.
pub fn reseal(
    entry: &Entry,
    password: &SecretString,
    native_metadata: Option<String>,
) -> Result<EncryptedNote, ProtectionError> {
    let native_metadata = native_metadata.map(Zeroizing::new);
    let protection = entry
        .protection
        .as_ref()
        .ok_or(ProtectionError::NotProtected)?;
    protection.validate()?;
    encrypt_note(
        entry,
        password,
        native_metadata,
        protection.identity.clone(),
    )
}

fn encrypt_note(
    entry: &Entry,
    password: &SecretString,
    native_metadata: Option<Zeroizing<String>>,
    identity: String,
) -> Result<EncryptedNote, ProtectionError> {
    validate_password(password)?;
    let payload = PayloadRef {
        format: PAYLOAD_FORMAT,
        version: PROTECTION_VERSION,
        identity: &identity,
        title: &entry.title,
        date: entry.date,
        note: NoteRef {
            content: &entry.content,
            tags: &entry.tags,
            priority: entry.priority,
            native_metadata: native_metadata.as_ref().map(|value| value.as_str()),
        },
    };
    let mut plaintext = PlaintextBuffer(Zeroizing::new(Vec::new()));
    serde_json::to_writer(&mut plaintext, &payload).map_err(|_| ProtectionError::TooLarge)?;
    let mut recipient = age::scrypt::Recipient::new(password.clone());
    // Keep the same password strength in debug and release builds and bound memory use.
    recipient.set_work_factor(SCRYPT_WORK_FACTOR);
    let ciphertext = age::encrypt_and_armor(&recipient, &plaintext.0)
        .map_err(|_| ProtectionError::EncryptionFailed)?;
    Ok(EncryptedNote {
        version: PROTECTION_VERSION,
        identity,
        ciphertext,
    })
}

pub fn unlock(entry: &Entry, password: &SecretString) -> Result<UnlockedNote, ProtectionError> {
    validate_password(password)?;
    validate_protected_entry(entry)?;
    let protection = entry
        .protection
        .as_ref()
        .ok_or(ProtectionError::NotProtected)?;
    let armored = age::armor::ArmoredReader::new(protection.ciphertext.as_bytes());
    let decryptor = age::Decryptor::new(armored).map_err(|_| ProtectionError::UnlockFailed)?;
    if !decryptor.is_scrypt() {
        return Err(ProtectionError::InvalidEnvelope);
    }
    let mut identity = age::scrypt::Identity::new(password.clone());
    identity.set_max_work_factor(SCRYPT_WORK_FACTOR);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| ProtectionError::UnlockFailed)?;
    let mut plaintext = Zeroizing::new(Vec::new());
    reader
        .take(MAX_PAYLOAD_BYTES as u64 + 1)
        .read_to_end(&mut plaintext)
        .map_err(|_| ProtectionError::UnlockFailed)?;
    if plaintext.len() > MAX_PAYLOAD_BYTES {
        return Err(ProtectionError::TooLarge);
    }
    let payload: Payload =
        serde_json::from_slice(&plaintext).map_err(|_| ProtectionError::UnlockFailed)?;
    if payload.format != PAYLOAD_FORMAT || payload.version != PROTECTION_VERSION {
        return Err(ProtectionError::UnsupportedVersion);
    }
    if payload.identity != protection.identity
        || payload.title != entry.title
        || payload.date != entry.date
    {
        return Err(ProtectionError::MetadataMismatch);
    }
    Ok(payload.note)
}

fn validate_password(password: &SecretString) -> Result<(), ProtectionError> {
    if password.expose_secret().is_empty() || password.expose_secret().len() > 4096 {
        return Err(ProtectionError::InvalidPassword);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, TimeZone};
    use serde_json::{Value, json};

    use super::*;

    const PASSWORD: &str = "test passphrase for portable notes";
    const CANARY: &str = "private-content-canary-7a953f";

    fn sample() -> Entry {
        Entry::new(
            1,
            Utc.with_ymd_and_hms(2026, 10, 24, 10, 11, 12).unwrap(),
            "Visible title".to_string(),
            format!("{CANARY}\nUnicode: 日本語 🦀"),
            vec!["private-tag".to_string()],
            Some(6),
        )
    }

    fn password() -> SecretString {
        SecretString::from(PASSWORD.to_string())
    }

    fn payload(entry: &Entry, identity: &str) -> Value {
        json!({
            "format": PAYLOAD_FORMAT,
            "version": PROTECTION_VERSION,
            "identity": identity,
            "title": entry.title,
            "date": entry.date,
            "note": {
                "content": entry.content,
                "tags": entry.tags,
                "priority": entry.priority,
                "native_metadata": "private-native-property"
            }
        })
    }

    fn clear_private_fields(entry: &mut Entry) {
        entry.content.zeroize();
        entry.tags.zeroize();
        entry.priority = None;
    }

    fn fixture() -> (Entry, Value) {
        let mut entry = sample();
        let identity = age::x25519::Identity::generate().to_public().to_string();
        let payload = payload(&entry, &identity);
        entry.protection = Some(EncryptedNote {
            version: PROTECTION_VERSION,
            identity,
            ciphertext: fixture_ciphertext(&serde_json::to_vec(&payload).unwrap()),
        });
        clear_private_fields(&mut entry);
        (entry, payload)
    }

    // Fast valid age inputs exercise decoding without changing the production KDF.
    fn fixture_ciphertext(bytes: &[u8]) -> String {
        let mut recipient = age::scrypt::Recipient::new(password());
        recipient.set_work_factor(1);
        age::encrypt_and_armor(&recipient, bytes).unwrap()
    }

    fn replace_payload(entry: &mut Entry, payload: &Value) {
        entry.protection.as_mut().unwrap().ciphertext =
            fixture_ciphertext(&serde_json::to_vec(payload).unwrap());
    }

    #[test]
    fn protects_and_reseals_with_standard_age_and_a_stable_identity() {
        let mut entry = sample();
        entry.protection =
            Some(protect(&entry, &password(), Some("private-native-property".into())).unwrap());
        clear_private_fields(&mut entry);
        let envelope = entry.protection.as_ref().unwrap();
        let stored = serde_json::to_string(&entry).unwrap();
        for secret in [CANARY, "private-tag", "private-native-property", PASSWORD] {
            assert!(!stored.contains(secret));
        }
        assert!(stored.contains("Visible title"));
        let mut raw_age = Vec::new();
        age::armor::ArmoredReader::new(envelope.ciphertext.as_bytes())
            .read_to_end(&mut raw_age)
            .unwrap();
        let header = String::from_utf8_lossy(
            &raw_age[..raw_age
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(raw_age.len())
                .min(150)],
        );
        assert!(header.contains(" 18\n"));
        let identity = age::scrypt::Identity::new(password());
        let mut plaintext = Zeroizing::new(Vec::new());
        age::Decryptor::new(&raw_age[..])
            .unwrap()
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .unwrap()
            .read_to_end(&mut plaintext)
            .unwrap();
        let recovered: Value = serde_json::from_slice(&plaintext).unwrap();
        assert!(
            recovered["note"]["content"]
                .as_str()
                .unwrap()
                .contains(CANARY)
        );

        let mut unlocked = unlock(&entry, &password()).unwrap();
        assert_eq!(unlocked.tags, ["private-tag"]);
        assert_eq!(unlocked.priority, Some(6));
        let old_envelope = envelope.clone();
        entry.content = std::mem::take(&mut unlocked.content);
        entry.content.push_str("\nEdited");
        entry.tags = std::mem::take(&mut unlocked.tags);
        entry.priority = unlocked.priority;
        entry.title = "Edited public title".into();
        let new_password = SecretString::from("a different test password".to_string());
        entry.protection =
            Some(reseal(&entry, &new_password, unlocked.native_metadata.take()).unwrap());
        clear_private_fields(&mut entry);
        assert_eq!(
            entry.protection.as_ref().unwrap().identity,
            old_envelope.identity
        );
        assert_ne!(
            entry.protection.as_ref().unwrap().ciphertext,
            old_envelope.ciphertext
        );
        assert!(unlock(&entry, &password()).is_err());
        let edited = unlock(&entry, &new_password).unwrap();
        assert!(edited.content.ends_with("Edited"));
        assert_eq!(
            edited.native_metadata.as_deref(),
            Some("private-native-property")
        );
    }

    #[test]
    fn metadata_authentication_rejects_title_date_and_identity_changes_but_allows_new_ids() {
        let (entry, _) = fixture();
        let mut changed = entry.clone();
        changed.id = 42;
        assert!(unlock(&changed, &password()).is_ok());
        changed.title.push_str(" tampered");
        assert!(matches!(
            unlock(&changed, &password()),
            Err(ProtectionError::MetadataMismatch)
        ));
        changed = entry.clone();
        changed.date += Duration::seconds(1);
        assert!(matches!(
            unlock(&changed, &password()),
            Err(ProtectionError::MetadataMismatch)
        ));
        changed = entry;
        changed.protection.as_mut().unwrap().identity =
            age::x25519::Identity::generate().to_public().to_string();
        assert!(matches!(
            unlock(&changed, &password()),
            Err(ProtectionError::MetadataMismatch)
        ));
    }

    #[test]
    fn rejects_wrong_password_corruption_and_truncation_without_releasing_plaintext() {
        let (entry, mut payload) = fixture();
        let wrong = SecretString::from("wrong password".to_string());
        assert!(matches!(
            unlock(&entry, &wrong),
            Err(ProtectionError::UnlockFailed)
        ));
        let mut damaged = entry.clone();
        let ciphertext = &mut damaged.protection.as_mut().unwrap().ciphertext;
        let index = ciphertext.len() - ARMOR_END.len() - 10;
        let replacement = if &ciphertext[index..index + 1] == "A" {
            "B"
        } else {
            "A"
        };
        ciphertext.replace_range(index..index + 1, replacement);
        assert!(unlock(&damaged, &password()).is_err());
        damaged = entry;
        payload["note"]["content"] = json!(CANARY.repeat(10_000));
        replace_payload(&mut damaged, &payload);
        let ciphertext = &mut damaged.protection.as_mut().unwrap().ciphertext;
        let cut = ciphertext.len() / 2;
        ciphertext.truncate(cut);
        ciphertext.push('\n');
        ciphertext.push_str(ARMOR_END);
        assert!(unlock(&damaged, &password()).is_err());
    }

    #[test]
    fn rejects_unknown_versions_formats_and_payload_fields() {
        let (entry, original) = fixture();
        let mut changed = entry.clone();
        changed.protection.as_mut().unwrap().version += 1;
        assert!(matches!(
            unlock(&changed, &password()),
            Err(ProtectionError::UnsupportedVersion)
        ));
        for (field, value) in [
            ("version", json!(2)),
            ("format", json!("other application")),
        ] {
            let mut payload = original.clone();
            payload[field] = value;
            changed = entry.clone();
            replace_payload(&mut changed, &payload);
            assert!(matches!(
                unlock(&changed, &password()),
                Err(ProtectionError::UnsupportedVersion)
            ));
        }
        let mut payload = original;
        payload["note"]["unexpected"] = json!(CANARY);
        replace_payload(&mut changed, &payload);
        assert!(matches!(
            unlock(&changed, &password()),
            Err(ProtectionError::UnlockFailed)
        ));
    }

    #[test]
    fn refuses_protected_records_with_plaintext_private_fields() {
        let (entry, _) = fixture();
        for field in 0..3 {
            let mut changed = entry.clone();
            match field {
                0 => changed.content = CANARY.into(),
                1 => changed.tags.push(CANARY.into()),
                _ => changed.priority = Some(1),
            }
            assert!(matches!(
                validate_protected_entry(&changed),
                Err(ProtectionError::PlaintextFields)
            ));
            assert!(matches!(
                unlock(&changed, &password()),
                Err(ProtectionError::PlaintextFields)
            ));
        }
    }

    #[test]
    fn caps_password_ciphertext_and_serialized_plaintext_sizes() {
        for value in [String::new(), "x".repeat(4097)] {
            assert!(matches!(
                protect(&sample(), &SecretString::from(value), None),
                Err(ProtectionError::InvalidPassword)
            ));
        }
        let (mut entry, _) = fixture();
        entry.protection.as_mut().unwrap().ciphertext = "x".repeat(MAX_CIPHERTEXT_BYTES + 1);
        assert!(matches!(
            unlock(&entry, &password()),
            Err(ProtectionError::TooLarge)
        ));
        let mut plaintext = sample();
        plaintext.content = "\u{0}".repeat(MAX_PAYLOAD_BYTES / 6 + 1);
        assert!(matches!(
            protect(&plaintext, &password(), None),
            Err(ProtectionError::TooLarge)
        ));
    }

    #[test]
    fn rejects_oversized_decrypted_payload_without_returning_a_partial_note() {
        let (mut entry, _) = fixture();
        entry.protection.as_mut().unwrap().ciphertext =
            fixture_ciphertext(&vec![b' '; MAX_PAYLOAD_BYTES + 1]);
        assert!(matches!(
            unlock(&entry, &password()),
            Err(ProtectionError::TooLarge)
        ));
    }

    #[test]
    fn rejects_excessive_scrypt_cost_before_decryption() {
        let (mut entry, _) = fixture();
        let envelope = entry.protection.as_mut().unwrap();
        let mut binary = Vec::new();
        age::armor::ArmoredReader::new(envelope.ciphertext.as_bytes())
            .read_to_end(&mut binary)
            .unwrap();
        let offset = binary
            .windows(3)
            .position(|window| window == b" 1\n")
            .unwrap();
        binary.splice(offset..offset + 3, *b" 63\n");
        let mut armored = Vec::new();
        let mut writer =
            age::armor::ArmoredWriter::wrap_output(&mut armored, age::armor::Format::AsciiArmor)
                .unwrap();
        writer.write_all(&binary).unwrap();
        writer.finish().unwrap();
        envelope.ciphertext = String::from_utf8(armored).unwrap();
        assert!(matches!(
            unlock(&entry, &password()),
            Err(ProtectionError::UnlockFailed)
        ));
    }

    #[test]
    fn rejects_non_passphrase_age_files() {
        let (mut entry, _) = fixture();
        let identity = age::x25519::Identity::generate();
        entry.protection.as_mut().unwrap().ciphertext =
            age::encrypt_and_armor(&identity.to_public(), b"{}").unwrap();
        assert!(matches!(
            unlock(&entry, &password()),
            Err(ProtectionError::InvalidEnvelope)
        ));
    }

    #[test]
    fn debug_output_and_error_messages_do_not_expose_private_content() {
        let (entry, _) = fixture();
        let unlocked = unlock(&entry, &password()).unwrap();
        for debug in [format!("{unlocked:?}"), format!("{:?}", entry.protection)] {
            for secret in [CANARY, PASSWORD, "private-tag", "private-native-property"] {
                assert!(!debug.contains(secret));
            }
        }
        assert_eq!(format!("{unlocked:?}"), "UnlockedNote { [REDACTED] }");
        assert!(!ProtectionError::UnlockFailed.to_string().contains(CANARY));
    }
}
