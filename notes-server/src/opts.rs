use handlebars::Handlebars;
use structopt::StructOpt;
use std::collections::BTreeMap;
use tracing_jaeger::{
    new_opentelemetry_layer
};
use tracing_subscriber::registry;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::filter::LevelFilter;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "notes server",
    about = "provides notes server-specific functionality"
)]
pub struct Opt {
    #[structopt(short = "p", long = "port")]
    port: u16,

    #[structopt(short = "j", long = "jaeger-agent")]
    jaeger_agent: String,

    #[structopt(short = "u", long = "dag_store_url")]
    dag_store_url: String,
}

impl Opt {
    pub fn into_runtime(self) -> Runtime {

        let exporter = opentelemetry_jaeger::Exporter::builder()
            .with_agent_endpoint(self.jaeger_agent.parse().unwrap())
            .with_process(opentelemetry_jaeger::Process {
                service_name: "notes-server".to_string(),
                tags: vec![],
            })
            .init()
            .unwrap();

        let telemetry_layer = new_opentelemetry_layer(
            "notes-server", // TODO: duplication of service name here
            Box::new(exporter),
            Default::default(),
        );

        let subscriber = registry::Registry::default() // provide underlying span data store
            .with(LevelFilter::INFO) // filter out low-level debug tracing (eg tokio executor)
            .with(tracing_subscriber::fmt::Layer::default()) // log to stdout
            .with(telemetry_layer); // publish to honeycomb backend

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
