use handlebars::Handlebars;
use honeycomb_tracing::TelemetryLayer;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use structopt::StructOpt;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::Layer;
use tracing_subscriber::registry;

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

pub struct Runtime {
    pub port: u16,
    pub dag_store_url: String,
    pub hb: Handlebars,
}

impl Runtime {
    pub fn render<T>(&self, template: WithTemplate<T>) -> impl warp::Reply
    where
        T: serde::Serialize,
    {
        let body = self
            .hb
            .render(template.name, &template.value)
            .unwrap_or_else(|err| err.description().to_owned());

        warp::reply::html(body)
    }
}

pub struct WithTemplate<T: serde::Serialize> {
    pub name: &'static str,
    pub value: T,
}

impl Opt {
    pub fn into_runtime(self) -> Runtime {
        let mut file =
            File::open(self.honeycomb_key_file).expect("failed opening honeycomb key file");
        let mut honeycomb_key = String::new();
        file.read_to_string(&mut honeycomb_key)
            .expect("failed reading honeycomb key file");

        // NOTE: underlying lib is not really something I trust rn? just write my own queue + batch sender state machine...
        // TODO/FIXME/TODO/TODO: srsly, do this ^^
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
        let layer = TelemetryLayer::new("notes-server".to_string(), honeycomb_config)
            .and_then(tracing_subscriber::fmt::Layer::builder().finish())
            .and_then(LevelFilter::INFO);

        let subscriber = layer.with_subscriber(registry::Registry::default());
        tracing::subscriber::set_global_default(subscriber).expect("setting global default failed");

        let template = "<!doctype html>
            <html>
                <head>
                    <meta charset=\"utf-8\" />
                    <title>Yew • Merkle • Notes</title>
                    <link rel=\"stylesheet\" href=\"/tree.css\"/ >
                </head>
                <body>
                    <script>
                        window.starting_hash=\"{{initial_hash}}\";
                    </script>
                    <script src=\"/notes.js\"></script>
                </body>
            </html>";

        let mut hb = Handlebars::new();
        hb.register_template_string("index.html", template).unwrap();

        Runtime {
            port: self.port,
            dag_store_url: self.dag_store_url,
            hb,
        }
    }
}
