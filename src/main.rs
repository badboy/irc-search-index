#![feature(plugin, custom_derive)]
#![plugin(rocket_codegen)]

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate serde_derive;
extern crate regex;
extern crate tantivy;
extern crate walkdir;
extern crate rocket;
extern crate rocket_contrib;
extern crate structopt;
#[macro_use]
extern crate structopt_derive;
#[macro_use]
extern crate error_chain;

use structopt::StructOpt;

mod errors;
mod index;
mod serve;

use errors::*;
use index::build_index;
use serve::serve;

#[derive(StructOpt, Debug)]
#[structopt(name = "irc-index")]
/// the simple IRC log searcher
enum Opt {
    #[structopt(name = "index")]
    /// Indexes all logs from the given path
    Index {
        #[structopt(short = "d", long = "data-path", default_value = "data")]
        data_path: String,
        #[structopt(short = "i", long = "index-path", default_value = "idx")]
        index_path: String,
    },
    #[structopt(name = "serve")]
    /// Serves the web interface
    Serve {
        #[structopt(short = "i", long = "index-path", default_value = "idx")]
        index_path: String,
    },
}

quick_main!(run);

fn run() -> Result<()> {
    let matches = Opt::from_args();

    match matches {
        Opt::Index { data_path, index_path } => {
            build_index(&index_path, &data_path)?;
            println!("Everything indexed.");
        }
        Opt::Serve { index_path } => {
            serve(&index_path)?;
        }
    }

    Ok(())
}
