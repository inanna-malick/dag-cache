#![recursion_limit = "512"]

use dag_store_types::types::api as api_types;
use dag_store_types::types::domain::TypedHash;
use dag_store_types::types::validated_tree::ValidatedTree_;
use notes_types::notes::{CannonicalNode, Node, NodeId, NodeRef, RemoteNodeRef};
use std::collections::HashMap;
use stdweb::js;
use stdweb::unstable::TryInto;
use stdweb::web::event::IEvent;
use stdweb::web::*; // FIXME
use stdweb::Value;
use yew::events::IKeyboardEvent;
use yew::events::{KeyDownEvent, KeyPressEvent};
use yew::format::{Json, Nothing};
use yew::html::InputData;
use yew::services::{
    dialog::DialogService,
    fetch::{FetchService, FetchTask, Request, Response},
    interval::{IntervalService, IntervalTask},
};
use yew::{html, Component, ComponentLink, Html, Properties, ShouldRender};

// new design ideas:
// as in workflowy, collapse controls down to single button on
// - enter finishes current, adds child on parent (except if root node)
// - tab makes current node child of previous node
// - shift-tab places current node after parent

macro_rules! println {
    ($($tt:tt)*) => {{
        let msg = format!($($tt)*);
        js! { @(no_return) console.log(@{ msg }) }
    }}
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
    node_focus: NodeId, // node id & not ref because _must_ be local node
    // CONCEPT: remove this entirely, track inline as value of focused node (fetch via id), maintain by just grabbing current value (via id) on re-renders
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
    // concept: have this be EditState | KeyboardNavFocusState
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
    UpdateEdit(String),
    OnBlur(NodeId),
    OnEnter,
    OnTab,
    OnShiftTab,
    OnBackspace,
    // TODO[eventually]: msg type for moving node up/down in list (eg swapping node position in child tree)
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

        let (root_node, last_known_hash, edit_state) = match opt_hash {
            Arg { hash: None } => {
                let fresh_root = InMemNode {
                    hash: None,             // not persisted
                    inner: Node::new(None), // None b/c node is root (no parent)
                };
                let id = NodeId::root();
                nodes.insert(id, fresh_root);
                let es = EditState {
                    node_focus: id,
                    edited_contents: "".to_string(),
                };

                (NodeRef::Modified(id), None, Some(es))
            }
            Arg { hash: Some(h) } => (
                NodeRef::Unmodified(RemoteNodeRef(NodeId::root(), h)),
                Some(h),
                None,
            ),
        };

        // repeatedly wake up save process - checks root node, save (recursively) if modifed
        let mut interval_service = IntervalService::new();
        let callback = link.callback(move |_: ()| Msg::Backend(BackendMsg::StartSave));

        let save_interval = std::time::Duration::new(20, 0);
        let interval_task = interval_service.spawn(save_interval, callback);

        let mut s = State {
            nodes: nodes,
            focus_node: root_node,
            root_node,
            last_known_hash,
            edit_state,
            // TODO: split out display-relevant state and capabilities
            link,
            fetch_service: FetchService::new(),
            fetch_tasks: HashMap::new(),
            save_task: None, // no active save op
            interval_service,
            interval_task,
            expanded_nodes: HashMap::new(),
        };

        if let NodeRef::Unmodified(root_node) = s.root_node {
            s.start_fetch(root_node);
        }

        s
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::Navigation(n) => self.update_navigation(n),
            Msg::Edit(e) => self.update_edit(e),
            Msg::Backend(b) => self.update_backend(b),
            Msg::NoOp => false,
        }
    }

    // NOTE: setting selection here is a neat hack but may fuck w/ preexisting selection on rerender - should only do so if no selection exists
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

    fn update_navigation(&mut self, msg: NavigationMsg) -> ShouldRender {
        println!("handle navigation msg: {:?}", &msg);
        match msg {
            NavigationMsg::Maximize(node_id) => {
                self.set_expanded(node_id, true);
                true
            }
            NavigationMsg::Minimize(node_id) => {
                // doing this here makes my life significantly simpler,
                // at the cost of having to re-enter edit state sometimes
                // (for case where minimized node is parent of edit focus)
                self.commit_edit();
                self.set_expanded(node_id, false);
                true
            }
            NavigationMsg::FocusOnRoot => {
                self.focus_node = self.root_node;
                true
            }
            NavigationMsg::FocusOn(node_ref) => {
                println!("focus on: {:?}", &node_ref);
                // doing this here makes my life significantly simpler,
                // at the cost of having to re-enter edit state sometimes
                // (for case where edit focus node is parent/sibling of focus node)
                self.commit_edit();

                self.focus_node = node_ref;
                true
            }
        }
    }

    fn update_edit(&mut self, msg: EditMsg) -> ShouldRender {
        println!("handle edit msg: {:?}", &msg);
        match msg {
            EditMsg::EnterHeaderEdit { target } => {
                // commit pre-existing edit if exists
                if let Some(_) = &self.edit_state {
                    self.commit_edit();
                };

                let node = self.get_node(&target);
                self.edit_state = Some(EditState {
                    node_focus: target,
                    edited_contents: node.header.clone(),
                });
                true
            }
            EditMsg::UpdateEdit(new_s) => {
                let es: &mut EditState = self
                    .edit_state
                    .as_mut()
                    .expect("no edit state, attempting to update");
                es.edited_contents = new_s;
                false
            }
            // NOTE/FIXME: shifttab and tab are merging nodes or something similarly weird and idgi
            EditMsg::OnShiftTab => {
                // move edit focus up one, as sibling of parent
                // NOTE: does not commit edit - should be able to get away with this?
                if let Some(es) = &self.edit_state {
                    let focused_node = es.node_focus;

                    // no-op if on root node
                    if let Some(parent) = self.get_node(&focused_node).parent {
                        if let Some(grandparent) = self.get_node(&parent).parent {
                            // remove current node from parent node
                            let parent_node = self.get_node_mut(&parent);
                            let current_node_idx = parent_node
                                .children
                                .iter()
                                .position(|x| &x.node_id() == &focused_node)
                                .expect("current node id not found in parent children, bug");
                            parent_node.children.remove(current_node_idx);
                            drop(parent_node);

                            // add current node to grandparent node after parent node
                            let grandparent_node = self.get_node_mut(&grandparent);

                            println!(
                                "parent node {:?}, grandparent children {:?}",
                                &parent, &grandparent_node.children
                            );

                            let parent_node_idx = grandparent_node
                                .children
                                .iter()
                                .position(|x| &x.node_id() == &parent)
                                .expect("parent node id not found in grandparent children, bug");
                            grandparent_node
                                .children
                                .insert(parent_node_idx + 1, NodeRef::Modified(focused_node));
                            drop(grandparent_node);

                            let node = self.get_node_mut(&focused_node);
                            node.parent = Some(grandparent);

                            self.set_parent_nodes_to_modified(focused_node);
                            self.set_parent_nodes_to_modified(parent);


                            // FIXME: huge hack, sets focus w/ delay of 50ms so as to get in after re-render
                            // removeallranges might only work on firefox
                            js! {
                                // hax hax hax, but should preserve selection
                                var sel = window.getSelection();
                                var start = sel.focusOffset;
                                console.log("selection start was: ");
                                console.log(start);
                                console.log(sel);

                                window.getSelection().removeAllRanges();
                                document.activeElement.blur();


                                setTimeout(function() {
                                    var tag = document.getElementById("edit-focus");
                                    var r=new Range();
                                    var sel=getSelection();
                                    console.log("attempting to sel edit-focus");
                                    if (sel.rangeCount == 0) {
                                        console.log("sel edit-focus");
                                        r.setStart(tag.childNodes[0], start);
                                        sel.removeAllRanges();
                                        sel.addRange(r);
                                    };
                                }, 50);
                            }

                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            EditMsg::OnTab => {
                // move edit focus down one, as child of predecessor (iff one exists)
                // NOTE: does not commit edit - should be able to get away with this?
                if let Some(es) = &self.edit_state {
                    let focused_node = es.node_focus;

                    // no-op if on root node
                    if let Some(parent) = self.get_node(&focused_node).parent {
                        let parent_node = self.get_node_mut(&parent);
                        let current_node_idx = parent_node
                            .children
                            .iter()
                            .position(|x| &x.node_id() == &focused_node)
                            .expect("current node id not found in parent children, bug");

                        // no-op if already first child of parent - not yet supporting multiple-deep nesting in that fashion
                        if current_node_idx > 0 {
                            parent_node.children.remove(current_node_idx);
                            let new_parent_node_id = parent_node.children[current_node_idx - 1];
                            drop(parent_node);

                            let new_parent_node = self.get_node_mut(&new_parent_node_id.node_id());
                            new_parent_node
                                .children
                                .push(NodeRef::Modified(focused_node));
                            drop(new_parent_node);

                            let node = self.get_node_mut(&focused_node);
                            node.parent = Some(new_parent_node_id.node_id());

                            self.set_parent_nodes_to_modified(focused_node); // set as modified from this node to root, will hit old parent + new

                            // FIXME: huge hack, sets focus w/ delay of 50ms so as to get in after re-render
                            // removeallranges might only work on firefox
                            js! {
                                // hax hax hax, but should preserve selection
                                var sel = window.getSelection();
                                var start = sel.focusOffset;
                                console.log("selection start was: ");
                                console.log(start);
                                console.log(sel);

                                window.getSelection().removeAllRanges();
                                document.activeElement.blur();

                                setTimeout(function() {
                                    var tag = document.getElementById("edit-focus");
                                    var r=new Range();
                                    var sel=getSelection();
                                    console.log("attempting to sel edit-focus");
                                    if (sel.rangeCount == 0) {
                                        console.log("sel edit-focus");
                                        r.setStart(tag.childNodes[0], start);
                                        sel.removeAllRanges();
                                        sel.addRange(r);
                                    };
                                }, 50);
                            }


                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            EditMsg::OnBlur(id) => {
                // // only commit if the current edit state is the blurred node,
                // // to avoid commiting after blurs triggered by code & not user
                // if self.edit_state.iter().filter(|e| e.node_focus == id).next().is_some() {
                //     println!("onblur");
                //     self.commit_edit();
                //     true
                // } else {
                //     println!("onblur noop");
                //     false
                // }

                // FIXME: disabling onblur, it caused problems when blur was generated by programatic modification of DOM (moving nodes)
                false
            }
            EditMsg::OnBackspace => {
                if let Some(es) = &self.edit_state {
                    let focused_node = es.node_focus;
                    println!("node contents are empty");
                    if es.edited_contents.len() == 0 {
                        let node = self.get_node(&focused_node);
                        println!("and it has no subnodes, {:?}", node.children);
                        if node.children.len() ==0 {
                            println!("and it's not the root node");
                            if let Some(parent) = node.parent {
                                println!("then delete it from its parent's children vector");
                                let parent = self.get_node_mut(&parent);
                                parent
                                    .children
                                    .retain(|node_ref| node_ref.node_id() != focused_node);

                                println!("and remove the node");
                                self.nodes
                                    .remove(&focused_node)
                                    .expect("error - attempting to delete nonexisting node");

                            }

                            // drop the edit state
                            self.edit_state = None;

                            // and redraw
                            return true
                        }
                    }
                }

                false
            }
            EditMsg::OnEnter => {
                if let Some(es) = self.edit_state.take() {
                    let edited = es.edited_contents;
                    let focused_node = es.node_focus;

                    let sel: stdweb::web::Selection = window().get_selection().unwrap(); // FIXME: fails if no selection

                    // NOTE: sel start/end can be in different orders if caret at beginning or end of selection
                    let selection_start = sel.anchor_offset().min(sel.focus_offset()) as usize;
                    let selection_end = sel.focus_offset().max(sel.anchor_offset()) as usize;

                    println!(
                        "selection end: {}, start: {}, contents: {}, len {}",
                        selection_end,
                        selection_start,
                        &edited,
                        edited.len()
                    );

                    println!("split nodes");
                    // TODO: focus on next node at beginnning instead of on current one
                    // can now split this node to create a new node based on cursor position as a preceding sibling on the parent node

                    let node = self.get_node_mut(&focused_node);
                    node.header = if edited.len() < selection_start {
                        "".to_string()
                    } else {
                        edited[..selection_start].to_string()
                    };


                    // created as immediate predecessor of current node;
                    let new_node_id = gen_node_id();

                    let mut new_node = if let Some(parent) = self.get_node(&focused_node).parent {
                        let parent_node = self.get_node_mut(&parent);
                        let current_node_idx = parent_node
                            .children
                            .iter()
                            .position(|x| &x.node_id() == &focused_node)
                            .expect("current node id not found in parent children, bug");
                        parent_node
                            .children
                            .insert(current_node_idx + 1, NodeRef::Modified(new_node_id)); // insert reference to new node
                        InMemNode {
                            hash: None,
                            inner: Node::new(Some(parent)),
                        }
                    } else {
                        // on root node, create new child node at position 0
                        let root_node = self.get_node_mut(&focused_node);
                        root_node.children.insert(0, NodeRef::Modified(new_node_id)); // insert reference to new node
                        InMemNode {
                            hash: None,
                            inner: Node::new(Some(focused_node)),
                        }
                    };

                    let new_node_header = if edited.len() < selection_end {
                        "".to_string()
                    } else {
                        edited[selection_end..].to_string()
                    };

                    new_node.header = new_node_header.clone();

                    self.nodes.insert(new_node_id, new_node); // insert new node

                    self.set_parent_nodes_to_modified(focused_node); // set as modified from this node to root

                    // FIXME: huge hack, sets focus w/ delay of 100ms so as to get in after re-render
                    // removeallranges might only work on firefox
                    js! {
                        window.getSelection().removeAllRanges();
                        document.activeElement.blur();

                        setTimeout(function() {
                            var tag = document.getElementById("edit-focus");
                            var r=new Range();
                            var sel=getSelection();
                            console.log("attempting to sel edit-focus");
                            if (sel.rangeCount == 0) {
                                console.log("sel edit-focus");
                                r.setStart(tag.childNodes[0], 0);
                                sel.removeAllRanges();
                                sel.addRange(r);
                            };
                        }, 100);
                    }

                    self.edit_state = Some(EditState {
                        node_focus: new_node_id,
                        edited_contents: new_node_header,
                    });
                } else {
                    println!("on enter w/ no edit state");
                };


                true
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
                true
            }
        }
    }

    fn update_backend(&mut self, msg: BackendMsg) -> ShouldRender {
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
                    true
                } else {
                    println!("no modified nodes found, not saving");
                    false
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
                true
            }
            BackendMsg::Fetch(remote_node_ref) => {
                self.start_fetch(remote_node_ref);
                false
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
                true
            }
        }
    }

    fn start_fetch(&mut self, remote_node_ref: RemoteNodeRef) {
        let request = Request::get(format!(
            "/node/{}",
            (remote_node_ref.1.to_base58()).to_string()
        ))
        .body(Nothing)
        .expect("fetch req builder failed");

        let callback = self.link.callback(
            move |response: Response<Json<Result<notes_types::api::GetResp, anyhow::Error>>>| {
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

    fn commit_edit(&mut self) -> Option<NodeId> {
        // use take to remove edit focus, if any
        if let Some(es) = self.edit_state.take() {
            let node = self.get_node_mut(&es.node_focus);

            node.header = es.edited_contents;
            self.set_parent_nodes_to_modified(es.node_focus); // set as modified from this node to root
            Some(es.node_focus)
        } else {
            None
        }
    }

    fn render_node(&self, node_ref: NodeRef) -> Html {
        if let Some(node) = self.nodes.get(&node_ref.node_id()) {
            html! {
                <div class = "note">
                    { self.render_note_menu_widget(node_ref) }
                    { self.render_note_body(node_ref, &node) }
                    { if self.is_expanded(&node_ref.node_id()) {
                        html!{
                            <div class = "note-children">
                            { for node.children.iter().map(|node_ref| {
                                self.render_node(*node_ref)
                            })
                            }
                            </div>
                        }
                    } else {
                        html!{ <div class = "note-children" > {"..."} </div> }
                    }
                }
                </div>
            }
        } else {
            if let NodeRef::Unmodified(remote_node_ref) = node_ref {
                // TODO: move triggering of more loads to state update fn - traverse note tree, find visible nodes that are not loaded, send fetch
                html! {
                    <div class = "note-lazy">
                        <button class="load-note" onclick=self.link.callback(move |_| Msg::Backend(BackendMsg::Fetch(remote_node_ref)))>
                    {"load note"}
                    </button>
                    </div>
                }
            } else {
                panic!("can't lazily load modified node ref, indicative of bug")
            }
        }
    }

    fn render_note_menu_widget(&self, node_ref: NodeRef) -> Html {
        let node_id = node_ref.node_id();

        html! {
            <div class="menu">
            <span class="submenu">
                <a class="smallButton" onclick=self.link.callback(move |_| Msg::Edit(EditMsg::Delete(node_id)))>
                {"[X]"}
                </a>
                {
                    if self.is_expanded(&node_ref.node_id()) {
                        html! {
                            <a onclick=self.link.callback(move |_| Msg::Navigation(NavigationMsg::Minimize(node_id)))>
                            {"[-]"}
                            </a>
                        }
                    } else {
                        html! {
                            <a onclick=self.link.callback(move |_| Msg::Navigation(NavigationMsg::Maximize(node_id)))>
                            {"[+]"}
                            </a>
                        }
                    }
                }
            </span>
                <a class="smallButton" onclick=self.link.callback(move |_| Msg::Navigation(NavigationMsg::FocusOn(node_ref)))>
                {"[@]"}
                </a>
            </div>
        }
    }

    fn render_note_body<T>(&self, node_ref: NodeRef, node: &Node<T>) -> Html {
        let node_id = node_ref.node_id();

        if let Some(focus_str) = &self
            .edit_state
            .iter()
            .filter(|e| e.node_focus == node_id)
            .map(|e| &e.edited_contents)
            .next()
        {
            let is_saving = self.save_task.is_some();
            // FIXME: lazy hack, disallow commiting edits during save task lifetime (TODO: refactor, dedup)
            let onblur_msg = if is_saving {
                Msg::NoOp
            } else {
                Msg::Edit(EditMsg::OnBlur(node_id))
            };

            // NOTE: running through full loop on every input event, shouldn't be neccessary - mb just grab contents by id on enter?
            // NOTE: somewhat irritatingly, it might not be - need contents to set initial value with if rebuild triggered during edit
            // update: addressed somewhat by reducing # of re-renders

            html! {
            <div class="note-contents"
                 contentEditable="true"
                 id = "edit-focus"
                 oninput= self.link.callback( move |e: InputData| Msg::Edit(EditMsg::UpdateEdit(e.value)) )
                 onblur = self.link.callback( move |_| onblur_msg.clone() )
                 onkeypress=self.link.callback( move |e: KeyPressEvent| {
                        // FIXME: I bet I can delete this
                     if e.key() == "Enter" {
                         println!("suppress enter keypress");
                         e.prevent_default();
                         e.stop_propagation();
                     };
                     Msg::NoOp
                 })
                 onkeydown=self.link.callback( move |e: KeyDownEvent| {
                     println!("keydown: {:?}", e.key());
                     if is_saving {
                         Msg::NoOp
                     } else {
                         match e.key().as_str() {
                             "Enter" => {
                                 println!("match on enter");
                                 e.prevent_default();
                                 e.stop_propagation();
                                 Msg::Edit(EditMsg::OnEnter)
                             }
                             "Backspace" => {
                                 Msg::Edit(EditMsg::OnBackspace)
                             }
                             "Tab" => {
                                 println!("match on tab");
                                 e.prevent_default();
                                 e.stop_propagation();
                                 if e.shift_key() {
                                     Msg::Edit(EditMsg::OnShiftTab)
                                 } else {
                                     Msg::Edit(EditMsg::OnTab)
                                 }
                             }
                             _ => {
                                 Msg::NoOp
                             }
                         }
                     }
                 })
             >
                { &focus_str }
            </div>
            }
        } else {
            html! {
                <div class="note-contents"
                    contentEditable="true"
                    onclick= self.link.callback( move |_| Msg::Edit(EditMsg::EnterHeaderEdit{target: node_id}) )>
                { &node.header }
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
