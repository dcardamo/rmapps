use inkapp_readwise_reader::{Location, Readwise};

#[test]
fn cassette_still_loads_with_defaults() {
    let rw = Readwise::from_cassette();
    let all = rw.queue();
    assert!(!all.is_empty());
    assert!(all.iter().all(|a| !a.title.is_empty()));
}

#[test]
fn move_and_delete_enqueue_and_hide() {
    let rw = Readwise::fake();
    let id = rw.queue()[0].id.clone();
    rw.move_to(&id, Location::Archive);
    assert!(
        rw.queue().iter().all(|a| a.id != id),
        "archived/moved leaves the queue"
    );
    let id2 = rw.queue()[0].id.clone();
    rw.delete(&id2);
    assert!(
        rw.queue().iter().all(|a| a.id != id2),
        "deleted leaves the queue"
    );
}
