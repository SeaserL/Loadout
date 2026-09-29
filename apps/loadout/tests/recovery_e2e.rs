// SPDX-License-Identifier: Apache-2.0

//! A damaged database does not keep the daemon down (ExecPlan 007): the file is moved
//! aside, an empty database is created and the status reports it.

mod common;

use common::Daemon;

#[test]
fn damaged_database_is_moved_aside_and_reported() {
    let data = tempfile::tempdir().expect("tempdir");
    let db_dir = data.path().join("data");
    std::fs::create_dir_all(&db_dir).expect("mkdir");
    std::fs::write(db_dir.join("loadout.db"), vec![0x42; 16 * 1024]).expect("garbage");

    let d = Daemon::start_in(data, None);
    let status = d.json(&["daemon", "status"], None);
    let moved = status["recovered_from"].as_str().expect("recovered_from");
    assert!(moved.contains("loadout.db.corrupt-"), "{moved}");
    assert_eq!(std::fs::read(moved).expect("kept").len(), 16 * 1024);

    // The new database works.
    d.json(&["client", "create", "after-recovery"], None);
    assert_eq!(d.json(&["client", "list"], None)[0]["name"], "after-recovery");

    // The human-readable status explains how to restore.
    let text = String::from_utf8(d.cli(&["daemon", "status"], None).stdout).expect("utf8");
    assert!(text.contains("backup import"), "{text}");
}
