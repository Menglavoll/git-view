//! Gitee Provider 集成测试。
//!
//! 覆盖两种认证模式：
//!   - Header `Authorization: token <token>`（默认）
//!   - Query `?access_token=<token>`
//!
//! 以及统一的错误码映射与 token 不泄漏断言。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use gitview_lib::errors::GitViewError;
use gitview_lib::models::account::GitPlatform;
use gitview_lib::models::repository::{CreateRepoRequest, RemoteRepository, Visibility};
use gitview_lib::services::gitee_service::{GiteeAuthMode, GiteeProvider};
use gitview_lib::services::provider::{GitHostingProvider, RemoteTagType};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_TOKEN: &str = "gitee_test_token_for_integration_testing";

fn test_repo() -> RemoteRepository {
    RemoteRepository {
        id: "repo-1".to_string(),
        account_id: "acc-1".to_string(),
        platform: GitPlatform::Gitee,
        remote_id: "42".to_string(),
        full_name: "alice/demo".to_string(),
        name: "demo".to_string(),
        owner: "alice".to_string(),
        description: None,
        visibility: Visibility::Private,
        default_branch: "main".to_string(),
        html_url: "https://gitee.com/alice/demo".to_string(),
        ssh_url: None,
        clone_url: "https://gitee.com/alice/demo.git".to_string(),
        is_favorite: false,
        last_pushed_at: None,
        synced_at: chrono::Utc::now(),
    }
}

/// create_repository：成功映射（Header 认证 + form 提交）
#[tokio::test]
async fn create_repository_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/user/repos"))
        .and(header("authorization", format!("token {TEST_TOKEN}")))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": 777,
            "full_name": "alice/new-repo",
            "name": "new-repo",
            "owner": { "login": "alice" },
            "description": "demo",
            "default_branch": "master",
            "private": true,
            "html_url": "https://gitee.com/alice/new-repo",
            "ssh_url": "git@gitee.com:alice/new-repo.git",
            "clone_url": "https://gitee.com/alice/new-repo.git",
        })))
        .mount(&server)
        .await;

    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Header,
    )
    .expect("应能构造");
    let req = CreateRepoRequest {
        name: "new-repo".to_string(),
        description: Some("demo".to_string()),
        visibility: Visibility::Private,
    };
    let repo = provider
        .create_repository(&req, "acc-1")
        .await
        .expect("应成功");
    assert_eq!(repo.remote_id, "777");
    assert_eq!(repo.full_name, "alice/new-repo");
    assert!(matches!(repo.visibility, Visibility::Private));
}

/// create_repository：400 且含中文「存在」→ RepoNameTaken
#[tokio::test]
async fn create_repository_conflict_maps_to_name_taken() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/user/repos"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "message": "仓库已存在"
        })))
        .mount(&server)
        .await;

    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Header,
    )
    .expect("应能构造");
    let req = CreateRepoRequest {
        name: "dup".to_string(),
        description: None,
        visibility: Visibility::Private,
    };
    let err = provider.create_repository(&req, "acc-1").await.unwrap_err();
    assert!(matches!(err, GitViewError::RepoNameTaken));
}

#[tokio::test]
async fn get_current_user_with_header_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("authorization", format!("token {TEST_TOKEN}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "login": "alice",
            "name": "Alice",
            "avatar_url": "https://gitee.com/avatars/alice.png",
        })))
        .mount(&server)
        .await;

    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Header,
    )
    .expect("应能构造");
    let profile = provider.get_current_user().await.expect("应成功");
    assert_eq!(profile.username, "alice");
    assert_eq!(profile.display_name.as_deref(), Some("Alice"));
}

#[tokio::test]
async fn get_current_user_with_query_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(query_param("access_token", TEST_TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "login": "bob",
            "name": null,
            "avatar_url": null,
        })))
        .mount(&server)
        .await;

    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Query,
    )
    .expect("应能构造");
    let profile = provider.get_current_user().await.expect("应成功");
    assert_eq!(profile.username, "bob");
    assert!(profile.display_name.is_none());
    assert!(profile.avatar_url.is_none());
}

#[tokio::test]
async fn unauthorized_maps_to_token_invalid() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Header,
    )
    .expect("应能构造");
    let err = provider.get_current_user().await.unwrap_err();
    assert!(matches!(err, GitViewError::TokenInvalid));
}

#[tokio::test]
async fn forbidden_maps_to_forbidden() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Header,
    )
    .expect("应能构造");
    let err = provider.get_current_user().await.unwrap_err();
    assert!(matches!(err, GitViewError::Forbidden));
}

#[tokio::test]
async fn error_message_does_not_leak_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Header,
    )
    .expect("应能构造");
    let err = provider.get_current_user().await.unwrap_err();
    let display = err.to_string();
    assert!(
        !display.contains(TEST_TOKEN),
        "错误消息不应包含 token 明文：{display}"
    );
}

#[tokio::test]
async fn create_lightweight_tag_uses_header_auth() {
    let server = MockServer::start().await;
    let sha = "0123456789abcdef0123456789abcdef01234567";
    Mock::given(method("GET"))
        .and(path("/repos/alice/demo/branches/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "main", "commit": { "sha": sha, "commit": { "message": "release" } }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/alice/demo/tags/v1.0.0"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/alice/demo/tags"))
        .and(header("authorization", format!("token {TEST_TOKEN}")))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let provider = GiteeProvider::new(
        Some(server.uri()),
        TEST_TOKEN.to_string(),
        None,
        GiteeAuthMode::Header,
    )
    .expect("应能构造");
    let created = provider
        .create_tag(
            &test_repo(),
            "main",
            "v1.0.0",
            RemoteTagType::Lightweight,
            None,
        )
        .await
        .expect("轻量 Tag 应创建成功");
    assert_eq!(created.target_sha, sha);
}
