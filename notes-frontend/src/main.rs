use dag_store_types::types::domain::Hash;
use notes_types::notes::CannonicalNode;
use stdweb::js;

fn main() {
    // TODO: replace this call with HTML template of some sort
    // - can include props in page instead of two-step static index + fetch
    let value = js!({
        return window.starting_hash;
    });
    let arg: notes::Arg = match value {
        stdweb::Value::String(s) => {
            if s.is_empty() {
                notes::Arg { hash: None }
            } else {
                let hash = Hash::from_string(&s)
                    .expect("unable to parse hash (handlebar template bug, FIXME)")
                    .promote::<CannonicalNode>();
                notes::Arg { hash: Some(hash) }
            }
        }
        _ => panic!("unexpected type from handlebar template, FIXME"),
    };
    yew::start_app_with_props::<notes::State>(arg);
}
