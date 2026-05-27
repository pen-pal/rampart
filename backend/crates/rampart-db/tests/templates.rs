//! Integration tests for `rampart_db::templates`.

use rampart_db::templates::{
    create, delete, get, get_render_strings, list, update, NewTemplate, UpdateTemplate,
};
use sqlx::PgPool;

fn sample(name: &str) -> NewTemplate {
    NewTemplate {
        name: name.into(),
        channel_kinds: vec!["slack".into(), "discord".into()],
        event_kind: "monitor_down".into(),
        subject_template: Some("[{{status}}] {{monitor.name}}".into()),
        body_template: "{{monitor.name}} went {{status}}".into(),
        is_default: false,
    }
}

fn empty_patch() -> UpdateTemplate {
    UpdateTemplate {
        name: None,
        channel_kinds: None,
        event_kind: None,
        subject_template: None,
        body_template: None,
        is_default: None,
    }
}

#[sqlx::test(migrations = "../../migrations")]
async fn list_empty_initially(pool: PgPool) {
    assert!(list(&pool).await.unwrap().is_empty());
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_round_trips_all_fields(pool: PgPool) {
    let t = create(&pool, sample("Concise")).await.unwrap();
    assert_eq!(t.name, "Concise");
    assert_eq!(t.event_kind, "monitor_down");
    assert_eq!(
        t.channel_kinds,
        vec!["slack".to_string(), "discord".to_string()]
    );
    assert_eq!(
        t.subject_template.as_deref(),
        Some("[{{status}}] {{monitor.name}}")
    );

    let again = get(&pool, t.id).await.unwrap();
    assert_eq!(again.name, t.name);
    assert_eq!(again.body_template, t.body_template);
}

#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_name_conflicts(pool: PgPool) {
    create(&pool, sample("Dup")).await.unwrap();
    let err = create(&pool, sample("Dup")).await.unwrap_err();
    assert!(
        matches!(err, rampart_db::DbError::Conflict(_)),
        "got: {err:?}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn update_changes_body_only(pool: PgPool) {
    let t = create(&pool, sample("Updatable")).await.unwrap();
    let mut patch = empty_patch();
    patch.body_template = Some("new body".into());
    let patched = update(&pool, t.id, patch).await.unwrap();
    assert_eq!(patched.body_template, "new body");
    // Other fields preserved.
    assert_eq!(patched.name, "Updatable");
    assert_eq!(patched.event_kind, "monitor_down");
}

#[sqlx::test(migrations = "../../migrations")]
async fn update_can_clear_subject_with_explicit_none(pool: PgPool) {
    let t = create(&pool, sample("Sub")).await.unwrap();
    assert!(t.subject_template.is_some());
    let mut patch = empty_patch();
    patch.subject_template = Some(None);
    let patched = update(&pool, t.id, patch).await.unwrap();
    assert!(patched.subject_template.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn delete_removes_template(pool: PgPool) {
    let t = create(&pool, sample("Gone")).await.unwrap();
    delete(&pool, t.id).await.unwrap();
    assert!(get(&pool, t.id).await.is_err());
}

#[sqlx::test(migrations = "../../migrations")]
async fn get_render_strings_returns_subject_and_body(pool: PgPool) {
    let t = create(&pool, sample("Render")).await.unwrap();
    let r = get_render_strings(&pool, t.id).await.unwrap();
    assert_eq!(r.subject.as_deref(), Some("[{{status}}] {{monitor.name}}"));
    assert_eq!(r.body, "{{monitor.name}} went {{status}}");
}
