use std::path::{Path, PathBuf};

use rocket;
use rocket::State;
use rocket::response::{Redirect, NamedFile};
use rocket_contrib::Template;

use tantivy::Index;
use tantivy::schema::*;
use tantivy::collector::{self, CountCollector, TopCollector};
use tantivy::query::QueryParser;

use errors::*;

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
        schema: schema,
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

    let hits = doc_addresses
        .into_iter()
        .map(|da| {
            let retrieved_doc = searcher.doc(&da).expect("Can't get document");
            let doc = idx.schema.to_named_doc(&retrieved_doc);
            let map = doc.0;

            Hit {
                time: map["time"][0].text().to_owned(),
                nick: map["nick"][0].text().to_owned(),
                msg: map["msg"][0].text().to_owned(),
            }
        })
        .collect::<Vec<_>>();


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
pub fn serve(index_path: &str) -> Result<()> {
    rocket::ignite()
        .mount("/", routes![index_site, search_site_no_query, search_site, files])
        .attach(Template::fairing())
        .manage(init_index(&index_path)?)
        .launch();

    Ok(())
}
