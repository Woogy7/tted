#[derive(Debug)]
struct Document {
    title: String,
    dirty: bool,
}

impl Document {
    fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            dirty: false,
        }
    }
}

fn main() {
    let document = Document::new("Welcome to TTED");
    println!("{document:#?}");
}
