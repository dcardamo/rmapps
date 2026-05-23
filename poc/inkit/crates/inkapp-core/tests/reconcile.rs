use inkapp_core::document::DocKey;
use inkapp_core::reconcile::{reconcile, DocOp};

fn k(s: &str) -> DocKey {
    DocKey::new(s)
}

#[test]
fn create_update_delete_noop() {
    // prev: a@1, b@1, c@1 ; next: a@1 (noop), b@2 (update), d@1 (create); c deleted.
    let prev = vec![(k("a"), 1u64), (k("b"), 1), (k("c"), 1)];
    let next = vec![(k("a"), 1u64), (k("b"), 2), (k("d"), 1)];
    let ops = reconcile(&prev, &next);
    assert_eq!(
        ops,
        vec![
            DocOp::Update(k("b")),
            DocOp::Create(k("d")),
            DocOp::Delete(k("c"))
        ]
    );
}

#[test]
fn all_new_is_all_create() {
    let ops = reconcile(&[], &[(k("a"), 1), (k("b"), 1)]);
    assert_eq!(ops, vec![DocOp::Create(k("a")), DocOp::Create(k("b"))]);
}

#[test]
fn all_gone_is_all_delete() {
    let ops = reconcile(&[(k("a"), 1), (k("b"), 1)], &[]);
    assert_eq!(ops, vec![DocOp::Delete(k("a")), DocOp::Delete(k("b"))]);
}
