#![recursion_limit = "512"]

use dag_store_types::types::api as api_types;
use dag_store_types::types::domain::TypedHash;
use dag_store_types::types::validated_tree::ValidatedTree_;
use notes_types::notes::{CannonicalNode, Node, NodeId, NodeRef, RemoteNodeRef};
use rand;
use std::collections::HashMap;
use stdweb::js;
use yew::events::IKeyboardEvent;
use yew::events::KeyPressEvent;
use yew::format::{Json, Nothing};
use yew::html::InputData;
use yew::services::{
    dialog::DialogService,
    fetch::{FetchService, FetchTask, Request, Response},
    interval::{IntervalService, IntervalTask},
};
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

// NOTE: optional hash indicates if node is modified or not
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
    root_node: NodeRef, // NOTE: can contain stale hashes.. is this the wrong way to store root? mb just node id?
    // TODO: maintain in-memory navigation stack to enable navigating back (navigating to parent isn't safe w/o hash b/c remote nodes
    focus_node: NodeRef, // to allow zooming in on subnodes
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
    expanded_nodes: HashMap<NodeId, bool>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum Msg {
    Navigation(NavigationMsg),
    Backend(BackendMsg),
    Edit(EditMsg),
    NoOp,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum NavigationMsg {
    Maximize(NodeId),
    Minimize(NodeId),
    FocusOn(NodeRef), // set top-level focus to node
    FocusOnRoot,      // up one node from current node
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum EditMsg {
    EnterHeaderEdit { target: NodeId },
    EnterBodyEdit { target: NodeId },
    UpdateEdit(String),
    CommitEdit,
    CreateOn { at_idx: usize, parent: NodeId },
    // TODO: msg type for moving node up/down in list (eg swapping node position in child tree)
    Delete(NodeId),
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum BackendMsg {
    Fetch(RemoteNodeRef),                                    // HTTP fetch from store
    FetchComplete(RemoteNodeRef, notes_types::api::GetResp), // HTTP fetch from store (domain type)
    StartSave, // init backup of everything in store - blocking operation, probably
    SaveComplete(api_types::bulk_put::Resp),
}

#[derive(serde::Deserialize, Clone, Debug, PartialEq, Properties)]
pub struct Arg {
    #[props(required)]
    pub hash: Option<TypedHash<CannonicalNode>>,
}

impl Component for State {
    type Message = Msg;
    type Properties = Arg;

    fn create(opt_hash: Self::Properties, link: ComponentLink<Self>) -> Self {
        let mut nodes = HashMap::new();

        let (root_node, last_known_hash) = match opt_hash {
            Arg { hash: None } => {
                let fresh_root = InMemNode {
                    hash: None,             // not persisted
                    inner: Node::new(None), // None b/c node is root (no parent)
                };
                let id = NodeId::root();
                nodes.insert(id, fresh_root);
                (NodeRef::Modified(id), None)
            }
            Arg { hash: Some(h) } => (
                NodeRef::Unmodified(RemoteNodeRef(NodeId::root(), h)),
                Some(h),
            ),
        };

        // repeatedly wake up save process - checks root node, save (recursively) if modifed
        let mut interval_service = IntervalService::new();
        let callback = link.callback(move |_: ()| Msg::Backend(BackendMsg::StartSave));

        let save_interval = std::time::Duration::new(60, 0);
        let interval_task = interval_service.spawn(save_interval, callback);

        State {
            nodes: nodes,
            focus_node: root_node,
            root_node,
            last_known_hash,
            edit_state: None,
            // TODO: split out display-relevant state and capabilities
            link,
            fetch_service: FetchService::new(),
            fetch_tasks: HashMap::new(),
            save_task: None, // no active save op
            interval_service,
            interval_task,
            expanded_nodes: HashMap::new(),
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Navigation(n) => self.update_navigation(n),
            Msg::Edit(e) => self.update_edit(e),
            Msg::Backend(b) => self.update_backend(b),
            Msg::NoOp => {}
        }
        true
    }

    fn view(&self) -> Html {
        html! {
            <div class="wrapper">
                <button class="smallButton trigger-save" onclick=self.link.callback( |_| Msg::Backend(BackendMsg::StartSave) ) >
                {"[S]"}
                </button>
                <button class="smallButton" onclick= self.link.callback( |_| Msg::Navigation(NavigationMsg::FocusOnRoot) )>
                {"[^]"}
                </button>
                <div> { render_is_modified_widget(self.root_node) } </div>
                <ul class = "top-level">
                    { self.render_node(self.focus_node) }
                </ul>
            </div>
        }
    }
}

impl State {
    fn get_node(&self, id: &NodeId) -> &InMemNode {
        self.nodes
            .get(id)
            .expect(&format!("broken pointer: {:?}", id))
    }

    fn get_node_mut(&mut self, id: &NodeId) -> &mut InMemNode {
        self.nodes
            .get_mut(id)
            .expect(&format!("broken pointer (mut): {:?}", id))
    }

    fn update_navigation(&mut self, msg: NavigationMsg) {
        println!("handle navigation msg: {:?}", &msg);
        match msg {
            NavigationMsg::Maximize(node_id) => {
                self.set_expanded(node_id, true);
            }
            NavigationMsg::Minimize(node_id) => {
                // doing this here makes my life significantly simpler,
                // at the cost of having to re-enter edit state sometimes
                // (for case where minimized node is parent of edit focus)
                self.commit_edit();
                self.set_expanded(node_id, false);
            }
            NavigationMsg::FocusOnRoot => {
                self.focus_node = self.root_node;
            }
            NavigationMsg::FocusOn(node_ref) => {
                println!("focus on: {:?}", &node_ref);
                // doing this here makes my life significantly simpler,
                // at the cost of having to re-enter edit state sometimes
                // (for case where edit focus node is parent/sibling of focus node)
                self.commit_edit();

                self.focus_node = node_ref;
            }
        }
    }

    fn update_edit(&mut self, msg: EditMsg) {
        println!("handle edit msg: {:?}", &msg);
        match msg {
            EditMsg::EnterHeaderEdit { target } => {
                // commit pre-existing edit if exists
                if let Some(_) = &self.edit_state {
                    self.commit_edit();
                };

                let node = self.get_node(&target);
                self.edit_state = Some(EditState {
                    component_focus: EditFocus::NodeHeader,
                    node_focus: target,
                    edited_contents: node.header.clone(),
                });
            }
            EditMsg::EnterBodyEdit { target } => {
                // commit pre-existing edit if exists
                if let Some(_) = &self.edit_state {
                    self.commit_edit();
                };

                let node = self.get_node(&target);
                self.edit_state = Some(EditState {
                    component_focus: EditFocus::NodeBody,
                    node_focus: target,
                    edited_contents: node.body.clone(),
                });
            }
            EditMsg::UpdateEdit(new_s) => {
                let es: &mut EditState = self
                    .edit_state
                    .as_mut()
                    .expect("no edit state, attempting to update");
                es.edited_contents = new_s;
            }
            EditMsg::CommitEdit => {
                self.commit_edit();
            }
            EditMsg::Delete(node_id) => {
                // doing this here makes my life significantly simpler,
                // at the cost of having to re-enter edit state sometimes
                self.commit_edit();

                let mut svc = DialogService::new();
                let node = self.get_node(&node_id);

                if svc.confirm(&format!("delete node with header {}", node.header)) {
                    // set node & parents to modified before removing it
                    self.set_parent_nodes_to_modified(node_id);

                    let node = self
                        .nodes
                        .remove(&node_id)
                        .expect("error - attempting to delete nonexisting node");

                    if let Some(parent) = &node.parent {
                        let parent = self.get_node_mut(&parent);
                        parent
                            .children
                            .retain(|node_ref| node_ref.node_id() != node_id)
                    }

                    let mut stack: Vec<NodeId> = node
                        .inner
                        .children
                        .into_iter()
                        .map(|x| x.node_id())
                        .collect();

                    // garbage-collect any locally present children of deleted node
                    while let Some(next_id) = stack.pop() {
                        // children of deleted nodes may not exist locally, fine if not exists
                        if let Some(node) = self.nodes.remove(&next_id) {
                            for next_id in node.inner.children.into_iter().map(|x| x.node_id()) {
                                stack.push(next_id);
                            }
                        }
                    }
                };
            }
            EditMsg::CreateOn { parent, at_idx } => {
                // we're modifying this node, so walk back to root and make sure all parent nodes reflect modification
                //TODO: still needed, figure out new sig
                self.set_parent_nodes_to_modified(parent);

                // close out any pre-existing edit ops
                self.commit_edit(); // may call set_parent_nodes_to_modified & overlap w/ above walk_path_to_root nodes

                let node = self.get_node_mut(&parent);
                let new_node_id = gen_node_id();
                node.children.insert(at_idx, NodeRef::Modified(new_node_id)); // insert reference to new node
                let new_node = InMemNode {
                    hash: None,
                    inner: Node::new(Some(parent)),
                };
                self.nodes.insert(new_node_id, new_node); // insert new node

                // enter edit mode with empty header
                self.edit_state = Some(EditState {
                    component_focus: EditFocus::NodeHeader,
                    node_focus: new_node_id,
                    edited_contents: "".to_string(),
                });
            }
        }
    }

    fn update_backend(&mut self, msg: BackendMsg) {
        println!("handle backend msg: {:?}", &msg);
        match msg {
            BackendMsg::StartSave => {
                if let NodeRef::Modified(modified_root_id) = &self.root_node {
                    // construct map of all nodes that have been modified
                    let mut extra_nodes: HashMap<NodeId, Node<NodeRef>> = HashMap::new();
                    let head_node = self.get_node(&modified_root_id).inner.clone();

                    let mut stack: Vec<NodeId> = Vec::new();

                    for node_ref in head_node.children.iter() {
                        if let NodeRef::Modified(id) = node_ref {
                            stack.push(*id);
                        };
                    }

                    while let Some(id) = stack.pop() {
                        let node = self.get_node(&id);
                        for node_ref in node.children.iter() {
                            if let NodeRef::Modified(id) = node_ref {
                                stack.push(*id);
                            };
                        }
                        extra_nodes.insert(id, node.inner.clone());
                    }

                    // unable to validate tree, error is in above algorithm
                    let tree = ValidatedTree_::validate_(head_node, extra_nodes, |n| {
                        n.children.clone().into_iter().filter_map(|x| match x {
                            NodeRef::Modified(x) => Some(x),
                            _ => None,
                        })
                    })
                    .expect("failure validating tree while building put request");

                    let req = notes_types::api::PutReq {
                        tree,
                        cas_hash: self.last_known_hash,
                    };

                    self.push_nodes(req);
                } else {
                    println!("no modified nodes found, not saving");
                    // no-op if root node not modified
                }
            }
            // TODO: handle CAS violation, currently will just explode, lmao
            // kinda gnarly, basically just writes every saved node to the nodes store and in the process wipes out the modified nodes
            BackendMsg::SaveComplete(resp) => {
                let root_id = NodeId::root();

                self.save_task = None; // re-enable updates
                let root_hash = resp.root_hash.promote::<CannonicalNode>();
                self.last_known_hash = Some(root_hash); // set last known hash for future CAS use
                let root_node_ref = RemoteNodeRef(root_id, root_hash);
                // update entry point to newly-persisted root node
                self.root_node = NodeRef::Unmodified(root_node_ref);

                // build client id -> hash map for lookups
                let mut node_id_to_hash = HashMap::new();
                for (id, hash) in resp.additional_uploaded.clone().into_iter() {
                    node_id_to_hash.insert(NodeId::from_generic(id).unwrap(), hash);
                }

                // update root node with hash
                let mut node = self.get_node_mut(&root_id);
                node.hash = Some(root_hash);

                for (id, hash) in resp.additional_uploaded.into_iter() {
                    let hash = hash.promote::<CannonicalNode>();
                    let id = NodeId::from_generic(id).unwrap(); // FIXME - type conversion gore
                    let mut node = self.get_node_mut(&id);
                    node.hash = Some(hash);
                    if let Some(parent_id) = node.parent {
                        drop(node);
                        let parent_node = self.get_node_mut(&parent_id);
                        parent_node.map_mut(|node_ref| {
                            if node_ref.node_id() == id {
                                *node_ref = NodeRef::Unmodified(RemoteNodeRef(id, hash));
                            }
                        })
                    }
                }
            }
            BackendMsg::Fetch(remote_node_ref) => {
                let request = Request::get(format!(
                    "/node/{}",
                    (remote_node_ref.1.to_base58()).to_string()
                ))
                .body(Nothing)
                .expect("fetch req builder failed");

                let callback = self.link.callback(
                    move |response: Response<
                        Json<Result<notes_types::api::GetResp, anyhow::Error>>,
                    >| {
                        if let (meta, Json(Ok(body))) = response.into_parts() {
                            if meta.status.is_success() {
                                Msg::Backend(BackendMsg::FetchComplete(remote_node_ref, body))
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
            BackendMsg::FetchComplete(node_ref, get_resp) => {
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
        }
    }

    fn commit_edit(&mut self) -> Option<(EditFocus, NodeId)> {
        // use take to remove edit focus, if any
        if let Some(es) = self.edit_state.take() {
            let node = self.get_node_mut(&es.node_focus);

            let res = match es.component_focus {
                EditFocus::NodeHeader => {
                    node.header = es.edited_contents;
                    Some((EditFocus::NodeHeader, es.node_focus))
                }
                EditFocus::NodeBody => {
                    node.body = es.edited_contents;
                    Some((EditFocus::NodeBody, es.node_focus))
                }
            };
            self.set_parent_nodes_to_modified(es.node_focus); // set as modified from this node to root
            res
        } else {
            None
        }
    }

    fn render_node(&self, node_ref: NodeRef) -> Html {
        let is_saving = self.save_task.is_some();

        if self.is_expanded(&node_ref.node_id()) {
            if let Some(node) = self.nodes.get(&node_ref.node_id()) {
                let node_child_count = node.children.len();
                let children: &Vec<NodeRef> = &node.children;
                html! {
                    <li class = "node">
                        { self.render_node_header(node_ref, &node) }
                        { self.render_node_body(node_ref.node_id(), &node) }
                            <ul class = "nested-list">
                                { for children.iter().map(|node_ref| {
                                        self.render_node(*node_ref)
                                    })
                                }
                    <button class="smallButton add-sub-node" onclick= self.link.callback( move |_|
                                if is_saving {
                                    Msg::NoOp
                                } else { Msg::Edit(
                                    EditMsg::CreateOn{ at_idx: node_child_count,
                                                       parent: node_ref.node_id(),
                                    })})>
                                {"[+]"}
                            </button>
                        </ul>
                    </li>
                }
            } else {
                if let NodeRef::Unmodified(remote_node_ref) = node_ref {
                    html! {
                        <li class = "node-lazy">
                            <button class="load-node" onclick=self.link.callback(move |_| Msg::Backend(BackendMsg::Fetch(remote_node_ref)))>
                        {"load node"}
                        </button>
                        </li>
                    }
                } else {
                    panic!("can't lazily load modified node ref")
                }
            }
        } else {
            let node_id = node_ref.node_id();
            html! {
                <button class="smallButton" onclick= self.link.callback( move |_| Msg::Navigation(NavigationMsg::Maximize(node_id)))>
                {"[+]"}
                </button>
            }
        }
    }

    fn render_node_header<T>(&self, node_ref: NodeRef, node: &Node<T>) -> Html {
        let node_id = node_ref.node_id();

        if let Some(focus_str) = &self
            .edit_state
            .iter()
            .filter(|e| e.node_focus == node_id && e.component_focus == EditFocus::NodeHeader)
            .map(|e| &e.edited_contents)
            .next()
        {
            let is_saving = self.save_task.is_some();
            // FIXME: lazy hack, disallow commiting edits during save task lifetime (TODO: refactor, dedup)
            let commit_msg = if is_saving {
                Msg::NoOp
            } else {
                Msg::Edit(EditMsg::CommitEdit)
            };
            let onkeypress_send = commit_msg.clone();
            let onblur_send = commit_msg.clone();
            html! {
                <div>
                    <input class="edit node-header"
                    type="text"
                    value=&focus_str
                    id = "edit-focus"
                    oninput= self.link.callback( move |e: InputData| Msg::Edit(EditMsg::UpdateEdit(e.value)) )
                    onblur = self.link.callback( move |_| onblur_send.clone() )
                    onkeypress=self.link.callback( move |e: KeyPressEvent| {
                        if e.key() == "Enter" { onkeypress_send.clone() } else { Msg::NoOp }
                    })
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
                <div>
                    <button class="smallButton" onclick=self.link.callback(move |_| Msg::Edit(EditMsg::Delete(node_id)))>
                        {"X"}
                    </button>
                    <button class="smallButton" onclick=self.link.callback(move |_| Msg::Navigation(NavigationMsg::Minimize(node_id)))>
                    {"[-]"}
                    </button>
                    <button class="smallButton" onclick=self.link.callback(move |_| Msg::Navigation(NavigationMsg::FocusOn(node_ref)))>
                    {"[z]"}
                    </button>
                    <div class="node-header" onclick= self.link.callback( move |_| Msg::Edit(EditMsg::EnterHeaderEdit{target: node_id}) )>
                    { &node.header }
                    </div>
                </div>
            }
        }
    }

    fn render_node_body<T>(&self, node_id: NodeId, node: &Node<T>) -> Html {
        if let Some(focus_str) = &self
            .edit_state
            .iter()
            .filter(|e| e.node_focus == node_id && e.component_focus == EditFocus::NodeBody)
            .map(|e| &e.edited_contents)
            .next()
        {
            let is_saving = self.save_task.is_some();
            // FIXME: lazy hack, disallow commiting edits during save task lifetime (TODO: refactor, dedup)
            let commit_msg = if is_saving {
                Msg::NoOp
            } else {
                Msg::Edit(EditMsg::CommitEdit)
            };
            let onkeypress_send = commit_msg.clone();
            let onblur_send = commit_msg.clone();

            html! {
                <div>
                    <textarea class="edit node-body"
                    value=&focus_str
                    id = "edit-focus"
                    oninput = self.link.callback( move |e: InputData| Msg::Edit(EditMsg::UpdateEdit(e.value)) )
                    onblur = self.link.callback( move |_| onblur_send.clone() )
                    onkeypress=self.link.callback ( move |e: KeyPressEvent| {
                        if e.key() == "Enter" { onkeypress_send.clone() } else { Msg::NoOp }
                    })
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
                <div class = "node-body" onclick=self.link.callback( move |_| Msg::Edit(EditMsg::EnterBodyEdit{ target: node_id }))>
                { &node.body }
                </div>
            }
        }
    }

    // NOTE: despite name also sets target node to modified
    // walk path up from newly modified node, setting all to modified incl links
    fn set_parent_nodes_to_modified(&mut self, starting_point: NodeId) {
        let mut prev = None;
        let mut target = starting_point;
        loop {
            let node = self.get_node(&target);
            if let Some(_stale_hash) = &node.hash {
                // if this is the root/entry point node, demote it to modified
                if self.root_node.node_id() == target {
                    self.root_node = NodeRef::Modified(target);
                };
            };

            let node = self.get_node_mut(&target);
            node.hash = None;

            // update pointer to previous node to indicate modification
            if let Some(prev) = prev {
                node.map_mut(|node_ref| {
                    if node_ref.node_id() == prev {
                        // downgrade any pointers to the prev node to modified
                        *node_ref = NodeRef::Modified(prev);
                    }
                })
            }

            // TODO: retain prev node id, map_mut over refs to update to local ref. combination should yield correct refs

            match &node.parent {
                Some(id) => {
                    prev = Some(target);
                    target = *id;
                }
                None => {
                    break;
                }
            }
        }
    }

    // succeeds if root node has no children, fails if it does
    fn push_nodes(&mut self, req: notes_types::api::PutReq) -> () {
        let request = Request::post("/nodes")
            .header("Content-Type", "application/json")
            .body(Json(&req))
            .expect("push node request");

        let callback = self.link.callback(
            move |response: Response<Json<Result<api_types::bulk_put::Resp, anyhow::Error>>>| {
                let (meta, Json(res)) = response.into_parts();
                if let Ok(body) = res {
                    if meta.status.is_success() {
                        Msg::Backend(BackendMsg::SaveComplete(body))
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

    fn set_expanded(&mut self, node_id: NodeId, is_expanded: bool) {
        self.expanded_nodes.insert(node_id, is_expanded);
    }

    fn is_expanded(&self, node_id: &NodeId) -> bool {
        *self.expanded_nodes.get(node_id).unwrap_or(&true)
    }
}

fn gen_node_id() -> NodeId {
    let u = rand::random::<u128>();
    if u == 0 {
        gen_node_id()
    } else {
        NodeId(u)
    }
}

// TODO: unicode, css, etc (currently just a debug indicator)
fn render_is_modified_widget(x: NodeRef) -> Html {
    match x {
        NodeRef::Modified(_) => {
            html! { <span class="state-is-modified"> {"[[modified!]]"} </span> }
        }
        NodeRef::Unmodified(_) => {
            html! { <span class="state-is-unmodified"> {"[[unmodified!]]"} </span> }
        }
    }
}
