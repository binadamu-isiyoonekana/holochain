use super::*;
use crate::here;
use crate::prelude::mutations_helpers::insert_valid_integrated_op;
use crate::prelude::*;
use holochain_trace;
use holochain_types::db::DbWrite;
use holochain_types::record::SignedActionHashedExt;

#[derive(Clone)]
struct TestData {
    link_add: CreateLink,
    link_remove: DeleteLink,
    base_hash: EntryHash,
    zome_index: ZomeIndex,
    link_type: LinkType,
    tag: LinkTag,
    expected_link: Link,
    env: DbWrite<DbKindDht>,
    scratch: Scratch,
    query: GetLinksQuery,
    query_no_tag: GetLinksQuery,
}

fn fixtures(env: DbWrite<DbKindDht>, n: usize) -> Vec<TestData> {
    let mut tag_fix = BytesFixturator::new(Predictable);
    let mut data = Vec::new();
    let mut agent_pub_key_fixt = AgentPubKeyFixturator::new(Predictable);
    let mut base_hash_fixt = EntryHashFixturator::new(Predictable);
    let mut target_hash_fixt = EntryHashFixturator::new(Unpredictable);
    for i in 0..n {
        // Create a known link add
        let base_address = base_hash_fixt.next().unwrap();
        let target_address = target_hash_fixt.next().unwrap();
        let agent_pub_key = agent_pub_key_fixt.next().unwrap();

        let tag = LinkTag::new(tag_fix.next().unwrap());
        let zome_index = ZomeIndex(i as u8);
        let link_type = LinkType(i as u8);

        let link_add = KnownCreateLink {
            author: agent_pub_key.clone(),
            base_address: base_address.clone().into(),
            target_address: target_address.clone().into(),
            zome_index,
            link_type,
            tag: tag.clone(),
        };

        let link_add = CreateLinkFixturator::new(link_add).next().unwrap();

        // Create the expected link result
        let (_, link_add_hash): (_, ActionHash) =
            ActionHashed::from_content_sync(Action::CreateLink(link_add.clone())).into();

        let expected_link = Link {
            author: agent_pub_key,
            create_link_hash: link_add_hash.clone(),
            target: target_address.clone().into(),
            zome_index,
            link_type,
            timestamp: link_add.timestamp.clone().into(),
            tag: tag.clone(),
        };

        let link_remove = KnownDeleteLink {
            link_add_address: link_add_hash,
            base_address: link_add.base_address.clone(),
        };
        let link_remove = DeleteLinkFixturator::new(link_remove).next().unwrap();
        let query = GetLinksQuery::new(
            link_add.base_address.clone(),
            LinkTypeFilter::single_dep(zome_index),
            Some(link_add.tag.clone()),
        );
        let query_no_tag = GetLinksQuery::base(link_add.base_address.clone(), vec![zome_index]);

        let td = TestData {
            link_add,
            link_remove,
            base_hash: base_address.clone(),
            zome_index,
            link_type,
            tag,
            expected_link,
            env: env.clone(),
            scratch: Scratch::new(),
            query,
            query_no_tag,
        };
        data.push(td);
    }
    data
}

impl TestData {
    /// Create the same test data with a new timestamp
    fn with_same_keys(mut td: Self) -> Self {
        td.link_add.timestamp = holochain_zome_types::Timestamp::now().into();
        let link_add_hash =
            ActionHashed::from_content_sync(Action::CreateLink(td.link_add.clone())).into_hash();
        td.link_remove.link_add_address = link_add_hash.clone();
        td.expected_link.timestamp = td.link_add.timestamp.clone().into();
        td.expected_link.create_link_hash = link_add_hash;
        td
    }

    fn empty<'a>(&'a self, test: &'static str) {
        let val = fresh_reader_test(self.env.clone(), |txn| {
            self.query
                .run(DbScratch::new(&[&txn], &self.scratch))
                .unwrap()
                .is_empty()
        });
        assert!(val, "{}", test);
    }

    fn only_on_full_key<'a>(&'a self, test: &'static str) {
        let val = fresh_reader_test(self.env.clone(), |txn| {
            self.query
                .run(DbScratch::new(&[&txn], &self.scratch))
                .unwrap()
        });
        assert_eq!(val, &[self.expected_link.clone()], "{}", test);
    }

    fn not_on_full_key<'a>(&'a self, test: &'static str) {
        let val = fresh_reader_test(self.env.clone(), |txn| {
            self.query
                .run(DbScratch::new(&[&txn], &self.scratch))
                .unwrap()
                .contains(&self.expected_link)
        });
        assert!(
            !val,
            "LinkMetaVal: {:?} should not be present {}",
            self.expected_link, test
        );
    }

    fn only_on_base<'a>(&'a self, test: &'static str) {
        let val = fresh_reader_test(self.env.clone(), |txn| {
            self.query_no_tag
                .run(DbScratch::new(&[&txn], &self.scratch))
                .unwrap()
        });
        assert_eq!(val, &[self.expected_link.clone()], "{}", test);
    }

    fn is_on_type<'a>(&'a self, test: &'static str) {
        let query = GetLinksQuery::new(
            self.base_hash.clone().into(),
            LinkTypeFilter::single_type(self.zome_index, self.link_type),
            None,
        );
        let val = fresh_reader_test(self.env.clone(), |txn| {
            query
                .run(DbScratch::new(&[&txn], &self.scratch))
                .unwrap()
                .contains(&self.expected_link)
        });
        assert!(
            val,
            "Results should contain link: {:?} in test: {}",
            self.expected_link, test
        );
    }

    fn is_on_type_query<'a>(&'a self, type_query: LinkTypeFilter, test: &'static str) {
        let query = GetLinksQuery::new(self.base_hash.clone().into(), type_query, None);
        let val = fresh_reader_test(self.env.clone(), |txn| {
            query
                .run(DbScratch::new(&[&txn], &self.scratch))
                .unwrap()
                .contains(&self.expected_link)
        });
        assert!(
            val,
            "Results should contain link: {:?} in test: {}",
            self.expected_link, test
        );
    }

    fn only_on_half_tag<'a>(&'a self, test: &'static str) {
        let tag_len = self.tag.0.len();
        // Make sure there is at least some tag
        let half_tag = if tag_len > 1 { tag_len / 2 } else { tag_len };
        let half_tag = LinkTag::new(&self.tag.0[..half_tag]);
        let query = GetLinksQuery::new(
            self.base_hash.clone().into(),
            LinkTypeFilter::single_type(self.zome_index, self.link_type),
            Some(half_tag),
        );
        let val = fresh_reader_test(self.env.clone(), |txn| {
            query.run(DbScratch::new(&[&txn], &self.scratch)).unwrap()
        });
        assert_eq!(val, &[self.expected_link.clone()], "{}", test);
    }

    fn is_on_half_tag<'a>(&'a self, test: &'static str) {
        let tag_len = self.tag.0.len();
        // Make sure there is at least some tag
        let half_tag = if tag_len > 1 { tag_len / 2 } else { tag_len };
        let half_tag = LinkTag::new(&self.tag.0[..half_tag]);
        let query = GetLinksQuery::new(
            self.base_hash.clone().into(),
            LinkTypeFilter::single_type(self.zome_index, self.link_type),
            Some(half_tag),
        );
        let val = fresh_reader_test(self.env.clone(), |txn| {
            query
                .run(DbScratch::new(&[&txn], &self.scratch))
                .unwrap()
                .contains(&self.expected_link)
        });
        assert!(
            val,
            "Results should contain LinkMetaVal: {:?} in test: {}",
            self.expected_link, test
        );
    }

    fn add_link(&self) {
        let op = DhtOpHashed::from_content_sync(DhtOp::RegisterAddLink(
            fixt!(Signature),
            self.link_add.clone(),
        ));
        self.env
            .conn()
            .unwrap()
            .with_commit_test(|txn| insert_valid_integrated_op(txn, &op).unwrap())
            .unwrap();
    }
    fn add_link_scratch(&mut self) {
        let action = SignedActionHashed::from_content_sync(SignedAction(
            Action::CreateLink(self.link_add.clone()),
            fixt!(Signature),
        ));
        self.scratch.add_action(action, ChainTopOrdering::default());
    }
    fn add_link_given_scratch(&mut self, scratch: &mut Scratch) {
        let action = SignedActionHashed::from_content_sync(SignedAction(
            Action::CreateLink(self.link_add.clone()),
            fixt!(Signature),
        ));
        scratch.add_action(action, ChainTopOrdering::default());
    }
    fn delete_link(&self) {
        let op = DhtOpHashed::from_content_sync(DhtOp::RegisterRemoveLink(
            fixt!(Signature),
            self.link_remove.clone(),
        ));
        self.env
            .conn()
            .unwrap()
            .with_commit_test(|txn| insert_valid_integrated_op(txn, &op).unwrap())
            .unwrap();
    }
    fn delete_link_scratch(&mut self) {
        let action = SignedActionHashed::from_content_sync(SignedAction(
            Action::DeleteLink(self.link_remove.clone()),
            fixt!(Signature),
        ));
        self.scratch.add_action(action, ChainTopOrdering::default());
    }
    fn clear_scratch(&mut self) {
        self.scratch.drain_actions().for_each(|_| ());
    }

    fn only_these_on_base<'a>(td: &'a [Self], test: &'static str) {
        // Check all base hash are the same
        for d in td {
            assert_eq!(d.base_hash, td[0].base_hash, "{}", test);
        }
        let base_hash = td[0].base_hash.clone();
        let expected = td
            .iter()
            .map(|d| d.expected_link.clone())
            .collect::<Vec<_>>();
        let mut val = Vec::new();
        for d in td {
            let query = GetLinksQuery::new(
                base_hash.clone().into(),
                LinkTypeFilter::single_type(d.zome_index, d.link_type),
                None,
            );
            fresh_reader_test(d.env.clone(), |txn| {
                val.extend(
                    query
                        .run(DbScratch::new(&[&txn], &d.scratch))
                        .unwrap()
                        .into_iter(),
                );
            });
        }
        assert_eq!(val, expected, "{}", test);
    }

    fn only_these_on_query<'a>(
        td: &'a [Self],
        scratch: &Scratch,
        query: impl Into<LinkTypeFilter>,
        test: &'static str,
    ) {
        // Check all base hash are the same
        for d in td {
            assert_eq!(d.base_hash, td[0].base_hash, "{}", test);
        }
        let base_hash = td[0].base_hash.clone();
        let expected = td
            .iter()
            .map(|d| d.expected_link.clone())
            .collect::<HashSet<_>>();
        let query = GetLinksQuery::new(base_hash.clone().into(), query.into(), None);
        let val: HashSet<_> = fresh_reader_test(td[0].env.clone(), |txn| {
            query.run(DbScratch::new(&[&txn], &scratch)).unwrap()
        })
        .into_iter()
        .collect();
        assert_eq!(val, expected, "{}", test);
    }

    fn only_these_on_full_key<'a>(td: &'a [Self], test: &'static str) {
        // Check all base hash, link type, tag are the same
        for d in td {
            assert_eq!(d.base_hash, td[0].base_hash, "{}", test);
            assert_eq!(d.link_type, td[0].link_type, "{}", test);
            assert_eq!(d.tag, td[0].tag, "{}", test);
        }
        let base_hash = td[0].base_hash.clone();
        let zome_index = td[0].zome_index;
        let tag = td[0].tag.clone();
        let expected = td
            .iter()
            .map(|d| d.expected_link.clone())
            .collect::<Vec<_>>();
        let query = GetLinksQuery::new(
            base_hash.into(),
            LinkTypeFilter::single_dep(zome_index),
            Some(tag),
        );
        let mut val = Vec::new();
        for d in td {
            fresh_reader_test(d.env.clone(), |txn| {
                val.extend(
                    query
                        .run(DbScratch::new(&[&txn], &d.scratch))
                        .unwrap()
                        .into_iter(),
                );
            });
        }
        assert_eq!(val, expected, "{}", test);
    }

    fn only_these_on_half_key<'a>(td: &'a [Self], test: &'static str) {
        let tag_len = td[0].tag.0.len();
        // Make sure there is at least some tag
        let tag_len = if tag_len > 1 { tag_len / 2 } else { tag_len };
        let half_tag = LinkTag::new(&td[0].tag.0[..tag_len]);
        // Check all base hash, zome_index, half tag are the same
        for d in td {
            assert_eq!(d.base_hash, td[0].base_hash, "{}", test);
            assert_eq!(d.link_type, td[0].link_type, "{}", test);
            assert_eq!(&d.tag.0[..tag_len], &half_tag.0[..], "{}", test);
        }
        let base_hash = td[0].base_hash.clone();
        let zome_index = td[0].zome_index;
        let expected = td
            .iter()
            .map(|d| d.expected_link.clone())
            .collect::<Vec<_>>();
        let query = GetLinksQuery::new(
            base_hash.into(),
            LinkTypeFilter::single_dep(zome_index),
            Some(half_tag),
        );
        let mut val = Vec::new();
        for d in td {
            fresh_reader_test(d.env.clone(), |txn| {
                val.extend(
                    query
                        .run(DbScratch::new(&[&txn], &d.scratch))
                        .unwrap()
                        .into_iter(),
                );
            });
        }
        assert_eq!(val, expected, "{}", test);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn can_add_and_delete_link() {
    let test_db = test_dht_db();
    let arc = test_db.to_db();

    let mut td = fixtures(arc.clone(), 1).into_iter().next().unwrap();

    // Check it's empty
    td.empty(here!("empty at start"));

    // Add a link
    // Add
    td.add_link_scratch();
    // Is in scratch
    td.only_on_full_key(here!("add link in scratch"));

    // Remove from scratch
    td.delete_link_scratch();

    // Is empty
    td.empty(here!("empty after remove"));

    let new_td = TestData::with_same_keys(td.clone());
    td = new_td;

    // Add again
    td.add_link_scratch();

    // Is in scratch again
    td.only_on_full_key(here!("Is still in the scratch"));

    // Remove from scratch
    td.delete_link_scratch();

    // Is empty
    td.empty(here!("empty after remove"));

    // Check it's in db
    td.clear_scratch();
    td.add_link();

    td.only_on_full_key(here!("It's in the db"));

    // Remove the link
    td.delete_link();
    // Is empty

    td.empty(here!("empty after remove in db"));

    // Add a link
    let new_td = TestData::with_same_keys(td.clone());
    td = new_td;
    // Add
    td.add_link_scratch();
    // Is in scratch
    td.only_on_full_key(here!("add link in scratch"));
    // No zome, no tag
    td.only_on_base(here!("scratch"));
    // Half the tag
    td.only_on_half_tag(here!("scratch"));

    td.delete_link_scratch();
    td.empty(here!("empty after remove in db"));

    // Partial matching
    td.clear_scratch();
    td.add_link();

    td.only_on_full_key(here!("db"));
    // No zome, no tag
    td.only_on_base(here!("db"));
    // Half the tag
    td.only_on_half_tag(here!("db"));
}

#[tokio::test(flavor = "multi_thread")]
async fn multiple_links() {
    let test_db = test_dht_db();
    let arc = test_db.to_db();

    let mut td = fixtures(arc.clone().into(), 10);

    // Add links
    {
        // Add
        for d in &mut td {
            d.add_link_scratch();
        }
        // Is in scratch
        for d in &mut td {
            d.only_on_full_key(here!("add link in scratch"));
        }

        // Remove from scratch
        td[5].delete_link_scratch();

        td[5].not_on_full_key(here!("removed in scratch"));

        for d in td[0..5].iter().chain(&td[6..]) {
            d.only_on_full_key(here!("all except 5 scratch"));
        }
        // Can't add back the same action because removes are tombstones
        // so add one with the same key
        let new_td = TestData::with_same_keys(td[5].clone());
        td[5] = new_td;
        // Add again
        td[5].add_link_scratch();

        // Is in scratch again
        td[5].only_on_full_key(here!("Is back in the scratch"));

        for d in &mut td {
            d.only_on_full_key(here!("add link in scratch"));
        }
        for d in &mut td {
            d.clear_scratch();
        }
    }

    {
        for d in &mut td {
            d.add_link();
        }
        for d in &mut td {
            d.only_on_full_key(here!("all in db"));
        }
        td[0].delete_link();

        for d in &td[1..] {
            d.only_on_full_key(here!("all except 0 scratch"));
        }

        td[0].not_on_full_key(here!("removed in scratch"));
    }

    for d in &td[1..] {
        d.only_on_full_key(here!("all except 0"));
    }
    td[0].not_on_full_key(here!("removed in db"));
}
#[tokio::test(flavor = "multi_thread")]
async fn duplicate_links() {
    holochain_trace::test_run().ok();
    let test_db = test_dht_db();
    let arc = test_db.to_db();

    let mut td = fixtures(arc.clone(), 10);
    // Add to db then the same to scratch and expect on one result
    {
        // Add
        for d in &mut td {
            d.add_link_scratch();
        }
        // Is in scratch
        for d in &mut td {
            d.only_on_full_key(here!("re add"));
            // No zome, no tag
            d.only_on_base(here!("re add"));
            // Half the tag
            d.is_on_half_tag(here!("re add"));
        }
        // Add Again
        for d in &mut td {
            d.add_link_scratch();
        }
        // Is in scratch
        for d in &mut td {
            d.only_on_full_key(here!("re add"));
            // No zome, no tag
            d.only_on_base(here!("re add"));
            // Half the tag
            d.is_on_half_tag(here!("re add"));
        }
    }
    {
        // Add
        for d in &mut td {
            d.add_link();
        }
        // Is in scratch
        for d in &mut td {
            d.only_on_full_key(here!("re add"));
            // No zome, no tag
            d.only_on_base(here!("re add"));
            // Half the tag
            d.is_on_half_tag(here!("re add"));
        }
    }

    for d in &mut td {
        d.clear_scratch();
    }
    // Is in db
    for d in &mut td {
        d.only_on_full_key(here!("re add"));
        // No zome, no tag
        d.only_on_base(here!("re add"));
        // Half the tag
        d.is_on_half_tag(here!("re add"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn links_on_same_base() {
    holochain_trace::test_run().ok();
    let test_db = test_dht_db();
    let arc = test_db.to_db();

    let mut td = fixtures(arc.clone(), 2);
    let base_hash = td[0].base_hash.clone();
    let base_hash = &base_hash;
    for d in td.iter_mut() {
        d.base_hash = base_hash.clone();
        d.link_add.base_address = base_hash.clone().into();
        // Create the new hash
        let (_, link_add_hash): (_, ActionHash) =
            ActionHashed::from_content_sync(Action::CreateLink(d.link_add.clone())).into();
        d.expected_link.create_link_hash = link_add_hash.clone();
        d.link_remove.link_add_address = link_add_hash;
        d.link_remove.base_address = base_hash.clone().into();
        d.query = GetLinksQuery::new(
            base_hash.clone().into(),
            LinkTypeFilter::single_dep(d.zome_index),
            Some(d.tag.clone()),
        );
        d.query_no_tag = GetLinksQuery::base(base_hash.clone().into(), vec![d.zome_index]);
    }
    {
        // Add
        for d in &mut td {
            d.add_link_scratch();
        }
        // Is in scratch
        for d in &mut td {
            d.only_on_full_key(here!("same base"));
            // Half the tag
            d.is_on_half_tag(here!("same base"));
        }
        TestData::only_these_on_base(&td, here!("check all return on same base"));
    }
    {
        for d in &mut td {
            d.add_link();
        }
        // In db
        for d in &mut td {
            d.only_on_full_key(here!("same base"));
            // Half the tag
            d.is_on_half_tag(here!("same base"));
        }
        TestData::only_these_on_base(&td, here!("check all return on same base"));
    }
    {
        for d in &mut td {
            d.clear_scratch();
        }
        // In db
        for d in &mut td {
            d.only_on_full_key(here!("same base"));
            // Half the tag
            d.is_on_half_tag(here!("same base"));
        }
        TestData::only_these_on_base(&td, here!("check all return on same base"));
    }
    // Check removes etc.
    {
        for d in &mut td {
            d.add_link_scratch();
        }
        td[0].delete_link_scratch();
        for d in &td[1..] {
            d.only_on_full_key(here!("same base"));
            // Half the tag
            d.is_on_half_tag(here!("same base"));
        }
        TestData::only_these_on_base(&td[1..], here!("check all return on same base"));
        td[0].not_on_full_key(here!("removed in scratch"));
    }
    {
        for d in &mut td {
            d.clear_scratch();
        }
        td[0].delete_link();
        for d in &td[1..] {
            d.only_on_full_key(here!("same base"));
            d.is_on_half_tag(here!("same base"));
        }
        TestData::only_these_on_base(&td[1..], here!("check all return on same base"));
        td[0].not_on_full_key(here!("removed in scratch"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn links_on_same_tag() {
    holochain_trace::test_run().ok();
    let test_db = test_dht_db();
    let arc = test_db.to_db();

    let mut td = fixtures(arc.clone(), 10);
    let base_hash = td[0].base_hash.clone();
    let link_type = td[0].link_type;
    let zome_index = td[0].zome_index;
    let tag = td[0].tag.clone();

    for d in td.iter_mut() {
        d.base_hash = base_hash.clone();
        d.zome_index = zome_index;
        d.link_type = link_type;
        d.tag = tag.clone();
        d.link_add.base_address = base_hash.clone().into();
        d.link_add.zome_index = zome_index;
        d.link_add.link_type = link_type;
        d.link_add.tag = tag.clone();
        d.link_remove.base_address = base_hash.clone().into();

        // Create the new hash
        let (_, link_add_hash): (_, ActionHash) =
            ActionHashed::from_content_sync(Action::CreateLink(d.link_add.clone())).into();
        d.expected_link.create_link_hash = link_add_hash.clone();
        d.expected_link.tag = tag.clone();
        d.expected_link.zome_index = zome_index;
        d.expected_link.link_type = link_type;
        d.link_remove.link_add_address = link_add_hash;

        d.query = GetLinksQuery::new(
            base_hash.clone().into(),
            LinkTypeFilter::single_dep(d.zome_index),
            Some(tag.clone()),
        );
        d.query_no_tag = GetLinksQuery::base(base_hash.clone().into(), vec![d.zome_index]);
    }
    {
        // Add
        for d in &mut td {
            d.add_link_scratch();
        }
        TestData::only_these_on_base(&td[..], here!("check all return on same base"));
        TestData::only_these_on_full_key(&td[..], here!("check all return on same base"));
        TestData::only_these_on_half_key(&td[..], here!("check all return on same base"));
    }
    {
        // In db
        TestData::only_these_on_base(&td[..], here!("check all return on same base"));
        TestData::only_these_on_full_key(&td[..], here!("check all return on same base"));
        TestData::only_these_on_half_key(&td[..], here!("check all return on same base"));
    }
    // Check removes etc.
    {
        td[5].delete_link();
        td[6].delete_link();
        let partial_td = &td[..5].iter().chain(&td[7..]).cloned().collect::<Vec<_>>();
        TestData::only_these_on_base(&partial_td[..], here!("check all return on same base"));
        TestData::only_these_on_full_key(&partial_td[..], here!("check all return on same base"));
        TestData::only_these_on_half_key(&partial_td[..], here!("check all return on same base"));
    }
    {
        let partial_td = &td[..5].iter().chain(&td[7..]).cloned().collect::<Vec<_>>();
        TestData::only_these_on_base(&partial_td[..], here!("check all return on same base"));
        TestData::only_these_on_full_key(&partial_td[..], here!("check all return on same base"));
        TestData::only_these_on_half_key(&partial_td[..], here!("check all return on same base"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn links_on_same_type() {
    holochain_trace::test_run().ok();
    let test_db = test_dht_db();
    let arc = test_db.to_db();

    let mut td = fixtures(arc.clone(), 10);
    let base_hash = td[0].base_hash.clone();
    let link_type = td[0].link_type;

    for d in td.iter_mut() {
        d.base_hash = base_hash.clone();
        d.link_type = link_type;
        d.link_add.base_address = base_hash.clone().into();
        d.link_add.link_type = link_type;

        // Create the new hash
        let (_, link_add_hash): (_, ActionHash) =
            ActionHashed::from_content_sync(Action::CreateLink(d.link_add.clone())).into();
        d.expected_link.create_link_hash = link_add_hash.clone();
        d.expected_link.link_type = link_type;
    }

    for d in &mut td {
        d.add_link_scratch();
    }
    for d in &td {
        d.is_on_type(here!("Each link is returned for a type"));
        d.is_on_type_query(
            LinkTypeFilter::Dependencies(td.iter().map(|d| d.zome_index).collect()),
            here!("Each link is returned for a type"),
        );
        d.is_on_type_query(
            LinkTypeFilter::single_type(d.zome_index, d.link_type),
            here!("Each link is returned for a type"),
        );
    }
    for d in &mut td {
        d.add_link();
    }
    for d in &td {
        d.is_on_type(here!("Each link is returned for a type"));
        d.is_on_type_query(
            LinkTypeFilter::Dependencies(td.iter().map(|d| d.zome_index).collect()),
            here!("Each link is returned for a type"),
        );
        d.is_on_type_query(
            LinkTypeFilter::single_type(d.zome_index, d.link_type),
            here!("Each link is returned for a type"),
        );
        d.is_on_type_query(
            LinkTypeFilter::Types(vec![(d.zome_index, vec![d.link_type])]),
            here!("Each link is returned for a type"),
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn link_type_ranges() {
    holochain_trace::test_run().ok();
    let test_db = test_dht_db();
    let arc = test_db.to_db();

    let mut td = fixtures(arc.clone(), 10);
    let base_hash = td[0].base_hash.clone();
    let mut scratch = Scratch::new();

    for (i, d) in td.iter_mut().enumerate() {
        d.base_hash = base_hash.clone();
        d.link_type = LinkType(i as u8);
        d.link_add.base_address = base_hash.clone().into();
        d.link_add.link_type = LinkType(i as u8);

        // Create the new hash
        let link_add_hash = ActionHash::with_data_sync(&Action::CreateLink(d.link_add.clone()));
        d.expected_link.create_link_hash = link_add_hash.clone();
    }

    // Add
    for d in &mut td {
        d.add_link_given_scratch(&mut scratch);
    }
    TestData::only_these_on_query(
        &td,
        &scratch,
        LinkTypeFilter::Dependencies(td.iter().map(|d| d.zome_index).collect()),
        here!("all return on full range"),
    );
    TestData::only_these_on_query(
        &td[0..=0],
        &scratch,
        LinkTypeFilter::single_type(0.into(), 0.into()),
        here!("only single on single range"),
    );
    TestData::only_these_on_query(
        &td[4..=9],
        &scratch,
        LinkTypeFilter::Types(vec![
            (4.into(), vec![4.into()]),
            (5.into(), vec![5.into()]),
            (6.into(), vec![6.into()]),
            (7.into(), vec![7.into()]),
            (8.into(), vec![8.into()]),
            (9.into(), vec![9.into()]),
        ]),
        here!("range matches"),
    );
    let partial_td = &td[2..5]
        .iter()
        .chain(&td[7..9])
        .cloned()
        .collect::<Vec<_>>();
    TestData::only_these_on_query(
        &partial_td[..],
        &scratch,
        LinkTypeFilter::Types(vec![
            (2.into(), vec![2.into()]),
            (3.into(), vec![3.into()]),
            (8.into(), vec![8.into()]),
            (7.into(), vec![7.into()]),
            (4.into(), vec![4.into()]),
        ]),
        here!("individual types"),
    );
    let partial_td = &td[2..5]
        .iter()
        .chain(&td[7..9])
        .cloned()
        .collect::<Vec<_>>();
    TestData::only_these_on_query(
        &partial_td[..],
        &scratch,
        LinkTypeFilter::Types(vec![
            (7.into(), vec![7.into()]),
            (8.into(), vec![8.into()]),
            (2.into(), vec![2.into()]),
            (3.into(), vec![3.into()]),
            (4.into(), vec![4.into()]),
        ]),
        here!("individual types"),
    );
    for d in &mut td {
        d.add_link();
    }
    TestData::only_these_on_query(
        &td,
        &Scratch::new(),
        LinkTypeFilter::Dependencies(td.iter().map(|d| d.zome_index).collect()),
        here!("all return on full range"),
    );
    TestData::only_these_on_query(
        &td[0..=0],
        &Scratch::new(),
        LinkTypeFilter::single_type(0.into(), 0.into()),
        here!("all return on full range"),
    );
    let partial_td = &td[2..5]
        .iter()
        .chain(&td[7..9])
        .cloned()
        .collect::<Vec<_>>();
    TestData::only_these_on_query(
        &partial_td[..],
        &scratch,
        LinkTypeFilter::Types(vec![
            (7.into(), vec![7.into()]),
            (8.into(), vec![8.into()]),
            (2.into(), vec![2.into()]),
            (3.into(), vec![3.into()]),
            (4.into(), vec![4.into()]),
        ]),
        here!("individual types"),
    );
}
