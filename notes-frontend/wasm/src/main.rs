use notes_types::api::InitialState;
use stdweb::js;
use stdweb::unstable::TryInto;


macro_rules! println {
    ($($tt:tt)*) => {{
        let msg = format!($($tt)*);
        js! { @(no_return) console.log(@{ msg }) }
    }}
}

fn main() {
    println!("init");
    let value = js!({
        return window.starting_hash;
    });
    println!("got wsh");
    let initial_state: stdweb::serde::Serde<InitialState> = value
        .try_into()
        .expect("unable to parse initial state from template");
    println!("parsed initialstate");
    let arg = notes::IgnoringProperties(initial_state.0);
    yew::start_app_with_props::<notes::State>(arg);
}
