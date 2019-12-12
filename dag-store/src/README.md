

TODO:

- monomorphized servers w/ validation (via heavyweight Server type)
- use rust instead of haskell for now. BRANCHING CHOICE: 
-- expose web version of grpc API (at least get/put) and use rust WASM
-- use rust to build desktop app and use normal rust grpc client
-- COMBINATION: use rust to build localhost server that provides web interface (becomes obv/ choice w/ ADVANCED choice)
-- ADVANCED: actually implement rust grpc-web server (should be about 2 weeks work max, would be excellent resume builder)


CHOSEN: no grpc to frontend. intermediate server that handles auth, understands domain model, etc
