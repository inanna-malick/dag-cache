use handlebars::Handlebars;
use std::{fs::File, io::prelude::*};
use structopt::StructOpt;
use tracing_honeycomb::new_honeycomb_telemetry_layer;
use tracing_subscriber::{filter::LevelFilter, layer::Layer, registry};
use std::collections::BTreeMap;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "notes server",
    about = "provides notes server-specific functionality"
)]
pub struct Opt {
    #[structopt(short = "p", long = "port")]
    port: u16,

    #[structopt(short = "h", long = "honeycomb_key_file")]
    honeycomb_key_file: String,

    #[structopt(short = "u", long = "dag_store_url")]
    dag_store_url: String,
}

impl Opt {
    pub fn into_runtime(self) -> Runtime {
        let mut file =
            File::open(self.honeycomb_key_file).expect("failed opening honeycomb key file");
        let mut honeycomb_key = String::new();
        file.read_to_string(&mut honeycomb_key)
            .expect("failed reading honeycomb key file");

        let honeycomb_config = libhoney::Config {
            options: libhoney::client::Options {
                api_key: honeycomb_key,
                dataset: "dag-cache".to_string(), // TODO: better name for this
                ..libhoney::client::Options::default()
            },
            transmission_options: libhoney::transmission::Options {
                max_batch_size: 1,
                ..libhoney::transmission::Options::default()
            },
        };

        let telemetry_layer = new_honeycomb_telemetry_layer("notes-server", honeycomb_config);

        let subscriber = telemetry_layer // publish to tracing
            .and_then(tracing_subscriber::fmt::Layer::builder().finish()) // log to stdout
            .and_then(LevelFilter::INFO) // omit low-level debug tracing (eg tokio executor)
            .with_subscriber(registry::Registry::default()); // provide underlying span data store

        tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

        Runtime {
            port: self.port,
            dag_store_url: self.dag_store_url,
            hb: mk_template(),
        }
    }
}

pub struct Runtime {
    pub port: u16,
    pub dag_store_url: String,
    pub hb: Handlebars,
}

impl Runtime {
    pub fn render<T>(&self, t: T) -> impl warp::Reply
        where
            T: serde::Serialize,
    {
        let mut data = BTreeMap::new();
        data.insert("initial_state", t);
        let body = self
            .hb
            .render("index.html", &data)
            .unwrap_or_else(|err| err.to_string());

        warp::reply::html(body)
    }
}

pub fn mk_template() -> Handlebars {
    let template = "<!doctype html>
            <html>
                <head>
                    <meta charset=\"utf-8\" />
                    <title>Merkle Tree Note App</title>
                    <link rel=\"stylesheet\" href=\"/tree.css\"/ >
                </head>
                <body>
                    <script>
                        window.starting_hash={{initial_state}};
                    </script>
                    <script src=\"/notes.js\"></script>
                </body>
            </html>";

    let mut hb = Handlebars::new();
    hb.register_template_string("index.html", template).unwrap();
    hb.register_escape_fn(|s| s.to_string());
    hb
}
