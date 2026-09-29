// SPDX-License-Identifier: Apache-2.0

use loadout_storage::{Database, ModelInput, NewAccount, NewProvider, StorageError};

fn db_with_provider() -> (Database, String) {
    let db = Database::open_in_memory().expect("db");
    let p = db
        .providers()
        .create(&NewProvider {
            slug: "acme",
            kind: "openai-compatible",
            display_name: "Acme",
            base_url: Some("https://acme.test/v1"),
            config_json: "{}",
        })
        .expect("provider");
    (db, p.id)
}

fn model(id: &str) -> ModelInput {
    ModelInput {
        upstream_model_id: id.into(),
        protocols_json: r#"["chat"]"#.into(),
        capabilities_json: r#"["streaming","tools"]"#.into(),
        ..ModelInput::default()
    }
}

#[test]
fn provider_slug_is_unique() {
    let (db, _) = db_with_provider();
    let dup = db.providers().create(&NewProvider {
        slug: "acme",
        kind: "anthropic-compatible",
        display_name: "x",
        base_url: None,
        config_json: "{}",
    });
    assert!(matches!(dup, Err(StorageError::Conflict(_))));
}

#[test]
fn accounts_are_ordered_and_alias_unique_per_provider() {
    let (db, pid) = db_with_provider();
    let a = db
        .accounts()
        .create(&NewAccount {
            provider_id: &pid,
            alias: "main",
            display_name: None,
            account_type: "api_key",
        })
        .expect("a");
    let b = db
        .accounts()
        .create(&NewAccount {
            provider_id: &pid,
            alias: "backup",
            display_name: None,
            account_type: "api_key",
        })
        .expect("b");
    assert_eq!((a.position, b.position), (0, 1));
    assert!(matches!(
        db.accounts().create(&NewAccount {
            provider_id: &pid,
            alias: "main",
            display_name: None,
            account_type: "api_key"
        }),
        Err(StorageError::Conflict(_))
    ));
    let listed: Vec<_> = db
        .accounts()
        .list(Some(&pid))
        .expect("list")
        .into_iter()
        .map(|a| a.alias)
        .collect();
    assert_eq!(listed, vec!["main", "backup"]);
}

#[test]
fn catalog_sync_preserves_user_choices() {
    let (db, pid) = db_with_provider();
    let acc = db
        .accounts()
        .create(&NewAccount {
            provider_id: &pid,
            alias: "main",
            display_name: None,
            account_type: "api_key",
        })
        .expect("acc");
    let repo = db.account_models();

    let first = repo
        .sync_catalog(&acc.id, &[model("a"), model("b"), model("c")])
        .expect("sync");
    assert_eq!((first.added, first.updated, first.disabled), (3, 0, 0));

    // User disables "a", prices "b", and adds a manual model.
    let a = repo.find(&acc.id, "a").expect("find").expect("a");
    repo.set_enabled(&a.id, false).expect("disable");
    let b = repo.find(&acc.id, "b").expect("find").expect("b");
    repo.set_prices(&b.id, Some(1.0), Some(2.0), None, None)
        .expect("price");
    repo.add_manual(&acc.id, &model("custom")).expect("manual");

    // Upstream now lists a, b (with a catalog price), d; c disappeared.
    let mut b_priced = model("b");
    b_priced.input_price = Some(9.0);
    b_priced.context_window = Some(64_000);
    let second = repo
        .sync_catalog(&acc.id, &[model("a"), b_priced, model("d")])
        .expect("sync");
    assert_eq!((second.added, second.updated, second.disabled), (1, 2, 1));

    let get = |id: &str| repo.find(&acc.id, id).expect("find").expect("exists");
    assert!(!get("a").enabled, "user disable survives sync");
    assert_eq!(get("b").input_price, Some(1.0), "user price survives sync");
    assert_eq!(get("b").context_window, Some(64_000), "metadata refreshed");
    assert!(!get("c").enabled, "vanished catalog model disabled");
    assert!(get("d").enabled);
    assert!(get("custom").enabled, "manual models untouched");
    assert_eq!(get("custom").source, "manual");

    assert!(matches!(
        repo.add_manual(&acc.id, &model("custom")),
        Err(StorageError::Conflict(_))
    ));
}

#[test]
fn routable_models_join_and_filter() {
    let (db, pid) = db_with_provider();
    let main = db
        .accounts()
        .create(&NewAccount {
            provider_id: &pid,
            alias: "main",
            display_name: None,
            account_type: "api_key",
        })
        .expect("main");
    let off = db
        .accounts()
        .create(&NewAccount {
            provider_id: &pid,
            alias: "off",
            display_name: None,
            account_type: "api_key",
        })
        .expect("off");
    db.accounts()
        .add_credential(&main.id, "api_key", "user", Some("ref-1"))
        .expect("cred");
    db.account_models()
        .sync_catalog(&main.id, &[model("m1"), model("m2")])
        .expect("sync");
    db.account_models()
        .sync_catalog(&off.id, &[model("m1")])
        .expect("sync");
    db.accounts().set_enabled(&off.id, false).expect("disable");
    let m2 = db
        .account_models()
        .find(&main.id, "m2")
        .expect("find")
        .expect("m2");
    db.account_models()
        .set_enabled(&m2.id, false)
        .expect("disable");

    let rows = db.routable_models().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].provider.slug, "acme");
    assert_eq!(rows[0].account_alias, "main");
    assert_eq!(rows[0].model.upstream_model_id, "m1");
    assert_eq!(rows[0].secret_ref.as_deref(), Some("ref-1"));
}

#[test]
fn deleting_provider_cascades_and_returns_secret_refs() {
    let (db, pid) = db_with_provider();
    let acc = db
        .accounts()
        .create(&NewAccount {
            provider_id: &pid,
            alias: "main",
            display_name: None,
            account_type: "api_key",
        })
        .expect("acc");
    db.accounts()
        .add_credential(&acc.id, "api_key", "user", Some("ref-x"))
        .expect("cred");
    db.account_models()
        .sync_catalog(&acc.id, &[model("m")])
        .expect("sync");

    let refs = db
        .providers()
        .delete_by_slug("acme")
        .expect("delete")
        .expect("existed");
    assert_eq!(refs, vec!["ref-x"]);
    assert!(db.accounts().list(None).expect("list").is_empty());
    assert!(db.routable_models().expect("rows").is_empty());
    assert!(
        db.providers()
            .delete_by_slug("acme")
            .expect("delete")
            .is_none()
    );
}

#[test]
fn replacing_credentials_returns_old_refs() {
    let (db, pid) = db_with_provider();
    let acc = db
        .accounts()
        .create(&NewAccount {
            provider_id: &pid,
            alias: "main",
            display_name: None,
            account_type: "api_key",
        })
        .expect("acc");
    db.accounts()
        .add_credential(&acc.id, "api_key", "user", Some("old"))
        .expect("cred");
    let old = db
        .accounts()
        .replace_credential(&acc.id, "api_key", "user", Some("new"))
        .expect("replace");
    assert_eq!(old, vec!["old"]);
    let creds = db.accounts().credentials(&acc.id).expect("creds");
    assert_eq!(creds.len(), 1);
    assert_eq!(creds[0].secret_ref.as_deref(), Some("new"));
}
