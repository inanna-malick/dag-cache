// // all the LRU set stuff here is _heavily_ inspired by https://docs.rs/crate/lru/0.1.17/source/src/lib.rs
// use crate::cache::CacheCapability;
// use crate::ipfs_types::{DagNode, IPFSHash};
// use hashbrown::hash_map::DefaultHashBuilder;
// use lru::Associated;
// use lru::LruCache;
// use petgraph::stable_graph::StableGraph;
// use petgraph::{graph, Direction};
// use petgraph::visit::EdgeRef;
// use std::sync::Mutex;

// pub struct GraphLru(Mutex<GraphLruState>);

// // NOTE: not all dag node links correspond to graph entries - this is a cache, not a full representation
// struct AssociatedGraph {
//     graph: StableGraph<Node, IPFSHash, petgraph::Directed, usize>,
//     // map of ipfs hash to stub node representing same - updated when removing a node that has incoming edges
//     // so that the structure is retained
//     stub_nodes: hashbrown::HashMap<IPFSHash, StubNode>,
// }

// enum Node {
//     Remote, // aka STUB - invariant - should never have any outgoing edges, used only to ensure structural sharing
//     Local(DagNode),
// }

// impl Associated<IPFSHash, graph::NodeIndex<usize>> for AssociatedGraph {
//     fn witness_removal(&mut self, _k: &IPFSHash, idx: &graph::NodeIndex<usize>) {
//         // todo: walk refs _this node points to_ and do GC if ref_count -= 1  is == 0

//         let outbound_edges = self.graph.edges_directed(idx.clone(), Direction::Outgoing);
//         for edge in outbound_edges {
//             match self.stub_nodes.get_mut(edge.weight()) {
//                 None => {} // no-op, node must be actual local node in LRU proper
//                 Some(stub_node) => {
//                     // stub node now has one less referent, decrement ref count
//                     stub_node.ref_count -= 1;
//                     if stub_node.ref_count <= 0 {
//                         self.stub_nodes.remove(edge.weight());
//                     };
//                 }
//             };
//             // remove edge - this node will either be deleted or turned into a stub, and stubs shouldn't have outgoing edges
//             self.graph.remove_edge(edge.id());
//         }

//         let refs_to_this_node: Vec<_> = self
//             .graph
//             .neighbors_directed(idx.clone(), Direction::Incoming)
//             .collect();

//         if refs_to_this_node.len() > 0 {
//             // preserve inbound edges by downgrading to stub node
//             match self.graph.node_weight_mut(idx.clone()) {
//                 None => {
//                     // Bug!
//                     panic!("expected idx to resolve");
//                 }
//                 Some(Node::Remote) => {
//                     // Bug!
//                     panic!("expected idx to resolve to full node (Local) and not stub");
//                 }
//                 Some(n @ Node::Local(_)) => {
//                     // downgrade full node in graph to stub node to preserve edges pointing to this node
//                     *n = Node::Remote;
//                 }
//             }
//         } else {
//             // either 0 inbound refs to this node or a node with this usize id doesn't exist
//             match self.graph.remove_node(idx.clone()) {
//                 // NOTE: mb this should take value not ref? no longer used by lru..
//                 None => {
//                     // BUG!
//                     panic!(
//                         "witnessed removal of node from managing LRU cache but not in associated graph"
//                     );
//                 }
//                 Some(Node::Remote) => {
//                     // BUG!
//                     panic!("witnessed removal of stub node from managing LRU cache - should not have been in LRU");
//                 }
//                 Some(Node::Local(_)) => {
//                     // Expected case, node has been removed. success.
//                 }
//             };
//         }
//     }
// }

// #[derive(Clone)]
// struct StubNode {
//     idx: graph::NodeIndex<usize>,
//     ref_count: usize, // so I can remove stub nodes from the graph after every directed reference to them is removed
// }

// pub struct GraphLruState {
//     lru: LruCache<IPFSHash, graph::NodeIndex<usize>, DefaultHashBuilder, AssociatedGraph>,
// }

// impl CacheCapability for GraphLru {
//     fn get(&self, k: IPFSHash) -> Option<DagNode> {
//         // succeed or die. failure is unrecoverable (mutex poisoned)
//         let mut state = self.0.lock().unwrap();
//         let idx = state.lru.get(&k)?;
//         let idx = idx.clone();
//         match state.lru.associated.graph.node_weight(idx.clone()) {
//             Some(Node::Local(node)) => Some(node.clone()),
//             // BUG!
//             Some(Node::Remote) => panic!("stub node found in graph for idx stored in lru cache"),
//             // BUG!
//             None => panic!("no node found in graph for idx stored in lru cache"),
//         }
//     }

//     fn put(&self, hash: IPFSHash, node: DagNode) {
//         // succeed or die. failure is unrecoverable (mutex poisoned)
//         let mut state = self.0.lock().unwrap();

//         let opt_stub_node = state.lru.associated.stub_nodes.get(&hash).cloned();
//         let idx = match opt_stub_node {
//             None => state
//                 .lru
//                 .associated
//                 .graph
//                 .add_node(Node::Local(node.clone())), // no stub for hash, add full node
//             Some(stub) => {
//                 // stub for node, promoted to full node
//                 state.lru.associated.stub_nodes.remove(&hash); // no longer tracked via stub node map (b/c will be added to LRU)
//                 let stub_idx = stub.idx;
//                 match state.lru.associated.graph.node_weight_mut(stub_idx.clone()) {
//                     None => {
//                         // Bug!
//                         panic!("expected stub idx to resolve")
//                     }
//                     Some(Node::Local(_)) => {
//                         // Bug!
//                         panic!("expected stub idx to resolve to stub (Remote) node and not local")
//                     }
//                     Some(n @ Node::Remote) => {
//                         *n = Node::Local(node.clone());
//                         stub_idx.clone() // stub... no longer
//                     }
//                 }
//             }
//         };

//         for hdr in node.links.iter() {
//             let hdr_idx = match state.lru.peek(&hdr.hash) {
//                 Some(full_node_idx) => full_node_idx.clone(), // already in graph as actual node, just add edge
//                 None => {
//                     // not in graph as full node (not in lru), add stub to track edge
//                     match state.lru.associated.stub_nodes.get_mut(&hdr.hash) {
//                         None => {
//                             let stub_idx = state.lru.associated.graph.add_node(Node::Remote);
//                             state.lru.associated.stub_nodes.insert(
//                                 hdr.hash.clone(),
//                                 StubNode {
//                                     idx: stub_idx,
//                                     ref_count: 1,
//                                 },
//                             );
//                             stub_idx.clone()
//                         }
//                         Some(stub_node) => {
//                             stub_node.ref_count += 1; // increment ref count
//                             stub_node.idx.clone()
//                         }
//                     }
//                 }
//             };

//             state
//                 .lru
//                 .associated
//                 .graph
//                 .add_edge(idx, hdr_idx, hdr.hash.clone());
//         }

//         state.lru.put(hash, idx);
//     }
// }
