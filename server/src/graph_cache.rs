// // caches graph structure. never performs evictions.
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
