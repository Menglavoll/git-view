//! GitLab Provider 集成测试。
//!
//! 覆盖：
//!   - 200 成功（PRIVATE-TOKEN 头）
//!   - 401 → TokenInvalid
//!   - 403 → Forbidden
//!   - Token 不泄漏
//!   - derive_gitlab_api_url 表驱动用例（虽然单元测试已覆盖，这里再次确认契约）

#![allow(clippy::unwrap_used, clippy::expect_used)]

use gitview_lib::errors::GitViewError;
use gitview_lib::models::account::GitPlatform;
use gitview_lib::models::repository::{CreateRepoRequest, RemoteRepository, Visibility};
use gitview_lib::services::gitlab_service::{
    derive_gitlab_api_url, GitLabClientConfig, GitLabProvider,
};
use gitview_lib::services::provider::{GitHostingProvider, RemoteTagType};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TEST_TOKEN: &str = "glpat-abcd1234efgh5678ijkl"; // allow-token-pattern: 测试样本

fn client_config(api_base: &str) -> GitLabClientConfig {
    GitLabClientConfig {
        api_base_url: api_base.to_string(),
        allow_invalid_certs: false,
        proxy_url: None,
        request_timeout_seconds: Some(5),
    }
}

fn test_repo() -> RemoteRepository {
    RemoteRepository {
        id: "repo-1".to_string(),
        account_id: "acc-1".to_string(),
        platform: GitPlatform::Gitlab,
        remote_id: "42".to_string(),
        full_name: "group/demo".to_string(),
        name: "demo".to_string(),
        owner: "group".to_string(),
        description: None,
        visibility: Visibility::Private,
        default_branch: "main".to_string(),
        html_url: "https://gitlab.example/group/demo".to_string(),
        ssh_url: None,
        clone_url: "https://gitlab.example/group/demo.git".to_string(),
        is_favorite: false,
        last_pushed_at: None,
        synced_at: chrono::Utc::now(),
    }
}

/// create_repository：201 成功，project 响应映射为 RemoteRepository
#[tokio::test]
async fn create_repository_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/projects"))
        .and(header("private-token", TEST_TOKEN))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "id": 909,
            "path_with_namespace": "alice/new-proj",
            "name": "new-proj",
            "namespace": { "path": "alice" },
            "description": "demo",
            "default_branch": "main",
            "visibility": "private",
            "web_url": "https://gitlab.example.com/alice/new-proj",
            "http_url_to_repo": "https://gitlab.example.com/alice/new-proj.git",
            "ssh_url_to_repo": "git@gitlab.example.com:alice/new-proj.git",
        })))
        .mount(&server)
        .await;

    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
        .expect("应能构造");
    let req = CreateRepoRequest {
        name: "new-proj".to_string(),
        description: Some("demo".to_string()),
        visibility: Visibility::Private,
    };
    let repo = provider
        .create_repository(&req, "acc-1")
        .await
        .expect("应成功");
    assert_eq!(repo.remote_id, "909");
    assert_eq!(repo.full_name, "alice/new-proj");
    assert!(matches!(repo.visibility, Visibility::Private));
}

/// create_repository：400 且含 "has already been taken" → RepoNameTaken
#[tokio::test]
async fn create_repository_conflict_maps_to_name_taken() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/projects"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "message": { "name": ["has already been taken"] }
        })))
        .mount(&server)
        .await;

    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
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
async fn get_current_user_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("private-token", TEST_TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42,
            "username": "alice",
            "name": "Alice Liddell",
            "avatar_url": "https://avatars.example.com/alice.png",
            "web_url": "https://gitlab.example.com/alice",
        })))
        .mount(&server)
        .await;

    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
        .expect("应能构造 GitLabProvider");
    let profile = provider.get_current_user().await.expect("应成功");
    assert_eq!(profile.username, "alice");
    assert_eq!(profile.display_name.as_deref(), Some("Alice Liddell"));
}

#[tokio::test]
async fn unauthorized_maps_to_token_invalid() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
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
    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
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
    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
        .expect("应能构造");
    let err = provider.get_current_user().await.unwrap_err();
    let display = err.to_string();
    assert!(
        !display.contains(TEST_TOKEN),
        "错误消息不应包含 token 明文：{display}"
    );
}

#[tokio::test]
async fn create_annotated_tag_sends_message_to_tags_api() {
    let server = MockServer::start().await;
    let sha = "0123456789abcdef0123456789abcdef01234567";
    Mock::given(method("GET"))
        .and(path("/projects/42/repository/branches/main"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "name": "main", "commit": { "id": sha, "title": "release commit" }
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/projects/42/repository/tags/v1.0.0"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/projects/42/repository/tags"))
        .and(header("private-token", TEST_TOKEN))
        .and(query_param("tag_name", "v1.0.0"))
        .and(query_param("ref", "main"))
        .and(query_param("message", "release notes"))
        .respond_with(ResponseTemplate::new(201))
        .mount(&server)
        .await;

    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
        .expect("应能构造");
    let created = provider
        .create_tag(
            &test_repo(),
            "main",
            "v1.0.0",
            RemoteTagType::Annotated,
            Some("release notes"),
        )
        .await
        .expect("附注 Tag 应创建成功");
    assert_eq!(created.target_sha, sha);
}

/// list_repositories：完整响应（含 visibility）正确解析与映射，x-next-page 驱动 has_next
#[tokio::test]
async fn list_repositories_full_response_maps_fields() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/projects"))
        .and(header("private-token", TEST_TOKEN))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-next-page", "2")
                .set_body_json(serde_json::json!([{
                    "id": 101,
                    "path_with_namespace": "group/proj",
                    "name": "proj",
                    "namespace": { "path": "group" },
                    "description": "demo",
                    "default_branch": "main",
                    "visibility": "public",
                    "web_url": "https://gitlab.example.com/group/proj",
                    "http_url_to_repo": "https://gitlab.example.com/group/proj.git",
                    "ssh_url_to_repo": "git@gitlab.example.com:group/proj.git",
                    "last_activity_at": "2024-01-02T03:04:05.000Z",
                }])),
        )
        .mount(&server)
        .await;

    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
        .expect("应能构造");
    let page = provider
        .list_repositories(1, 20, "acc-1")
        .await
        .expect("应成功");

    assert!(page.has_next, "x-next-page=2 时应有下一页");
    assert_eq!(page.items.len(), 1);
    let repo = &page.items[0];
    assert_eq!(repo.full_name, "group/proj");
    assert_eq!(repo.name, "proj");
    assert_eq!(repo.owner, "group");
    assert_eq!(repo.default_branch, "main");
    assert!(matches!(repo.visibility, Visibility::Public));
}

/// list_repositories：精简响应（缺 visibility / default_branch）必须仍能解析
///
/// 这是对 simple=true 缺字段导致反序列化崩溃的回归防护：缺省字段应安全回退而非报错。
#[tokio::test]
async fn list_repositories_simple_response_without_visibility_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/projects"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                "id": 202,
                "path_with_namespace": "user/simple",
                "name": "simple",
                "namespace": { "path": "user" },
                "web_url": "https://gitlab.example.com/user/simple",
                "http_url_to_repo": "https://gitlab.example.com/user/simple.git",
            }])),
        )
        .mount(&server)
        .await;

    let provider = GitLabProvider::new(client_config(&server.uri()), TEST_TOKEN.to_string())
        .expect("应能构造");
    let page = provider
        .list_repositories(1, 20, "acc-1")
        .await
        .expect("缺 visibility 字段时也应成功解析，而非报错");

    assert!(!page.has_next, "无 x-next-page 头时应无下一页");
    assert_eq!(page.items.len(), 1);
    let repo = &page.items[0];
    // 缺省 visibility 安全回退私有
    assert!(matches!(repo.visibility, Visibility::Private));
    // 缺省 default_branch 回退 main
    assert_eq!(repo.default_branch, "main");
}

/// derive_gitlab_api_url 表驱动测试（与单元测试互补，强调集成层契约稳定）
#[test]
fn derive_api_url_table_driven() {
    let cases: &[(&str, &str)] = &[
        ("https://gitlab.com", "https://gitlab.com/api/v4"),
        ("https://gitlab.com/", "https://gitlab.com/api/v4"),
        (
            "https://gitlab.company.com:8443",
            "https://gitlab.company.com:8443/api/v4",
        ),
        ("http://10.0.0.5", "http://10.0.0.5/api/v4"),
        (
            "https://code.company.com/gitlab",
            "https://code.company.com/gitlab/api/v4",
        ),
        (
            "https://code.company.com/gitlab/",
            "https://code.company.com/gitlab/api/v4",
        ),
    ];
    for (input, expected) in cases {
        let got = derive_gitlab_api_url(input).expect("应成功");
        assert_eq!(got, *expected, "input={input}");
    }
}
