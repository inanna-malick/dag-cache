use dag_cache_types::types::ipfs::IPFSHash;
use stdweb::js;

fn main() {
    // TODO: replace this call with HTML template of some sort
    // - can include props in page instead of two-step static index + fetch
    let value = js!({
        return window.starting_hash;
    });
    let arg: notes::Arg = match value {
        stdweb::Value::Null => notes::Arg(None),
        stdweb::Value::String(s) => {
            let hash = IPFSHash::from_string(&s)
                .expect("unable to parse hash (handlebar template bug, FIXME)");
            notes::Arg(Some(hash))
        }
        _ => panic!("unexpected type from handlebar template, FIXME"),
    };
    yew::start_app_with_props::<notes::State>(arg);
}
