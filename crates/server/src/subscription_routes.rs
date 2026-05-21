use bytes::Bytes;
use http_body_util::Full;
use hyper::{Method, Request, Response, StatusCode, body::Incoming, header};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::forms_db;
use crate::subscription_db;
use crate::shared_data::SharedData;
use crate::{
    bad_request_response, build_response, internal_error_response, limit_and_collect,
    not_found_response, ok_response, unauthorized_response,
};

const RAZORPAY_API_BASE: &str = "https://api.razorpay.com/v1";

// ─── Route Dispatch ──────────────────────────────────────────────────────────

pub async fn handle_request(
    req: Request<Incoming>,
    sd: Arc<SharedData>,
    path: &str,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let method = req.method().clone();
    match (method, path) {
        (Method::POST, "/subscription/create") => create_subscription_route(req, sd).await,
        (Method::POST, "/subscription/cancel") => cancel_subscription_route(req, sd).await,
        (Method::GET, "/subscription/status") => status_route(req, sd).await,
        (Method::POST, "/subscription/webhook") => webhook_route(req, sd).await,
        (Method::GET, "/subscription/config") => config_route(req, sd).await,
        _ => Ok(not_found_response()),
    }
}

// ─── Auth helper ─────────────────────────────────────────────────────────────

/// Extracts the owner string (e.g. "email:user@example.com") from the Authorization header.
async fn get_owner(
    headers: &header::HeaderMap,
    sd: &Arc<SharedData>,
) -> Option<String> {
    let auth = headers.get(header::AUTHORIZATION)?.to_str().ok()?;

    if *crate::utils::EMULATOR_MODE {
        if let Some(u) = auth.strip_prefix("Bearer ") {
            return Some(format!("user:{}", u));
        }
    }

    if let Some(token) = auth.strip_prefix("EmailOnly ") {
        if let Ok((email, _)) =
            forms_db::verify_email_jwt(sd, token, &sd.config.auth_iss, &sd.config.auth_aud).await
        {
            return Some(format!("email:{}", email));
        }
    }
    None
}

// ─── Razorpay JSON types ──────────────────────────────────────────────────────

#[derive(Serialize)]
struct CreateSubRequest {
    plan_id: String,
    total_count: u32,
    quantity: u32,
    customer_notify: u8,
    notes: serde_json::Value,
}

#[derive(Deserialize)]
struct CreateSubResponse {
    id: String,
}

#[derive(Deserialize)]
struct WebhookEvent {
    event: String,
    payload: WebhookPayload,
}

#[derive(Deserialize)]
struct WebhookPayload {
    subscription: Option<WebhookSubWrapper>,
}

#[derive(Deserialize)]
struct WebhookSubWrapper {
    entity: RzpSubEntity,
}

#[derive(Deserialize)]
struct RzpSubEntity {
    id: String,
    plan_id: String,
    current_end: Option<i64>,
}

// ─── Webhook signature verification ──────────────────────────────────────────

fn verify_razorpay_signature(webhook_secret: &str, body: &[u8], signature: &str) -> bool {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, webhook_secret.as_bytes());
    match hex::decode(signature) {
        Ok(sig_bytes) => ring::hmac::verify(&key, body, &sig_bytes).is_ok(),
        Err(_) => false,
    }
}

// ─── Routes ──────────────────────────────────────────────────────────────────

/// POST /subscription/create
/// Body: {"tier": "pro"} or {"tier": "team"}
/// Returns: {"subscription_id": "sub_xxx", "key_id": "rzp_test_xxx"}
async fn create_subscription_route(
    req: Request<Incoming>,
    sd: Arc<SharedData>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let rzp = match &sd.config.razorpay {
        Some(r) => r,
        None => {
            return Ok(build_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Payment integration not configured",
            ))
        }
    };

    let owner = match get_owner(req.headers(), &sd).await {
        Some(o) => o,
        None => return Ok(unauthorized_response()),
    };

    let body_bytes = match limit_and_collect(req.into_body(), 1024).await {
        Ok(b) => b,
        Err(_) => return Ok(bad_request_response()),
    };

    let body: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(_) => return Ok(bad_request_response()),
    };

    let tier = match body.get("tier").and_then(|v| v.as_str()) {
        Some(t) if t == "pro" || t == "team" => t.to_string(),
        _ => {
            return Ok(build_response(
                StatusCode::BAD_REQUEST,
                "tier must be \"pro\" or \"team\"",
            ))
        }
    };

    let plan_id = if tier == "pro" {
        rzp.pro_plan_id.clone()
    } else {
        rzp.team_plan_id.clone()
    };

    if plan_id.is_empty() {
        return Ok(build_response(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Plan ID for {} tier is not configured yet", tier),
        ));
    }

    let payload = CreateSubRequest {
        plan_id: plan_id.clone(),
        total_count: 12, // 12 billing cycles
        quantity: 1,
        customer_notify: 1,
        notes: serde_json::json!({ "owner": owner }),
    };

    let resp = match sd
        .http_client
        .post(format!("{}/subscriptions", RAZORPAY_API_BASE))
        .basic_auth(&rzp.key_id, Some(&rzp.key_secret))
        .json(&payload)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::error!("Razorpay create subscription error: {:?}", e);
            return Ok(internal_error_response());
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        log::error!("Razorpay returned {}: {}", status, text);
        return Ok(internal_error_response());
    }

    let sub: CreateSubResponse = match resp.json().await {
        Ok(s) => s,
        Err(e) => {
            log::error!("Razorpay response parse error: {:?}", e);
            return Ok(internal_error_response());
        }
    };

    // Store as pending — webhook will activate it
    if let Err(e) = subscription_db::upsert_pending_subscription(
        &sd.db,
        &owner,
        &sub.id,
        &plan_id,
        &tier,
    )
    .await
    {
        log::error!("upsert_pending_subscription error: {:?}", e);
        return Ok(internal_error_response());
    }

    let response_json = serde_json::json!({
        "subscription_id": sub.id,
        "key_id": rzp.key_id
    });

    Ok(ok_response(response_json.to_string()))
}

/// POST /subscription/cancel
async fn cancel_subscription_route(
    req: Request<Incoming>,
    sd: Arc<SharedData>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let rzp = match &sd.config.razorpay {
        Some(r) => r,
        None => {
            return Ok(build_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Payment integration not configured",
            ))
        }
    };

    let owner = match get_owner(req.headers(), &sd).await {
        Some(o) => o,
        None => return Ok(unauthorized_response()),
    };

    // Get current subscription info
    let info = match subscription_db::get_subscription_info(&sd.db, &owner).await {
        Ok(i) => i,
        Err(e) => {
            log::error!("get_subscription_info error: {:?}", e);
            return Ok(internal_error_response());
        }
    };

    let sub_id = match info.razorpay_subscription_id {
        Some(id) if !id.is_empty() => id,
        _ => return Ok(build_response(StatusCode::BAD_REQUEST, "No active subscription found")),
    };

    // Call Razorpay to cancel
    let resp = match sd
        .http_client
        .post(format!("{}/subscriptions/{}/cancel", RAZORPAY_API_BASE, sub_id))
        .basic_auth(&rzp.key_id, Some(&rzp.key_secret))
        .json(&serde_json::json!({
            "cancel_at_cycle_end": 0
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::error!("Razorpay cancel subscription error: {:?}", e);
            return Ok(internal_error_response());
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        log::error!("Razorpay cancel returned {}: {}", status, text);
        return Ok(build_response(status, text));
    }

    Ok(ok_response("Subscription cancellation requested"))
}

/// GET /subscription/status
/// Returns current subscription info for the authenticated user.
async fn status_route(
    req: Request<Incoming>,
    sd: Arc<SharedData>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let owner = match get_owner(req.headers(), &sd).await {
        Some(o) => o,
        None => return Ok(unauthorized_response()),
    };

    match subscription_db::get_subscription_info(&sd.db, &owner).await {
        Ok(info) => {
            let json = serde_json::json!({
                "owner": info.owner,
                "tier": info.tier,
                "subscription_status": info.subscription_status.as_str(),
                "razorpay_subscription_id": info.razorpay_subscription_id,
                "current_period_end": info.current_period_end,
                "max_participants": sd.config.limits.max_participants_for(&info.tier)
            });
            Ok(ok_response(json.to_string()))
        }
        Err(e) => {
            log::error!("get_subscription_info error: {:?}", e);
            Ok(internal_error_response())
        }
    }
}

/// POST /subscription/webhook
/// Razorpay sends signed events here. No user auth — verified via HMAC-SHA256.
async fn webhook_route(
    req: Request<Incoming>,
    sd: Arc<SharedData>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let rzp = match &sd.config.razorpay {
        Some(r) => r,
        None => return Ok(ok_response("OK")), // Silently ignore if not configured
    };

    // Must capture signature BEFORE consuming body
    let signature = match req
        .headers()
        .get("X-Razorpay-Signature")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
    {
        Some(s) => s,
        None => {
            log::warn!("Webhook received without X-Razorpay-Signature");
            return Ok(build_response(StatusCode::BAD_REQUEST, "Missing signature"));
        }
    };

    let body_bytes = match limit_and_collect(req.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return Ok(bad_request_response()),
    };

    if !verify_razorpay_signature(&rzp.webhook_secret, &body_bytes, &signature) {
        log::warn!("Webhook signature verification failed");
        return Ok(build_response(StatusCode::UNAUTHORIZED, "Invalid signature"));
    }

    let event: WebhookEvent = match serde_json::from_slice(&body_bytes) {
        Ok(e) => e,
        Err(e) => {
            log::error!("Webhook parse error: {:?}", e);
            return Ok(bad_request_response());
        }
    };

    log::info!("Razorpay webhook received: {}", event.event);

    let sub_entity = match event.payload.subscription.map(|w| w.entity) {
        Some(e) => e,
        None => {
            log::info!("Webhook event {} has no subscription payload, ignoring", event.event);
            return Ok(ok_response("OK"));
        }
    };

    if sub_entity.plan_id != rzp.pro_plan_id && sub_entity.plan_id != rzp.team_plan_id {
        log::info!("Ignoring webhook event for unrelated plan_id: {}", sub_entity.plan_id);
        return Ok(ok_response("OK"));
    }

    match event.event.as_str() {
        "subscription.activated" | "subscription.charged" => {
            // Determine tier from plan_id
            let tier = if sub_entity.plan_id == rzp.pro_plan_id {
                "pro"
            } else if sub_entity.plan_id == rzp.team_plan_id {
                "team"
            } else {
                // Should be unreachable due to check above, but keeping for safety
                log::warn!("Unknown plan_id in webhook: {}", sub_entity.plan_id);
                return Ok(ok_response("OK"));
            };

            // Ensure tier is set correctly (handles charged events after plan change)
            let client = match sd.db.get().await {
                Ok(c) => c,
                Err(e) => {
                    log::error!("DB pool error in webhook: {:?}", e);
                    return Ok(internal_error_response());
                }
            };
            let _ = client
                .query_typed(
                    "UPDATE user_subscriptions SET tier = $2, updated_at = NOW() \
                     WHERE razorpay_subscription_id = $1",
                    &[
                        (&sub_entity.id.as_str(), deadpool_postgres::tokio_postgres::types::Type::VARCHAR),
                        (&tier, deadpool_postgres::tokio_postgres::types::Type::VARCHAR),
                    ],
                )
                .await;
            drop(client);

            if let Err(e) = subscription_db::activate_subscription(
                &sd.db,
                &sub_entity.id,
                "active",
                sub_entity.current_end,
            )
            .await
            {
                log::error!("activate_subscription error: {:?}", e);
                return Ok(internal_error_response());
            }
            log::info!(
                "Subscription {} activated/charged, tier={}, period_end={:?}",
                sub_entity.id,
                tier,
                sub_entity.current_end
            );
        }

        "subscription.pending" => {
            if let Err(e) = subscription_db::activate_subscription(
                &sd.db,
                &sub_entity.id,
                "pending",
                sub_entity.current_end,
            )
            .await
            {
                log::error!("activate_subscription(pending) error: {:?}", e);
            }
        }

        "subscription.cancelled" | "subscription.halted" | "subscription.completed" => {
            let status = event.event.strip_prefix("subscription.").unwrap_or("cancelled");
            if let Err(e) =
                subscription_db::cancel_subscription(&sd.db, &sub_entity.id, status).await
            {
                log::error!("cancel_subscription error: {:?}", e);
                return Ok(internal_error_response());
            }
            log::info!("Subscription {} → downgraded to free ({})", sub_entity.id, status);
        }

        other => {
            log::debug!("Unhandled Razorpay event: {}", other);
        }
    }

    Ok(ok_response("OK"))
}

/// GET /subscription/config
/// Returns the tier configuration limits.
async fn config_route(
    _req: Request<Incoming>,
    sd: Arc<SharedData>,
) -> Result<Response<Full<Bytes>>, std::convert::Infallible> {
    let json = serde_json::json!({
        "free_max_participants": sd.config.limits.free_max_participants,
        "pro_max_participants": sd.config.limits.pro_max_participants,
        "team_max_participants": sd.config.limits.team_max_participants,
    });
    Ok(ok_response(json.to_string()))
}
