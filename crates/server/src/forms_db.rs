use anyhow::Context;
use deadpool_postgres::Pool;
use deadpool_postgres::tokio_postgres::types::Type;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use noon_core::blind::BlindSigner;
use quick_protobuf::{MessageRead, MessageWrite};
use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::otp::generate_otp;
use crate::pb::forms::{Form, FormSubmission};

use crate::SharedData;

pub async fn get_or_create_blind_signer(sd: &SharedData) -> anyhow::Result<Arc<BlindSigner>> {
    {
        let cache = sd.cache.blind_signer.read().await;
        if let Some(signer) = &*cache {
            return Ok(signer.clone());
        }
    }

    let client = sd.db.get().await?;

    let rows = client
        .query_typed(
            "SELECT rsa_key FROM keys ORDER BY created_at DESC LIMIT 1",
            &[],
        )
        .await?;
    let row = rows.into_iter().next();
    if let Some(row) = row {
        let rsa_key_bytes: Vec<u8> = row.get(0);
        let private_key = rsa::RsaPrivateKey::from_pkcs8_der(&rsa_key_bytes)
            .context("Failed to parse pkcs8 der from db")?;
        let signer = Arc::new(BlindSigner::new(private_key));
        let mut cache = sd.cache.blind_signer.write().await;
        *cache = Some(signer.clone());
        return Ok(signer);
    }

    log::info!("No existing RSA key found. Generating a new BlindSigner key...");
    let private_key = rsa::RsaPrivateKey::new(&mut rand::rngs::OsRng::default(), 1024).unwrap();
    let pkcs8_doc = private_key.to_pkcs8_der().unwrap();
    let pkcs8_bytes = pkcs8_doc.as_bytes();

    client
        .query_typed(
            "INSERT INTO keys (rsa_key) VALUES ($1)",
            &[(&pkcs8_bytes, Type::BYTEA)],
        )
        .await?;

    let signer = Arc::new(BlindSigner::new(private_key));
    let mut cache = sd.cache.blind_signer.write().await;
    *cache = Some(signer.clone());

    Ok(signer)
}

pub async fn create_form(pool: &Pool, mut form: Form<'_>, owner: String) -> anyhow::Result<u64> {
    let client = pool.get().await?;

    let mut out = Vec::new();
    let mut writer = quick_protobuf::Writer::new(&mut out);
    form.write_message(&mut writer)?;

    let allowed_participants: Vec<&str> = form
        .allowed_participants
        .iter()
        .map(|s| s.as_ref())
        .collect();

    let deadline = if form.deadline == 0 {
        None
    } else {
        Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(form.deadline))
    };

    let stmt = "INSERT INTO forms (name, description, owner, fields, mentioned_emails, deadline) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id";
    let name_str = form.name.to_string();
    let desc_str = form.description.to_string();
    let rows = client
        .query_typed(
            stmt,
            &[
                (&name_str, Type::VARCHAR),
                (&desc_str, Type::TEXT),
                (&owner, Type::VARCHAR),
                (&out, Type::BYTEA),
                (&allowed_participants, Type::TEXT_ARRAY),
                (&deadline, Type::TIMESTAMPTZ),
            ],
        )
        .await?;
    let row = rows
        .into_iter()
        .next()
        .context("Failed to get id from insert")?;

    let form_id: i64 = row.get(0);
    form.id = form_id as u64;

    for participant in &form.allowed_participants {
        let stmt = "INSERT INTO form_allowed_participants (form_id, participant) VALUES ($1, $2) ON CONFLICT DO NOTHING";
        let part_str = participant.as_ref();
        let formatted_participant = if part_str.contains('@') {
            format!("email:{}", part_str)
        } else {
            format!("user:{}", part_str)
        };
        client
            .query_typed(
                stmt,
                &[
                    (&form_id, Type::INT8),
                    (&formatted_participant, Type::VARCHAR),
                ],
            )
            .await?;
    }

    Ok(form_id as u64)
}

pub async fn get_form_bytes(pool: &Pool, form_id: u64) -> anyhow::Result<Vec<u8>> {
    let client = pool.get().await?;
    let form_id_i64 = form_id as i64;
    let rows = client
        .query_typed(
            "SELECT name, description, owner, fields, extract(epoch from created_at)::bigint as created_at, mentioned_emails, extract(epoch from deadline)::bigint as deadline FROM forms WHERE id = $1",
            &[(&form_id_i64, Type::INT8)],
        )
        .await?;
    let row = rows.into_iter().next().context("Form not found")?;

    let name: String = row.get(0);
    let description: String = row.get(1);
    let owner: String = row.get(2);
    let bytes: Vec<u8> = row.get(3);
    let created_at: i64 = row.get(4);
    let mentioned_emails: Vec<String> = row.get(5);
    let deadline: Option<i64> = row.get(6);

    let mut reader = quick_protobuf::BytesReader::from_bytes(&bytes);
    let parsed_form = Form::from_reader(&mut reader, &bytes).unwrap_or_default();

    let mut form = parsed_form;
    form.id = form_id;
    form.name = name.into();
    form.description = description.into();
    form.owner = owner.into();
    form.created_at = created_at as u64;
    form.allowed_participants = mentioned_emails.into_iter().map(|s| s.into()).collect();
    form.deadline = deadline.unwrap_or(0) as u64;

    let form_id_i64 = form_id as i64;
    let rows = client
        .query_typed(
            "SELECT participant FROM form_allowed_participants WHERE form_id = $1",
            &[(&form_id_i64, Type::INT8)],
        )
        .await?;
    form.allowed_participants = rows
        .into_iter()
        .map(|r| {
            let p: String = r.get(0);
            p.into()
        })
        .collect();

    let mut final_out = Vec::new();
    let mut writer = quick_protobuf::Writer::new(&mut final_out);
    form.write_message(&mut writer)?;

    Ok(final_out)
}

pub async fn check_and_mark_participant_accepted(
    pool: &Pool,
    form_id: u64,
    participant: &str,
) -> anyhow::Result<bool> {
    let mut client = pool.get().await?;
    let tx = client.transaction().await?;

    let form_id_i64 = form_id as i64;
    let rows = tx
        .query_typed(
            "SELECT accepted FROM form_allowed_participants WHERE form_id = $1 AND participant = $2 FOR UPDATE",
            &[(&form_id_i64, Type::INT8), (&participant, Type::VARCHAR)],
        )
        .await?;
    let row = rows.into_iter().next();
    if let Some(r) = row {
        let accepted: bool = r.get(0);
        if accepted {
            return Ok(false); // already accepted
        }

        let form_id_i64 = form_id as i64;
        tx.query_typed(
            "UPDATE form_allowed_participants SET accepted = true WHERE form_id = $1 AND participant = $2",
            &[(&form_id_i64, Type::INT8), (&participant, Type::VARCHAR)],
        )
        .await?;
        tx.commit().await?;
        return Ok(true);
    }

    Ok(false)
}

pub async fn submit_form(pool: &Pool, submission: FormSubmission<'_>) -> anyhow::Result<()> {
    let client = pool.get().await?;

    let mut out = Vec::new();
    let mut writer = quick_protobuf::Writer::new(&mut out);
    submission.write_message(&mut writer)?;

    let form_id_i64 = submission.form_id as i64;
    client
        .query_typed(
            "INSERT INTO form_submissions (form_id, data) VALUES ($1, $2)",
            &[(&form_id_i64, Type::INT8), (&out, Type::BYTEA)],
        )
        .await?;

    Ok(())
}

pub async fn get_form_submissions(
    pool: &Pool,
    form_id: u64,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<Vec<u8>>> {
    let client = pool.get().await?;
    let form_id_i64 = form_id as i64;
    let rows = client
        .query_typed(
            "SELECT data FROM form_submissions WHERE form_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            &[(&form_id_i64, Type::INT8), (&limit, Type::INT8), (&offset, Type::INT8)],
        )
        .await?;

    Ok(rows.into_iter().map(|r| r.get(0)).collect())
}

pub async fn get_forms_by_owner(pool: &Pool, owner: &str) -> anyhow::Result<Vec<Vec<u8>>> {
    let client = pool.get().await?;
    let owner_str = owner.to_string();
    let rows = client
        .query_typed(
            "SELECT id, name, description, extract(epoch from created_at)::bigint as created_at, extract(epoch from deadline)::bigint as deadline FROM forms WHERE owner = $1 ORDER BY created_at DESC",
            &[(&owner_str, Type::VARCHAR)],
        )
        .await?;

    let mut results = Vec::new();
    for row in rows {
        let id: i64 = row.get(0);
        let name: String = row.get(1);
        let description: String = row.get(2);
        let created_at: i64 = row.get(3);
        let deadline: Option<i64> = row.get(4);

        let mut form = Form::default();
        form.id = id as u64;
        form.name = name.into();
        form.description = description.into();
        form.created_at = created_at as u64;
        form.deadline = deadline.unwrap_or(0) as u64;

        let mut out = Vec::new();
        let mut writer = quick_protobuf::Writer::new(&mut out);
        form.write_message(&mut writer)?;
        results.push(out);
    }

    Ok(results)
}

pub async fn create_otp(pool: &Pool, email: &str, form_id: Option<u64>) -> anyhow::Result<String> {
    let client = pool.get().await?;

    let code = generate_otp();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as f64;
    let expires_at = now + 300.0;
    let form_id_i64 = form_id.map(|id| id as i64);

    client
        .query_typed(
            "INSERT INTO otp_codes (email, code, form_id, expires_at) VALUES ($1, $2, $3, to_timestamp($4))",
            &[
                (&email, Type::VARCHAR),
                (&code, Type::VARCHAR),
                (&form_id_i64, Type::INT8),
                (&expires_at, Type::FLOAT8),
            ],
        )
        .await?;

    Ok(code)
}

pub async fn verify_otp(
    pool: &Pool,
    email: &str,
    code: &str,
    form_id: Option<u64>,
) -> anyhow::Result<bool> {
    let client = pool.get().await?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as f64;

    let form_id_i64 = form_id.map(|id| id as i64);

    let row = if let Some(fid) = form_id_i64 {
        let rows = client
            .query_typed(
                "SELECT id FROM otp_codes WHERE email = $1 AND code = $2 AND form_id = $3 AND used = false AND expires_at > to_timestamp($4)",
                &[
                    (&email, Type::VARCHAR),
                    (&code, Type::VARCHAR),
                    (&fid, Type::INT8),
                    (&now, Type::FLOAT8),
                ],
            )
            .await?;
        rows.into_iter().next()
    } else {
        let rows = client
            .query_typed(
                "SELECT id FROM otp_codes WHERE email = $1 AND code = $2 AND form_id IS NULL AND used = false AND expires_at > to_timestamp($3)",
                &[
                    (&email, Type::VARCHAR),
                    (&code, Type::VARCHAR),
                    (&now, Type::FLOAT8),
                ],
            )
            .await?;
        rows.into_iter().next()
    };

    if let Some(row) = row {
        let id: i64 = row.get(0);
        client
            .query_typed(
                "UPDATE otp_codes SET used = true WHERE id = $1",
                &[(&id, Type::INT8)],
            )
            .await?;
        return Ok(true);
    }

    Ok(false)
}

pub async fn is_participant_allowed(
    pool: &Pool,
    form_id: u64,
    participant: &str,
) -> anyhow::Result<bool> {
    let client = pool.get().await?;

    let form_id_i64 = form_id as i64;
    let rows = client
        .query_typed(
            "SELECT 1 FROM form_allowed_participants WHERE form_id = $1 AND participant = $2",
            &[(&form_id_i64, Type::INT8), (&participant, Type::VARCHAR)],
        )
        .await?;
    let row = rows.into_iter().next();

    Ok(row.is_some())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EmailClaims {
    pub sub: String, // email
    pub exp: usize,
    pub iat: usize,
    pub iss: String,
    pub aud: String,
    pub form_id: Option<u64>,
}

pub async fn get_jwt_secret(sd: &SharedData) -> anyhow::Result<Vec<u8>> {
    {
        let cache = sd.cache.jwt_secret.read().await;
        if let Some((key, created_at)) = &*cache {
            let now = SystemTime::now();
            if now
                .duration_since(*created_at)
                .unwrap_or_default()
                .as_secs()
                < 3600 * 24
            {
                return Ok(key.clone());
            }
        }
    }

    let client = sd.db.get().await?;
    let rows = client
        .query_typed(
            "SELECT key_data, created_at FROM secrets ORDER BY created_at DESC LIMIT 1",
            &[],
        )
        .await?;
    let row = rows.into_iter().next();

    if let Some(row) = row {
        let key_data: Vec<u8> = row.get(0);
        let created_at: SystemTime = row.get(1);
        let now = SystemTime::now();

        // Rotate every 24 hours for security (can be adjusted)
        if now.duration_since(created_at).unwrap_or_default().as_secs() > 3600 * 24 {
            return rotate_jwt_secret(sd).await;
        }

        let mut cache = sd.cache.jwt_secret.write().await;
        *cache = Some((key_data.clone(), created_at));

        Ok(key_data)
    } else {
        rotate_jwt_secret(sd).await
    }
}

pub async fn rotate_jwt_secret(sd: &SharedData) -> anyhow::Result<Vec<u8>> {
    let client = sd.db.get().await?;
    let mut key = vec![0u8; 32];
    rand::Rng::fill(&mut rand::thread_rng(), &mut key[..]);
    client
        .query_typed(
            "INSERT INTO secrets (key_data) VALUES ($1)",
            &[(&key, Type::BYTEA)],
        )
        .await?;

    let now = SystemTime::now();
    let mut cache = sd.cache.jwt_secret.write().await;
    *cache = Some((key.clone(), now));

    // Clear recent secrets cache so it will be reloaded
    let mut recent_cache = sd.cache.jwt_recent_secrets.write().await;
    *recent_cache = None;

    Ok(key)
}

pub async fn generate_email_jwt(
    sd: &SharedData,
    email: &str,
    form_id: Option<u64>,
    iss: String,
    aud: String,
) -> anyhow::Result<String> {
    let secret = get_jwt_secret(sd).await?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;

    let claims = EmailClaims {
        sub: email.to_string(),
        exp: now + 3600 * 24 * 7, // 7 days
        iat: now,
        iss,
        aud,
        form_id,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(&secret),
    )?;

    Ok(token)
}

pub async fn verify_email_jwt(
    sd: &SharedData,
    token: &str,
    iss: &str,
    aud: &str,
) -> anyhow::Result<(String, Option<u64>)> {
    let secrets = {
        let cache = sd.cache.jwt_recent_secrets.read().await;
        if let Some(s) = &*cache {
            s.clone()
        } else {
            drop(cache);
            let client = sd.db.get().await?;
            let rows = client
                .query_typed(
                    "SELECT key_data FROM secrets ORDER BY created_at DESC LIMIT 3",
                    &[],
                )
                .await?;
            let s: Vec<Vec<u8>> = rows.into_iter().map(|row| row.get(0)).collect();
            let mut cache = sd.cache.jwt_recent_secrets.write().await;
            *cache = Some(s.clone());
            s
        }
    };

    for secret in secrets {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[iss]);
        validation.set_audience(&[aud]);

        if let Ok(token_data) =
            decode::<EmailClaims>(token, &DecodingKey::from_secret(&secret), &validation)
        {
            return Ok((token_data.claims.sub, token_data.claims.form_id));
        }
    }

    Err(anyhow::anyhow!("Invalid token"))
}

pub async fn get_form_submissions_count(pool: &Pool, form_id: u64) -> anyhow::Result<u64> {
    let client = pool.get().await?;
    let form_id_i64 = form_id as i64;
    let rows = client
        .query_typed(
            "SELECT COUNT(id) FROM form_submissions WHERE form_id = $1",
            &[(&form_id_i64, Type::INT8)],
        )
        .await?;
    let row = rows
        .into_iter()
        .next()
        .context("No row returned from COUNT")?;
    let count: i64 = row.get(0);
    Ok(count as u64)
}

use crate::subscription_db::SubscriptionStatus;

pub async fn get_user_subscription_tier(pool: &Pool, owner: &str) -> anyhow::Result<String> {
    let client = pool.get().await?;
    let rows = client
        .query_typed(
            "SELECT tier, subscription_status FROM user_subscriptions WHERE owner = $1",
            &[(&owner, Type::VARCHAR)],
        )
        .await?;
    
    if let Some(row) = rows.into_iter().next() {
        let tier: String = row.get(0);
        let status_val: i16 = row.get(1);
        let status = SubscriptionStatus::from_i16(status_val);
        
        if status == SubscriptionStatus::Active {
            return Ok(tier);
        }
    }
    
    Ok("free".to_string())
}

pub async fn get_owner_form_count(pool: &Pool, owner: &str) -> anyhow::Result<u64> {
    let client = pool.get().await?;
    let rows = client
        .query_typed(
            "SELECT COUNT(id) FROM forms WHERE owner = $1",
            &[(&owner, Type::VARCHAR)],
        )
        .await?;
    let row = rows
        .into_iter()
        .next()
        .context("No row returned from COUNT")?;
    let count: i64 = row.get(0);
    Ok(count as u64)
}
