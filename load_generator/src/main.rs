use clap::Parser;
mod runner;
mod client;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct LoadGeneratorInputs {
    #[arg(short, long)]
    duration: u64,
    #[arg(short, long)]
    rps: u32 // TODO: Make this be > 1
}


#[tokio::main]
async fn main() {
    let input = LoadGeneratorInputs::parse();
    println!("Duration: {:?}", input.duration);
    println!("RPS: {:?}", input.rps);

    runner::start_test(input.duration, input.rps).await;
}
