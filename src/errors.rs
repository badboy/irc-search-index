use std::io;
use tantivy;

error_chain! {
    foreign_links {
        TantivyError(tantivy::Error);
        Io(io::Error);
    }
}
