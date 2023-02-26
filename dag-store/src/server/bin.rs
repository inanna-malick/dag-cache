#![deny(warnings)]
use dag_store::{opts, run};
use opts::Opt;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = Opt::from_args();
    // TODO: move addr parsing _into_ opts
    let bind_to = format!("0.0.0.0:{}", &opt.port);
    let runtime = opt.into_runtime();

    let addr = bind_to.parse().unwrap();

    run(runtime, addr).await?;
    Ok(())
}
