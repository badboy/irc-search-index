use std::path::Path;
use std::io::prelude::*;
use std::io::BufReader;
use std::fs::File;
use std::time::Instant;

use tantivy::Index;
use tantivy::schema::*;

use walkdir::WalkDir;

use regex::Regex;

use errors::*;

lazy_static! {
    static ref RE: Regex = Regex::new(r"(?x)
    (?P<time>\d{2}:\d{2})\s
    [+@&]?
    \s*
    (?P<nick>[^\s][^>]+)
    >
    \s
    (?P<msg>.+)").unwrap();

    static ref WS: Regex = Regex::new(r"\s+").unwrap();
}

pub fn build_index(index_path: &str, data_path: &str) -> Result<()> {
    let mut schema_builder = SchemaBuilder::default();
    schema_builder.add_text_field("time", TEXT | STORED);
    schema_builder.add_text_field("nick", TEXT | STORED);
    schema_builder.add_text_field("msg", TEXT | STORED);
    let schema = schema_builder.build();

    let index_path = Path::new(index_path);
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

