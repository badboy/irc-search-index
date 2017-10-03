#[macro_use]
extern crate lazy_static;
extern crate regex;
extern crate tantivy;
extern crate walkdir;

use std::io::prelude::*;
use std::io::BufReader;
use std::fs::File;
use std::path::Path;
use std::env;
use std::time::Instant;

use regex::Regex;
use walkdir::WalkDir;

use tantivy::Index;
use tantivy::schema::*;
use tantivy::collector::{self, CountCollector, TopCollector};
use tantivy::query::QueryParser;

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

fn build_index(index_path: &Path, data_path: &str) -> Index {
    let mut schema_builder = SchemaBuilder::default();
    schema_builder.add_text_field("time", TEXT | STORED);
    schema_builder.add_text_field("nick", TEXT | STORED);
    schema_builder.add_text_field("msg", TEXT | STORED);
    let schema = schema_builder.build();

    let index = Index::create(index_path, schema.clone()).expect("Can't create index");
    let mut index_writer = index.writer(500_000_000).expect("Can't create index writer");

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

        let file = File::open(entry.path()).unwrap();
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line.unwrap();
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

    index
}


fn main() {
    let mut args = env::args().skip(1).peekable();
    let data_path = "data";
    let index_path = Path::new("idx");

    let skip_idx = match args.peek() {
        Some(a) if a == "skip" => true,
        _ => false
    };

    let index = if skip_idx {
        args.next();
        println!("Loading index from path");
        Index::open(index_path).expect("Can't load index")
    } else {
        build_index(&index_path, &data_path)
    };

    let has_limit = match args.peek() {
        Some(a) if a == "limit" => true,
        _ => false
    };

    let limit = if has_limit {
        args.next();
        args.next().unwrap().parse().unwrap_or(10)
    } else {
        10
    };

    let schema = index.schema();
    let nick_field = schema.get_field("nick").unwrap();
    let msg_field = schema.get_field("msg").unwrap();

    index.load_searchers().expect("Can't load searchers");
    let searcher = index.searcher();

    let query_parser = QueryParser::new(index.schema(), vec![nick_field, msg_field]);
    let user_query = args.collect::<Vec<_>>().join(" ");
    println!("Searching: {}", user_query);
    let query = query_parser.parse_query(&user_query).expect("Can't parse query");

    let mut count_collector = CountCollector::default();
    let mut top_collector = TopCollector::with_limit(limit);
    {
        let mut chained_collector = collector::chain()
            .push(&mut top_collector)
            .push(&mut count_collector);
        searcher.search(&*query, &mut chained_collector).expect("Can't search");
    }

    let doc_addresses = top_collector.score_docs();
    println!("Showing {} relevant results ({} total):", doc_addresses.len(), count_collector.count());
    for (score, doc_address) in doc_addresses {
        let retrieved_doc = searcher.doc(&doc_address).expect("Can't get document");
        let doc = schema.to_named_doc(&retrieved_doc);
        let map = doc.0;
        let time = &map["time"][0].text();
        let nick = &map["nick"][0].text();
        let msg = &map["msg"][0].text();

        println!("({:.2}) [{}] {:>12}> {}", score, time, nick, msg);
    }
}
