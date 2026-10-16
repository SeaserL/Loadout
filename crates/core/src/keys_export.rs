// SPDX-License-Identifier: Apache-2.0

//! Encrypted export / import of every API-key account, keys included (ExecPlan 020,
//! ADR-004 §6: an export with secrets must be encrypted with the user's passphrase).
//!
//! File: a JSON envelope (`format: "loadout-keys"`) holding the KDF and cipher parameters
//! and the base64 ciphertext. Key: PBKDF2-HMAC-SHA256 over the passphrase with a random
//! 16-byte salt; cipher: ChaCha20-Poly1305 with a random 12-byte nonce. Every envelope field
//! except the ciphertext is authenticated as associated data, so any edit fails to decrypt.
//! Plaintext keys only exist in memory, in zeroized buffers; they are never logged and never
//! appear in errors.

use std::num::NonZeroU32;
use std::path::Path;

use aws_lc_rs::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use aws_lc_rs::pbkdf2;
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use loadout_ipc::ErrorBody;
use loadout_ipc::api::{KEYS_EXPORT_CONFIRM, KeysExportResult, KeysImportReport, Passphrase};
use loadout_providers::Registry;
use loadout_secrets::SecretStore;
use loadout_storage::Database;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::{Zeroize, Zeroizing};

use crate::control::{ControlPlane, blocking};
use crate::route_build::{parse_protocols, read_secret};

const FORMAT: &str = "loadout-keys";
const VERSION: u32 = 1;
const KDF: &str = "pbkdf2-hmac-sha256";
const CIPHER: &str = "chacha20-poly1305";
/// OWASP 2023 recommendation for PBKDF2-HMAC-SHA256.
const ITERATIONS: u32 = 600_000;
/// Accepted range when reading a file (guards against a hostile iteration count).
const ITERATIONS_RANGE: std::ops::RangeInclusive<u32> = 100_000..=10_000_000;
const SALT_LEN: usize = 16;
pub(crate) const MIN_PASSPHRASE_CHARS: usize = 8;
const BAD_FILE: &str = "wrong passphrase, or the file is damaged or was changed";

#[derive(Serialize, Deserialize)]
struct Envelope {
    format: String,
    version: u32,
    kdf: String,
    iterations: u32,
    salt: String,
    cipher: String,
    nonce: String,
    ciphertext: String,
}

impl Envelope {
    /// Associated data: every field except the ciphertext.
    fn aad(&self) -> Vec<u8> {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.format,
            self.version,
            self.kdf,
            self.iterations,
            self.salt,
            self.cipher,
            self.nonce
        )
        .into_bytes()
    }
}

/// What the envelope protects. No `Debug`: it holds API keys.
#[derive(Serialize, Deserialize)]
struct Bundle {
    exported_at_ms: i64,
    loadout_version: String,
    providers: Vec<BundleProvider>,
}

#[derive(Serialize, Deserialize)]
struct BundleProvider {
    slug: String,
    kind: String,
    display_name: String,
    base_url: Option<String>,
    #[serde(default)]
    settings: Value,
    accounts: Vec<BundleAccount>,
}

#[derive(Serialize, Deserialize)]
struct BundleAccount {
    alias: String,
    display_name: Option<String>,
    api_key: String,
    /// Models the user added by hand (catalog models come back with the model sync).
    #[serde(default)]
    manual_models: Vec<BundleModel>,
}

impl Drop for BundleAccount {
    fn drop(&mut self) {
        self.api_key.zeroize();
    }
}

#[derive(Serialize, Deserialize)]
struct BundleModel {
    upstream_model_id: String,
    protocols: Vec<String>,
    context_window: Option<i64>,
}

fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0u8; 32]);
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(iterations).expect("non-zero iterations"),
        salt,
        passphrase.as_bytes(),
        key.as_mut(),
    );
    key
}

fn cipher_key(key: &[u8; 32]) -> Result<LessSafeKey, String> {
    UnboundKey::new(&CHACHA20_POLY1305, key)
        .map(LessSafeKey::new)
        .map_err(|_| "cipher setup failed".to_owned())
}

pub(crate) fn check_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(format!(
            "the passphrase needs at least {MIN_PASSPHRASE_CHARS} characters"
        ));
    }
    Ok(())
}

/// Encrypts `plain` into an envelope (JSON bytes).
fn seal(plain: &[u8], passphrase: &str, iterations: u32) -> Result<Vec<u8>, String> {
    check_passphrase(passphrase)?;
    let rng = SystemRandom::new();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rng.fill(&mut salt)
        .and_then(|()| rng.fill(&mut nonce))
        .map_err(|_| "no system randomness".to_owned())?;
    let mut envelope = Envelope {
        format: FORMAT.into(),
        version: VERSION,
        kdf: KDF.into(),
        iterations,
        salt: B64.encode(salt),
        cipher: CIPHER.into(),
        nonce: B64.encode(nonce),
        ciphertext: String::new(),
    };
    let key = cipher_key(&derive_key(passphrase, &salt, iterations))?;
    let mut buf = Zeroizing::new(plain.to_vec());
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::from(envelope.aad()),
        &mut *buf,
    )
    .map_err(|_| "encryption failed".to_owned())?;
    envelope.ciphertext = B64.encode(&*buf);
    serde_json::to_vec_pretty(&envelope).map_err(|e| e.to_string())
}

/// Decrypts an envelope. Every failure after parsing reads as [`BAD_FILE`].
fn open(file: &[u8], passphrase: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let envelope: Envelope = serde_json::from_slice(file)
        .map_err(|_| "not a Loadout key export (.loadout-keys)".to_owned())?;
    if envelope.format != FORMAT || envelope.version > VERSION {
        return Err(format!(
            "unsupported file (format {} version {})",
            envelope.format, envelope.version
        ));
    }
    if envelope.kdf != KDF
        || envelope.cipher != CIPHER
        || !ITERATIONS_RANGE.contains(&envelope.iterations)
    {
        return Err(BAD_FILE.into());
    }
    let salt = B64.decode(&envelope.salt).map_err(|_| BAD_FILE)?;
    let nonce: [u8; NONCE_LEN] = B64
        .decode(&envelope.nonce)
        .ok()
        .and_then(|n| n.try_into().ok())
        .ok_or(BAD_FILE)?;
    let mut buf = Zeroizing::new(B64.decode(&envelope.ciphertext).map_err(|_| BAD_FILE)?);
    let key = cipher_key(&derive_key(passphrase, &salt, envelope.iterations))?;
    let len = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(envelope.aad()),
            &mut buf,
        )
        .map_err(|_| BAD_FILE)?
        .len();
    buf.truncate(len);
    Ok(buf)
}

/// Reads every API-key account with its key. Accounts whose key cannot be read are listed
/// in the second value.
fn collect(
    db: &Database,
    secrets: &dyn SecretStore,
    registry: &Registry,
) -> Result<(Bundle, Vec<String>), String> {
    let s = |e: loadout_storage::StorageError| e.to_string();
    let mut providers = Vec::new();
    let mut skipped = Vec::new();
    for p in db.providers().list().map_err(s)? {
        if registry.get(&p.kind).is_none() {
            continue;
        }
        let mut accounts = Vec::new();
        for a in db.accounts().list(Some(&p.id)).map_err(s)? {
            let creds = db.accounts().credentials(&a.id).map_err(s)?;
            let Some(secret_ref) = creds
                .iter()
                .find(|c| c.credential_type == "api_key")
                .and_then(|c| c.secret_ref.clone())
            else {
                // OAuth, official-CLI and browser-session logins cannot be shared.
                continue;
            };
            let Some(secret) = read_secret(secrets, &secret_ref) else {
                skipped.push(format!("{}/{}", p.slug, a.alias));
                continue;
            };
            let manual_models = db
                .account_models()
                .list(&a.id)
                .map_err(s)?
                .into_iter()
                .filter(|m| m.source == "manual")
                .map(|m| BundleModel {
                    protocols: parse_protocols(&m.protocols_json)
                        .iter()
                        .map(|p| p.as_str().to_owned())
                        .collect(),
                    upstream_model_id: m.upstream_model_id,
                    context_window: m.context_window,
                })
                .collect();
            accounts.push(BundleAccount {
                alias: a.alias.clone(),
                display_name: a.display_name.clone(),
                api_key: secret.expose().to_owned(),
                manual_models,
            });
        }
        if !accounts.is_empty() {
            providers.push(BundleProvider {
                slug: p.slug.clone(),
                kind: p.kind.clone(),
                display_name: p.display_name.clone(),
                base_url: p.base_url.clone(),
                settings: serde_json::from_str(&p.config_json).unwrap_or(Value::Null),
                accounts,
            });
        }
    }
    Ok((
        Bundle {
            exported_at_ms: loadout_storage::now_ms(),
            loadout_version: env!("CARGO_PKG_VERSION").to_owned(),
            providers,
        },
        skipped,
    ))
}

/// Writes the encrypted export of every API-key account.
pub(crate) fn export(
    db: &Database,
    secrets: &dyn SecretStore,
    registry: &Registry,
    data_dir: &Path,
    path: Option<String>,
    passphrase: &Passphrase,
    confirm: &str,
) -> Result<KeysExportResult, String> {
    if confirm.trim() != KEYS_EXPORT_CONFIRM {
        return Err(format!(
            "exporting every API key needs the confirmation {KEYS_EXPORT_CONFIRM:?}"
        ));
    }
    check_passphrase(&passphrase.0)?;
    let (bundle, skipped) = collect(db, secrets, registry)?;
    let accounts = bundle.providers.iter().map(|p| p.accounts.len()).sum();
    if accounts == 0 {
        return Err("there are no API-key accounts to export".into());
    }
    let plain = Zeroizing::new(serde_json::to_vec(&bundle).map_err(|e| e.to_string())?);
    let sealed = seal(&plain, &passphrase.0, ITERATIONS)?;
    let target = crate::backup::target_path(
        data_dir,
        path,
        &format!(
            "loadout-keys-{}.loadout-keys",
            loadout_provider_sdk::time::utc_stamp(loadout_storage::now_ms())
        ),
    )?;
    std::fs::write(&target, sealed)
        .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
    // Audit trail (ADR-004 §6); never the keys or the passphrase.
    tracing::warn!(path = %target.display(), accounts, "exported every API key (encrypted)");
    Ok(KeysExportResult {
        path: target.display().to_string(),
        providers: bundle.providers.len(),
        accounts,
        skipped,
    })
}

impl ControlPlane {
    /// Imports an encrypted key export: missing providers and accounts are created (and
    /// their models synced); existing accounts are skipped, or get the new key with
    /// `overwrite`.
    pub async fn import_keys(
        &self,
        path: String,
        passphrase: Passphrase,
        overwrite: bool,
    ) -> Result<KeysImportReport, ErrorBody> {
        let bundle: Bundle = blocking(move || {
            let file = std::fs::read(&path)
                .map_err(|e| ErrorBody::invalid_params(format!("cannot read {path}: {e}")))?;
            let plain = open(&file, &passphrase.0).map_err(ErrorBody::invalid_params)?;
            serde_json::from_slice(&plain)
                .map_err(|_| ErrorBody::invalid_params("the decrypted export is not valid"))
        })
        .await?;

        let mut report = KeysImportReport::default();
        for p in &bundle.providers {
            let existing = self
                .list_providers()
                .await?
                .into_iter()
                .find(|x| x.slug == p.slug);
            match existing {
                Some(x) if x.kind != p.kind => {
                    report.skipped.push(format!(
                        "provider {} (already used by a {} provider)",
                        p.slug, x.kind
                    ));
                    continue;
                }
                Some(_) => {}
                None => {
                    let settings = if p.settings.is_object() {
                        p.settings.clone()
                    } else {
                        Value::Object(Default::default())
                    };
                    match self
                        .add_provider(
                            p.slug.clone(),
                            p.kind.clone(),
                            p.base_url.clone(),
                            settings,
                            Some(p.display_name.clone()),
                        )
                        .await
                    {
                        Ok(_) => report.providers += 1,
                        Err(e) => {
                            report
                                .skipped
                                .push(format!("provider {}: {}", p.slug, e.message));
                            continue;
                        }
                    }
                }
            }
            let accounts = self.list_accounts(Some(p.slug.clone())).await?;
            for a in &p.accounts {
                let name = format!("{}/{}", p.slug, a.alias);
                if accounts.iter().any(|x| x.alias == a.alias) {
                    if overwrite {
                        match self
                            .set_account_key(p.slug.clone(), a.alias.clone(), a.api_key.clone())
                            .await
                        {
                            Ok(_) => report.keys_updated += 1,
                            Err(e) => report.skipped.push(format!("{name}: {}", e.message)),
                        }
                    } else {
                        report.skipped.push(format!("{name} (already exists)"));
                    }
                    continue;
                }
                match self
                    .add_account(
                        p.slug.clone(),
                        a.alias.clone(),
                        Some(a.api_key.clone()),
                        a.display_name.clone(),
                        true,
                    )
                    .await
                {
                    Ok((_, sync)) => {
                        report.accounts += 1;
                        if let Some(Err(e)) = sync {
                            report.sync_errors.push(format!("{name}: {}", e.message));
                        }
                    }
                    Err(e) => {
                        report.skipped.push(format!("{name}: {}", e.message));
                        continue;
                    }
                }
                for m in &a.manual_models {
                    if let Err(e) = self
                        .add_model(
                            p.slug.clone(),
                            a.alias.clone(),
                            m.upstream_model_id.clone(),
                            m.protocols.clone(),
                            m.context_window,
                        )
                        .await
                    {
                        report
                            .skipped
                            .push(format!("{name}/{}: {}", m.upstream_model_id, e.message));
                    }
                }
            }
        }
        tracing::warn!(
            providers = report.providers,
            accounts = report.accounts,
            keys_updated = report.keys_updated,
            "imported API keys from an encrypted export"
        );
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use loadout_secrets::{InMemorySecretStore, Secret, SecretRef};
    use loadout_storage::{ModelInput, NewAccount, NewProvider};

    use super::*;

    /// Fast KDF for tests; the format check accepts it because it is in range.
    const TEST_ITERATIONS: u32 = 100_000;

    #[test]
    fn seal_open_round_trip_and_tamper() {
        let sealed = seal(b"hello keys", "correct horse", TEST_ITERATIONS).expect("seal");
        let text = String::from_utf8(sealed.clone()).expect("utf8");
        assert!(!text.contains("hello keys"));
        assert_eq!(
            &*open(&sealed, "correct horse").expect("open"),
            b"hello keys"
        );
        assert_eq!(
            open(&sealed, "wrong horse!").err().as_deref(),
            Some(BAD_FILE)
        );

        // Any authenticated field changed → fails.
        let mut v: Value = serde_json::from_slice(&sealed).expect("json");
        v["iterations"] = Value::from(TEST_ITERATIONS + 1);
        let changed = serde_json::to_vec(&v).expect("json");
        assert!(open(&changed, "correct horse").is_err());

        let mut v: Value = serde_json::from_slice(&sealed).expect("json");
        let mut ct = B64
            .decode(v["ciphertext"].as_str().expect("ct"))
            .expect("b64");
        ct[0] ^= 1;
        v["ciphertext"] = Value::from(B64.encode(ct));
        assert!(open(&serde_json::to_vec(&v).expect("json"), "correct horse").is_err());

        // Hostile iteration counts are refused before any work.
        let mut v: Value = serde_json::from_slice(&sealed).expect("json");
        v["iterations"] = Value::from(u32::MAX);
        assert!(open(&serde_json::to_vec(&v).expect("json"), "correct horse").is_err());

        assert!(seal(b"x", "short", TEST_ITERATIONS).is_err());
        assert!(open(b"not json", "correct horse").is_err());
    }

    fn db_with_accounts(secrets: &InMemorySecretStore) -> Database {
        let db = Database::open_in_memory().expect("db");
        let p = db
            .providers()
            .create(&NewProvider {
                slug: "sensenova",
                kind: "sensenova",
                display_name: "SenseNova",
                base_url: None,
                config_json: "{}",
            })
            .expect("provider");
        for (alias, key) in [("a", Some("sk-aaaa-secret")), ("b", None)] {
            let acc = db
                .accounts()
                .create(&NewAccount {
                    provider_id: &p.id,
                    alias,
                    display_name: None,
                    account_type: if key.is_some() { "api_key" } else { "local" },
                })
                .expect("account");
            if let Some(key) = key {
                let r = SecretRef::generate();
                secrets.put(&r, &Secret::new(key)).expect("put");
                db.accounts()
                    .add_credential(&acc.id, "api_key", "user", Some(&r.to_string()))
                    .expect("cred");
                db.account_models()
                    .insert(
                        &acc.id,
                        &ModelInput {
                            upstream_model_id: "hand-made".into(),
                            protocols_json: r#"["chat"]"#.into(),
                            context_window: Some(1000),
                            max_output_tokens: None,
                            capabilities_json: "[]".into(),
                            input_price: None,
                            output_price: None,
                            cache_read_price: None,
                            cache_write_price: None,
                        },
                        true,
                        "manual",
                    )
                    .expect("model");
            }
        }
        db
    }

    #[test]
    fn export_requires_confirmation_and_hides_keys() {
        let secrets = InMemorySecretStore::new();
        let db = db_with_accounts(&secrets);
        let dir = tempfile::tempdir().expect("dir");
        let pass = Passphrase("long enough pass".into());
        let registry = Registry::builtin();
        let err = export(&db, &secrets, &registry, dir.path(), None, &pass, "yes")
            .expect_err("needs confirmation");
        assert!(err.contains("EXPORT"));
        assert!(
            export(
                &db,
                &secrets,
                &registry,
                dir.path(),
                None,
                &Passphrase("short".into()),
                "EXPORT"
            )
            .is_err()
        );

        let r =
            export(&db, &secrets, &registry, dir.path(), None, &pass, "EXPORT").expect("export");
        assert_eq!((r.providers, r.accounts), (1, 1));
        let file = std::fs::read(&r.path).expect("read");
        let text = String::from_utf8_lossy(&file);
        assert!(!text.contains("sk-aaaa-secret"));
        assert!(
            !text.contains("sensenova"),
            "provider names are encrypted too"
        );

        let plain = open(&file, &pass.0).expect("open");
        let bundle: Bundle = serde_json::from_slice(&plain).expect("bundle");
        let a = &bundle.providers[0].accounts[0];
        assert_eq!(a.alias, "a");
        assert_eq!(a.api_key, "sk-aaaa-secret");
        assert_eq!(a.manual_models[0].upstream_model_id, "hand-made");
        assert_eq!(a.manual_models[0].protocols, ["chat"]);
    }
}
