use noon_server::pb::forms::{
    BlindSubmission, FieldType, FieldValue, Form, FormSubmission, mod_FieldValue, mod_Form,
};
use noon_server::start_http_server;
use quick_protobuf::{MessageWrite, Writer};
use reqwest::{Client, StatusCode};
use std::borrow::Cow;
use std::time::Duration;

use base64::Engine;
use noon_core::blind::{create_blinded_message, unblind_signature};

fn serialize_proto<T: MessageWrite>(msg: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut writer = Writer::new(&mut bytes);
    msg.write_message(&mut writer)
        .expect("Failed to serialize protobuf");
    bytes
}

async fn setup_test_server(port: u16) -> String {
    unsafe {
        std::env::set_var("NO_TOKEN_VERIFICATION", "true");
        std::env::set_var(
            "DB_CONN_STR",
            "postgres://postgres:password123@localhost:39222/noondb?sslmode=disable",
        );
        std::env::set_var("EMULATOR_MODE", "true");
        std::env::set_var("FREE_MAX_FORMS", "1000");
    }

    tokio::spawn(async move {
        let _ = start_http_server(port).await;
    });

    tokio::time::sleep(Duration::from_millis(1000)).await;
    format!("http://127.0.0.1:{}", port)
}

#[tokio::test]
async fn test_create_form() {
    let port = 40216;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Test Form".into();
    form.description = "A test form".into();
    form.owner = "testuser".into();
    form.allowed_participants.push("testuser".into());

    let res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer testuser")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    assert_eq!(
        res.status(),
        StatusCode::OK,
        "Failed to create form: {}",
        res.text().await.unwrap()
    );
}

#[tokio::test]
async fn test_get_form() {
    let port = 40217;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Get Test Form".into();
    form.description = "Form to test retrieval".into();
    form.owner = "testuser".into();
    form.allowed_participants.push("testuser".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer testuser")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    let get_res = client
        .get(format!("{}/forms/{}", base_url, form_id))
        .header("Authorization", "Bearer testuser")
        .send()
        .await
        .expect("Failed to get form");

    assert_eq!(get_res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_form_not_found() {
    let port = 40218;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let res = client
        .get(format!("{}/forms/{}", base_url, 999999))
        .header("Authorization", "Bearer testuser")
        .send()
        .await
        .expect("Failed to get form");

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

async fn submit_form_blind(
    client: &Client,
    base_url: &str,
    form_id: u64,
    submission: &FormSubmission<'_>,
    auth_token: Option<&str>,
) -> StatusCode {
    // 1. Get public key
    let pk_res = client
        .get(format!("{}/forms/{}/public_key", base_url, form_id))
        .send()
        .await
        .expect("Failed to get public key");
    assert_eq!(pk_res.status(), StatusCode::OK);
    let pk_body = pk_res
        .json::<serde_json::Value>()
        .await
        .expect("Failed to parse public key");

    let n_bytes = base64::prelude::BASE64_STANDARD
        .decode(pk_body["n"].as_str().unwrap())
        .unwrap();
    let e_bytes = base64::prelude::BASE64_STANDARD
        .decode(pk_body["e"].as_str().unwrap())
        .unwrap();

    let n = rsa::BigUint::from_bytes_le(&n_bytes);
    let e = rsa::BigUint::from_bytes_le(&e_bytes);
    let public_key = rsa::RsaPublicKey::new(n, e).expect("Failed to create public key");

    // 2. Prepare payload
    let submission_bytes = serialize_proto(submission);
    let nonce = vec![1u8, 2, 3, 4];
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&submission_bytes);
    hasher.update(&nonce);
    let payload = hasher.finalize().to_vec();

    let blinded = create_blinded_message(&payload, &public_key);

    // 3. Request blind signature
    let mut req = client.post(format!("{}/forms/{}/blind_sign", base_url, form_id));
    if let Some(token) = auth_token {
        req = req.header("Authorization", format!("Bearer {}", token));
    }
    let sign_res = req
        .body(blinded.blinded_message())
        .send()
        .await
        .expect("Failed to request blind sign");

    if sign_res.status() != StatusCode::OK {
        return sign_res.status();
    }

    let blinded_sig = sign_res.bytes().await.unwrap();
    let signature = unblind_signature(&blinded, &blinded_sig, &public_key);

    // 4. Submit
    let mut blind_sub = BlindSubmission::default();
    blind_sub.payload = Cow::Owned(payload);
    blind_sub.signature = Cow::Owned(signature);
    blind_sub.submission = Cow::Owned(submission_bytes);
    blind_sub.nonce = Cow::Owned(nonce);

    let submit_res = client
        .post(format!("{}/forms/{}/submit", base_url, form_id))
        .body(serialize_proto(&blind_sub))
        .send()
        .await
        .expect("Failed to submit form");

    submit_res.status()
}

#[tokio::test]
async fn test_submit_form() {
    let port = 40219;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Submit Test Form".into();
    form.description = "Form to test submission".into();
    form.owner = "owner".into();
    form.allowed_participants.push("owner".into());
    form.allowed_participants.push("submitter".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer owner")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    let mut submission = FormSubmission::default();
    submission.form_id = form_id;
    let mut fv = FieldValue::default();
    fv.field_id = "field_1".into();
    fv.value = mod_FieldValue::OneOfvalue::string_value("Test Answer".into());
    submission.values.push(fv);

    let status =
        submit_form_blind(&client, &base_url, form_id, &submission, Some("submitter")).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_submit_form_unauthorized() {
    let port = 40220;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Unauthorized Test Form".into();
    form.description = "Form to test unauthorized submission".into();
    form.owner = "owner".into();
    form.allowed_participants.push("owner".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer owner")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    let mut submission = FormSubmission::default();
    submission.form_id = form_id;

    let status = submit_form_blind(&client, &base_url, form_id, &submission, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_submit_to_anonymous_form_fails() {
    let port = 40221;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Anonymous Form".into();
    form.description = "This form requires blind signatures".into();
    form.owner = "owner".into();
    form.allowed_participants.push("owner".into());
    form.allowed_participants.push("submitter".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer owner")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    let mut submission = FormSubmission::default();
    submission.form_id = form_id;

    // Direct submit fails with 400 because it expects BlindSubmission, not FormSubmission
    // (Actually it might fail because of signature verification if fields are empty)
    let submit_res = client
        .post(format!("{}/forms/{}/submit", base_url, form_id))
        .header("Authorization", "Bearer submitter")
        .body(serialize_proto(&submission))
        .send()
        .await
        .expect("Failed to submit form");

    assert_eq!(submit_res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_public_key() {
    let port = 40223;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Public Key Test Form".into();
    form.description = "Form to test public key retrieval".into();
    form.owner = "testuser".into();
    form.allowed_participants.push("testuser".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer testuser")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    let pk_res = client
        .get(format!("{}/forms/{}/public_key", base_url, form_id))
        .send()
        .await
        .expect("Failed to get public key");

    assert_eq!(pk_res.status(), StatusCode::OK);
    let body = pk_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(parsed.get("n").is_some());
    assert!(parsed.get("e").is_some());
}

#[tokio::test]
async fn test_create_and_submit_form_with_no_token_verification() {
    let port = 40224;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Integration Test Form".into();
    form.description = "Test form description".into();
    form.owner = "testuser".into();
    form.allowed_participants.push("testuser".into());
    form.allowed_participants.push("submitter_token".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer custom_user_token")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    assert_eq!(create_res.status(), StatusCode::OK, "Form creation failed");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().expect("Form ID not returned as u64");

    let mut submission = FormSubmission::default();
    submission.form_id = form_id;

    let mut fv = FieldValue::default();
    fv.field_id = "field_1".into();
    fv.value = mod_FieldValue::OneOfvalue::string_value("Test Value".into());
    submission.values.push(fv);

    let status = submit_form_blind(
        &client,
        &base_url,
        form_id,
        &submission,
        Some("submitter_token"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "Form submission failed");
}

#[tokio::test]
async fn test_create_form_with_fields() {
    let port = 40225;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Form With Fields".into();
    form.description = "Form containing field definitions".into();
    form.owner = "testuser".into();
    form.allowed_participants.push("testuser".into());

    let mut field = mod_Form::Field::default();
    field.id = "email_field".into();
    field.name = "email".into();
    field.label = "Email Address".into();
    field.type_pb = FieldType::TEXT;
    field.required = true;
    field.placeholder = "Enter your email".into();
    form.fields.push(field);

    let mut field2 = mod_Form::Field::default();
    field2.id = "age_field".into();
    field2.name = "age".into();
    field2.label = "Age".into();
    field2.type_pb = FieldType::NUMBER;
    field2.required = false;
    form.fields.push(field2);

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer testuser")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    assert_eq!(create_res.status(), StatusCode::OK);

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    let get_res = client
        .get(format!("{}/forms/{}", base_url, form_id))
        .header("Authorization", "Bearer testuser")
        .send()
        .await
        .expect("Failed to get form");

    assert_eq!(get_res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_create_form_with_otp_verification() {
    let port = 40226;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "OTP Test Form".into();
    form.description = "Form with OTP verification".into();
    form.owner = "testuser".into();
    form.allowed_participants.push("test@example.com".into());

    let res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer testuser")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    assert_eq!(res.status(), StatusCode::OK);
    let body = res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    use noon_server::pb::forms::OtpRequest;
    let mut otp_req = OtpRequest::default();
    otp_req.email = "test@example.com".into();
    otp_req.form_id = form_id;

    let otp_res = client
        .post(format!("{}/email/request_otp", base_url))
        .body(serialize_proto(&otp_req))
        .send()
        .await
        .expect("Failed to request OTP");

    let otp_status = otp_res.status();
    if otp_status != StatusCode::OK {
        let body = otp_res.text().await.unwrap();
        eprintln!("OTP request failed: {} - {}", otp_status, body);
    }
    assert_eq!(otp_status, StatusCode::OK);

    use noon_server::pb::forms::OtpVerify;
    let mut otp_verify = OtpVerify::default();
    otp_verify.email = "test@example.com".into();
    otp_verify.code = "123456".into();
    otp_verify.form_id = form_id;

    let verify_res = client
        .post(format!("{}/email/verify_otp", base_url))
        .body(serialize_proto(&otp_verify))
        .send()
        .await
        .expect("Failed to verify OTP");

    assert_eq!(verify_res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_request_otp_for_non_otp_form_fails() {
    let port = 40227;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "Regular Form".into();
    form.description = "Form without OTP".into();
    form.owner = "testuser".into();
    form.allowed_participants.push("testuser".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer testuser")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    use noon_server::pb::forms::OtpRequest;
    let mut otp_req = OtpRequest::default();
    otp_req.email = "test@example.com".into();
    otp_req.form_id = form_id;

    let otp_res = client
        .post(format!("{}/email/request_otp", base_url))
        .body(serialize_proto(&otp_req))
        .send()
        .await
        .expect("Failed to request OTP");

    assert_eq!(otp_res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_request_otp_for_unauthorized_email_fails() {
    let port = 40228;
    let base_url = setup_test_server(port).await;
    let client = Client::new();

    let mut form = Form::default();
    form.name = "OTP Form".into();
    form.description = "Form with OTP".into();
    form.owner = "testuser".into();
    form.allowed_participants
        .push("authorized@example.com".into());

    let create_res = client
        .post(format!("{}/forms/create", base_url))
        .header("Authorization", "Bearer testuser")
        .body(serialize_proto(&form))
        .send()
        .await
        .expect("Failed to create form");

    let create_body = create_res.text().await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&create_body).unwrap();
    let form_id = parsed["id"].as_u64().unwrap();

    use noon_server::pb::forms::OtpRequest;
    let mut otp_req = OtpRequest::default();
    otp_req.email = "unauthorized@example.com".into();
    otp_req.form_id = form_id;

    let otp_res = client
        .post(format!("{}/email/request_otp", base_url))
        .body(serialize_proto(&otp_req))
        .send()
        .await
        .expect("Failed to request OTP");

    assert_eq!(otp_res.status(), StatusCode::FORBIDDEN);
}
