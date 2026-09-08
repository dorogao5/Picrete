//! ITMO.ID Authorization Code + PKCE. Browser binding lives only in sessionStorage.
use crate::{
    api::{errors::ApiError, guards::CurrentUser},
    core::{security, state::AppState},
    repositories,
    schemas::{auth::TokenResponse, user::UserResponse},
};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use jsonwebtoken::{decode, decode_header, jwk::JwkSet, Algorithm, DecodingKey, Validation};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use uuid::Uuid;

const ISSUER: &str = "https://id.itmo.ru/auth/realms/itmo";
fn random() -> String {
    let mut b = [0u8; 32];
    OsRng.fill_bytes(&mut b);
    URL_SAFE_NO_PAD.encode(b)
}
fn hash(v: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(v.as_bytes()))
}
fn unavailable() -> ApiError {
    ApiError::BadRequest("Вход через ITMO.ID ещё не подключён".into())
}
fn rejected() -> ApiError {
    ApiError::Unauthorized("Не удалось подтвердить вход через ITMO.ID. Начните вход заново.")
}
fn internal(e: impl std::fmt::Display) -> ApiError {
    tracing::error!("ITMO operation failed: {}", e);
    ApiError::Internal("ITMO.ID временно недоступен".into())
}
pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/config", get(config))
        .route("/start", post(start))
        .route("/link", post(link))
        .route("/finish", post(finish))
}
async fn config(State(s): State<AppState>) -> Json<Value> {
    Json(json!({"enabled":s.settings().itmo.enabled}))
}
#[derive(Deserialize)]
struct Start {
    consent: bool,
}
async fn start(State(s): State<AppState>, Json(p): Json<Start>) -> Result<Json<Value>, ApiError> {
    begin(s, p, None).await
}
async fn link(
    State(s): State<AppState>,
    CurrentUser(u): CurrentUser,
    Json(p): Json<Start>,
) -> Result<Json<Value>, ApiError> {
    begin(s, p, Some(u.id)).await
}
async fn begin(s: AppState, p: Start, link: Option<String>) -> Result<Json<Value>, ApiError> {
    let c = &s.settings().itmo;
    if !c.enabled {
        return Err(unavailable());
    }
    if !p.consent {
        return Err(ApiError::BadRequest("Подтвердите согласие на обработку данных".into()));
    }
    let allowed = s.redis().rate_limit("rl:itmo:start", 120, 60).await.map_err(internal)?;
    if !allowed {
        return Err(ApiError::TooManyRequests("Повторите вход позже"));
    }
    let state = random();
    let binding = random();
    let verifier = random();
    let nonce = random();
    sqlx::query("DELETE FROM oidc_login_states WHERE expires_at < now()")
        .execute(s.db())
        .await
        .map_err(internal)?;
    sqlx::query("INSERT INTO oidc_login_states(state_hash,binding_hash,verifier,nonce,expires_at,consent,link_user_id) VALUES($1,$2,$3,$4,now()+interval '10 minutes',true,$5)").bind(hash(&state)).bind(hash(&binding)).bind(&verifier).bind(&nonce).bind(link).execute(s.db()).await.map_err(internal)?;
    let mut url =
        reqwest::Url::parse(&format!("{ISSUER}/protocol/openid-connect/auth")).map_err(internal)?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", c.client_id.as_str()),
        ("redirect_uri", &c.redirect_uri),
        ("response_type", "code"),
        ("scope", "openid name email edu"),
        ("state", &state),
        ("nonce", &nonce),
        ("code_challenge", &hash(&verifier)),
        ("code_challenge_method", "S256"),
    ]);
    Ok(Json(json!({"authorization_url":url.to_string(),"state":state,"binding":binding})))
}
#[derive(Deserialize)]
struct Finish {
    state: String,
    binding: String,
    code: String,
}
#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    id_token: String,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Identity {
    sub: String,
    #[serde(default)]
    isu: Option<i64>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    family_name: Option<String>,
    #[serde(default)]
    given_name: Option<String>,
    #[serde(default)]
    middle_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<bool>,
    #[serde(default)]
    is_student: Option<bool>,
    #[serde(default)]
    groups: Vec<Group>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Group {
    name: String,
    course: i64,
}
impl Identity {
    fn eligible(&self, allowed: &[String], study_year: i64) -> bool {
        self.is_student == Some(true)
            && self.groups.iter().any(|g| g.course == study_year && allowed.contains(&g.name))
    }
    fn full_name(&self) -> String {
        let parts: Vec<_> = [&self.family_name, &self.given_name, &self.middle_name]
            .into_iter()
            .filter_map(|v| v.as_deref())
            .filter(|v| !v.trim().is_empty())
            .collect();
        if parts.is_empty() {
            self.name.clone().unwrap_or_else(|| "Пользователь ИТМО".into())
        } else {
            parts.join(" ")
        }
    }
}
static KEYS: OnceLock<Mutex<Option<(Instant, JwkSet)>>> = OnceLock::new();
async fn key(client: &reqwest::Client, kid: &str) -> Result<DecodingKey, ApiError> {
    let mut cache = KEYS.get_or_init(|| Mutex::new(None)).lock().await;
    if cache.as_ref().map_or(true, |(t, _)| t.elapsed() > Duration::from_secs(3600)) {
        let keys = client
            .get(format!("{ISSUER}/protocol/openid-connect/certs"))
            .send()
            .await
            .map_err(internal)?
            .error_for_status()
            .map_err(internal)?
            .json::<JwkSet>()
            .await
            .map_err(internal)?;
        *cache = Some((Instant::now(), keys));
    }
    let jwk = cache.as_ref().and_then(|(_, k)| k.find(kid)).ok_or_else(rejected)?;
    DecodingKey::from_jwk(jwk).map_err(|_| rejected())
}
fn validate_id_token(
    token: &str,
    key: &DecodingKey,
    client_id: &str,
    nonce: &str,
    access_token: &str,
) -> Result<Value, ApiError> {
    let mut v = Validation::new(Algorithm::RS256);
    v.set_audience(&[client_id]);
    v.set_issuer(&[ISSUER]);
    v.set_required_spec_claims(&["exp", "iss", "aud", "sub", "iat"]);
    let claims = decode::<Value>(token, key, &v).map_err(|_| rejected())?.claims;
    if claims.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return Err(rejected());
    }
    let azp = claims.get("azp").and_then(Value::as_str);
    if azp.is_some_and(|a| a != client_id)
        || (claims["aud"].as_array().is_some_and(|a| a.len() > 1) && azp != Some(client_id))
    {
        return Err(rejected());
    }
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    if claims["iat"].as_i64().is_none_or(|iat| iat > now + 60) {
        return Err(rejected());
    }
    if let Some(at_hash) = claims.get("at_hash") {
        let digest = Sha256::digest(access_token.as_bytes());
        if at_hash.as_str() != Some(URL_SAFE_NO_PAD.encode(&digest[..16]).as_str()) {
            return Err(rejected());
        }
    }
    Ok(claims)
}
async fn consume_state(
    s: &AppState,
    p: &Finish,
) -> Result<(String, String, Option<String>), ApiError> {
    sqlx::query_as::<_,(String,String,Option<String>)>("DELETE FROM oidc_login_states WHERE state_hash=$1 AND binding_hash=$2 AND expires_at>now() AND consent RETURNING verifier,nonce,link_user_id").bind(hash(&p.state)).bind(hash(&p.binding)).fetch_optional(s.db()).await.map_err(internal)?.ok_or_else(rejected)
}
async fn finish(
    State(s): State<AppState>,
    Json(p): Json<Finish>,
) -> Result<Json<TokenResponse>, ApiError> {
    let c = &s.settings().itmo;
    if !c.enabled {
        return Err(unavailable());
    }
    if p.state.len() > 256 || p.binding.len() > 256 || p.code.len() > 4096 {
        return Err(rejected());
    }
    let row = consume_state(&s, &p).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(internal)?;
    let token = client
        .post(format!("{ISSUER}/protocol/openid-connect/token"))
        .form(&[
            ("client_id", c.client_id.as_str()),
            ("client_secret", &c.client_secret),
            ("grant_type", "authorization_code"),
            ("redirect_uri", &c.redirect_uri),
            ("code", &p.code),
            ("code_verifier", &row.0),
        ])
        .send()
        .await
        .map_err(internal)?
        .error_for_status()
        .map_err(|_| rejected())?
        .json::<Tokens>()
        .await
        .map_err(|_| rejected())?;
    let h = decode_header(&token.id_token).map_err(|_| rejected())?;
    if h.alg != Algorithm::RS256 {
        return Err(rejected());
    }
    let k = key(&client, h.kid.as_deref().ok_or_else(rejected)?).await?;
    let claims = validate_id_token(&token.id_token, &k, &c.client_id, &row.1, &token.access_token)?;
    let identity = client
        .get(format!("{ISSUER}/protocol/openid-connect/userinfo"))
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(internal)?
        .error_for_status()
        .map_err(|_| rejected())?
        .json::<Identity>()
        .await
        .map_err(|_| rejected())?;
    if identity.sub.is_empty()
        || claims.get("sub").and_then(Value::as_str) != Some(identity.sub.as_str())
    {
        return Err(rejected());
    }
    let user_id = persist(&s, &identity, row.2).await?;
    let user = repositories::users::find_by_id(s.db(), &user_id)
        .await
        .map_err(internal)?
        .ok_or_else(rejected)?;
    if !user.is_active {
        return Err(rejected());
    }
    let memberships = super::auth::load_memberships(&s, &user).await?;
    let active_course_id = memberships
        .iter()
        .find(|m| {
            m.course_slug == "infochem-29" && m.status == crate::db::types::MembershipStatus::Active
        })
        .or_else(|| memberships.first())
        .map(|m| m.course_id.clone());
    Ok(Json(TokenResponse {
        access_token: security::create_access_token(&user.id, s.settings(), None)
            .map_err(internal)?,
        token_type: "bearer".into(),
        user: UserResponse::from_db(user),
        memberships,
        active_course_id,
    }))
}
async fn persist(s: &AppState, i: &Identity, link: Option<String>) -> Result<String, ApiError> {
    let mut tx = s.db().begin().await.map_err(internal)?;
    // Serialise identity creation/linking across concurrent callbacks.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("{ISSUER}:{}", i.sub))
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM external_identities WHERE issuer=$1 AND subject=$2",
    )
    .bind(ISSUER)
    .bind(&i.sub)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if matches!((&existing,&link),(Some(a),Some(b)) if a!=b) {
        return Err(ApiError::Conflict(
            "Этот ITMO.ID уже связан с другим аккаунтом Picrete".into(),
        ));
    }
    let uid = if let Some(id) = existing.or(link) {
        let active =
            sqlx::query_scalar::<_, bool>("SELECT is_active FROM users WHERE id=$1 FOR UPDATE")
                .bind(&id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(internal)?;
        if active != Some(true) {
            return Err(rejected());
        }
        id
    } else {
        let id = Uuid::new_v4().to_string();
        let password = security::hash_password(&random()).map_err(internal)?;
        sqlx::query("INSERT INTO users(id,username,hashed_password,full_name,is_platform_admin,is_active,pd_consent,pd_consent_at,pd_consent_version,terms_accepted_at,terms_version,privacy_version) VALUES($1,$2,$3,$4,false,true,true,now(),$5,now(),$6,$7)").bind(&id).bind(format!("itmo_{}",Uuid::new_v4().simple())).bind(password).bind(i.full_name()).bind(&s.settings().api().pd_consent_version).bind(&s.settings().api().terms_version).bind(&s.settings().api().privacy_version).execute(&mut *tx).await.map_err(internal)?;
        id
    };
    let other = sqlx::query_scalar::<_, String>(
        "SELECT subject FROM external_identities WHERE issuer=$1 AND user_id=$2",
    )
    .bind(ISSUER)
    .bind(&uid)
    .fetch_optional(&mut *tx)
    .await
    .map_err(internal)?;
    if other.as_deref().is_some_and(|sub| sub != i.sub) {
        return Err(ApiError::Conflict("К этому аккаунту уже привязан другой ITMO.ID".into()));
    }
    let profile = serde_json::to_value(i).map_err(internal)?;
    sqlx::query("INSERT INTO external_identities(issuer,subject,user_id,profile) VALUES($1,$2,$3,$4) ON CONFLICT(issuer,subject) DO UPDATE SET profile=EXCLUDED.profile,synced_at=now()").bind(ISSUER).bind(&i.sub).bind(&uid).bind(&profile).execute(&mut *tx).await.map_err(internal)?;
    sqlx::query("UPDATE users SET full_name=$2,updated_at=now(),pd_consent=true,pd_consent_at=now(),pd_consent_version=$3,terms_accepted_at=now(),terms_version=$4,privacy_version=$5 WHERE id=$1")
        .bind(&uid)
        .bind(i.full_name())
        .bind(&s.settings().api().pd_consent_version)
        .bind(&s.settings().api().terms_version)
        .bind(&s.settings().api().privacy_version)
        .execute(&mut *tx)
        .await
        .map_err(internal)?;
    for rule in &s.settings().itmo.enrollment_rules {
        let teacher = i.isu.is_some_and(|isu| rule.teacher_isu.contains(&isu));
        if !teacher && !i.eligible(&rule.groups, rule.study_year) {
            continue;
        }
        let membership = Uuid::new_v4().to_string();
        let inserted=sqlx::query_scalar::<_,String>("INSERT INTO course_memberships(id,course_id,user_id,status,identity_payload) SELECT $1,id,$2,'active',$3 FROM courses WHERE slug=$4 AND is_active ON CONFLICT(course_id,user_id) DO UPDATE SET identity_payload=course_memberships.identity_payload || EXCLUDED.identity_payload WHERE course_memberships.status='active' RETURNING id").bind(&membership).bind(&uid).bind(json!({"provider":"itmo.id","groups":i.groups,"isu":i.isu})).bind(&rule.course_slug).fetch_optional(&mut *tx).await.map_err(internal)?;
        if let Some(mid) = inserted {
            sqlx::query(
                "INSERT INTO course_membership_roles(membership_id,role) VALUES($1,$2) ON CONFLICT DO NOTHING",
            )
            .bind(mid)
            .bind(if teacher { crate::db::types::CourseRole::Teacher } else { crate::db::types::CourseRole::Student })
            .execute(&mut *tx)
            .await
            .map_err(internal)?;
        }
    }
    tx.commit().await.map_err(internal)?;
    Ok(uid)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enrollment_requires_trusted_student_year_and_explicit_group() {
        let mut i: Identity = serde_json::from_value(
            json!({"sub":"x","is_student":true,"groups":[{"name":"K123","course":2}]}),
        )
        .unwrap();
        assert!(!i.eligible(&[], 2));
        assert!(i.eligible(&["K123".into()], 2));
        i.is_student = None;
        assert!(!i.eligible(&["K123".into()], 2));
        i.is_student = Some(true);
        i.groups[0].course = 1;
        assert!(!i.eligible(&["K123".into()], 2));
    }
    #[test]
    fn full_name_uses_russian_order() {
        let i: Identity = serde_json::from_value(
            json!({"sub":"x","family_name":"Иванов","given_name":"Иван","middle_name":"Иванович"}),
        )
        .unwrap();
        assert_eq!(i.full_name(), "Иванов Иван Иванович");
    }
}

#[cfg(test)]
mod database_tests {
    use super::*;
    use crate::test_support;
    #[tokio::test]
    async fn identities_are_stable_and_linking_never_merges_other_users() {
        let ctx = test_support::setup_test_context().await;
        let first: Identity = serde_json::from_value(
            json!({"sub":"first","family_name":"Иванов","email":"same@example.org"}),
        )
        .unwrap();
        let second: Identity =
            serde_json::from_value(json!({"sub":"second","email":"same@example.org"})).unwrap();
        let a = persist(&ctx.state, &first, None).await.unwrap();
        assert_eq!(a, persist(&ctx.state, &first, None).await.unwrap());
        let b = persist(&ctx.state, &second, None).await.unwrap();
        assert_ne!(a, b);
        assert!(persist(&ctx.state, &first, Some(b)).await.is_err());
        sqlx::query("UPDATE users SET is_active=false WHERE id=$1")
            .bind(&a)
            .execute(ctx.state.db())
            .await
            .unwrap();
        assert!(persist(&ctx.state, &first, None).await.is_err());
    }
    #[tokio::test]
    async fn browser_state_is_bound_expiring_and_single_use() {
        let ctx = test_support::setup_test_context().await;
        sqlx::query("INSERT INTO oidc_login_states VALUES($1,$2,'verifier','nonce',now()+interval '1 minute',true,NULL)").bind(hash("state")).bind(hash("browser")).execute(ctx.state.db()).await.unwrap();
        let request = |binding: &str| Finish {
            state: "state".into(),
            binding: binding.into(),
            code: "unused".into(),
        };
        assert!(consume_state(&ctx.state, &request("attacker")).await.is_err());
        assert!(consume_state(&ctx.state, &request("browser")).await.is_ok());
        assert!(consume_state(&ctx.state, &request("browser")).await.is_err());
    }
}

#[cfg(test)]
mod token_tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    #[test]
    fn rejects_wrong_signature_nonce_issuer_audience_expiry_and_access_binding() {
        let key =
            EncodingKey::from_rsa_pem(include_bytes!("../../tests/fixtures/oidc-test-key.pem"))
                .unwrap();
        let public =
            DecodingKey::from_rsa_pem(include_bytes!("../../tests/fixtures/oidc-test-public.pem"))
                .unwrap();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let good = json!({"sub":"student","iss":ISSUER,"aud":"picrete","nonce":"nonce","exp":now+300,"iat":now});
        let sign = |v: &Value| encode(&Header::new(Algorithm::RS256), v, &key).unwrap();
        assert!(validate_id_token(&sign(&good), &public, "picrete", "nonce", "access").is_ok());
        for (field, bad) in [
            ("nonce", json!("other")),
            ("iss", json!("https://attacker")),
            ("aud", json!("other")),
            ("exp", json!(now - 300)),
            ("iat", json!(now + 300)),
            ("azp", json!("other")),
            ("at_hash", json!("wrong")),
        ] {
            let mut v = good.clone();
            v[field] = bad;
            assert!(
                validate_id_token(&sign(&v), &public, "picrete", "nonce", "access").is_err(),
                "{field}"
            );
        }
        let mut signed = sign(&good).into_bytes();
        let n = signed.len() - 10;
        signed[n] = if signed[n] == b'A' { b'B' } else { b'A' };
        assert!(validate_id_token(
            std::str::from_utf8(&signed).unwrap(),
            &public,
            "picrete",
            "nonce",
            "access"
        )
        .is_err());
    }
}

#[cfg(test)]
mod enrollment_tests {
    use super::*;
    use crate::test_support;
    #[tokio::test]
    async fn eligible_student_joins_main_course_without_restoring_suspended_membership() {
        let ctx = test_support::setup_test_context().await;
        let teacher =
            test_support::insert_user(ctx.state.db(), "teacher", "Учитель", "password123").await;
        let course = test_support::insert_course(
            ctx.state.db(),
            "infochem-29",
            "Неорганическая химия",
            &teacher.id,
        )
        .await;
        let mut settings = ctx.state.settings().clone();
        settings.itmo.enrollment_rules[0].groups = vec!["K222".into()];
        let state =
            AppState::new(settings, ctx.state.db().clone(), ctx.state.redis().clone(), None);
        let identity: Identity = serde_json::from_value(
            json!({"sub":"eligible","is_student":true,"groups":[{"name":"K222","course":2}]}),
        )
        .unwrap();
        let uid = persist(&state, &identity, None).await.unwrap();
        let roles =
            repositories::course_memberships::list_for_user(state.db(), &uid).await.unwrap();
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].course_id, course.id);
        assert_eq!(roles[0].roles, vec![crate::db::types::CourseRole::Student]);
        sqlx::query("UPDATE course_memberships SET status='suspended' WHERE user_id=$1")
            .bind(&uid)
            .execute(state.db())
            .await
            .unwrap();
        persist(&state, &identity, None).await.unwrap();
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status::text FROM course_memberships WHERE user_id=$1",
        )
        .bind(uid)
        .fetch_one(state.db())
        .await
        .unwrap();
        assert_eq!(status, "suspended");
    }
}

#[cfg(test)]
mod teacher_tests {
    use super::*;
    use crate::test_support;
    #[tokio::test]
    async fn teachers_get_only_explicitly_assigned_courses_without_registration() {
        let ctx = test_support::setup_test_context().await;
        let owner =
            test_support::insert_user(ctx.state.db(), "owner", "Владелец", "password123").await;
        let course =
            test_support::insert_course(ctx.state.db(), "infochem-29", "Химия", &owner.id).await;
        let mut settings = ctx.state.settings().clone();
        settings.itmo.enrollment_rules[0].teacher_isu = vec![123456];
        let state =
            AppState::new(settings, ctx.state.db().clone(), ctx.state.redis().clone(), None);
        let person = |sub: &str, isu: i64| {
            serde_json::from_value::<Identity>(
                json!({"sub":sub,"isu":isu,"is_student":false,"name":"Преподаватель"}),
            )
            .unwrap()
        };
        let uid = persist(&state, &person("assigned", 123456), None).await.unwrap();
        let memberships =
            repositories::course_memberships::list_for_user(state.db(), &uid).await.unwrap();
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].course_id, course.id);
        assert_eq!(memberships[0].roles, vec![crate::db::types::CourseRole::Teacher]);
        let other = persist(&state, &person("unassigned", 123457), None).await.unwrap();
        assert!(repositories::course_memberships::list_for_user(state.db(), &other)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(uid, persist(&state, &person("assigned", 123456), None).await.unwrap());
    }
}

#[cfg(test)]
mod sso_signup_tests {
    use super::*;
    use crate::test_support;
    use tower::ServiceExt;
    #[tokio::test]
    async fn sso_mode_rejects_separate_registration() {
        let ctx = test_support::setup_test_context().await;
        let mut settings = ctx.state.settings().clone();
        settings.itmo.enabled = true;
        let state =
            AppState::new(settings, ctx.state.db().clone(), ctx.state.redis().clone(), None);
        let app = crate::api::router::router(state);
        let response=app.oneshot(test_support::json_request(axum::http::Method::POST,"/api/v1/auth/signup",None,Some(json!({"username":"student","password":"password123","full_name":"Студент","pd_consent":true})))).await.unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
        let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
            .fetch_one(ctx.state.db())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
}
