use keygrip::Schema;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Schema)]
#[entity(pk(id), index(name = "byName", pk(name), projection = "all"))]
struct Record {
    id: String,
    name: String,
}

fn main() {}
