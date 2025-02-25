//! Links interrelate entries in a source chain.

use holo_hash::ActionHash;
use holo_hash::AgentPubKey;
use holo_hash::AnyLinkableHash;
use holo_hash::EntryHash;
use holochain_serialized_bytes::prelude::*;
use holochain_zome_types::prelude::*;
use regex::Regex;

use crate::dht_op::error::DhtOpError;
use crate::dht_op::error::DhtOpResult;
use crate::dht_op::DhtOpType;
use crate::dht_op::RenderedOp;
use crate::dht_op::RenderedOps;

/// Links interrelate entries in a source chain.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Hash, SerializedBytes)]
pub struct Link {
    base: EntryHash,
    target: EntryHash,
    tag: LinkTag,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, SerializedBytes)]
/// Link key for sending across the wire for get links requests.
pub struct WireLinkKey {
    /// Base the links are on.
    pub base: AnyLinkableHash,
    /// The zome the links are in.
    pub type_query: LinkTypeFilter,
    /// Optionally specify a tag for more specific queries.
    pub tag: Option<LinkTag>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, SerializedBytes, Default)]
/// Condensed link ops for sending across the wire in response to get links.
pub struct WireLinkOps {
    /// create links that match this query.
    pub creates: Vec<WireCreateLink>,
    /// delete links that match this query.
    pub deletes: Vec<WireDeleteLink>,
}

impl WireLinkOps {
    /// Create an empty wire response.
    pub fn new() -> Self {
        Default::default()
    }
    /// Render these ops to their full types.
    pub fn render(self, key: &WireLinkKey) -> DhtOpResult<RenderedOps> {
        let Self { creates, deletes } = self;
        let mut ops = Vec::with_capacity(creates.len() + deletes.len());
        // We silently ignore ops that fail to render as they come from the network.
        ops.extend(creates.into_iter().filter_map(|op| op.render(key).ok()));
        ops.extend(deletes.into_iter().filter_map(|op| op.render(key).ok()));
        Ok(RenderedOps {
            ops,
            ..Default::default()
        })
    }
}

#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, SerializedBytes)]
/// Condensed version of a [`CreateLink`]
pub struct WireCreateLink {
    pub author: AgentPubKey,
    pub timestamp: Timestamp,
    pub action_seq: u32,
    pub prev_action: ActionHash,

    pub target_address: AnyLinkableHash,
    pub zome_index: ZomeIndex,
    pub link_type: LinkType,
    pub tag: Option<LinkTag>,
    pub signature: Signature,
    pub validation_status: ValidationStatus,
    pub weight: RateWeight,
}

#[allow(missing_docs)]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, SerializedBytes)]
/// Condensed version of a [`DeleteLink`]
pub struct WireDeleteLink {
    pub author: AgentPubKey,
    pub timestamp: Timestamp,
    pub action_seq: u32,
    pub prev_action: ActionHash,

    pub link_add_address: ActionHash,
    pub signature: Signature,
    pub validation_status: ValidationStatus,
}

impl WireCreateLink {
    fn new(
        h: CreateLink,
        signature: Signature,
        validation_status: ValidationStatus,
        tag: bool,
    ) -> Self {
        Self {
            author: h.author,
            timestamp: h.timestamp,
            action_seq: h.action_seq,
            prev_action: h.prev_action,
            target_address: h.target_address,
            zome_index: h.zome_index,
            link_type: h.link_type,
            tag: if tag { Some(h.tag) } else { None },
            signature,
            validation_status,
            weight: h.weight,
        }
    }
    /// Condense down a create link op for the wire without a tag.
    pub fn condense_base_only(
        h: CreateLink,
        signature: Signature,
        validation_status: ValidationStatus,
    ) -> Self {
        Self::new(h, signature, validation_status, false)
    }
    /// Condense down a create link op for the wire with a tag.
    pub fn condense(
        h: CreateLink,
        signature: Signature,
        validation_status: ValidationStatus,
    ) -> Self {
        Self::new(h, signature, validation_status, true)
    }
    /// Render these ops to their full types.
    pub fn render(self, key: &WireLinkKey) -> DhtOpResult<RenderedOp> {
        let tag = self
            .tag
            .or_else(|| key.tag.clone())
            .ok_or(DhtOpError::LinkKeyTagMissing)?;
        let action = Action::CreateLink(CreateLink {
            author: self.author,
            timestamp: self.timestamp,
            action_seq: self.action_seq,
            prev_action: self.prev_action,
            base_address: key.base.clone(),
            target_address: self.target_address,
            zome_index: self.zome_index,
            link_type: self.link_type,
            weight: self.weight,
            tag,
        });
        let signature = self.signature;
        let validation_status = Some(self.validation_status);
        RenderedOp::new(
            action,
            signature,
            validation_status,
            DhtOpType::RegisterAddLink,
        )
    }
}

impl WireDeleteLink {
    /// Condense down a delete link op for the wire.
    pub fn condense(
        h: DeleteLink,
        signature: Signature,
        validation_status: ValidationStatus,
    ) -> Self {
        Self {
            author: h.author,
            timestamp: h.timestamp,
            action_seq: h.action_seq,
            prev_action: h.prev_action,
            signature,
            validation_status,
            link_add_address: h.link_add_address,
        }
    }
    /// Render these ops to their full types.
    pub fn render(self, key: &WireLinkKey) -> DhtOpResult<RenderedOp> {
        let action = Action::DeleteLink(DeleteLink {
            author: self.author,
            timestamp: self.timestamp,
            action_seq: self.action_seq,
            prev_action: self.prev_action,
            base_address: key.base.clone(),
            link_add_address: self.link_add_address,
        });
        let signature = self.signature;
        let validation_status = Some(self.validation_status);
        RenderedOp::new(
            action,
            signature,
            validation_status,
            DhtOpType::RegisterRemoveLink,
        )
    }
}
// TODO: Probably don't want to send the whole actions.
// We could probably come up with a more compact
// network Wire type in the future
/// Link response to get links
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, SerializedBytes)]
pub struct GetLinksResponse {
    /// All the link adds on the key you searched for
    pub link_adds: Vec<(CreateLink, Signature)>,
    /// All the link removes on the key you searched for
    pub link_removes: Vec<(DeleteLink, Signature)>,
}

impl Link {
    /// Construct a new link.
    pub fn new(base: &EntryHash, target: &EntryHash, tag: &LinkTag) -> Self {
        Link {
            base: base.to_owned(),
            target: target.to_owned(),
            tag: tag.to_owned(),
        }
    }

    /// Get the base address of this link.
    pub fn base(&self) -> &EntryHash {
        &self.base
    }

    /// Get the target address of this link.
    pub fn target(&self) -> &EntryHash {
        &self.target
    }

    /// Get the tag of this link.
    pub fn tag(&self) -> &LinkTag {
        &self.tag
    }
}

/// How do we match this link in queries?
pub enum LinkMatch<S: Into<String>> {
    /// Match all/any links.
    Any,

    /// Match exactly by string.
    Exactly(S),

    /// Match by regular expression.
    Regex(S),
}

impl<S: Into<String>> LinkMatch<S> {
    /// Build a regular expression string for this link match.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_regex_string(self) -> Result<String, String> {
        let re_string: String = match self {
            LinkMatch::Any => ".*".into(),
            LinkMatch::Exactly(s) => "^".to_owned() + &regex::escape(&s.into()) + "$",
            LinkMatch::Regex(s) => s.into(),
        };
        // check that it is a valid regex
        match Regex::new(&re_string) {
            Ok(_) => Ok(re_string),
            Err(_) => Err("Invalid regex passed to get_links".into()),
        }
    }
}
