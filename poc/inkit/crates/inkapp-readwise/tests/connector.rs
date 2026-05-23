use inkapp_readwise::Readwise;

#[test]
fn cassette_loads_articles() {
    let rw = Readwise::from_cassette();
    assert!(rw.queue().len() >= 3, "committed cassette has articles");
}

#[test]
fn archive_removes_from_queue_and_records() {
    let rw = Readwise::fake();
    let before = rw.queue().len();
    let id = rw.queue()[0].id.clone();
    rw.archive(&id);
    assert_eq!(
        rw.queue().len(),
        before - 1,
        "archived article leaves the queue"
    );
    assert_eq!(rw.archived(), vec![id]);
}

#[test]
fn highlight_is_recorded_and_merged() {
    let rw = Readwise::fake();
    let id = rw.queue()[0].id.clone();
    rw.add_highlight(&id, "patience");
    assert_eq!(rw.highlights(&id), vec!["patience".to_string()]);
    let art = rw.queue().into_iter().find(|a| a.id == id).unwrap();
    assert!(
        art.highlights.contains(&"patience".to_string()),
        "queue merges overlay highlights"
    );
}

#[test]
fn duplicate_highlight_recorded_once() {
    let rw = Readwise::fake();
    let id = rw.queue()[0].id.clone();
    rw.add_highlight(&id, "patience");
    rw.add_highlight(&id, "patience");
    assert_eq!(
        rw.highlights(&id),
        vec!["patience".to_string()],
        "duplicate highlight deduped"
    );
}
