# Capabilities Style

This is a document describing my attempts to implement one of my favorite Haskell patterns in Rust.

When writing an application with multiple capabilities it can be helpful to separate the implementation of these capabilities from how they're used. For example, if your app has some complex business logic interleaved with key/value store reads and writes then you can separate the key/value store _capability_ from the business logic that invokes it and test them each in isolation. The capability tests would need to spawn a key/value store process to test the integration with that external dependency, and the businesss logic tests would run using a simple in-memory map.


The use of this pattern in Haskell is best described in this article about the ReaderT design pattern. If you're not familiar with Haskell, it involves passing around a capabilities object holding the actual database connections, process handles, etc, and gating access on these capabilities via traits. This way, each function can declare the capabilities it requires via trait bounds.
https://www.fpcomplete.com/blog/2017/06/readert-design-pattern

## HasIPFSCapability

This app is designed to interact with a local IPFS daemon that it uses as a DAG node store. 

First, let's define a capability that lets us abstract over the basic operations provided by this capability, `get` and `put`. `get` fetches a DAG node from the store given a hash, `put` uploads a DAG node to the store and returns a hash. Both operations are async, and can fail with a `DagCacheError`.

```rust
pub trait IPFSCapability {
    fn get(&self, k: ipfs_types::IPFSHash) -> BoxFuture<ipfs_types::DagNode, DagCacheError>;
    fn put(&self, v: ipfs_types::DagNode) -> BoxFuture<ipfs_types::IPFSHash, DagCacheError>;
}
```

We also provide a trait that lets us assert an object that provides multiple capabilities gives us access to this capability. This way, we can just pass around one `Arc<C>` where traits are used to assert that `C` provides access to all the capabilities required by each function.


```rust
pub trait HasIPFSCap {
    type Output: IPFSCapability;

    fn ipfs_caps(&self) -> &Self::Output;
}
 
```

`C: HasIPFSCache` tells us we have the ability to get a reference to some fixed `Output` type implementing the `IPFSCapability` trait from a reference to `C`, with the `Output` type being determined by the trait implementation on `C`. 


## IPFSNode

```
pub struct IPFSNode(reqwest::Url);
```

All we need to interact with an IPFS node is the base URL. Given this, we can `get` and `put`

TODO

## IPFS API tests

TODO: test in isolation of get/put

## Bulk Upload

Each DAG node has as part of its text the hashes of every node it refrences. Therefore, a node cannot be uploaded unil every node it references has been uploaded, returning a hash of IPFS's hard-to-replicate hash of the DAG node's text.

This project provides a bulk node upload API designed for uploading trees of nodes that reference each other. To minimize network round trips, it builds a tree structure from the provided nodes and collapses it by uploading each leaf node and using it to build the next layer of nodes (phrasing?). For those familiar with recursion-schemes, it's essentially just a catamorphism (fancy word for a consuming change).

```rust
pub fn ipfs_publish_cata<C: 'static + HasCacheCap + HasIPFSCap + Sync + Send>(
    caps: Arc<C>,
    hash: ClientSideHash,
    tree: DagTree, // todo: better name?
) -> impl Future<Item = IPFSHeader, Error = api_types::DagCacheError> + 'static + Send {...}
```

## MockIPFS 

Having already written tests for IPFS get/put it would be ideal if we could focus on just testing this async algorithm and not the IPFS integration. 

```rust
struct MockIPFS(Mutex<HashMap<IPFSHash, DagNode>>);


impl IPFSCapability for MockIPFS {
    fn get(&self, k: IPFSHash) -> BoxFuture<DagNode, DagCacheError> {...}

    fn put(&self, v: DagNode) -> BoxFuture<IPFSHash, DagCacheError> {...}
}

```

This is all we need for an in-memory mock IPFS instance. This way, there's no need to spin up an external process as in the `IPFSNode` `IPFSCapability` tests. The full implementation is in /server/src/bulk_upload.rs, but basically it just uses a HashMap that's wrapped by a mutex for async safety to track upload operations and returns a short hash.

## Review

- separate definition of capabilities from how your program uses them
- use a single `Arc<SomeStruct: HasFooCap + HasBarCap + HasBazCap>` to manage capabilities
- integration test capabilities against external services
- unit test program using mock capabilities backed by in-memory state










