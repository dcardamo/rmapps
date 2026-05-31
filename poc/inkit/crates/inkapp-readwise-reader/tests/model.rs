use inkapp_readwise_reader::{Location, Readwise};

#[test]
fn cassette_still_loads_with_defaults() {
    let rw = Readwise::from_cassette();
    let all = rw.queue();
    assert!(!all.is_empty());
    assert!(all.iter().all(|a| !a.title.is_empty()));
    // Verify serde defaults applied for fields absent in the cassette JSON.
    let first = &all[0];
    assert_eq!(
        first.location,
        Location::New,
        "location should default to New"
    );
    assert!(first.url.is_empty(), "url should default to empty string");
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

/// Failing-test-as-spec for a planned refinement (Spec #11 review, issue #1):
/// an optimistic move to a *visible* Library location should keep the article in
/// view at its new location, not hide it until the next refresh. Today `move_to`
/// hides via the overlay regardless of target, so this fails — un-ignore it when
/// the optimistic location-override lands.
#[test]
#[ignore = "spec for a future refinement: optimistic move to a visible location should keep the article visible"]
fn move_to_visible_location_keeps_it_visible() {
    let rw = Readwise::fake(); // fake articles default to Location::New (a Library location)
    let id = rw.library()[0].id.clone();
    rw.move_to(&id, Location::Later);
    let found = rw.library().into_iter().find(|a| a.id == id);
    assert!(
        found.is_some(),
        "a move to a visible location should keep the article in the library view"
    );
    assert_eq!(
        found.unwrap().location,
        Location::Later,
        "and show it at its new location"
    );
}
