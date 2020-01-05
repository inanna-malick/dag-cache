#![recursion_limit = "512"]

use dag_store_types::types::api as api_types;
use dag_store_types::types::domain::TypedHash;
use dag_store_types::types::validated_tree::ValidatedTree_;
use notes_types::notes::{CannonicalNode, Node, NodeId, NodeRef, RemoteNodeRef};
use rand;
use std::collections::HashMap;
use stdweb::js;
use yew::events::IKeyboardEvent;
use yew::format::{Json, Nothing};
use yew::services::fetch::{FetchService, FetchTask, Request, Response};
use yew::services::interval::{IntervalService, IntervalTask};
use yew::{html, Component, ComponentLink, Html, Properties, ShouldRender};

macro_rules! println {
    ($($tt:tt)*) => {{
        let msg = format!($($tt)*);
        js! { @(no_return) console.log(@{ msg }) }
    }}
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum EditFocus {
    NodeHeader,
    NodeBody,
}

// NOTE: optional ipfs hash indicates if node is modified or not
#[derive(PartialEq, Eq, Clone, Debug)]
pub struct InMemNode {
    hash: Option<TypedHash<CannonicalNode>>,
    inner: Node<NodeRef>,
}

impl core::ops::Deref for InMemNode {
    type Target = Node<NodeRef>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl core::ops::DerefMut for InMemNode {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct EditState {
    component_focus: EditFocus,
    node_focus: NodeId, // node id & not ref because _must_ be local node
    edited_contents: String,
}

#[derive(Debug)]
pub struct State {
    nodes: HashMap<NodeId, InMemNode>, // note: node ref's can contain stale hashes.. is ok?
    entry_point: NodeRef, // NOTE: can contain stale hashes.. is this the wrong way to store root? mb just node id?
    // node (header or body) being edited, as property of state & note node tree
    // focus includes path to root from that node
    last_known_hash: Option<TypedHash<CannonicalNode>>, // for CAS
    edit_state: Option<EditState>,
    fetch_service: FetchService,
    link: ComponentLink<State>, // used to send callbacks
    fetch_tasks: HashMap<RemoteNodeRef, FetchTask>, // ideally would not need to hold on to these
    interval_service: IntervalService,
    interval_task: IntervalTask, // I guess I just need to hold on to this forever (???)
    save_task: Option<FetchTask>, // this one is good, can be dropped to abort save tasks
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Msg {
    EnterHeaderEdit { target: NodeId },
    EnterBodyEdit { target: NodeId },
    UpdateEdit(String),
    CommitEdit,
    CreateOn { at_idx: usize, parent: NodeId },
    Fetch(RemoteNodeRef), // HTTP fetch from store
    FetchComplete(RemoteNodeRef, notes_types::api::GetResp), // HTTP fetch from store (domain type)
    StartSave,            // init backup of everything in store - blocking operation, probably
    SaveComplete(api_types::bulk_put::Resp),
    NoOp,
    // TODO: use https://gomakethings.com/how-to-test-if-an-element-is-in-the-viewport-with-vanilla-javascript/
    // TODO: to trigger lazy loading when a lazy-loaded element enters the viewport (is on-screen)
    // LazyLoadOnScroll,
}

#[derive(serde::Deserialize, Debug, PartialEq, Properties)]
pub struct Arg {
    #[props(required)]
    pub hash: Option<TypedHash<CannonicalNode>>,
}

impl Component for State {
    type Message = Msg;
    type Properties = Arg;

    fn create(opt_hash: Self::Properties, mut link: ComponentLink<Self>) -> Self {
        let mut nodes = HashMap::new();

        let (entry_point, last_known_hash) = match opt_hash {
            Arg { hash: None } => {
                let fresh_root = InMemNode {
                    hash: None,             // not persisted
                    inner: Node::new(None), // None b/c node is root (no parent)
                };
                let id = gen_node_id();
                nodes.insert(id.clone(), fresh_root);
                (NodeRef::Modified(id), None)
            }
            Arg { hash: Some(h) } => (
                NodeRef::Unmodified(RemoteNodeRef(NodeId::root(), h.clone())),
                Some(h),
            ),
        };

        // repeatedly wake up save process - checks root node, save (recursively) if modifed
        let mut interval_service = IntervalService::new();
        let thirty_seconds = std::time::Duration::new(10, 0);
        let callback = link.send_back(move |_: ()| Msg::StartSave);

        let interval_task = interval_service.spawn(thirty_seconds, callback);

        State {
            nodes: nodes,
            entry_point,
            last_known_hash,
            edit_state: None,
            // TODO: split out display-relevant state and capabilities
            link: link,
            fetch_service: FetchService::new(),
            fetch_tasks: HashMap::new(),
            save_task: None, // no active save op
            interval_service,
            interval_task,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::StartSave => {
                if let NodeRef::Modified(modified_root_id) = &self.entry_point {
                    let modified_root_id = modified_root_id.clone();
                    // construct map of all nodes that have been modified
                    let mut extra_nodes: HashMap<NodeId, Node<NodeRef>> = HashMap::new();
                    let head_node = self
                        .nodes
                        .get(&modified_root_id)
                        .expect("root node lookup failed!")
                        .inner
                        .clone();

                    let mut stack: Vec<NodeId> = Vec::new();


                    for node_ref in head_node.children.iter() {
                        if let NodeRef::Modified(id) = node_ref {
                            stack.push(id.clone());
                        };
                    };

                    while let Some(id) = stack.pop() {
                        let node = self.nodes.get(&id).expect("node lookup failed");
                        for node_ref in node.children.iter() {
                            if let NodeRef::Modified(id) = node_ref {
                                stack.push(id.clone());
                            };
                        };
                        extra_nodes.insert(id, node.inner.clone());
                    }

                    // unable to validate tree, error is in above algorithm
                    let tree = ValidatedTree_::validate_(head_node, extra_nodes, |n| {
                        n.children.clone().into_iter().filter_map ( |x| match x {
                            NodeRef::Modified(x) => Some(x),
                            _ => None
                        })
                    })
                    .expect("failure validating tree while building put request");

                    let req = notes_types::api::PutReq {
                        tree,
                        cas_hash: self.last_known_hash.clone(),
                    };

                    self.push_nodes(req);
                } else {
                    println!("no modified nodes found, not saving");
                    // no-op if root node not modified
                }
            }
            // TODO: handle CAS violation, currently will just explode, lmao
            // kinda gnarly, basically just writes every saved node to the nodes store and in the process wipes out the modified nodes
            Msg::SaveComplete(resp) => {
                let root_id = NodeId::root();

                self.save_task = None; // re-enable updates
                let root_hash = resp.root_hash.promote::<CannonicalNode>();
                self.last_known_hash = Some(root_hash.clone()); // set last known hash for future CAS use
                let root_node_ref = RemoteNodeRef(root_id.clone(), root_hash.clone());
                // update entry point to newly-persisted root node
                self.entry_point = NodeRef::Unmodified(root_node_ref);

                // build client id -> hash map for lookups
                let mut node_id_to_hash = HashMap::new();
                for (id, hash) in resp.additional_uploaded.clone().into_iter() {
                    node_id_to_hash.insert(NodeId::from_generic(id.0).unwrap(), hash.clone());
                }

                // update root node with hash
                let mut node = self.nodes.get_mut(&root_id).unwrap();
                node.hash = Some(root_hash);

                for (id, hash) in resp.additional_uploaded.into_iter() {
                    let hash = hash.promote::<CannonicalNode>();
                    let id = NodeId::from_generic(id.0).unwrap(); // FIXME - type conversion gore
                    let mut node = self.nodes.get_mut(&id).unwrap();
                    node.hash = Some(hash.clone());
                    if let Some(parent_id) = &node.parent {
                        let parent_id = parent_id.clone();
                        drop(node);
                        let parent_node = self.nodes.get_mut(&parent_id).unwrap();
                        parent_node.map_mut(|node_ref| {
                            if node_ref.node_id() == &id {
                                *node_ref =
                                    NodeRef::Unmodified(RemoteNodeRef(id.clone(), hash.clone()));
                            }
                        })
                    }
                }
            }
            Msg::Fetch(remote_node_ref) => {
                let request = Request::get(format!("/node/{}", (remote_node_ref.1).to_string()))
                    .body(Nothing)
                    .unwrap();

                let remote_node_ref_2 = remote_node_ref.clone();

                let callback = self.link.send_back(
                    move |response: Response<
                        Json<Result<notes_types::api::GetResp, failure::Error>>,
                    >| {
                        if let (meta, Json(Ok(body))) = response.into_parts() {
                            if meta.status.is_success() {
                                Msg::FetchComplete(remote_node_ref_2.clone(), body)
                            } else {
                                panic!("lmao, todo (panic during resp handler)")
                            }
                        } else {
                            panic!("lmao, todo (panic in outer get resp callback (???))")
                        }
                    },
                );

                let task = self.fetch_service.fetch(request, callback);
                self.fetch_tasks.insert(remote_node_ref, task); // stash task handle
            }
            Msg::FetchComplete(node_ref, get_resp) => {
                self.fetch_tasks.remove(&node_ref); // drop fetch task (cancels, presumably)
                let fetched_node = InMemNode {
                    hash: Some(node_ref.1),
                    inner: get_resp.requested_node.map(NodeRef::Unmodified),
                };
                self.nodes.insert(node_ref.0, fetched_node);
                for (node_ref, node) in get_resp.extra_nodes.into_iter() {
                    let fetched_node = InMemNode {
                        hash: Some(node_ref.1),
                        inner: node.map(NodeRef::Unmodified),
                    };
                    self.nodes.insert(node_ref.0, fetched_node);
                }
            }
            Msg::EnterHeaderEdit { target } => {
                // commit pre-existing edit if exists
                if let Some(_) = &self.edit_state {
                    self.commit_edit();
                };

                if let Some(node) = self.nodes.get(&target) {
                    self.edit_state = Some(EditState {
                        component_focus: EditFocus::NodeHeader,
                        node_focus: target,
                        edited_contents: node.header.clone(),
                    });
                } else {
                    panic!("node lookup failed (BUG)");
                }
            }
            Msg::EnterBodyEdit { target } => {
                // commit pre-existing edit if exists
                if let Some(_) = &self.edit_state {
                    self.commit_edit();
                };

                if let Some(node) = self.nodes.get(&target) {
                    self.edit_state = Some(EditState {
                        component_focus: EditFocus::NodeBody,
                        node_focus: target,
                        edited_contents: node.body.clone(),
                    });
                } else {
                    panic!("node lookup failed (BUG)");
                }
            }
            Msg::UpdateEdit(new_s) => match &mut self.edit_state {
                None => {
                    panic!("error, no edit state to update");
                }
                Some(es) => es.edited_contents = new_s,
            },
            Msg::CommitEdit => {
                self.commit_edit();
            }
            Msg::CreateOn { parent, at_idx } => {
                // we're modifying this node, so walk back to root and make sure all parent nodes reflect modification
                //TODO: still needed, figure out new sig
                self.set_parent_nodes_to_modified(&parent);

                // close out any pre-existing edit ops
                if let Some(_) = &self.edit_state {
                    self.commit_edit(); // may call set_parent_nodes_to_modified & overlap w/ above walk_path_to_root nodes
                };

                if let Some(node) = self.nodes.get_mut(&parent) {
                    let new_node_id = gen_node_id();
                    node.children
                        .insert(at_idx, NodeRef::Modified(new_node_id.clone())); // insert reference to new node
                    let new_node = InMemNode {
                        hash: None,
                        inner: Node::new(Some(parent)),
                    };
                    self.nodes.insert(new_node_id.clone(), new_node); // insert new node

                    // enter edit mode with empty header
                    self.edit_state = Some(EditState {
                        component_focus: EditFocus::NodeHeader,
                        node_focus: new_node_id,
                        edited_contents: "".to_string(),
                    });
                } else {
                    panic!("node lookup failed (BUG)");
                }
            }
            Msg::NoOp => {}
        }
        true
    }

    fn view(&self) -> Html<Self> {
        html! {
            <div class="wrapper">
                <div> { render_is_modified_widget(&self.entry_point) } </div>
                <ul class = "top-level">
                    { self.render_node(&self.entry_point) }
                </ul>
            </div>
        }
    }
}

impl State {
    // weird type, essentially want to defer throw-or-not to caller & dislike using bool flags
    // NOTE: should never happen while save task is Some(_) -- all view components that would fire this disabled (ditto adding subnode)
    fn commit_edit(&mut self) {
        match self.edit_state.take() {
            // NOTE: use take to remove edit focus, if any
            None => {
                panic!("tried to commit edit but no edit focus exists");
            }
            Some(es) => {
                if let Some(node) = self.nodes.get_mut(&es.node_focus) {
                    match es.component_focus {
                        EditFocus::NodeHeader => {
                            node.header = es.edited_contents;
                        }
                        EditFocus::NodeBody => {
                            node.body = es.edited_contents;
                        }
                    };
                    self.set_parent_nodes_to_modified(&es.node_focus); // set as modified from this node to root
                } else {
                    panic!("no node found while committing edits");
                }
            }
        };
    }

    fn render_node(&self, node_ref: &NodeRef) -> Html<Self> {
        let node_ref = node_ref.clone();
        let is_saving = self.save_task.is_some();
        if let Some(node) = self.nodes.get(&node_ref.node_id()) {
            let node_child_count = node.children.len();
            let children: &Vec<NodeRef> = &node.children;
            html! {
                <li class = "node">
                    { self.render_node_header(&node_ref.node_id(), &node) }
                    { self.render_node_body(&node_ref.node_id(), &node) }
                        <ul class = "nested-list">
                            { for children.iter().map(|node_ref| {
                                    self.render_node(node_ref)
                                })
                            }
                        <button class="add-subnode" onclick=|_|
                            if is_saving {
                                Msg::NoOp
                            } else { Msg::CreateOn{ at_idx: node_child_count,
                                                    parent: node_ref.node_id().clone(),
                            }
                            }>
                            {"add subnode"}
                        </button>
                    </ul>
                </li>
            }
        } else {
            if let NodeRef::Unmodified(remote_node_ref) = node_ref {
                html! {
                    <li class = "node-lazy">
                    <button class="load-node" onclick=|_| Msg::Fetch(remote_node_ref.clone())>
                    {"load node"}
                    </button>
                    </li>
                }
            } else {
                panic!("can't lazily load modified node ref")
            }
        }
    }

    fn render_node_header<T>(&self, node_id: &NodeId, node: &Node<T>) -> Html<Self> {
        let node_id = node_id.clone();

        if let Some(focus_str) = &self
            .edit_state
            .iter()
            .filter(|e| e.node_focus == node_id && e.component_focus == EditFocus::NodeHeader)
            .map(|e| &e.edited_contents)
            .next()
        {
            let is_saving = self.save_task.is_some();
            // FIXME: lazy hack, disallow commiting edits during save task lifetime (TODO: refactor, dedup)
            let onkeypress_send = if is_saving {
                Msg::NoOp
            } else {
                Msg::CommitEdit
            };
            html! {
                <div>
                    <input class="edit node-header"
                    type="text"
                    value=&focus_str
                    id = "edit-focus"
                    oninput=|e| Msg::UpdateEdit(e.value)
                    onkeypress=|e| {
                        if e.key() == "Enter" { onkeypress_send.clone() } else { Msg::NoOp }
                    }
                />
                    <script>
                        { // focus immediately after loading
                            "document.getElementById(\"edit-focus\").focus();"
                        }
                    </script>
                </div>
            }
        } else {
            html! {
                <div class="node-header" ondoubleclick=|_| Msg::EnterHeaderEdit{target: node_id.clone()}>{ &node.header }</div>
            }
        }
    }

    fn render_node_body<T>(&self, node_id: &NodeId, node: &Node<T>) -> Html<Self> {
        let node_id = node_id.clone();

        if let Some(focus_str) = &self
            .edit_state
            .iter()
            .filter(|e| e.node_focus == node_id && e.component_focus == EditFocus::NodeBody)
            .map(|e| &e.edited_contents)
            .next()
        {
            let is_saving = self.save_task.is_some();
            // FIXME: lazy hack, disallow commiting edits during save task lifetime (TODO: refactor, dedup)
            let onkeypress_send = if is_saving {
                Msg::NoOp
            } else {
                Msg::CommitEdit
            };
            html! {
                <div>
                    <input class="edit node-body"
                    type="text"
                    value=&focus_str
                    id = "edit-focus"
                    oninput=|e| Msg::UpdateEdit(e.value)
                    onkeypress=|e| {
                        if e.key() == "Enter" { onkeypress_send.clone() } else { Msg::NoOp }
                    }
                />
                    <script>
                { // focus immediately after loading
                    "document.getElementById(\"edit-focus\").focus();"
                }
                </script>
                    </div>
            }
        } else {
            html! {
                <div class = "node-body" ondoubleclick=|_| Msg::EnterBodyEdit{ target: node_id.clone() }>{ &node.body }</div>
            }
        }
    }

    // NOTE: despite name also sets target node to modified
    // walk path up from newly modified node, setting all to modified incl links
    fn set_parent_nodes_to_modified(&mut self, starting_point: &NodeId) {
        let mut prev = None;
        let mut target = starting_point.clone();
        while let Some(node) = self.nodes.get_mut(&target) {
            if let Some(_stale_hash) = &node.hash {
                // if this is the root/entry point node, demote it to modified
                if self.entry_point.node_id() == &target {
                    self.entry_point = NodeRef::Modified(target.clone());
                };
                node.hash = None;
            };

            // update pointer to previous node to indicate modification
            if let Some(prev) = prev {
                node.map_mut(|node_ref| {
                    if node_ref.node_id() == &prev {
                        // downgrade any pointers to the prev node to modified
                        *node_ref = NodeRef::Modified(prev.clone());
                    }
                })
            }

            // TODO: retain prev node id, map_mut over refs to update to local ref. combination should yield correct refs

            match &node.parent {
                Some(id) => {
                    prev = Some(target);
                    target = id.clone();
                }
                None => {
                    break;
                }
            }
        }
    }

    fn push_nodes(&mut self, req: notes_types::api::PutReq) -> () {
        let request = Request::post("/nodes")
            // why is this is neccessary given Json body type on builder - mb I'm doing it wrong?
            .header("Content-Type", "application/json")
            .body(Json(&req))
            .unwrap();

        let callback = self.link.send_back(
            move |response: Response<Json<Result<api_types::bulk_put::Resp, failure::Error>>>| {
                let (meta, Json(res)) = response.into_parts();
                if let Ok(body) = res {
                    if meta.status.is_success() {
                        Msg::SaveComplete(body)
                    } else {
                        panic!("lmao, todo (panic during resp handler)")
                    }
                } else {
                    panic!("lmao, todo (panic in outer put resp callback {:?}", meta)
                }
            },
        );

        let task = self.fetch_service.fetch(request, callback);
        self.save_task = Some(task);
    }
}

fn gen_node_id() -> NodeId {
    let u = rand::random::<u128>();
    NodeId(format!("{}", u))
}


// TODO: unicode, css, etc (currently just a debug indicator)
fn render_is_modified_widget<X: yew::html::Component>(x: &NodeRef) -> Html<X> {
    match x {
        NodeRef::Modified(_) => html! { <span> {"[[modified!]]"} </span> },
        NodeRef::Unmodified(_) => html! { <span> {"[[unmodified!]]"} </span> },
    }
}