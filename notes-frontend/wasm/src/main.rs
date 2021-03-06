use dag_store_types::types::domain::Hash;
use notes_types::notes::CannonicalNode;
use stdweb::js;

fn main() {
    let value = js!({
        return window.starting_hash;
    });
    let arg: notes::Arg = match value {
        stdweb::Value::String(s) => {
            if s.is_empty() {
                notes::Arg { hash: None }
            } else {
                let hash = Hash::from_base58(&s)
                    .expect("unable to parse hash (handlebar template bug, FIXME)")
                    .promote::<CannonicalNode>();
                notes::Arg { hash: Some(hash) }
            }
        }
        _ => panic!("unexpected type from handlebar template, FIXME"),
    };
    yew::start_app_with_props::<notes::State>(arg);
}
