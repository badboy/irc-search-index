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
#[macro_use] extern crate structopt_derive;
#[macro_use] extern crate error_chain;

use std::io::prelude::*;
use std::io::BufReader;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Instant;

use regex::Regex;
use walkdir::WalkDir;

use tantivy::Index;
use tantivy::schema::*;
use tantivy::collector::{self, CountCollector, TopCollector};
use tantivy::query::QueryParser;

use rocket::State;
use rocket::response::{Redirect, NamedFile};
use rocket_contrib::Template;

use structopt::StructOpt;

mod errors;
use errors::*;

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
    }
}

lazy_static! {
    static ref RE: Regex = Regex::new(r"(?x)
    (?P<time>\d{2}:\d{2})\s
    [+@&]?
    \s*
    (?P<nick>[^\s][^>]+)
    >
    \s
    (?P<msg>.+)
                                      ").unwrap();

    static ref WS: Regex = Regex::new(r"\s+").unwrap();
}

fn build_index(index_path: &Path, data_path: &str) -> Result<()> {
    let mut schema_builder = SchemaBuilder::default();
    schema_builder.add_text_field("time", TEXT | STORED);
    schema_builder.add_text_field("nick", TEXT | STORED);
    schema_builder.add_text_field("msg", TEXT | STORED);
    let schema = schema_builder.build();

    let index = Index::create(index_path, schema.clone())?;
    let mut index_writer = index.writer(500_000_000)?;

    let time_field = schema.get_field("time").unwrap();
    let nick_field = schema.get_field("nick").unwrap();
    let msg_field = schema.get_field("msg").unwrap();

    let mut count = 0;
    println!("Indexing...");

    let now = Instant::now();
    for entry in WalkDir::new(data_path) {
        let entry = entry.unwrap();
        if entry.file_type().is_dir() { continue; }
        let date = entry.path().file_stem().expect("Can't stem filename");
        let date = date.to_string_lossy();

        let file = File::open(entry.path())?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line?;
            let caps = match RE.captures(&line) {
                Some(m) => m,
                None => continue
            };

            let datetime = format!("{} {}", date, &caps["time"]);

            if WS.is_match(&caps["nick"]) { continue; }

            let mut doc = Document::default();
            doc.add_text(time_field.clone(), &datetime);
            doc.add_text(nick_field.clone(), &caps["nick"]);
            doc.add_text(msg_field.clone(),  &caps["msg"]);
            index_writer.add_document(doc);

            count += 1;
        }
    }
    println!("Indexing took {} seconds", now.elapsed().as_secs());
    let now = Instant::now();
    index_writer.commit().expect("Can't write index");
    println!("Writing index took {} seconds", now.elapsed().as_secs());

    println!("Indexed {} lines", count);

    Ok(())
}

struct IndexServer {
    index: Index,
    query_parser: QueryParser,
    schema: Schema,
}

fn init_index(index_path: &str) -> Result<IndexServer> {
    println!("Loading index from path");
    let index_path = Path::new(index_path);
    let index = Index::open(index_path)?;

    let schema = index.schema();
    let nick_field = schema.get_field("nick").unwrap();
    let msg_field = schema.get_field("msg").unwrap();

    let query_parser = QueryParser::new(index.schema(), vec![nick_field, msg_field]);

    Ok(IndexServer {
        index: index,
        query_parser: query_parser,
        schema: schema
    })
}

#[get("/")]
fn index_site() -> Redirect {
    Redirect::to("/search")
}

#[derive(FromForm)]
struct Query {
    q: Option<String>,
    limit: Option<usize>,
    _search: String,
}

#[derive(Serialize)]
struct SearchResult {
    q: String,
    num_hits: usize,
    shown_hits: usize,
    hits: Vec<Hit>,
    limit_10: bool,
    limit_50: bool,
    limit_100: bool,
}

#[derive(Serialize)]
struct Hit {
    time: String,
    nick: String,
    msg: String,
}

#[get("/search")]
fn search_site_no_query() -> Template {
    Template::render("search", None::<()>)
}

#[get("/search?<query>")]
fn search_site(idx: State<IndexServer>, query: Query) -> Result<Template> {
    if query.q.is_none() {
        return Ok(Template::render("search", None::<()>));
    }

    let user_query = query.q.unwrap();
    let limit = query.limit.unwrap_or(10);

    idx.index.load_searchers()?;
    let searcher = idx.index.searcher();

    let query = idx.query_parser.parse_query(&user_query).expect("Can't parse query");

    let mut count_collector = CountCollector::default();
    let mut top_collector = TopCollector::with_limit(limit);
    {
        let mut chained_collector = collector::chain()
            .push(&mut top_collector)
            .push(&mut count_collector);
        searcher.search(&*query, &mut chained_collector)?;
    }

    let doc_addresses = top_collector.docs();

    let hits = doc_addresses.into_iter().map(|da| {
        let retrieved_doc = searcher.doc(&da).expect("Can't get document");
        let doc = idx.schema.to_named_doc(&retrieved_doc);
        let map = doc.0;

        Hit {
            time: map["time"][0].text().to_owned(),
            nick: map["nick"][0].text().to_owned(),
            msg:  map["msg"][0].text().to_owned(),
        }
    }).collect::<Vec<_>>();


    let results = SearchResult {
        q: user_query,
        num_hits: count_collector.count(),
        shown_hits: hits.len(),
        hits: hits,
        limit_10: limit == 10,
        limit_50: limit == 50,
        limit_100: limit == 100,
    };

    Ok(Template::render("search", results))
}

#[get("/<file..>")]
fn files(file: PathBuf) -> Option<NamedFile> {
    NamedFile::open(Path::new("static/").join(file)).ok()
}

quick_main!(run);

fn run() -> Result<()> {
    let matches = Opt::from_args();

    match matches {
        Opt::Index { data_path, index_path } => {
            let index_path = Path::new(&index_path);
            build_index(&index_path, &data_path)?;
            println!("Everything indexed.");
        }
        Opt::Serve { index_path } => {
            rocket::ignite()
                .mount("/", routes![index_site, search_site_no_query, search_site, files])
                .attach(Template::fairing())
                .manage(init_index(&index_path)?)
                .launch();
        }
    }

    Ok(())
}
