extern crate structopt;
extern crate paw;
#[macro_use]
extern crate log;
#[macro_use]
extern crate shellfn;
#[macro_use]
extern crate failure;
extern crate ctrlc;

mod docker;


use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::fs::remove_dir_all;

use log::Level;
use structopt::StructOpt;
use failure::Error;

use docker::Docker;


#[derive(Debug, StructOpt)]
#[structopt(about = "Run an Albatross DevNet locally")]
struct Args {
    #[structopt(parse(from_os_str))]
    /// Path to the environment. Have a look at `devnet-environments`.
    env: PathBuf,
}


#[shell]
fn build_client<S: ToString>(env_dir: S) -> Result<impl Iterator<Item=Result<String, Error>>, Error> { r#"
    mkdir $ENV_DIR/build/
    export NIMIQ_OVERRIDE_DEVNET_CONFIG=$(realpath $ENV_DIR/dev-albatross.toml)
    cargo build --bin nimiq-client -Z unstable-options --out-dir $ENV_DIR/build/
"# }



fn run_devnet(args: Args, keyboard_interrupt: Arc<AtomicBool>) -> Result<(), Error> {
    let docker = Docker::new(&args.env);

    // Build `nimiq-client` binary
    info!("Building nimiq-client");
    for line in build_client(args.env.to_str().unwrap())? {
        println!("{}", line?);
    }


    // build docker containers
    docker.build("target/debug/nimiq-client")?;

    // run and print output
    // TODO: We could split up the streams from validators
    for line in docker.up()? {
        if keyboard_interrupt.load(Ordering::SeqCst) {
            break;
        }
        println!("{}", line?);
    }

    docker.down()?;

    // TODO: prune docker containers

    // delete build directory
    remove_dir_all(args.env.join("build"))
        .expect("Failed to delete build directory");

    Ok(())
}


#[paw::main]
fn main(args: Args) {
    simple_logger::init_with_level(Level::Info)
        .expect("Failed to initialize logging");

    debug!("{:#?}", args);

    // register handler for Ctrl-C
    let keyboard_interrupt = Arc::new(AtomicBool::new(false));
    {
        let keyboard_interrupt = Arc::clone(&keyboard_interrupt);
        ctrlc::set_handler(move || {
            info!("Keyboard interrupt");
            keyboard_interrupt.store(false, Ordering::SeqCst);
        }).expect("Failed to register handler for Ctrl-C");
    }

    if let Err(e) = run_devnet(args, keyboard_interrupt) {
        error!("Error: {}", e);
    }
}