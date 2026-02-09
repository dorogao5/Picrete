use crate::api::pagination::default_limit;
use crate::core::time::primitive_now_utc;
use crate::repositories;
use crate::test_support;
use axum::http::{Method, StatusCode};
use serde_json::json;
use tower::ServiceExt;

#[tokio::test]
async fn admin_can_create_and_update_user() {
    let ctx = test_support::setup_test_context().await;

    let admin =
        test_support::insert_platform_admin(ctx.state.db(), "admin001", "Admin User", "admin-pass")
            .await;
    let token = test_support::bearer_token(&admin.id, ctx.state.settings());

    let create_payload = json!({
        "username": "student123",
        "full_name": "Student User",
        "password": "student-pass",
        "is_platform_admin": false,
        "is_active": true,
        "is_verified": false
    });

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/users",
            Some(&token),
            Some(create_payload),
        ))
        .await
        .expect("create user");

    let status = response.status();
    let created = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::CREATED, "response: {created}");
    let user_id = created["id"].as_str().expect("user id").to_string();
    assert_eq!(created["username"], "student123");
    assert_eq!(created["full_name"], "Student User");
    assert_eq!(created["is_platform_admin"], false);

    let update_payload = json!({
        "full_name": "Updated Student",
        "is_active": false
    });

    let response = ctx
        .app
        .clone()
        .oneshot(test_support::json_request(
            Method::PATCH,
            &format!("/api/v1/users/{user_id}"),
            Some(&token),
            Some(update_payload),
        ))
        .await
        .expect("update user");

    let status = response.status();
    let updated = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {updated}");
    assert_eq!(updated["full_name"], "Updated Student");
    assert_eq!(updated["is_active"], false);

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::GET,
            &format!("/api/v1/users/{user_id}"),
            Some(&token),
            None,
        ))
        .await
        .expect("get user");

    let status = response.status();
    let fetched = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::OK, "response: {fetched}");
    assert_eq!(fetched["full_name"], "Updated Student");
}

#[tokio::test]
async fn admin_create_user_rejects_short_password() {
    let ctx = test_support::setup_test_context().await;

    let admin =
        test_support::insert_platform_admin(ctx.state.db(), "admin051", "Admin User", "admin-pass")
            .await;
    let token = test_support::bearer_token(&admin.id, ctx.state.settings());

    let response = ctx
        .app
        .oneshot(test_support::json_request(
            Method::POST,
            "/api/v1/users",
            Some(&token),
            Some(json!({
                "username": "student450",
                "full_name": "Short Password",
                "password": "short"
            })),
        ))
        .await
        .expect("create user");

    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "response: {body}");
    assert!(body["detail"].as_str().unwrap_or("").contains("Password must be at least"));
}

#[test]
fn default_limit_is_positive() {
    assert!(default_limit() > 0);
}

#[tokio::test]
async fn inactive_user_token_is_rejected_by_guard() {
    let ctx = test_support::setup_test_context().await;

    let user =
        test_support::insert_user(ctx.state.db(), "student099", "Inactive User", "student-pass")
            .await;
    let token = test_support::bearer_token(&user.id, ctx.state.settings());

    repositories::users::update(
        ctx.state.db(),
        &user.id,
        repositories::users::UpdateUser {
            full_name: None,
            is_platform_admin: None,
            is_active: Some(false),
            is_verified: None,
            hashed_password: None,
            updated_at: primitive_now_utc(),
        },
    )
    .await
    .expect("deactivate user");

    let response = ctx
        .app
        .oneshot(test_support::json_request(Method::GET, "/api/v1/users/me", Some(&token), None))
        .await
        .expect("get me");

    let status = response.status();
    let body = test_support::read_json(response).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "response: {body}");
}
