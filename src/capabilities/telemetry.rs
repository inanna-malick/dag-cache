use ::libhoney::Value;
use libhoney::FieldHolder;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct Telemetry(Mutex<libhoney::Client<libhoney::transmission::Transmission>>);

impl Telemetry {
    pub fn new(honeycomb_key: String) -> Self {
        let honeycomb_client = libhoney::init(libhoney::Config {
            options: libhoney::client::Options {
                api_key: honeycomb_key,
                dataset: "dag-cache".to_string(),
                ..libhoney::client::Options::default()
            },
            transmission_options: libhoney::transmission::Options::default(),
        });

        // publishing requires &mut so just mutex-wrap it, lmao (FIXME)
        Telemetry(Mutex::new(honeycomb_client))
    }
}

impl Telemetry {
    pub fn report_data(&self, data: HashMap<String, Value>) {
        // succeed or die. failure is unrecoverable (mutex poisoned)
        let mut client = self.0.lock().unwrap();
        let mut ev = client.new_event();
        ev.add(data);
        let res = ev.send(&mut client); // todo check res? (FIXME)
        println!("event send res: {:?}", res);
    }
}
