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


## Bulk Upload

nontrivial algorithm that interacts with ipfs

## MockIPFS 










